use gpui::{div, px, relative, rgba, Hsla, IntoElement as _, ParentElement as _, Styled as _};
use ide_core::DbProvider;

pub mod collection_pane;
pub mod database_pane;
pub mod db_panel;
pub mod table_pane;

fn provider_brand_glyph(provider: DbProvider) -> char {
    match provider {
        DbProvider::MariaDb => '\u{ead9}',
        DbProvider::MongoDb => '\u{eaf5}',
        DbProvider::MySql => '\u{eafd}',
        DbProvider::PostgreSql => '\u{eb79}',
        DbProvider::SQLite => '\u{ec1e}',
        DbProvider::Supabase => '\u{ec2e}',
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
    }))
}

/// A Devicon font glyph rendered through GPUI's crisp text pipeline.
pub(crate) fn provider_brand_mark(provider: DbProvider, size: f32) -> gpui::AnyElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .child(
            div()
                .font_family(crate::theme::DEVICON_FONT_FAMILY)
                .text_size(px(size))
                .line_height(relative(1.))
                .text_color(provider_brand_color(provider))
                .child(provider_brand_glyph(provider).to_string()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_database_provider_has_a_unique_devicon_glyph() {
        let providers = [
            DbProvider::MongoDb,
            DbProvider::PostgreSql,
            DbProvider::Supabase,
            DbProvider::SQLite,
            DbProvider::MySql,
            DbProvider::MariaDb,
        ];

        let glyphs = providers.map(provider_brand_glyph);
        for glyph in glyphs {
            assert!((0xe000..=0xf8ff).contains(&(glyph as u32)));
        }
        let unique = glyphs.into_iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), providers.len());
    }

    #[test]
    fn database_ui_uses_shared_button_builders() {
        let sources = [
            ("collection_pane.rs", include_str!("collection_pane.rs")),
            ("database_pane.rs", include_str!("database_pane.rs")),
            ("db_panel.rs", include_str!("db_panel.rs")),
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
            ("database", include_str!("db_panel.rs")),
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
