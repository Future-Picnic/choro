// Modified by Choro contributors; see vendor/velotype/CHORO_MODIFICATIONS.md.
//! Embedded library facade for Velotype.

use std::collections::BTreeMap;

use gpui::{App, BorrowAppContext, Hsla, rgba};

mod app_identity;
mod app_menu;
mod components;
mod config;
pub mod editor;
mod export;
#[cfg(any(target_os = "macos", test))]
mod file_url;
mod i18n;
mod net;
mod theme;
mod window_chrome;

pub use components::{ReferenceData, reference_fence_markdown};
pub use editor::{
    Editor, EditorEvent, EmbeddedBlockStyle, MentionCandidate, MentionQuery, MentionTrigger,
    REFERENCE_TARGET_SCHEME,
};

/// Host app colors used when Velotype is embedded inside another GPUI surface.
#[derive(Clone, Copy, Debug)]
pub struct EmbeddedThemeColors {
    pub background: Hsla,
    pub surface: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub danger: Hsla,
}

/// Install the global state and keybindings Velotype's editor expects.
pub fn init_embedded(cx: &mut App) {
    i18n::I18nManager::init_with_language_id(cx, "en-US");
    theme::ThemeManager::init_with_theme_id(cx, "velotype");
    config::EditorSettings::init(cx, true);
    net::install_http_client(cx);
    components::init_with_keybindings(cx, &BTreeMap::new());
}

/// Syncs Velotype's internal theme with the host app theme. The embedded editor
/// renders its own scroll area, rows, tables, and menus, so the host has to
/// update Velotype's theme rather than only wrapping it in a themed container.
pub fn sync_embedded_theme(cx: &mut App, colors: EmbeddedThemeColors) {
    let _ = cx.update_global::<theme::ThemeManager, _>(|theme_manager, _cx| {
        let current = &theme_manager.current().colors;
        if current.editor_background == colors.background
            && current.text_default == colors.foreground
            && current.text_placeholder == colors.muted_foreground
            && current.table_border == colors.border
        {
            return false;
        }

        theme_manager.set_theme(host_theme(colors));
        true
    });
}

fn host_theme(colors: EmbeddedThemeColors) -> theme::Theme {
    let mut theme = if is_light(colors.background) {
        theme::Theme::light_theme()
    } else {
        theme::Theme::default_theme()
    };
    let inverse_primary_text = readable_text_on(colors.primary);
    let c = &mut theme.colors;

    theme.name = "Host".into();
    c.editor_background = colors.background;
    c.source_mode_block_bg = colors.surface;
    c.text_default = colors.foreground;
    c.text_link = colors.primary;
    c.text_placeholder = colors.muted_foreground;
    c.text_h1 = colors.foreground;
    c.text_h2 = colors.foreground;
    c.text_h3 = colors.foreground;
    c.text_h4 = colors.foreground;
    c.text_h5 = colors.foreground;
    c.text_h6 = colors.foreground;
    c.border_h1 = colors.border.opacity(0.86);
    c.border_h2 = colors.border.opacity(0.72);
    c.text_quote = colors.muted_foreground;
    c.border_quote = colors.primary.opacity(0.64);
    c.callout_note_bg = colors.primary.opacity(0.09);
    c.callout_note_border = colors.primary.opacity(0.72);
    c.callout_tip_bg = colors.primary.opacity(0.08);
    c.callout_tip_border = colors.primary.opacity(0.68);
    c.callout_important_bg = colors.primary.opacity(0.1);
    c.callout_important_border = colors.primary;
    c.callout_warning_bg = Hsla::from(rgba(0xe5c07b24));
    c.callout_warning_border = Hsla::from(rgba(0xe5c07bff));
    c.callout_caution_bg = colors.danger.opacity(0.1);
    c.callout_caution_border = colors.danger;
    c.footnote_bg = colors.surface;
    c.footnote_border = colors.border.opacity(0.65);
    c.footnote_badge_bg = colors.border.opacity(0.22);
    c.footnote_badge_text = colors.muted_foreground;
    c.footnote_backref = colors.primary;
    c.task_checkbox_border = colors.border;
    c.task_checkbox_bg = colors.background;
    c.task_checkbox_checked_bg = colors.primary;
    c.task_checkbox_check = inverse_primary_text;
    c.separator_color = colors.border;
    c.code_bg = colors.surface;
    c.code_text = colors.foreground;
    c.code_language_input_bg = colors.surface;
    c.code_language_input_border = colors.border;
    c.code_language_input_text = colors.foreground;
    c.code_language_input_placeholder = colors.muted_foreground;
    c.table_border = colors.border;
    c.table_header_bg = colors.surface;
    c.table_cell_bg = colors.background;
    c.table_cell_active_outline = colors.primary;
    c.table_axis_preview_bg = colors.primary.opacity(0.08);
    c.table_axis_selected_bg = colors.primary.opacity(0.16);
    c.table_append_button_bg = colors.surface;
    c.table_append_button_hover = colors.border.opacity(0.32);
    c.table_append_button_text = colors.foreground;
    c.image_placeholder_bg = colors.surface;
    c.image_placeholder_border = colors.border;
    c.image_placeholder_text = colors.muted_foreground;
    c.image_caption_text = colors.muted_foreground;
    c.scrollbar_thumb = colors.muted_foreground.opacity(0.68);
    c.cursor = colors.foreground;
    c.selection = colors.primary.opacity(0.28);
    c.dialog_surface = colors.surface;
    c.dialog_border = colors.border;
    c.dialog_title = colors.foreground;
    c.dialog_body = colors.foreground;
    c.dialog_muted = colors.muted_foreground;
    c.dialog_primary_button_bg = colors.primary;
    c.dialog_primary_button_hover = colors.primary.opacity(0.86);
    c.dialog_primary_button_text = inverse_primary_text;
    c.dialog_secondary_button_bg = colors.border.opacity(0.16);
    c.dialog_secondary_button_hover = colors.border.opacity(0.28);
    c.dialog_secondary_button_text = colors.foreground;
    c.dialog_danger_button_bg = colors.danger;
    c.dialog_danger_button_hover = colors.danger.opacity(0.86);
    c.dialog_danger_button_text = readable_text_on(colors.danger);

    theme
}

fn is_light(color: Hsla) -> bool {
    let rgb = color.to_rgb();
    let luminance = 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
    luminance > 0.58
}

fn readable_text_on(color: Hsla) -> Hsla {
    if is_light(color) {
        Hsla::from(rgba(0x111827ff))
    } else {
        Hsla::from(rgba(0xffffffff))
    }
}
