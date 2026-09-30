use std::path::PathBuf;

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, WeakEntity, Window,
};
use gpui_component::{
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex, Icon, IconName,
};
use ide_core::ProjectId;

use crate::state::docs::DocEntry;
use crate::state::{DocsState, Workspace};
use crate::ui::agent_status_style::status_accent;
use crate::ui::center::CenterArea;
use crate::ui::style;

pub struct DocsPanel {
    workspace: Entity<Workspace>,
    docs: Entity<DocsState>,
    center: WeakEntity<CenterArea>,
    templates_expanded: bool,
    hovered_entry: Option<PathBuf>,
}

impl DocsPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        docs: Entity<DocsState>,
        center: WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&docs, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                docs,
                center,
                templates_expanded: false,
                hovered_entry: None,
            }
        })
    }

    fn open_doc(&mut self, project: ProjectId, path: PathBuf, cx: &mut Context<Self>) {
        let _ = self
            .center
            .update(cx, |center, cx| center.open_doc(project, path, cx));
        cx.notify();
    }

    fn render_entry_row(
        &self,
        project: ProjectId,
        entry: DocEntry,
        index: usize,
        selected: Option<&PathBuf>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let path = entry.path.clone();
        let hover_path = path.clone();
        let open_path = path.clone();
        let is_template = entry.is_template;
        let is_selected = selected == Some(&path);
        let is_hovered = self.hovered_entry.as_ref() == Some(&path);
        let show_actions = is_hovered || is_selected;
        let accent = if is_template {
            crate::ui::design::accent(cx)
        } else {
            status_accent(
                self.docs.read(cx).doc_status(project, &entry.relative_path),
                cx,
            )
        };
        let label = SharedString::from(entry.title.clone());
        let actions_path = path.clone();
        let actions_title = entry.title.clone();
        let actions_docs = self.docs.clone();
        let actions_center = self.center.clone();
        let action_id = if is_template {
            ("template-row-actions", index)
        } else {
            ("doc-row-actions", index)
        };

        h_flex()
            .id(if is_template {
                ("template-row", index)
            } else {
                ("doc-row", index)
            })
            .mx_1()
            .min_h(px(36.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .bg(if is_selected {
                crate::ui::design::surface_2(cx).opacity(0.4)
            } else {
                gpui::transparent_black()
            })
            .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.56)))
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                if *hovered {
                    this.hovered_entry = Some(hover_path.clone());
                } else if this.hovered_entry.as_ref() == Some(&hover_path) {
                    this.hovered_entry = None;
                }
                cx.notify();
            }))
            .child(
                Icon::new(crate::ui::design::docs_icon())
                    .size(crate::ui::design::icon())
                    .text_color(accent),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_body())
                    .truncate()
                    .text_color(if is_selected {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(label),
            )
            .when(show_actions, |row| {
                row.child(
                    div()
                        .flex_none()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            style::header_icon_button(action_id, IconName::Ellipsis, cx)
                                .tooltip("Document actions")
                                .dropdown_menu(move |mut menu, _, _| {
                                    if !is_template {
                                        let save_docs = actions_docs.clone();
                                        let save_path = actions_path.clone();
                                        let save_title = actions_title.clone();
                                        menu = menu
                                            .item(
                                                PopupMenuItem::new("Save as template")
                                                    .icon(IconName::Copy)
                                                    .on_click(move |_, window, cx| {
                                                        crate::ui::doc_templates::open_save_template_dialog(
                                                            save_docs.clone(),
                                                            project,
                                                            save_path.clone(),
                                                            save_title.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    }),
                                            )
                                            .separator();
                                    }
                                    let delete_center = actions_center.clone();
                                    let delete_path = actions_path.clone();
                                    menu.item(
                                        PopupMenuItem::new("Delete")
                                            .icon(IconName::Delete)
                                            .on_click(move |_, window, cx| {
                                                let _ = delete_center.update(
                                                    cx,
                                                    |center, cx| {
                                                        center.confirm_delete_doc(
                                                            project,
                                                            delete_path.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                );
                                            }),
                                    )
                                }),
                        ),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_doc(project, open_path.clone(), cx);
            }))
            .into_any_element()
    }

    fn render_project(
        &self,
        project: &ide_core::Project,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let docs = self.docs.read(cx).docs_for_project(project.id);
        let templates = self.docs.read(cx).templates_for_project(project.id);
        let selected = self.docs.read(cx).selected_path(project.id);
        let project_id = project.id;
        let docs_count = docs.len();
        let templates_count = templates.len();

        v_flex()
            .w_full()
            .gap_3()
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(section_label("Documents", docs_count, cx))
                    .when(docs.is_empty(), |section| {
                        section.child(
                            div()
                                .mx_3()
                                .py_2()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child("No documents yet"),
                        )
                    })
                    .children(docs.into_iter().enumerate().map(|(index, entry)| {
                        self.render_entry_row(project_id, entry, index, selected.as_ref(), cx)
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .id("templates-section-toggle")
                            .mx_1()
                            .px_2()
                            .min_h(px(28.))
                            .gap_1p5()
                            .items_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.4)))
                            .child(
                                Icon::new(if self.templates_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::t4(cx)),
                            )
                            .child(section_label("Templates", templates_count, cx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.templates_expanded = !this.templates_expanded;
                                cx.notify();
                            })),
                    )
                    .when(self.templates_expanded && templates.is_empty(), |section| {
                        section.child(
                            div()
                                .mx_3()
                                .py_2()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child("No saved templates"),
                        )
                    })
                    .when(self.templates_expanded, |section| {
                        section.children(templates.into_iter().enumerate().map(|(index, entry)| {
                            self.render_entry_row(project_id, entry, index, selected.as_ref(), cx)
                        }))
                    }),
            )
            .into_any_element()
    }
}

impl Render for DocsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_project = self.workspace.read(cx).active_project().cloned();
        let has_active_project = active_project.is_some();
        let active_project_id = active_project.as_ref().map(|project| project.id);

        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(crate::ui::design::header::panel_identity(None, "Docs", cx))
                    .child(div().flex_1())
                    .when_some(active_project_id, |header, project_id| {
                        let docs = self.docs.clone();
                        let center = self.center.clone();
                        header.child(
                            style::context_panel_action_button(
                                "docs-project-new",
                                IconName::Plus,
                                "New",
                                cx,
                            )
                            .dropdown_caret(true)
                            .tooltip("New doc")
                            .dropdown_menu(move |menu, window, cx| {
                                crate::ui::doc_templates::doc_template_menu(
                                    menu,
                                    docs.clone(),
                                    center.clone(),
                                    project_id,
                                    window,
                                    cx,
                                )
                            }),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("docs-panel-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px_1()
                    .pt(crate::ui::design::context_panel_title_pad_top())
                    .pb_3()
                    .when_some(active_project, |list, project| {
                        list.child(self.render_project(&project, cx))
                    })
                    .when(!has_active_project, |list| {
                        list.child(
                            v_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(crate::ui::design::t3(cx))
                                .child(Icon::new(crate::ui::design::docs_icon()).size_8())
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .child("Open a project to see docs"),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn section_label(label: &'static str, count: usize, cx: &App) -> gpui::Div {
    h_flex()
        .min_w(px(0.))
        .gap_1p5()
        .items_center()
        .child(
            div()
                .text_size(crate::ui::design::text_label())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t4(cx))
                .child(label.to_uppercase()),
        )
        .child(
            div()
                .text_size(crate::ui::design::text_label())
                .text_color(crate::ui::design::t4(cx).opacity(0.7))
                .child(count.to_string()),
        )
}
