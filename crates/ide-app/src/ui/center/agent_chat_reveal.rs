//! Paced, word-by-word reveal for the streaming assistant message.
//!
//! The agent's output arrives from the terminal in bursts — a flush lands a
//! whole phrase at once (see `StreamChunkBuffer`) — so painting the raw text
//! makes it *pop* in chunks. Instead we keep the full text but reveal it on our
//! own steady cadence, decoupled from the burst delivery, so it reads as calm
//! typing. The most recently revealed characters glow lavender and cool to the
//! normal text colour, giving a live "writing" edge.
//!
//! The reveal is purely presentation: the stored message always holds the
//! complete text (copy, persistence and search are unaffected); only the painted
//! prefix lags behind while a message is actively streaming.
//!
//! ## Never re-type a finished message
//!
//! The one rule that keeps this from misbehaving: a message types out from zero
//! *only if it was first seen while the agent was running* (i.e. it is genuinely
//! streaming now). A message already tracked in [`CenterArea::agent_chat_reveal`]
//! is never reset, and a message first seen while idle (history, hydration, or an
//! earlier turn's answer that is momentarily still the "last" message when a new
//! turn starts) is shown in full immediately. Messages only ever append, so a
//! message that stops being the last one never becomes the last one again — which
//! is why keeping a single cursor per agent is enough.

use std::time::{Duration, Instant};

use uuid::Uuid;

use super::CenterArea;
use crate::state::agent_chat::{AgentChatMessage, AgentChatSession};

/// Baseline reveal speed (characters per second) once caught up.
const BASE_CPS: f32 = 24.0;
/// Extra characters/sec per character of backlog, so a big burst drains quickly
/// instead of the reveal lagging ever further behind.
const CATCHUP_CPS_PER_CHAR: f32 = 2.2;
/// Never reveal faster than this — keeps even a huge dump legible. Capped low so
/// the agent's big bursts type out visibly instead of flashing past.
const MAX_CPS: f32 = 180.0;
/// Largest per-frame time step we honour. Guards against a huge jump after the
/// window was backgrounded (we don't want to dump the whole message at once).
const MAX_DT: f32 = 0.2;
/// Repaint cadence while the reveal is animating (~30fps).
const FRAME: Duration = Duration::from_millis(33);

/// How many trailing characters carry the warm lavender tint.
pub(super) const WARM_CHARS: usize = 16;
/// Once the reveal catches up, the warm edge cools out over this long.
const WARM_FADE: f32 = 0.55;

/// Identity of the message being revealed. `created_at` alone can collide
/// (same-second messages), so we pair it with the backend message id — both are
/// stable across a message's growth (`append_or_extend_message` only appends
/// text).
pub(super) type RevealKey = (Option<String>, u64);

/// Per-agent reveal cursor, persisted across frames on [`CenterArea`].
#[derive(Clone)]
pub(super) struct RevealState {
    key: RevealKey,
    /// Characters revealed so far. Never decreases while the key is unchanged.
    revealed: usize,
    /// Wall-clock of the previous advance, to derive `dt`.
    last_frame: Instant,
    /// When `revealed` last increased — drives the cool-out once caught up.
    last_advance: Instant,
}

impl RevealState {
    /// A cursor for a message seen for the first time. A running turn types it
    /// out from zero; anything else (history, hydration, a prior answer) is shown
    /// in full and already cooled, so it neither re-types nor glows.
    fn fresh(key: RevealKey, full_chars: usize, now: Instant, is_running: bool) -> Self {
        let revealed = if is_running { 0 } else { full_chars };
        let last_advance = if is_running {
            now
        } else {
            now.checked_sub(Duration::from_secs_f32(WARM_FADE))
                .unwrap_or(now)
        };
        Self {
            key,
            revealed,
            last_frame: now,
            last_advance,
        }
    }

    /// Advance toward `full_chars` and return the prefix length plus warmth.
    fn advance(&mut self, full_chars: usize, now: Instant, is_running: bool) -> (usize, f32) {
        let dt = now
            .saturating_duration_since(self.last_frame)
            .as_secs_f32()
            .min(MAX_DT);
        self.last_frame = now;

        // A message never shrinks, but clamp defensively so slicing stays valid.
        self.revealed = self.revealed.min(full_chars);

        let backlog = full_chars - self.revealed;
        if backlog > 0 && dt > 0.0 {
            let cps = (BASE_CPS + backlog as f32 * CATCHUP_CPS_PER_CHAR).min(MAX_CPS);
            let step = ((cps * dt).round() as usize).max(1);
            self.revealed = (self.revealed + step).min(full_chars);
            self.last_advance = now;
        }

        let warmth = if self.revealed < full_chars {
            1.0
        } else if is_running {
            let since = now
                .saturating_duration_since(self.last_advance)
                .as_secs_f32();
            (1.0 - since / WARM_FADE).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (self.revealed, warmth)
    }
}

/// What to paint for the streaming message this frame, resolved during render.
#[derive(Clone)]
pub(super) struct ActiveReveal {
    pub(super) agent: Uuid,
    pub(super) key: RevealKey,
    /// Characters to show (a prefix of the full message).
    pub(super) revealed: usize,
    /// Full length of the message in characters.
    pub(super) full: usize,
    /// 0..=1 warmth of the trailing edge (1 = fully lit, 0 = fully cooled).
    pub(super) warmth: f32,
}

impl ActiveReveal {
    /// Whether the whole message is on screen (nothing left to reveal).
    pub(super) fn is_complete(&self) -> bool {
        self.revealed >= self.full
    }

    /// Whether this frame needs any special (reveal / warm) rendering at all.
    /// When false the message renders through the normal markdown path, byte for
    /// byte identical to a non-streaming message.
    pub(super) fn is_animating(&self) -> bool {
        !self.is_complete() || self.warmth > 0.0
    }
}

/// Byte offset of the `n`th character — used to slice the revealed prefix
/// without splitting a UTF-8 boundary.
pub(super) fn char_byte_offset(text: &str, n: usize) -> usize {
    text.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

impl CenterArea {
    /// Resolve the reveal for `agent`'s last assistant message this frame and,
    /// while it is still typing or cooling, schedule the next repaint. Stores the
    /// result in `agent_chat_active_reveal` for the message renderer to read.
    pub(super) fn sync_agent_chat_reveal(
        &mut self,
        agent_id: Uuid,
        session: &AgentChatSession,
        is_running: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        // The reveal only ever concerns the last assistant message (the only one
        // that can be growing). Everything above it is settled history.
        let candidate = session
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                AgentChatMessage::Assistant {
                    message_id,
                    text,
                    created_at,
                } => Some((message_id.clone(), *created_at, text.chars().count())),
                _ => None,
            });

        let Some((message_id, created_at, full)) = candidate else {
            self.agent_chat_active_reveal = None;
            return;
        };

        let key: RevealKey = (message_id, created_at);
        let now = Instant::now();

        // Reuse the existing cursor only when it tracks this same message; a
        // different (or first-seen) message gets a fresh cursor. Crucially we do
        // NOT drop the cursor when a turn ends, so a finished answer that is still
        // the "last" message when the next turn starts keeps its full reveal
        // instead of re-typing from scratch.
        let mut state = self
            .agent_chat_reveal
            .remove(&agent_id)
            .filter(|state| state.key == key)
            .unwrap_or_else(|| RevealState::fresh(key.clone(), full, now, is_running));

        let (revealed, warmth) = state.advance(full, now, is_running);
        self.agent_chat_reveal.insert(agent_id, state);

        let active = ActiveReveal {
            agent: agent_id,
            key,
            revealed,
            full,
            warmth,
        };
        let animating = active.is_animating();
        self.agent_chat_active_reveal = Some(active);

        // Keep repainting while there is text left to reveal or a warm edge left
        // to cool. `insert` returns false when a tick is already pending, so we
        // never stack timers.
        if animating && self.agent_chat_reveal_pending.insert(agent_id) {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FRAME).await;
                this.update(cx, |this, cx| {
                    this.agent_chat_reveal_pending.remove(&agent_id);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_byte_offset_handles_multibyte_and_overrun() {
        assert_eq!(char_byte_offset("abc", 2), 2);
        assert_eq!(char_byte_offset("abc", 9), 3);
        // 'á' is two bytes, so the second char begins at byte 2.
        assert_eq!(char_byte_offset("áb", 1), 2);
    }

    #[test]
    fn running_message_types_from_zero_and_reaches_full() {
        let key = (Some("m1".to_string()), 1);
        let t0 = Instant::now();
        let mut state = RevealState::fresh(key.clone(), 100, t0, true);
        let (r0, w0) = state.advance(100, t0, true);
        assert_eq!(r0, 0, "first frame reveals nothing (dt == 0)");
        assert_eq!(w0, 1.0, "warm while there is backlog");

        let mut now = t0;
        let mut prev = 0;
        for _ in 0..500 {
            now += Duration::from_millis(33);
            let (r, _) = state.advance(100, now, true);
            assert!(r >= prev, "reveal never goes backwards");
            prev = r;
            if r >= 100 {
                break;
            }
        }
        assert_eq!(prev, 100, "reveal eventually catches up to the full text");
    }

    #[test]
    fn idle_first_sight_shows_full_immediately_without_glow() {
        // A message first seen while NOT running is history/hydration: it must
        // appear in full with no warm edge — never re-typed on load.
        let key = (Some("hist".to_string()), 1);
        let t0 = Instant::now();
        let mut state = RevealState::fresh(key, 240, t0, false);
        let (revealed, warmth) = state.advance(240, t0, false);
        assert_eq!(revealed, 240, "whole message shown at once");
        assert_eq!(warmth, 0.0, "no glow on a historical message");
    }

    #[test]
    fn kept_cursor_never_retypes_a_finished_message() {
        // Reproduces the turn-boundary bug: when a finished answer is still the
        // "last" message as the next turn starts, its cursor is *reused* (same
        // key), and reuse must keep it fully revealed rather than typing it again.
        let key = (Some("done".to_string()), 1);
        let t0 = Instant::now();
        let mut state = RevealState::fresh(key, 120, t0, true);
        let mut now = t0;
        for _ in 0..500 {
            now += Duration::from_millis(33);
            let (r, _) = state.advance(120, now, true);
            if r >= 120 {
                break;
            }
        }
        // A new turn begins a second later; the same message is still last, so the
        // cursor is reused (not re-created) and must stay complete.
        now += Duration::from_secs(1);
        let (revealed, _) = state.advance(120, now, true);
        assert_eq!(revealed, 120, "a finished message is never re-typed");
    }

    #[test]
    fn bigger_backlog_reveals_faster() {
        let key = (Some("m".to_string()), 1);
        let t0 = Instant::now();
        let dt = Duration::from_millis(50);
        let mut small = RevealState::fresh(key.clone(), 0, t0, true);
        let mut big = RevealState::fresh(key, 0, t0, true);
        let (rs, _) = small.advance(20, t0 + dt, true);
        let (rb, _) = big.advance(500, t0 + dt, true);
        assert!(rb > rs, "a larger backlog drains faster: {rb} vs {rs}");
    }

    #[test]
    fn warmth_cools_out_once_caught_up_while_running() {
        let key = (Some("m".to_string()), 1);
        let t0 = Instant::now();
        let mut state = RevealState::fresh(key, 5, t0, true);
        // Reveal all 5 chars.
        let _ = state.advance(5, t0 + Duration::from_millis(200), true);
        let (_, warm_now) = state.advance(5, t0 + Duration::from_millis(201), true);
        assert!(warm_now > 0.0, "still warm right after catching up");
        let (_, warm_later) = state.advance(5, t0 + Duration::from_secs(2), true);
        assert_eq!(warm_later, 0.0, "fully cooled a second later");
    }
}
