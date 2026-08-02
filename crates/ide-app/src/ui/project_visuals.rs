use std::sync::OnceLock;

use gpui::{div, relative, AnyElement, App, Hsla, IntoElement, ParentElement, Pixels, Styled};
use lucide_icons::Icon as LucideIcon;

pub const LUCIDE_ICON_FONT_FAMILY: &str = "lucide";

#[derive(Clone)]
pub struct ProjectIconOption {
    pub id: String,
    pub label: String,
    pub icon: LucideIcon,
}

#[derive(Clone, Copy)]
pub struct ProjectColorOption {
    pub id: &'static str,
    pub label: &'static str,
    pub color: Option<Hsla>,
}

static PROJECT_ICON_OPTIONS: OnceLock<Vec<ProjectIconOption>> = OnceLock::new();

/// Returns every glyph exposed by the pinned Lucide icon font. Lucide assigns
/// its glyphs to the Unicode private-use area, so walking that small range lets
/// us build the searchable catalog without maintaining a second 1,700-item
/// name list beside the crate's generated enum.
pub fn project_icon_options() -> &'static [ProjectIconOption] {
    PROJECT_ICON_OPTIONS.get_or_init(|| {
        let mut icons = (0xE000..=0xE7FF)
            .filter_map(char::from_u32)
            .filter_map(|unicode| LucideIcon::try_from(unicode).ok())
            .map(|icon| {
                let id = icon.to_string();
                ProjectIconOption {
                    label: id.replace('-', " "),
                    id,
                    icon,
                }
            })
            .collect::<Vec<_>>();
        icons.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        icons
    })
}

/// A calm first page for the picker. Search still covers the full catalog.
pub const POPULAR_PROJECT_ICONS: &[&str] = &[
    "folder",
    "star",
    "briefcase-business",
    "code-2",
    "database",
    "file",
    "inbox",
    "globe",
    "git-branch",
    "search",
    "copy",
    "square-terminal",
    "bot",
    "settings",
    "wrench",
    "layout-dashboard",
    "panel-bottom-open",
    "panel-left-open",
    "book-open",
    "building-2",
    "chart-pie",
    "map",
    "bell",
    "info",
    "alert-triangle",
    "check-circle",
    "frame",
    "palette",
    "heart",
    "eye",
    "sun",
    "moon",
    "asterisk",
    "user",
    "circle-user",
    "calendar",
    "case-sensitive",
    "a-large-small",
    "replace",
    "redo-2",
    "undo-2",
    "thumbs-up",
    "thumbs-down",
    "rocket",
    "lightbulb",
    "package",
    "shopping-cart",
];

pub fn project_color_options() -> Vec<ProjectColorOption> {
    vec![
        ProjectColorOption {
            id: "default",
            label: "Default",
            color: None,
        },
        ProjectColorOption {
            id: "blue",
            label: "Slate Blue",
            color: Some(crate::ui::design::palette::project_avatar(0)),
        },
        ProjectColorOption {
            id: "green",
            label: "Sage",
            color: Some(crate::ui::design::palette::project_avatar(1)),
        },
        ProjectColorOption {
            id: "amber",
            label: "Honey",
            color: Some(crate::ui::design::palette::project_avatar(2)),
        },
        ProjectColorOption {
            id: "red",
            label: "Dusty Rose",
            color: Some(crate::ui::design::palette::project_avatar(3)),
        },
        ProjectColorOption {
            id: "pink",
            label: "Mauve",
            color: Some(crate::ui::design::palette::project_avatar(4)),
        },
        ProjectColorOption {
            id: "purple",
            label: "Amethyst",
            color: Some(crate::ui::design::palette::project_avatar(5)),
        },
        ProjectColorOption {
            id: "cyan",
            label: "Mist Cyan",
            color: Some(crate::ui::design::palette::project_avatar(6)),
        },
        ProjectColorOption {
            id: "gray",
            label: "Warm Gray",
            color: Some(crate::ui::design::palette::project_avatar(7)),
        },
        ProjectColorOption {
            id: "orange",
            label: "Terracotta",
            color: Some(crate::ui::design::palette::project_avatar(8)),
        },
        ProjectColorOption {
            id: "lime",
            label: "Olive",
            color: Some(crate::ui::design::palette::project_avatar(9)),
        },
        ProjectColorOption {
            id: "teal",
            label: "Patina",
            color: Some(crate::ui::design::palette::project_avatar(10)),
        },
        ProjectColorOption {
            id: "sky",
            label: "Cloud Blue",
            color: Some(crate::ui::design::palette::project_avatar(11)),
        },
        ProjectColorOption {
            id: "indigo",
            label: "Dusky Indigo",
            color: Some(crate::ui::design::palette::project_avatar(12)),
        },
        ProjectColorOption {
            id: "violet",
            label: "Velvet Violet",
            color: Some(crate::ui::design::palette::project_avatar(13)),
        },
        ProjectColorOption {
            id: "magenta",
            label: "Muted Orchid",
            color: Some(crate::ui::design::palette::project_avatar(14)),
        },
        ProjectColorOption {
            id: "coral",
            label: "Soft Coral",
            color: Some(crate::ui::design::palette::project_avatar(15)),
        },
        ProjectColorOption {
            id: "gold",
            label: "Antique Gold",
            color: Some(crate::ui::design::palette::project_avatar(16)),
        },
        ProjectColorOption {
            id: "mint",
            label: "Eucalyptus",
            color: Some(crate::ui::design::palette::project_avatar(17)),
        },
        ProjectColorOption {
            id: "brown",
            label: "Cocoa",
            color: Some(crate::ui::design::palette::project_avatar(18)),
        },
    ]
}

pub fn project_icon(icon_id: &str) -> LucideIcon {
    let canonical = match icon_id {
        // IDs from the original small picker, retained for compatibility.
        "terminal" => "square-terminal",
        "tools" => "settings-2",
        "dashboard" => "layout-dashboard",
        "panel" => "panel-bottom-open",
        "sidebar" => "panel-left-open",
        "book" => "book-open",
        "building" => "building-2",
        "inspector" => "scan-search",
        "chart" => "chart-pie",
        "alert" => "alert-triangle",
        "check" => "check-circle",
        "github" => "git-branch",
        "account" => "circle-user",
        "case" => "case-sensitive",
        "type" => "a-large-small",
        "redo" => "redo-2",
        "undo" => "undo-2",
        "thumbs_up" => "thumbs-up",
        "thumbs_down" => "thumbs-down",
        other => other,
    };
    LucideIcon::try_from(canonical).unwrap_or(LucideIcon::Folder)
}

pub fn project_icon_color(color_id: &str, cx: &App) -> Hsla {
    project_color_options()
        .into_iter()
        .find(|option| option.id == color_id)
        .and_then(|option| option.color)
        .unwrap_or_else(|| crate::ui::design::t3(cx))
}

pub fn project_icon_glyph(icon: LucideIcon, color: Hsla, glyph_size: Pixels) -> AnyElement {
    div()
        .flex_none()
        .font_family(LUCIDE_ICON_FONT_FAMILY)
        .text_size(glyph_size)
        .line_height(relative(1.))
        .text_color(color)
        .child(icon.unicode().to_string())
        .into_any_element()
}

pub fn project_icon_element(
    icon_id: &str,
    color_id: &str,
    size: Pixels,
    glyph_size: Pixels,
    cx: &App,
) -> AnyElement {
    div()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(project_icon_glyph(
            project_icon(icon_id),
            project_icon_color(color_id, cx),
            glyph_size,
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucide_catalog_exceeds_product_minimum() {
        assert!(project_icon_options().len() >= 1_000);
        assert!(project_icon_options()
            .iter()
            .any(|icon| icon.id == "folder"));
        for popular in POPULAR_PROJECT_ICONS {
            assert!(
                project_icon_options()
                    .iter()
                    .any(|icon| icon.id == *popular),
                "missing popular icon: {popular}"
            );
        }
    }

    #[test]
    fn project_palette_has_twenty_options() {
        assert_eq!(project_color_options().len(), 20);
    }
}
