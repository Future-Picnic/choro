use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use gpui::App;
use gpui_component::{menu::PopupMenuStyle, Theme, ThemeRegistry};
use ide_core::AppConfig;
use lucide_icons::LUCIDE_FONT_BYTES;

/// Our signature theme — the default, and pinned to the top of the picker.
pub const SIGNATURE_THEME: &str = "Choro Dark";
/// Saved by builds that predate the public Choro naming.
pub const LEGACY_SIGNATURE_THEME: &str = "Velvet Amethyst";

/// The Choro family, in picker order (signature first). Generated from design
/// tokens by `scripts/gen_themes.py` into `assets/themes/choro.json`.
pub const CHORO_THEMES: &[&str] = &[
    "Choro Dark",
    "Choro Indigo",
    "Choro Twilight",
    "Choro Dusk",
    "Choro Light",
    "Amethyst",
    "Ember",
    "Mint",
];

/// Theme JSON files bundled into the binary, installed to the config dir as
/// `(filename, contents)`. The whole Choro family lives in one generated file.
const BUNDLED_THEMES: &[(&str, &str)] =
    &[("choro.json", include_str!("../assets/themes/choro.json"))];
const OBSOLETE_SIGNATURE_THEME_FILE: &str = "choro-dark.json";
// The canonical Choro UI typeface: neutral, compact, and consistently clear
// across dense navigation, controls, metadata, and long agent conversations.
pub const UI_FONT_FAMILY: &str = "Inter";
pub const DEVICON_FONT_FAMILY: &str = "devicon";
const DEVICON_DATABASE_FONT_BASE64: &str =
    include_str!("../assets/fonts/devicon/devicon-databases.ttf.b64");
const UI_FONTS: &[&'static [u8]] = &[
    include_bytes!("../assets/fonts/inter/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Bold.ttf"),
    LUCIDE_FONT_BYTES,
];

fn themes_dir() -> Option<PathBuf> {
    Some(AppConfig::config_path().parent()?.join("themes"))
}

/// Installs bundled themes next to the config file and starts watching the
/// directory. Once themes load, re-applies the user's configured theme.
pub fn init(cx: &mut App) {
    install_ui_fonts(cx);
    apply_ui_font(cx);

    let Some(dir) = themes_dir() else { return };
    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!("cannot create themes dir: {error}");
        return;
    }
    archive_obsolete_signature_theme(&dir);
    for (file, contents) in BUNDLED_THEMES {
        let bundled = dir.join(file);
        let outdated = fs::read_to_string(&bundled)
            .map(|current| &current != contents)
            .unwrap_or(true);
        if outdated {
            if let Err(error) = fs::write(&bundled, contents) {
                eprintln!("cannot install bundled theme {file}: {error}");
            }
        }
    }

    if let Err(error) = ThemeRegistry::watch_dir(dir, cx, |cx| {
        // Themes load asynchronously; re-apply the saved choice when ready.
        if let Some(name) = AppConfig::load().theme_name {
            apply_named(&name, cx);
        } else {
            apply_ui_font(cx);
        }
    }) {
        eprintln!("cannot watch themes dir: {error}");
    }
}

/// An earlier Choro build installed a separate blue `Choro Dark` theme. Its
/// duplicate name can override the signature Velvet palette depending on file
/// watcher order, so archive only that exact app-generated file before loading.
fn archive_obsolete_signature_theme(dir: &Path) {
    let path = dir.join(OBSOLETE_SIGNATURE_THEME_FILE);
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    let is_obsolete = contents.contains("signature hybrid theme for my-ide")
        && contents.contains("\"background\": \"#0D1117\"")
        && contents.contains("\"name\": \"Choro Dark\"");
    if !is_obsolete {
        return;
    }
    let archived = dir.join("choro-dark.legacy-blue.json.disabled");
    if let Err(error) = fs::rename(&path, &archived) {
        eprintln!("cannot archive obsolete Choro Dark theme: {error}");
    }
}

fn install_ui_fonts(cx: &mut App) {
    let mut fonts = UI_FONTS
        .iter()
        .map(|font| Cow::Borrowed(*font))
        .collect::<Vec<_>>();
    match decode_devicon_database_font() {
        Ok(font) => fonts.push(Cow::Owned(font)),
        Err(error) => eprintln!("cannot decode bundled Devicon font: {error}"),
    }
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("cannot load bundled UI fonts: {error}");
    }
}

fn decode_devicon_database_font() -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_STANDARD.decode(DEVICON_DATABASE_FONT_BASE64.trim())
}

pub fn apply_ui_font(cx: &mut App) {
    // Choro themes ship precise `border` (the design system's `line` token), so
    // we no longer derive/soften it — the JSON is the source of truth.
    {
        let theme = Theme::global_mut(cx);
        theme.font_family = UI_FONT_FAMILY.into();
        theme.radius = crate::ui::design::r_sm();
        theme.radius_lg = crate::ui::design::r_md();
    }
    apply_popup_menu_tokens(cx);
}

/// Feed the application design system into gpui-component's shared popup
/// renderer. Every dropdown therefore uses one mock-aligned menu language,
/// including third-party controls that create their own `PopupMenu` instances.
fn apply_popup_menu_tokens(cx: &mut App) {
    PopupMenuStyle::set(
        PopupMenuStyle {
            background: crate::ui::design::focus(cx),
            foreground: crate::ui::design::t2(cx),
            hover_background: crate::ui::design::surface_2(cx),
            hover_foreground: crate::ui::design::t1(cx),
            disabled_foreground: crate::ui::design::t4(cx),
            label_foreground: crate::ui::design::t4(cx),
            border: crate::ui::design::line_2(cx),
            separator: crate::ui::design::menu_separator(cx),
            container_radius: crate::ui::design::r_md(),
            item_radius: crate::ui::design::r_sm(),
            min_width: crate::ui::design::menu_min_w(),
            max_width: crate::ui::design::menu_max_w(),
            max_height: crate::ui::design::menu_max_h(),
            item_height: crate::ui::design::control_h_sm(),
            text_size: crate::ui::design::text_ui(),
            label_text_size: crate::ui::design::text_label(),
            outer_padding: crate::ui::design::menu_outer_pad(),
            item_padding_x: crate::ui::design::menu_item_pad_x(),
            item_gap: crate::ui::design::menu_item_gap(),
            icon_size: crate::ui::design::icon_sm(),
            icon_slot_width: crate::ui::design::menu_icon_slot_w(),
            separator_margin_x: crate::ui::design::menu_separator_margin_x(),
            separator_margin_y: crate::ui::design::menu_separator_margin_y(),
            label_padding_top: crate::ui::design::menu_label_pad_top(),
            label_padding_bottom: crate::ui::design::menu_label_pad_bottom(),
            separator_height: crate::ui::design::menu_separator_h(),
            window_margin: crate::ui::design::menu_window_margin(),
            submenu_flip_offset: crate::ui::design::menu_submenu_flip_offset(),
            submenu_overlap: crate::ui::design::menu_submenu_overlap(),
            shadow: crate::ui::design::menu_shadow(),
        },
        cx,
    );
}

/// Applies a registered theme by name. Returns false if unknown.
pub fn apply_named(name: &str, cx: &mut App) -> bool {
    let name = if name == LEGACY_SIGNATURE_THEME {
        SIGNATURE_THEME
    } else {
        name
    };
    let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() else {
        return false;
    };
    Theme::global_mut(cx).apply_config(&config);
    apply_ui_font(cx);
    cx.refresh_windows();
    true
}

/// All selectable theme names: built-in defaults plus registered ones.
/// The theme picker shows *only* the Choro family — never the gpui-component
/// built-ins (Default Dark/Light, Catppuccin) or any stale installed JSON. We
/// filter to the ones actually loaded, in Choro order.
pub fn available_themes(cx: &App) -> Vec<String> {
    let loaded: std::collections::HashSet<String> = ThemeRegistry::global(cx)
        .sorted_themes()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    CHORO_THEMES
        .iter()
        .filter(|name| loaded.contains(**name))
        .map(|name| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_include_complete_editor_palettes() {
        let set: gpui_component::ThemeSet =
            serde_json::from_str(BUNDLED_THEMES[0].1).expect("Choro themes should parse");
        assert_eq!(set.themes.len(), CHORO_THEMES.len());
        for theme in set.themes {
            let highlight = theme
                .highlight
                .expect("every Choro theme has editor colors");
            assert!(highlight.editor_background.is_some());
            assert!(highlight.editor_foreground.is_some());
            assert!(highlight.editor_line_number.is_some());
            for syntax in ["comment", "keyword", "function", "string", "type"] {
                assert!(
                    highlight.syntax.style(syntax).is_some(),
                    "{} is missing {syntax} styling",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn bundled_devicon_subset_is_a_truetype_font() {
        let font = decode_devicon_database_font().expect("Devicon subset should decode");
        assert_eq!(&font[..4], &[0, 1, 0, 0]);
        assert!(font.len() < 8 * 1024, "database subset should stay compact");

        let face = ttf_parser::Face::parse(&font, 0).expect("Devicon subset should parse");
        assert!(
            face.glyph_index('m').is_some(),
            "GPUI rejects custom fonts without an ASCII m measurement glyph"
        );
        for codepoint in [0xead9, 0xeaf5, 0xeafd, 0xeb79, 0xec1e, 0xec2e] {
            let character = char::from_u32(codepoint).expect("valid Devicon codepoint");
            assert!(
                face.glyph_index(character).is_some(),
                "missing Devicon glyph U+{codepoint:04X}"
            );
        }
    }
}
