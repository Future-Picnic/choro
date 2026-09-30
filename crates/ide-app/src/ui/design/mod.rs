//! # Choro design system — the single source of truth.
//!
//! Every color, size, and canonical component in the app comes from here. No UI
//! file reads a raw `cx.theme().<field>` or a raw `px(..)` color/spacing — it
//! calls a token accessor ([`token`]) or a scale constant ([`scale`]), and
//! composes the canonical builders. Change the language in one place; the whole
//! app follows, across the full Choro theme family.
//!
//! ## The three laws
//! 1. **Color has three channels.** *Location* is neutral (fills / bright
//!    underlines, never accent). *Accent* is only live / AI / the one primary
//!    action per panel (plus a selection-about-to-act wash). *Semantic* color
//!    lives primarily on glyphs and numbers. Compact state controls may pair a
//!    semantic border/glyph with its soft semantic fill; destructive confirms
//!    use the solid danger pair.
//! 2. **Elevation is "dim".** The [`token::focus`] plane sits midway between
//!    `surface` and `surface_2`; elevation is carried by border + shadow, never
//!    a bright lift.
//! 3. **One grammar.** Every dropdown button shows its internal divider; every
//!    middle header follows the agent-header pattern; every menu uses the same
//!    row/label/✓/separator shape.
//!
//! Reference mockup: `~/Downloads/choro-design-system.html`.
//!
//! This module is intentionally allowed to carry unused tokens/builders: it is a
//! library the rest of the app grows into, not all of it is wired at once.
#![allow(dead_code)]

pub mod scale;
pub mod token;

// Builders are namespaced (`design::header::title`, `design::indicator::letter`)
// so their verbs never collide with tokens.
pub mod header;
pub mod indicator;
pub mod palette;

// Tokens + scale are flat so callers write `design::t1(cx)`, `design::r_sm()`,
// `design::TEXT_UI` — these names never collide with each other.
pub use scale::*;
pub use token::*;

/// Canonical Docs glyph used by navigation, document rows, and doc references.
pub fn docs_icon() -> gpui_component::IconName {
    gpui_component::IconName::FileText
}

/// Canonical Tasks glyph used by navigation and linked-task references.
pub fn tasks_icon() -> gpui_component::IconName {
    gpui_component::IconName::CircleCheck
}

/// Canonical Design glyph used by navigation, design rows, and linked-design
/// references.
pub fn design_icon() -> gpui_component::IconName {
    gpui_component::IconName::Wallpaper
}
