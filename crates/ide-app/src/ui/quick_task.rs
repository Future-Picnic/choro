use gpui::{
    actions, div, px, App, AppContext, Context, Entity, InteractiveElement, IntoElement,
    KeyBinding, ParentElement, Render, SharedString, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex, Icon, IconName, Sizable, WindowExt,
};
use ide_core::ProjectId;

use crate::state::{TasksState, Workspace};

actions!(quick_task, [QuickTaskConfirm]);

const CONTEXT: &str = "QuickTask";

pub fn bindings() -> Vec<KeyBinding> {
    // Bind on the input context (like the ⌘P palette) so Enter submits from the
    // title/description instead of being swallowed as a newline. Shift-enter
    // still inserts a newline in the multi-line description.
    vec![
        KeyBinding::new("enter", QuickTaskConfirm, Some("QuickTask > Input")),
        KeyBinding::new(
            "secondary-enter",
            QuickTaskConfirm,
            Some("QuickTask > Input"),
        ),
    ]
}

/// A global quick-add modal (⌘⇧T) for creating a Personal task from anywhere,
/// styled like the ⌘P palette. Title + description + which project it lands in.
pub struct QuickTaskModal {
    workspace: Entity<Workspace>,
    tasks: Entity<TasksState>,
    title: Entity<InputState>,
    description: Entity<InputState>,
    project: ProjectId,
}

impl QuickTaskModal {
    pub fn open(
        workspace: Entity<Workspace>,
        tasks: Entity<TasksState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let default_project = {
            let workspace = workspace.read(cx);
            workspace
                .active_project()
                .map(|project| project.id)
                .or_else(|| workspace.projects.first().map(|project| project.id))
        };
        let Some(project) = default_project else {
            return;
        };

        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Task title"));
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .placeholder("Details (optional)")
        });
        let modal = cx.new(|_| Self {
            workspace,
            tasks,
            title,
            description,
            project,
        });

        let dialog_modal = modal.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(560.))
                .margin_top(px(80.))
                .overlay(true)
                .close_button(false)
                .keyboard(true)
                .p_0()
                .child(dialog_modal.clone())
        });

        let title_input = modal.read(cx).title.clone();
        title_input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title.read(cx).value().trim().to_string();
        if title.is_empty() {
            return;
        }
        let description = self.description.read(cx).value().to_string();
        let project = self.project;
        self.tasks.update(cx, |tasks, cx| {
            tasks.create_personal_task(project, title, description, cx);
        });
        window.close_dialog(cx);
    }
}

impl Render for QuickTaskModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let projects = self.workspace.read(cx).projects.clone();
        let current_name = projects
            .iter()
            .find(|project| project.id == self.project)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Select project".to_string());
        let view = cx.entity().clone();

        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &QuickTaskConfirm, window, cx| {
                this.create(window, cx);
            }))
            .w_full()
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::accent(cx).opacity(0.55))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .child(
                h_flex()
                    .h(px(46.))
                    .w_full()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.45))
                    .child(
                        Icon::new(IconName::CircleCheck)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&self.title)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false),
                        ),
                    ),
            )
            .child(
                v_flex().w_full().px_3().py_3().gap_2().child(
                    div()
                        .w_full()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::line(cx).opacity(0.5))
                        .bg(crate::ui::design::base(cx).opacity(0.5))
                        .px_2()
                        .py_1()
                        .child(
                            Input::new(&self.description)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false),
                        ),
                ),
            )
            .child(
                h_flex()
                    .h(px(42.))
                    .w_full()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.45))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Personal · add to"),
                    )
                    .child(
                        Button::new("quick-task-project")
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_sm())
                            .px_2()
                            .dropdown_caret(true)
                            .custom(crate::ui::style::chip_dropdown_variant(cx))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        Icon::new(IconName::FolderOpen)
                                            .size(crate::ui::design::icon_md())
                                            .text_color(crate::ui::design::t3(cx)),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(180.))
                                            .truncate()
                                            .child(SharedString::from(current_name)),
                                    ),
                            )
                            .dropdown_menu({
                                let projects = projects.clone();
                                move |mut menu, window, _| {
                                    for project in &projects {
                                        let project_id = project.id;
                                        menu = menu.item(
                                            PopupMenuItem::new(project.name.clone()).on_click(
                                                window.listener_for(
                                                    &view,
                                                    move |this: &mut Self, _, _, cx| {
                                                        this.project = project_id;
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("⏎ create"),
                    )
                    .child(
                        Button::new("quick-task-cancel")
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_sm())
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::ui::style::primary_button_compact("quick-task-create", "Create", cx)
                            .icon(IconName::Plus)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create(window, cx);
                            })),
                    ),
            )
    }
}
