use gpui::{
    div, prelude::FluentBuilder as _, px, relative, rgba, App, Div, FontWeight, Hsla,
    IntoElement as _, ParentElement as _, Styled as _,
};
use gpui_component::h_flex;
use ide_core::DbProvider;

pub(crate) mod chrome;
pub mod collection_pane;
mod connection_editor;
mod data_grid;
pub mod database_pane;
pub mod db_panel;
pub(crate) mod home;
mod providers;
mod row_editor;
pub mod table_pane;
pub mod workspace;
#[cfg(all(test, feature = "ui-layout-tests"))]
mod workspace_layout_tests;

/// Devicon glyphs from the bundled database subset. Providers the subset does
/// not cover are drawn by `provider_brand_mark` instead of borrowing a glyph.
fn provider_brand_glyph(provider: DbProvider) -> Option<char> {
    match provider {
        DbProvider::MariaDb => Some('\u{ead9}'),
        DbProvider::MongoDb => Some('\u{eaf5}'),
        DbProvider::MySql => Some('\u{eafd}'),
        DbProvider::PostgreSql => Some('\u{eb79}'),
        DbProvider::SQLite => Some('\u{ec1e}'),
        DbProvider::Supabase => Some('\u{ec2e}'),
        DbProvider::Turso | DbProvider::ClickHouse => None,
    }
}

fn provider_brand_color(provider: DbProvider) -> Hsla {
    Hsla::from(rgba(match provider {
        DbProvider::MariaDb => 0xc49a6cff,
        DbProvider::MongoDb => 0x47a248ff,
        DbProvider::MySql => 0x4479a1ff,
        DbProvider::PostgreSql => 0x4169e1ff,
        DbProvider::SQLite => 0x0f80ccff,
        DbProvider::Supabase => 0x3ecf8eff,
        DbProvider::Turso => 0x4ff8d2ff,
        DbProvider::ClickHouse => 0xfaff69ff,
    }))
}

/// The dark plate behind drawn marks, matching both vendors' own logo tiles.
const DRAWN_MARK_PLATE: u32 = 0x1f2329ff;

/// A provider logo at `size`: a Devicon glyph through GPUI's crisp text
/// pipeline, or a small drawn mark for providers outside the Devicon subset.
pub(crate) fn provider_brand_mark(provider: DbProvider, size: f32) -> gpui::AnyElement {
    let frame = div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size));
    let color = provider_brand_color(provider);
    match provider_brand_glyph(provider) {
        Some(glyph) => frame
            .child(
                div()
                    .font_family(crate::theme::DEVICON_FONT_FAMILY)
                    .text_size(px(size))
                    .line_height(relative(1.))
                    .text_color(color)
                    .child(glyph.to_string()),
            )
            .into_any_element(),
        None if provider == DbProvider::ClickHouse => {
            frame.child(clickhouse_mark(size, color)).into_any_element()
        }
        None => frame
            .child(monogram_mark(size, color, "T"))
            .into_any_element(),
    }
}

/// ClickHouse's logo is four full-height columns and one short one.
fn clickhouse_mark(size: f32, color: Hsla) -> Div {
    let bar_w = (size * 0.1).max(1.5);
    let tall = size * 0.62;
    h_flex()
        .size(px(size))
        .rounded(px(size * 0.2))
        .bg(Hsla::from(rgba(DRAWN_MARK_PLATE)))
        .items_center()
        .justify_center()
        .gap(px(bar_w * 0.55))
        .children((0..4).map(|_| div().w(px(bar_w)).h(px(tall)).bg(color)))
        .child(div().w(px(bar_w)).h(px(tall * 0.24)).bg(color))
}

fn monogram_mark(size: f32, color: Hsla, letter: &'static str) -> Div {
    div()
        .size(px(size))
        .rounded(px(size * 0.2))
        .bg(Hsla::from(rgba(DRAWN_MARK_PLATE)))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.62))
        .line_height(relative(1.))
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter)
}

/// Safety posture of an open connection, following the indicator law: the
/// state color sits on the glyph and the label stays neutral text.
pub(crate) fn access_indicators(prod: bool, read_only: bool, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_3()
        .items_center()
        .when(prod, |row| {
            row.child(access_indicator(
                lucide_icons::Icon::ShieldAlert,
                "Production",
                crate::ui::design::rose(cx),
                cx,
            ))
        })
        .child(if read_only {
            access_indicator(
                lucide_icons::Icon::Lock,
                "Read only",
                crate::ui::design::t3(cx),
                cx,
            )
        } else {
            access_indicator(
                lucide_icons::Icon::LockOpen,
                "Edits allowed",
                crate::ui::design::amber(cx),
                cx,
            )
        })
}

fn access_indicator(icon: lucide_icons::Icon, label: &'static str, color: Hsla, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .text_size(crate::ui::design::text_ui())
        .text_color(crate::ui::design::t2(cx))
        .child(crate::ui::design::indicator::lucide_icon(
            icon,
            color,
            crate::ui::design::icon_sm(),
        ))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devicon_glyphs_are_unique_private_use_codepoints() {
        let glyphs = DbProvider::ALL
            .into_iter()
            .filter_map(provider_brand_glyph)
            .collect::<Vec<_>>();
        for glyph in &glyphs {
            assert!((0xe000..=0xf8ff).contains(&(*glyph as u32)));
        }
        let unique = glyphs.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), glyphs.len());
        assert_eq!(provider_brand_glyph(DbProvider::Turso), None);
        assert_eq!(provider_brand_glyph(DbProvider::ClickHouse), None);
    }

    #[test]
    fn database_ui_uses_shared_button_builders() {
        let sources = [
            (
                "center/db_workspace.rs",
                include_str!("../center/db_workspace.rs"),
            ),
            ("chrome.rs", include_str!("chrome.rs")),
            ("home.rs", include_str!("home.rs")),
            ("workspace.rs", include_str!("workspace.rs")),
            (
                "table_pane/inspector.rs",
                include_str!("table_pane/inspector.rs"),
            ),
            (
                "collection_pane/browser.rs",
                include_str!("collection_pane/browser.rs"),
            ),
            ("collection_pane.rs", include_str!("collection_pane.rs")),
            ("connection_editor.rs", include_str!("connection_editor.rs")),
            (
                "connection_editor/form.rs",
                include_str!("connection_editor/form.rs"),
            ),
            ("data_grid.rs", include_str!("data_grid.rs")),
            ("database_pane.rs", include_str!("database_pane.rs")),
            ("db_panel.rs", include_str!("db_panel.rs")),
            (
                "db_panel/tree_rows.rs",
                include_str!("db_panel/tree_rows.rs"),
            ),
            ("row_editor.rs", include_str!("row_editor.rs")),
            ("table_pane.rs", include_str!("table_pane.rs")),
        ];
        let forbidden = [
            "Button::new(",
            ".primary()",
            ".ghost()",
            ".outline()",
            ".danger()",
            "dialog_neutral_variant",
        ];

        for (name, source) in sources {
            for pattern in forbidden {
                assert!(
                    !source.contains(pattern),
                    "{name} bypasses the shared button theme with `{pattern}`; add or use a ui::style helper instead"
                );
            }
        }
    }

    #[test]
    fn connection_editors_confirm_themed_destructive_removal() {
        let sources = [
            ("database", include_str!("connection_editor.rs")),
            (
                "task tracker",
                include_str!("../tasks_panel/connection_editor.rs"),
            ),
        ];

        for (name, source) in sources {
            assert!(
                source.contains("style::destructive_icon_button"),
                "{name} connections should use the themed destructive icon button"
            );
            assert!(
                source.contains("ConfirmDialog::new"),
                "{name} connections should confirm removal"
            );
            assert!(
                !source.contains("IconName::Delete"),
                "{name} connections should not use the ambiguous legacy delete glyph"
            );
        }
    }
}
