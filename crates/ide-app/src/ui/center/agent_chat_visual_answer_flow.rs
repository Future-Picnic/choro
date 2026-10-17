//! Pure rules for Visual answer sends, notes and surface handoffs. They are
//! kept free of GPUI so the race-prone decisions can be tested directly.
use std::collections::HashMap;

use ide_core::visual_answers::{VisualActionKind, VisualAnswer, IMPLICIT_REFINE_ACTION};
use uuid::Uuid;

use super::agent_chat_visual_answer::merge_client_state;
use super::visual_answer_surface::{VisualAnswerEnvelope, VisualAnswerMessage};

/// Matches the store's draft bound (`VisualInteraction::validate`).
pub(super) const MAX_VISUAL_NOTE_BYTES: usize = 32 * 1024;
pub(super) const NOTE_TOO_LONG: &str = "This note is too long to keep with the visual answer (32 KB limit). Shorten it, or it will only stay in the composer.";
pub(super) const REVISION_CHANGED: &str = "The visual answer changed before this was sent, so nothing was sent. Review it, then send again.";
/// Outgoing surfaces whose final snapshot may still arrive.
const MAX_OUTGOING: usize = 4;

/// Exactly what a send applies to: the answer revision and surface the user
/// was looking at. A newer revision is never sent with an older note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SendTarget {
    pub answer_id: Uuid,
    pub revision: u64,
    pub surface: u64,
}

impl SendTarget {
    /// The send still applies to what the conversation shows.
    pub fn matches(&self, current: Option<&VisualAnswer>) -> bool {
        current
            .is_some_and(|answer| answer.id == self.answer_id && answer.revision == self.revision)
    }
}

/// Where a composer submit (Enter or the send arrow) goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ComposerRoute {
    /// No open visual answer applies: ordinary conversation send.
    Normal,
    /// Refine the open answer with this action.
    Visual { action_id: String },
    /// A visual send is in flight, or the note can't be sent: do nothing
    /// (never fall through to a plain duplicate without context).
    Blocked(Option<&'static str>),
}

/// `displayed` is the open, unsuppressed answer; `in_flight` covers the
/// snapshot wait, the queued reservation and dispatch; `note` is the plain
/// composer text (`None` with attachments or chips).
pub(super) fn composer_route(
    displayed: Option<&VisualAnswer>,
    in_flight: bool,
    note: Option<&str>,
) -> ComposerRoute {
    // An informational result is part of the conversation, not a question:
    // the composer keeps sending ordinary messages (its follow-up buttons
    // are the only way to send about it).
    if displayed.is_some_and(|answer| !answer.document.requires_user_response()) {
        return ComposerRoute::Normal;
    }
    if in_flight {
        return ComposerRoute::Blocked(None);
    }
    let Some(answer) = displayed else {
        return ComposerRoute::Normal;
    };
    if answer.interaction.submission.is_some() {
        // A sent or uncertain answer is a record; chat continues normally.
        return ComposerRoute::Normal;
    }
    let Some(note) = note else {
        return ComposerRoute::Normal;
    };
    if note.trim().is_empty() {
        return ComposerRoute::Normal;
    }
    if note.len() > MAX_VISUAL_NOTE_BYTES {
        return ComposerRoute::Blocked(Some(NOTE_TOO_LONG));
    }
    let action_id = answer
        .document
        .actions
        .iter()
        .find(|a| a.kind == VisualActionKind::Refine)
        .map(|a| a.id.clone())
        .unwrap_or_else(|| IMPLICIT_REFINE_ACTION.to_string());
    ComposerRoute::Visual { action_id }
}

/// Whether a composer note may be saved to the answer as its draft.
pub(super) fn note_for_answer(
    answer: &VisualAnswer,
    note: &str,
) -> Result<Option<String>, &'static str> {
    if answer.interaction.submission.is_some() || answer.interaction.draft == note {
        return Ok(None);
    }
    if note.len() > MAX_VISUAL_NOTE_BYTES {
        return Err(NOTE_TOO_LONG);
    }
    Ok(Some(note.to_string()))
}

/// A saved note returns only into an empty composer, so text the user typed
/// since (or a normal draft) is never replaced.
pub(super) fn note_to_restore(answer: &VisualAnswer, composer: &str) -> Option<String> {
    (composer.trim().is_empty()
        && answer.interaction.submission.is_none()
        && !answer.interaction.draft.trim().is_empty())
    .then(|| answer.interaction.draft.clone())
}

/// Debounced note saves, one timer generation per answer, so editing one
/// answer's note never cancels another answer's pending save.
#[derive(Default)]
pub(super) struct NoteTimers(HashMap<Uuid, u64>);

impl NoteTimers {
    pub fn bump(&mut self, answer_id: Uuid) -> u64 {
        let generation = self.0.entry(answer_id).or_default();
        *generation += 1;
        *generation
    }

    pub fn due(&self, answer_id: Uuid, generation: u64) -> bool {
        self.0.get(&answer_id) == Some(&generation)
    }
}

/// A store result (after a save, reservation or receipt update) merged into
/// the cached answer. Client fields edited locally since the write started
/// (a note typed while a save was in flight) always win; the store supplies
/// the interaction revision and the receipt. Returns `None` to keep the cache.
pub(super) fn accept_saved(
    cached: Option<&VisualAnswer>,
    saved: VisualAnswer,
) -> Option<VisualAnswer> {
    match cached {
        Some(cached) if cached.revision > saved.revision => None,
        Some(cached) if cached.revision == saved.revision => Some(VisualAnswer {
            interaction: merge_client_state(&saved.interaction, &cached.interaction),
            ..saved
        }),
        _ => Some(saved),
    }
}

/// A surface replaced within the same conversation (answer A → B, or a new
/// revision). Its one final snapshot may arrive after the switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OutgoingSurface {
    pub answer_id: Uuid,
    pub revision: u64,
    pub surface: u64,
}

#[derive(Default)]
pub(super) struct OutgoingSurfaces(Vec<OutgoingSurface>);

impl OutgoingSurfaces {
    pub fn register(&mut self, surface: OutgoingSurface) {
        self.0.retain(|s| s.surface != surface.surface);
        self.0.push(surface);
        if self.0.len() > MAX_OUTGOING {
            self.0.remove(0);
        }
    }

    /// Accept one final snapshot from a registered outgoing surface, only for
    /// its own answer and only while that answer is still at the same
    /// revision. Anything else (debounced state, repeats, newer revisions)
    /// is rejected. Returns the answer it applies to.
    pub fn take_final_snapshot(
        &mut self,
        envelope: &VisualAnswerEnvelope,
        cached: Option<&VisualAnswer>,
    ) -> Option<Uuid> {
        if !matches!(envelope.message, VisualAnswerMessage::Snapshot { .. }) {
            return None;
        }
        let index = self.0.iter().position(|s| {
            s.surface == envelope.surface
                && s.answer_id == envelope.answer_id
                && s.revision == envelope.revision
        })?;
        let outgoing = self.0.remove(index);
        cached
            .filter(|answer| {
                answer.id == outgoing.answer_id && answer.revision == outgoing.revision
            })
            .map(|answer| answer.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::center::visual_answer_surface::VisualRendererState;
    use ide_core::visual_answers::{
        VisualDocument, VisualInteraction, VisualSubmission, VisualSubmissionStatus,
    };

    fn answer(revision: u64, draft: &str) -> VisualAnswer {
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title":"Projects", "summary":"Sample", "kind":"table",
            "openui_lang":"root = Answer([t])\nt = DataTable(\"p\")",
            "datasets":{"p":{"columns":[{"key":"name","label":"Project","kind":"text"}],"rows":[]}},
            "sources":[{"label":"Sample data"}]
        }))
        .unwrap();
        VisualAnswer {
            id: Uuid::from_u128(1),
            project_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            revision,
            document,
            interaction: VisualInteraction {
                draft: draft.into(),
                ..Default::default()
            },
            created_at: 0,
            updated_at: 0,
        }
    }

    fn snapshot(
        answer_id: Uuid,
        revision: u64,
        surface: u64,
        search: &str,
    ) -> VisualAnswerEnvelope {
        VisualAnswerEnvelope {
            agent_id: Uuid::nil(),
            answer_id,
            revision,
            surface,
            message: VisualAnswerMessage::Snapshot {
                token: 9,
                state: VisualRendererState {
                    selected_option_id: None,
                    fields: serde_json::json!({"view":{"s:t":{"search":search}}}),
                    scroll_top: 12.0,
                },
            },
        }
    }

    /// The same answer with explicit options: a choice for the user.
    fn choice(mut answer: VisualAnswer) -> VisualAnswer {
        answer.document.options = ["solid", "tonal"]
            .into_iter()
            .map(|id| ide_core::visual_answers::VisualOption {
                id: id.into(),
                title: id.into(),
                description: String::new(),
                html: format!("<b>{id}</b>"),
            })
            .collect();
        answer
    }

    #[test]
    fn an_informational_result_leaves_composer_enter_to_ordinary_chat() {
        let table = answer(1, "");
        assert_eq!(
            composer_route(Some(&table), false, Some("Thanks, now fix the build")),
            ComposerRoute::Normal
        );
        // Its follow-up being sent never swallows an unrelated message.
        assert_eq!(
            composer_route(Some(&table), true, Some("Also check CI")),
            ComposerRoute::Normal
        );
        let mut followed = table.clone();
        followed.document.actions = vec![ide_core::visual_answers::VisualAction {
            id: "week".into(),
            label: "Show the last seven days".into(),
            kind: VisualActionKind::FollowUp,
        }];
        assert_eq!(
            composer_route(Some(&followed), false, Some("hi")),
            ComposerRoute::Normal,
            "an optional follow-up is not a question"
        );
    }

    /// Replays the composer's routing for Enter / the send arrow.
    #[test]
    fn a_second_enter_during_a_visual_send_never_becomes_a_plain_message() {
        let open = choice(answer(1, ""));
        // First Enter: refines the answer.
        assert_eq!(
            composer_route(Some(&open), false, Some("Only blocked")),
            ComposerRoute::Visual {
                action_id: IMPLICIT_REFINE_ACTION.into()
            }
        );
        // Snapshot wait / reservation / dispatch pending: a rapid second
        // Enter or send-arrow click is blocked, even with chips attached.
        assert_eq!(
            composer_route(Some(&open), true, Some("Only blocked")),
            ComposerRoute::Blocked(None)
        );
        assert_eq!(
            composer_route(Some(&open), true, None),
            ComposerRoute::Blocked(None)
        );
        assert_eq!(
            composer_route(None, true, Some("x")),
            ComposerRoute::Blocked(None)
        );
        // An oversized note is a visible visual error, not a fall-through.
        assert_eq!(
            composer_route(
                Some(&open),
                false,
                Some(&"x".repeat(MAX_VISUAL_NOTE_BYTES + 1))
            ),
            ComposerRoute::Blocked(Some(NOTE_TOO_LONG))
        );
        // Attachments/chips, no open answer, or a sent answer: normal send.
        assert_eq!(
            composer_route(Some(&open), false, None),
            ComposerRoute::Normal
        );
        assert_eq!(
            composer_route(None, false, Some("hi")),
            ComposerRoute::Normal
        );
        assert_eq!(
            composer_route(Some(&open), false, Some("  ")),
            ComposerRoute::Normal
        );
        let mut sent = open.clone();
        sent.interaction.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: IMPLICIT_REFINE_ACTION.into(),
            text: "x".into(),
            status: VisualSubmissionStatus::Sent,
        });
        assert_eq!(
            composer_route(Some(&sent), false, Some("thanks")),
            ComposerRoute::Normal
        );
    }

    #[test]
    fn a_note_typed_while_a_save_is_in_flight_survives_its_completion() {
        // Cached state: the user typed "b" after a save of "a" started.
        let local = answer(1, "b");
        let mut saved = answer(1, "a");
        saved.interaction.revision = 4;
        let merged = accept_saved(Some(&local), saved).unwrap();
        assert_eq!(merged.interaction.draft, "b", "the newer local note wins");
        assert_eq!(
            merged.interaction.revision, 4,
            "the store's counter is taken"
        );
        // A result for an older document revision never replaces a newer one.
        assert!(accept_saved(Some(&answer(2, "b")), answer(1, "a")).is_none());
        // A receipt fixes the draft that was sent.
        let mut receipt = answer(1, "sent note");
        receipt.interaction.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: IMPLICIT_REFINE_ACTION.into(),
            text: "sent note".into(),
            status: VisualSubmissionStatus::Sending,
        });
        assert_eq!(
            accept_saved(Some(&local), receipt)
                .unwrap()
                .interaction
                .draft,
            "sent note"
        );
    }

    #[test]
    fn note_saves_are_debounced_per_answer() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut timers = NoteTimers::default();
        let first_a = timers.bump(a);
        // Switching to answer B and typing within the delay does not cancel A.
        let first_b = timers.bump(b);
        assert!(timers.due(a, first_a));
        assert!(timers.due(b, first_b));
        // Typing again in A supersedes only A's earlier timer.
        let second_a = timers.bump(a);
        assert!(!timers.due(a, first_a));
        assert!(timers.due(a, second_a));
        assert!(timers.due(b, first_b));
    }

    #[test]
    fn sends_are_bound_to_the_revision_the_user_saw() {
        let target = SendTarget {
            answer_id: Uuid::from_u128(1),
            revision: 1,
            surface: 3,
        };
        assert!(target.matches(Some(&answer(1, ""))));
        // A revision published during the snapshot wait or while the send
        // waits behind a queued save: the old note/action is not sent.
        assert!(!target.matches(Some(&answer(2, ""))));
        assert!(!target.matches(None));
    }

    #[test]
    fn an_outgoing_answers_final_snapshot_survives_a_same_chat_switch() {
        let a = answer(1, "");
        let mut outgoing = OutgoingSurfaces::default();
        // A (surface 5) is replaced by B in the same conversation.
        outgoing.register(OutgoingSurface {
            answer_id: a.id,
            revision: 1,
            surface: 5,
        });
        let late = snapshot(a.id, 1, 5, "atl");
        assert_eq!(outgoing.take_final_snapshot(&late, Some(&a)), Some(a.id));
        // Applied to A's own record, which A's next surface (B → A) boots from.
        let VisualAnswerMessage::Snapshot { state, .. } = late.message.clone() else {
            unreachable!()
        };
        let restored =
            super::super::agent_chat_visual_answer::apply_renderer_state(&a, state).unwrap();
        assert_eq!(restored.fields["view"]["s:t"]["search"], "atl");
        assert_eq!(restored.scroll_top, 12.0);
        // One-time: a repeat (or a debounced state report) is rejected.
        assert_eq!(outgoing.take_final_snapshot(&late, Some(&a)), None);
        // A late snapshot can never touch a newer revision of A.
        outgoing.register(OutgoingSurface {
            answer_id: a.id,
            revision: 1,
            surface: 6,
        });
        let revised = answer(2, "");
        assert_eq!(
            outgoing.take_final_snapshot(&snapshot(a.id, 1, 6, "old"), Some(&revised)),
            None
        );
        // Or another answer, or an unregistered surface.
        let b = VisualAnswer {
            id: Uuid::from_u128(2),
            ..answer(1, "")
        };
        outgoing.register(OutgoingSurface {
            answer_id: a.id,
            revision: 1,
            surface: 7,
        });
        assert_eq!(
            outgoing.take_final_snapshot(&snapshot(a.id, 1, 7, "x"), Some(&b)),
            None
        );
        assert_eq!(
            outgoing.take_final_snapshot(&snapshot(a.id, 1, 99, "x"), Some(&a)),
            None
        );
        // The registry stays bounded.
        for surface in 10..20 {
            outgoing.register(OutgoingSurface {
                answer_id: a.id,
                revision: 1,
                surface,
            });
        }
        assert_eq!(outgoing.0.len(), MAX_OUTGOING);
    }
}
