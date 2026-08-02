use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, IntoElement, ParentElement,
    Render, SharedString, Styled, Window,
};
use gpui_component::{
    button::ButtonVariants,
    h_flex,
    input::{Input, InputState},
    v_flex, IconName, WindowExt,
};
use ide_core::ScriptPreset;
use uuid::Uuid;

use crate::state::Workspace;

struct PresetRow {
    id: Uuid,
    name: Entity<InputState>,
    command: Entity<InputState>,
}

/// Dialog content for editing a project's script presets.
pub struct PresetEditor {
    rows: Vec<PresetRow>,
}

impl PresetEditor {
    /// Opens the preset editor dialog for the active project.
    pub fn open(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        let Some(project) = workspace.read(cx).active_project() else {
            eprintln!("preset editor: no active project");
            return;
        };
        let project_id = project.id;
        let project_name = project.name.clone();
        let presets = project.presets.clone();
        eprintln!(
            "preset editor: opening for {project_name} ({} presets)",
            presets.len()
        );

        let editor = cx.new(|cx| Self {
            rows: presets
                .iter()
                .map(|preset| Self::row_from(preset, window, cx))
                .collect(),
        });
        let popover = crate::ui::design::focus(cx);
        let popover_foreground = crate::ui::design::t1(cx);
        let modal_border = crate::ui::design::t3(cx).opacity(0.18);

        let footer_editor = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let save_editor = footer_editor.clone();
            let save_workspace = workspace.clone();
            dialog
                .w(px(560.))
                .title(SharedString::from(format!("Run Scripts — {project_name}")))
                .overlay(false)
                .bg(popover)
                .text_color(popover_foreground)
                .border_color(modal_border)
                .child(footer_editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let workspace = save_workspace.clone();
                    vec![
                        crate::ui::style::primary_button_compact("save-presets", "Save", cx)
                            .icon(IconName::Check)
                            .on_click(move |_, window, cx| {
                                let presets = editor.read(cx).collect_presets(cx);
                                workspace.update(cx, |workspace, cx| {
                                    workspace.update_presets(project_id, presets, cx);
                                });
                                window.close_dialog(cx);
                            }),
                        crate::ui::style::ghost_button_compact("cancel-presets", "Cancel")
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn row_from(preset: &ScriptPreset, window: &mut Window, cx: &mut App) -> PresetRow {
        PresetRow {
            id: preset.id,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("name (e.g. run server)")
                    .default_value(preset.name.clone())
            }),
            command: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("command (e.g. npm run dev)")
                    .default_value(preset.command.clone())
            }),
        }
    }

    fn add_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows.push(PresetRow {
            id: Uuid::new_v4(),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("name (e.g. run server)")),
            command: cx
                .new(|cx| InputState::new(window, cx).placeholder("command (e.g. npm run dev)")),
        });
        cx.notify();
    }

    fn collect_presets(&self, cx: &App) -> Vec<ScriptPreset> {
        self.rows
            .iter()
            .filter_map(|row| {
                let name = row.name.read(cx).value().trim().to_string();
                let command = row.command.read(cx).value().trim().to_string();
                if name.is_empty() || command.is_empty() {
                    return None;
                }
                Some(ScriptPreset {
                    id: row.id,
                    name,
                    command,
                })
            })
            .collect()
    }
}

impl Render for PresetEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = |text: &'static str| {
            div()
                .text_size(crate::ui::design::text_ui())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t3(cx))
                .child(text)
        };
        v_flex()
            .w(px(512.))
            .gap_2()
            .when(!self.rows.is_empty(), |list| {
                list.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(div().w(px(168.)).child(label("Name")))
                        .child(div().flex_1().min_w(px(0.)).child(label("Command")))
                        .child(div().w(px(28.))),
                )
            })
            .children(self.rows.iter().enumerate().map(|(ix, row)| {
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(168.)).child(Input::new(&row.name)))
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&row.command)))
                    .child(
                        crate::ui::style::header_icon_button(
                            ("delete-preset", ix),
                            IconName::Delete,
                            cx,
                        )
                        .tooltip("Remove script")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.rows.remove(ix);
                            cx.notify();
                        })),
                    )
            }))
            .when(self.rows.is_empty(), |list| {
                list.child(
                    div()
                        .py_2()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No scripts yet — add one below."),
                )
            })
            .child(
                crate::ui::style::ghost_button_compact("add-preset", "Add script")
                    .icon(IconName::Plus)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.add_empty_row(window, cx);
                    })),
            )
    }
}
