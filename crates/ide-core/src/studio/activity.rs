//! Which screens a live agent turn is still filling in.
//!
//! The Studio contract has the agent publish every requested screen as an empty
//! document first, then write them one at a time in the requested order. That
//! leaves blank artboards on the canvas with nothing to say they are spoken
//! for, and editing an existing screen showed nothing at all.
//!
//! Most of this is derived from what is already on disk — an active turn scope
//! plus an empty screen document. The one added signal is a focus marker the
//! Studio MCP tools write as the agent reads or saves a screen and drop once
//! its review passes; it is scoped to a single turn, so a marker left behind
//! can never be mistaken for live work.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StudioScreenActivity {
    /// An empty screen the running turn is filling in now.
    Working,
    /// An authored screen the running turn is changing now. Its real design
    /// stays visible; only a blank screen may be covered by a placeholder.
    Editing,
    /// Published, waiting its turn in the same request.
    Queued,
}
impl StudioScreenActivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Editing => "editing",
            Self::Queued => "queued",
        }
    }
}

/// True when a screen exists but has no authored body yet.
pub fn is_blank_screen(document: Option<&StudioDocument>) -> bool {
    let Some(document) = document else {
        return true;
    };
    body_of(&document.html).trim().is_empty()
}

/// Locate the body without copying the document. This runs for every scoped
/// screen on a poll, so lowering whole screens here would be pure allocator
/// churn on a design with large documents.
fn body_of(html: &str) -> &str {
    let Some(open) = find_ignore_case(html, "<body") else {
        return html;
    };
    let Some(start) = html[open..].find('>').map(|offset| open + offset + 1) else {
        return "";
    };
    let end = find_ignore_case(&html[start..], "</body>").map_or(html.len(), |o| start + o);
    &html[start..end]
}

/// Both needles are ASCII, so a match always lands on a char boundary.
fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let (haystack, needle) = (haystack.as_bytes(), needle.as_bytes());
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

/// What a live turn is doing to each screen of `design`. Empty whenever no
/// agent holds an active scope on the design.
///
/// The agent's own focus marker is authoritative when it has one, so editing an
/// existing screen is reported too. Without one, the contract's publish-then-
/// fill order makes the first still-empty screen the one being built.
pub fn designing_screens(
    store: &StudioStore,
    design: &StudioDesign,
) -> BTreeMap<Uuid, StudioScreenActivity> {
    let live = store.active_scopes(design.manifest.id);
    if live.is_empty() {
        return BTreeMap::new();
    }
    let scoped: BTreeSet<Uuid> = live
        .iter()
        .flat_map(|(_, scope)| scope.screen_ids.iter().copied())
        .collect();
    let present = |id: Uuid| {
        design
            .manifest
            .screens
            .iter()
            .any(|screen| screen.id == id && !screen.archived)
    };
    // Only a marker from the turn that is still running may be trusted.
    let focus = live
        .iter()
        .filter_map(|(agent, scope)| {
            store
                .focus(*agent)
                .filter(|focus| focus.scope_id == scope.id && focus.design_id == scope.design_id)
        })
        .filter(|focus| present(focus.screen_id))
        .max_by_key(|focus| focus.at)
        .map(|focus| focus.screen_id);

    let blank: Vec<Uuid> = design
        .manifest
        .screens
        .iter()
        .filter(|screen| {
            !screen.archived
                && scoped.contains(&screen.id)
                && is_blank_screen(design.documents.get(&screen.id))
        })
        .map(|screen| screen.id)
        .collect();

    let mut activity: BTreeMap<_, _> = blank
        .iter()
        .map(|id| (*id, StudioScreenActivity::Queued))
        .collect();
    match focus {
        Some(id) if blank.contains(&id) => {
            activity.insert(id, StudioScreenActivity::Working);
        }
        Some(id) if scoped.contains(&id) => {
            activity.insert(id, StudioScreenActivity::Editing);
        }
        _ => {
            if let Some(first) = blank.first() {
                activity.insert(*first, StudioScreenActivity::Working);
            }
        }
    }
    activity
}

/// Cheap change key so the canvas only re-publishes metadata when activity moves.
pub fn activity_key(activity: &BTreeMap<Uuid, StudioScreenActivity>) -> String {
    activity
        .iter()
        .map(|(id, state)| format!("{id}:{}", state.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}
