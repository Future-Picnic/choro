use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use gpui_component::{
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex, IconName,
};
use ide_core::ProjectId;

use crate::state::{PenpotDesign, PenpotState, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::style;

pub struct PenpotPanel {
    workspace: Entity<Workspace>,
    penpot: Entity<PenpotState>,
    center: WeakEntity<CenterArea>,
}

impl PenpotPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        penpot: Entity<PenpotState>,
        center: WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&penpot, |_, _, cx| cx.notify()).detach();
            if let Some(center) = center.upgrade() {
                cx.observe(&center, |_, _, cx| cx.notify()).detach();
            }
            Self {
                workspace,
                penpot,
                center,
            }
        })
    }

    fn select_design(&mut self, project: ProjectId, design_id: uuid::Uuid, cx: &mut Context<Self>) {
        self.penpot.update(cx, |penpot, cx| {
            penpot.select_design(project, design_id, cx)
        });
        let _ = self.center.update(cx, |center, cx| center.show_design(cx));
        cx.notify();
    }

    fn render_design_row(
        &self,
        project: ProjectId,
        design: PenpotDesign,
        selected: bool,
        key: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let design_id = design.id;
        h_flex()
            .id(("penpot-design-row", key))
            .w_full()
            .items_center()
            .gap(crate::ui::design::context_panel_row_gap())
            .px(crate::ui::design::context_panel_row_pad_x())
            .py(crate::ui::design::context_panel_row_pad_y())
            .cursor_pointer()
            .when(selected, |row| {
                row.bg(crate::ui::design::surface_2(cx))
                    .text_color(crate::ui::design::t1(cx))
            })
            .when(!selected, |row| {
                row.hover(|row| row.bg(crate::ui::design::surface_2(cx)))
            })
            .child(
                gpui_component::Icon::new(crate::ui::design::design_icon())
                    .size(crate::ui::design::icon_md())
                    .text_color(crate::ui::design::accent(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_head())
                    .text_color(if selected {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .child(design.name),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_design(project, design_id, cx);
            }))
            .into_any_element()
    }
}

impl Render for PenpotPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_project = self
            .workspace
            .read(cx)
            .active_project()
            .map(|project| project.id);
        let penpot_enabled = self.penpot.read(cx).enabled();
        let configured = self.penpot.read(cx).is_configured();
        let creating = self.penpot.read(cx).creating_design();
        let assistant_busy =
            active_project.is_some_and(|project| self.penpot.read(cx).assistant_busy(project));
        let designs = active_project
            .map(|project| self.penpot.read(cx).designs_for_project(project))
            .unwrap_or_default();
        let selected = active_project.and_then(|project| {
            self.penpot
                .read(cx)
                .selected_design(project)
                .map(|design| design.id)
        });
        let mut rows = active_project
            .map(|project| {
                designs
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(key, design)| {
                        let is_selected = selected == Some(design.id);
                        self.render_design_row(project, design, is_selected, key, cx)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let penpot_rows = std::mem::take(&mut rows);
        if let (Some(project), Some(center)) = (active_project, self.center.upgrade()) {
            for (index, design) in center
                .read(cx)
                .studio_designs(project)
                .into_iter()
                .enumerate()
            {
                let id = design.id;
                let center = center.clone();
                rows.push(
                    style::ghost_button_compact(
                        ("studio-project-row", index),
                        format!("{} · Studio", design.name),
                    )
                    .on_click(move |_, _, cx| {
                        center.update(cx, |center, cx| center.open_studio(project, id, cx))
                    })
                    .into_any_element(),
                );
            }
        }
        rows.extend(penpot_rows);
        let center = self.center.clone();

        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(crate::ui::design::header::panel_identity(
                        None, "Designs", cx,
                    ))
                    .child(div().flex_1())
                    .when_some(active_project, |header, project| {
                        let systems_center = center.clone();
                        header
                            .child(
                                style::header_icon_button("studio-systems", IconName::Palette, cx)
                                    .tooltip("Project design systems")
                                    .on_click(move |_, _, cx| {
                                        let _ = systems_center.update(cx, |center, cx| {
                                            center.open_system_library(project, cx)
                                        });
                                    }),
                            )
                            .child(
                                style::context_panel_action_button(
                                    "penpot-new-design",
                                    IconName::Plus,
                                    "New Design",
                                    cx,
                                )
                                .tooltip("Create a Studio design or add a design link")
                                .dropdown_menu(
                                    move |menu, _, _| {
                                        let native_center = center.clone();
                                        let figma_center = center.clone();
                                        let studio_center = center.clone();
                                        menu.item(PopupMenuItem::new("New Studio design")
                                            .icon(crate::ui::design::design_icon())
                                            .on_click(move |_, window, cx| { let _ = studio_center.update(cx, |center, cx| center.create_studio_from_hub(project, window, cx)); }))
                                            .item(PopupMenuItem::new("Add Figma link").icon(IconName::Globe)
                                                .on_click(move |_, window, cx| { let _ = figma_center.update(cx, |center, cx| center.open_figma_design_dialog(project, window, cx)); }))
                                            .when(penpot_enabled, |menu| menu.item(PopupMenuItem::new("New Penpot design (beta)")
                                                .icon(crate::ui::design::design_icon()).disabled(!configured || creating || assistant_busy)
                                                .on_click(move |_, _, cx| { let _ = native_center.update(cx, |center, cx| center.create_penpot_design_from_hub(project, cx)); })))
                                    },
                                ),
                            )
                    }),
            )
            .when_some(
                self.penpot.read(cx).last_error().map(str::to_string),
                |panel, error| {
                    panel.child(
                        div()
                            .mx_2()
                            .mb_2()
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::design::rose(cx).opacity(0.24))
                            .bg(crate::ui::design::rose(cx).opacity(0.08))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(error),
                    )
                },
            )
            .when(active_project.is_none(), |panel| {
                panel.child(style::empty_state(
                    crate::ui::design::design_icon(),
                    "No project open",
                    "Open a Choro project to create designs",
                    cx,
                ))
            })
            .when(
                active_project.is_some() && rows.is_empty(),
                |panel| {
                    panel.child(style::empty_state(
                        crate::ui::design::design_icon(),
                        "No designs yet",
                        "Create a Studio design for this project.",
                        cx,
                    ))
                },
            )
            .when(
                active_project.is_some() && configured && designs.is_empty() && !creating,
                |panel| {
                    panel.child(style::empty_state(
                        crate::ui::design::design_icon(),
                        "No designs yet",
                        "Choose + New Design to create one",
                        cx,
                    ))
                },
            )
            .when(creating, |panel| {
                panel.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Creating design…"),
                )
            })
            .when(!rows.is_empty(), |panel| {
                panel.child(
                    v_flex()
                        .id("penpot-designs-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .children(rows),
                )
            })
            .into_any_element()
    }
}
