//! Palettes — the single home for colors that aren't one plain theme token.
//!
//! Two kinds live here: **semantic tints** (diff backgrounds, the loading
//! spinner) derived from the theme so they track every theme; and **fixed
//! identity palettes** (brand colors, project-avatar hues) that are intentionally
//! constant. Either way, this is the *one* place — nothing hardcoded at a call site.

use gpui::{App, Hsla};

use super::token;

fn rgb(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

// ---- semantic tints (theme-tracking) ------------------------------------

/// Diff line background for an added line — a whisper of the add color.
pub fn diff_add_bg(cx: &App) -> Hsla {
    token::sage(cx).opacity(0.16)
}
/// Diff line background for a removed line.
pub fn diff_remove_bg(cx: &App) -> Hsla {
    token::rose(cx).opacity(0.16)
}

/// The logo spinner's lavender sweep — the brand mark's own identity, fixed
/// across themes (it's the loading logo, not a themed surface). Centralized here
/// so the raw paint routine holds no literals: `TRACK` is the faint ring,
/// `SWEEP_*` the gradient, `LIGHT`/`SHADOW` the shading.
pub const SPINNER_TRACK: u32 = 0xA08CDC;
pub const SPINNER_SWEEP_BRIGHT: u32 = 0xCFC0FB;
pub const SPINNER_SWEEP_DEEP: u32 = 0x8F74E5;
pub const SPINNER_LIGHT: u32 = 0xEAE2FF;
pub const SPINNER_SHADOW: u32 = 0x563E9C;

/// The one blue in the identity: the node inside the mark's C, where every
/// strand converges. Like the spinner ramp it is brand, not theme — it holds its
/// colour on every palette, because it is the logo.
pub const MARK_NODE: u32 = 0x6EC6F5;

/// Contrast ink for text sitting on an arbitrary saturated fill (brand tiles,
/// avatars): near-black on a light fill, near-white on a dark one. Not a themed
/// surface — it's a legibility choice — but defined once, here.
pub fn ink_on_light() -> Hsla {
    rgb(0x11151C)
}
pub fn ink_on_dark() -> Hsla {
    rgb(0xFFFFFF)
}

// ---- fixed identity palettes --------------------------------------------

/// Distinct, stable hues for project icons. The Choro palette keeps enough hue
/// separation for recognition while holding saturation and luminance inside a
/// calm, low-glare band that complements Dark, Twilight, Dusk, and Light.
/// Indexed by a stable project hash; the one place these values live.
pub const PROJECT_AVATARS: &[u32] = &[
    // Original eight — keep their order so existing saved color IDs do not shift.
    0x6F8FB8, 0x7FA58B, 0xC3A16D, 0xB97078, 0xB77F9D, 0x917EB5, 0x6F9FA5, 0x8D8A94,
    // Expanded Choro project-icon palette.
    0xBB815F, 0x9AAA6A, 0x5E9B91, 0x729BB5, 0x7D83B7, 0x9A82C0, 0xB27BAE, 0xC17B7E, 0xB99A5D,
    0x75AA96, 0x9B806D,
];
pub fn project_avatar(index: usize) -> Hsla {
    rgb(PROJECT_AVATARS[index % PROJECT_AVATARS.len()])
}

/// Personal-task avatar hues (a calmer, five-colour set).
pub const TASK_AVATARS: &[u32] = &[0x5A6CC4, 0x7D5AA0, 0x5A9C8F, 0xC47D5A, 0x8A5FB0];
pub fn task_avatar(index: usize) -> Hsla {
    rgb(TASK_AVATARS[index % TASK_AVATARS.len()])
}

/// The Claude agent brand colour (fixed identity).
pub fn claude_brand() -> Hsla {
    rgb(0xD97757)
}

/// PocketComet's orange center in the workspace rail.
pub fn pocketcomet_brand() -> Hsla {
    rgb(0xF59A55)
}

/// Doc-label accent hues — feature/research/design plus a hashed fallback set,
/// so labels stay stable-coloured. Categorical identity, centralized here.
pub fn doc_label(label: &str) -> Hsla {
    match label.to_ascii_lowercase().as_str() {
        "feature" => rgb(0x5E8CFF),
        "research" => rgb(0xA06CFF),
        "design" => rgb(0xE27A3F),
        other => {
            const FALLBACK: &[u32] = &[0x48B883, 0xD75F6A, 0x3BA7B8, 0xD8A24A, 0x7A80D8];
            let hash = other
                .bytes()
                .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
            rgb(FALLBACK[hash % FALLBACK.len()])
        }
    }
}

/// Issue-tracker brand tints (fixed brand identity) for source pills, keyed by
/// a lowercase provider name so this module stays free of domain enums.
pub fn tracker_brand(provider: &str) -> Hsla {
    match provider {
        "jira" => rgb(0x0B1F45),
        "linear" => rgb(0x20222C),
        "asana" => rgb(0x2A1C22),
        "clickup" => rgb(0x141522),
        _ => rgb(0x3A3D52),
    }
}
