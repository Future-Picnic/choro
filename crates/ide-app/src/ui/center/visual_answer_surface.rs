//! The Visual answer web surface: its intent, isolated document and the narrow
//! IPC contract with the bundled OpenUI renderer. It shares the single native
//! web host with Preview but never uses Preview's intent, URL or controls.
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::App;
use serde::Deserialize;
use uuid::Uuid;

/// Renderer IPC larger than this is dropped; saved client state is ≤ 64 KiB.
pub const MAX_VISUAL_ANSWER_MESSAGE_BYTES: usize = 256 * 1024;
pub const VISUAL_ANSWER_URL: &str = "choro-visual://localhost/index.html";

const RENDERER_JS: &str = include_str!("../../../web/visual-answer/dist/visual-answer.js");
const RENDERER_CSS: &str = include_str!("../../../web/visual-answer/dist/visual-answer.css");
/// No network, no navigation, no forms. Design examples are `srcdoc` frames
/// with an empty sandbox (no scripts, opaque origin) inside this document.
const DOCUMENT_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; frame-src about:; child-src about:; base-uri 'none'; form-action 'none'";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualAnswerMode {
    Docked,
    Expanded,
}

impl VisualAnswerMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::Expanded => "expanded",
        }
    }
}

/// One rendered revision. `surface` is a host generation: every reopen of an
/// answer gets a new one, so messages from an earlier surface are discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualAnswerSurface {
    pub agent_id: Uuid,
    pub answer_id: Uuid,
    pub revision: u64,
    pub surface: u64,
    /// Bootstrap JSON (document, saved state, read-only flag). Fixed per surface.
    pub bootstrap: Arc<str>,
    /// Live-synced without rebuilding the surface.
    pub mode: VisualAnswerMode,
    pub theme: Arc<str>,
    /// Placed in its transcript card (scrolls with the conversation) rather
    /// than the composer panel. Host-only; the renderer never sees it.
    pub inline: bool,
}

impl VisualAnswerSurface {
    pub fn same_surface(&self, other: &Self) -> bool {
        (self.agent_id, self.answer_id, self.revision, self.surface)
            == (
                other.agent_id,
                other.answer_id,
                other.revision,
                other.surface,
            )
    }

    /// Script that applies mode/theme changes to an already-loaded surface.
    pub fn sync_script(&self, next: &Self) -> Option<String> {
        let mut script = String::new();
        if self.mode != next.mode {
            script.push_str(&format!(
                "window.choroVisual?.setMode({:?});",
                next.mode.as_str()
            ));
        }
        if self.theme != next.theme {
            script.push_str(&format!("window.choroVisual?.setTheme({});", next.theme));
        }
        (!script.is_empty()).then_some(script)
    }
}

/// Theme tokens for the renderer, taken from Choro's own design tokens.
pub fn theme_json(cx: &App) -> String {
    use crate::ui::design as d;
    use gpui_component::ActiveTheme;
    serde_json::json!({
        "dark": cx.theme().mode.is_dark(),
        "base": d::base(cx).to_string(),
        "surface": d::surface(cx).to_string(),
        "surface2": d::surface_2(cx).to_string(),
        "line": d::line(cx).to_string(),
        "line2": d::line_2(cx).to_string(),
        "t1": d::t1(cx).to_string(),
        "t2": d::t2(cx).to_string(),
        "t3": d::t3(cx).to_string(),
        "accent": d::accent(cx).to_string(),
        "accentSoft": d::accent_soft(cx).to_string(),
        "onAccent": d::on_accent(cx).to_string(),
        "focus": d::focus(cx).to_string(),
        "hover": d::hover(cx).to_string(),
        "sage": d::sage(cx).to_string(),
        "amber": d::amber(cx).to_string(),
        "rose": d::rose(cx).to_string(),
        "sky": d::sky(cx).to_string(),
        "teal": d::teal(cx).to_string(),
    })
    .to_string()
}

/// Saved client state as reported by the renderer. The host validates it
/// against the document before persisting; it can never carry a receipt.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualRendererState {
    pub selected_option_id: Option<String>,
    pub fields: serde_json::Value,
    pub scroll_top: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VisualAnswerMessage {
    Ready,
    State(VisualRendererState),
    /// The renderer's complete current state in reply to a host request,
    /// sent immediately (no debounce). The token acknowledges that request.
    Snapshot {
        token: u64,
        state: VisualRendererState,
    },
    Invalid(Vec<String>),
    /// Escape pressed in the content. The host applies the composer's rules
    /// (stop running work, dock, leave the answer); the renderer decides nothing.
    Escape,
    /// Laid-out content height in CSS pixels, already bounded.
    Size(f32),
    FocusExit {
        forward: bool,
    },
}

/// Content taller than this is reported as this; the panel caps it anyway.
pub const MAX_REPORTED_CONTENT_HEIGHT: f64 = 20_000.0;

/// How long an outgoing surface (or a submission) waits for its snapshot.
pub const SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(300);

/// A host request for the renderer's final snapshot. The outgoing surface is
/// kept alive (hidden) until the matching acknowledgment or the deadline, so
/// the last search keystroke or scroll is never dropped with the webview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotHandoff {
    pub surface: u64,
    pub token: u64,
    pub deadline: Instant,
}

impl SnapshotHandoff {
    pub fn new(surface: u64, token: u64, now: Instant) -> Self {
        Self {
            surface,
            token,
            deadline: now + SNAPSHOT_TIMEOUT,
        }
    }

    /// Only the reply to this exact request (same surface and token) counts.
    pub fn acknowledged_by(&self, envelope: &VisualAnswerEnvelope) -> bool {
        envelope.surface == self.surface
            && matches!(envelope.message, VisualAnswerMessage::Snapshot { token, .. } if token == self.token)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

pub fn snapshot_script(token: u64) -> String {
    format!("window.choroVisual?.snapshot({token});")
}

/// A message already matched to the surface that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualAnswerEnvelope {
    pub agent_id: Uuid,
    pub answer_id: Uuid,
    pub revision: u64,
    pub surface: u64,
    pub message: VisualAnswerMessage,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WireMessage {
    Ready,
    State {
        state: VisualRendererState,
    },
    Snapshot {
        token: u64,
        state: VisualRendererState,
    },
    Invalid {
        errors: Vec<String>,
    },
    Escape,
    Size {
        height: f64,
    },
    FocusExit {
        direction: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireIdentity {
    surface: u64,
    answer_id: Uuid,
    revision: u64,
}

/// Accepts a renderer message only when it echoes the identity of the surface
/// whose IPC handler received it. Anything else (oversized, malformed, from an
/// earlier surface or a different answer/revision) is dropped.
pub fn decode_message(body: &str, built: &VisualAnswerSurface) -> Option<VisualAnswerEnvelope> {
    if body.len() > MAX_VISUAL_ANSWER_MESSAGE_BYTES {
        return None;
    }
    let identity: WireIdentity = serde_json::from_str(body).ok()?;
    if (identity.surface, identity.answer_id, identity.revision)
        != (built.surface, built.answer_id, built.revision)
    {
        return None;
    }
    let mut value: serde_json::Value = serde_json::from_str(body).ok()?;
    if let Some(object) = value.as_object_mut() {
        for key in ["surface", "answerId", "revision"] {
            object.remove(key);
        }
    }
    let message = match serde_json::from_value::<WireMessage>(value).ok()? {
        WireMessage::Ready => VisualAnswerMessage::Ready,
        WireMessage::State { state } => VisualAnswerMessage::State(state),
        WireMessage::Snapshot { token, state } => VisualAnswerMessage::Snapshot { token, state },
        WireMessage::Invalid { errors } => VisualAnswerMessage::Invalid(
            errors
                .into_iter()
                .take(8)
                .map(|e| e.chars().take(300).collect())
                .collect(),
        ),
        WireMessage::Escape => VisualAnswerMessage::Escape,
        WireMessage::Size { height } if height.is_finite() && height > 0.0 => {
            VisualAnswerMessage::Size(height.min(MAX_REPORTED_CONTENT_HEIGHT) as f32)
        }
        WireMessage::Size { .. } => return None,
        WireMessage::FocusExit { direction } => VisualAnswerMessage::FocusExit {
            forward: direction != "backward",
        },
    };
    Some(VisualAnswerEnvelope {
        agent_id: built.agent_id,
        answer_id: built.answer_id,
        revision: built.revision,
        surface: built.surface,
        message,
    })
}

/// JSON that is safe inside an inline `<script>`: `<` only occurs inside JSON
/// strings, where `\u003c` is equivalent and cannot close the element.
fn script_safe_json(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn bootstrap_json(
    surface: u64,
    answer: &ide_core::visual_answers::VisualAnswer,
    theme: &str,
    mode: VisualAnswerMode,
) -> String {
    let interaction = &answer.interaction;
    let theme: serde_json::Value = serde_json::from_str(theme).unwrap_or_default();
    serde_json::json!({
        "surface": surface,
        "answerId": answer.id,
        "revision": answer.revision,
        "document": answer.document,
        "state": {
            "selectedOptionId": interaction.selected_option_id,
            "fields": interaction.fields,
            "scrollTop": interaction.scroll_top,
        },
        "theme": theme,
        "mode": mode.as_str(),
        "readOnly": interaction.submission.is_some(),
    })
    .to_string()
}

pub fn document(bootstrap: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{DOCUMENT_POLICY}\"><meta name=\"viewport\" content=\"width=device-width\"><style>{}</style></head><body><div id=\"root\"></div><script>window.__CHORO_VISUAL__={};</script><script>{}</script></body></html>",
        RENDERER_CSS,
        script_safe_json(bootstrap),
        RENDERER_JS.replace("</script", "<\\/script"),
    )
}

/// Only the answer document and its inert design frames may load.
pub fn allows_navigation(url: &str) -> bool {
    url == VISUAL_ANSWER_URL || url == "about:srcdoc" || url == "about:blank"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> VisualAnswerSurface {
        VisualAnswerSurface {
            agent_id: Uuid::new_v4(),
            answer_id: Uuid::new_v4(),
            revision: 3,
            surface: 11,
            bootstrap: Arc::from("{}"),
            mode: VisualAnswerMode::Docked,
            theme: Arc::from("{}"),
            inline: false,
        }
    }

    fn wire(built: &VisualAnswerSurface, body: serde_json::Value) -> String {
        let mut body = body;
        body["surface"] = built.surface.into();
        body["answerId"] = built.answer_id.to_string().into();
        body["revision"] = built.revision.into();
        body.to_string()
    }

    #[test]
    fn messages_must_match_the_surface_answer_and_revision() {
        let built = surface();
        let state = serde_json::json!({"type":"state","state":{"selectedOptionId":"a","fields":{"view":{}},"scrollTop":4}});
        let accepted = decode_message(&wire(&built, state.clone()), &built).unwrap();
        assert_eq!(accepted.surface, 11);
        assert!(
            matches!(accepted.message, VisualAnswerMessage::State(ref s) if s.selected_option_id.as_deref() == Some("a"))
        );

        for (field, value) in [
            ("surface", serde_json::json!(10)),
            ("revision", serde_json::json!(2)),
            ("answerId", serde_json::json!(Uuid::new_v4())),
        ] {
            let mut stale: serde_json::Value =
                serde_json::from_str(&wire(&built, state.clone())).unwrap();
            stale[field] = value;
            assert!(
                decode_message(&stale.to_string(), &built).is_none(),
                "{field}"
            );
        }
    }

    #[test]
    fn renderer_cannot_forge_receipts_or_unknown_messages() {
        let built = surface();
        let forged = serde_json::json!({"type":"state","state":{"selectedOptionId":null,"fields":{},"scrollTop":0,"submission":{"status":"sent"}}});
        assert!(decode_message(&wire(&built, forged), &built).is_none());
        let submit = serde_json::json!({"type":"submit","actionId":"choose"});
        assert!(decode_message(&wire(&built, submit), &built).is_none());
        let huge = format!(
            "{{\"type\":\"invalid\",\"errors\":[\"{}\"]}}",
            "x".repeat(MAX_VISUAL_ANSWER_MESSAGE_BYTES)
        );
        assert!(decode_message(&huge, &built).is_none());
        let exit = decode_message(
            &wire(
                &built,
                serde_json::json!({"type":"focusExit","direction":"backward"}),
            ),
            &built,
        )
        .unwrap();
        assert_eq!(
            exit.message,
            VisualAnswerMessage::FocusExit { forward: false }
        );
    }

    #[test]
    fn escape_and_content_size_are_bounded_requests_not_decisions() {
        let built = surface();
        let decode = |body: serde_json::Value| decode_message(&wire(&built, body), &built);
        assert_eq!(
            decode(serde_json::json!({"type":"escape"})).unwrap().message,
            VisualAnswerMessage::Escape
        );
        // The old self-docking message no longer exists.
        assert!(decode(serde_json::json!({"type":"dock"})).is_none());
        assert_eq!(
            decode(serde_json::json!({"type":"size","height":212})).unwrap().message,
            VisualAnswerMessage::Size(212.0)
        );
        assert_eq!(
            decode(serde_json::json!({"type":"size","height":1e9})).unwrap().message,
            VisualAnswerMessage::Size(MAX_REPORTED_CONTENT_HEIGHT as f32)
        );
        for bad in [serde_json::json!(0), serde_json::json!(-4), serde_json::json!("tall")] {
            assert!(decode(serde_json::json!({"type":"size","height":bad})).is_none());
        }
    }

    #[test]
    fn document_is_isolated_and_bootstrap_cannot_close_its_script() {
        let html = document(r#"{"title":"</script><script>alert(1)</script>"}"#);
        assert!(html.contains("default-src 'none'"));
        assert!(html.contains("connect-src 'none'"));
        assert!(!html.contains("</script><script>alert(1)"));
        assert!(html.contains("\\u003c/script>"));
        assert!(!html.contains("http://") || !html.contains("src=\"http"));
        assert!(allows_navigation(VISUAL_ANSWER_URL));
        assert!(!allows_navigation("https://example.com"));
        assert!(!allows_navigation("file:///etc/passwd"));
    }

    #[test]
    fn only_the_requested_snapshot_acknowledges_a_handoff() {
        let built = surface();
        let now = Instant::now();
        let handoff = SnapshotHandoff::new(built.surface, 42, now);
        let snapshot = |token: u64| {
            decode_message(
                &wire(
                    &built,
                    serde_json::json!({"type":"snapshot","token":token,"state":{"selectedOptionId":null,"fields":{"summary":"x"},"scrollTop":120}}),
                ),
                &built,
            )
            .unwrap()
        };
        let reply = snapshot(42);
        assert!(handoff.acknowledged_by(&reply));
        assert!(
            matches!(reply.message, VisualAnswerMessage::Snapshot { ref state, .. } if state.scroll_top == 120.0)
        );
        assert!(
            !handoff.acknowledged_by(&snapshot(41)),
            "an earlier request is not this one"
        );
        let debounced = decode_message(
            &wire(&built, serde_json::json!({"type":"state","state":{"selectedOptionId":null,"fields":{},"scrollTop":0}})),
            &built,
        )
        .unwrap();
        assert!(
            !handoff.acknowledged_by(&debounced),
            "a debounced report is not an acknowledgment"
        );
        let other = SnapshotHandoff::new(built.surface + 1, 42, now);
        assert!(
            !other.acknowledged_by(&reply),
            "another surface's reply never counts"
        );
        assert!(!handoff.expired(now));
        assert!(handoff.expired(now + SNAPSHOT_TIMEOUT));
        assert_eq!(snapshot_script(42), "window.choroVisual?.snapshot(42);");
    }

    #[test]
    fn mode_and_theme_changes_sync_without_a_new_surface() {
        let docked = surface();
        let expanded = VisualAnswerSurface {
            mode: VisualAnswerMode::Expanded,
            ..docked.clone()
        };
        assert!(docked.same_surface(&expanded));
        assert_eq!(
            docked.sync_script(&expanded).unwrap(),
            "window.choroVisual?.setMode(\"expanded\");"
        );
        let reopened = VisualAnswerSurface {
            surface: 12,
            ..docked.clone()
        };
        assert!(!docked.same_surface(&reopened));
        // Expanding an inline result moves the same surface; nothing reloads.
        let inline = VisualAnswerSurface { inline: true, ..docked.clone() };
        assert!(inline.same_surface(&expanded));
        assert_eq!(inline.sync_script(&docked), None);
    }
}
