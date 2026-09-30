use super::*;

mod board;
mod board_helpers;
mod comments;
mod controller;
mod detail;
mod display_helpers;
mod indicators;
mod overview;
mod personal_editor;
pub(super) mod prompt;
mod rich_text;

use board_helpers::*;
use display_helpers::*;
use prompt::*;
use rich_text::*;

use crate::ui::tasks_panel::TaskTrackerConnectionsEditor;

/// Velotype owns its internal canvas colors, so embedded personal-task editors
/// must explicitly follow Choro's active theme instead of its standalone dark
/// default.
fn sync_personal_task_editor_theme(cx: &mut App) {
    let colors = EmbeddedThemeColors {
        background: crate::ui::design::base(cx),
        surface: crate::ui::design::nav(cx),
        foreground: crate::ui::style::focus_text(cx),
        muted_foreground: crate::ui::design::t3(cx),
        border: crate::ui::design::line(cx),
        primary: crate::ui::design::accent(cx),
        danger: crate::ui::design::rose(cx),
    };
    velotype::sync_embedded_theme(cx, colors);
}

/// A personal task open as an inline live-doc editor in the middle pane.
/// The velotype editor notifies on edits; we observe it and autosave the
/// description markdown to the local store (only when it actually changed).
pub(super) struct PersonalEditorState {
    task: ide_core::PersonalTaskRecord,
    editor: Entity<velotype::Editor>,
    last_saved: String,
    _subscription: gpui::Subscription,
}

struct PersonalTaskEditorDialog {
    task: Option<ide_core::PersonalTaskRecord>,
    name: Entity<InputState>,
    description: Entity<InputState>,
}

impl PersonalTaskEditorDialog {
    fn new(
        task: Option<ide_core::PersonalTaskRecord>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let name = task
            .as_ref()
            .map(|task| task.title.clone())
            .unwrap_or_default();
        let description = task
            .as_ref()
            .map(|task| task.description_markdown.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Task name")
                .default_value(name)
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(8)
                .placeholder("Describe the task…")
                .default_value(description)
        });
        name.update(cx, |input, cx| input.focus(window, cx));
        cx.new(|_| Self {
            task,
            name,
            description,
        })
    }
}

impl Render for PersonalTaskEditorDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .w_full()
            .max_h(px(560.))
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(section_title_for_dialog("Name", cx))
                    .child(Input::new(&self.name).w_full()),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(section_title_for_dialog("Description", cx))
                    .child(Input::new(&self.description).w_full().h(px(200.))),
            )
    }
}

fn section_title_for_dialog(
    label: &'static str,
    cx: &mut Context<PersonalTaskEditorDialog>,
) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t2(cx))
        .child(label)
        .into_any_element()
}
