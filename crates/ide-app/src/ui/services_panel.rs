use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use gpui_component::{Icon, IconName};

use ide_core::{ProjectId, SubAppServices};

use crate::state::{ServicesMode, ServicesState, Workspace};
use crate::ui::center::CenterArea;

/// The right-sidebar navigator for Services. The canonical mock uses one panel
/// title plus a quiet flip action: Services lists sources, while Env groups the
/// actual env files beneath each source. Selecting a row drives the center.
pub struct ServicesPanel {
    workspace: Entity<Workspace>,
    services: Entity<ServicesState>,
    #[allow(dead_code)]
    center: WeakEntity<CenterArea>,
}

impl ServicesPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        services: Entity<ServicesState>,
        center: WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&services, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                services,
                center,
            }
        })
    }

    fn active_project(&self, cx: &App) -> Option<(ProjectId, std::path::PathBuf)> {
        self.workspace
            .read(cx)
            .active_project()
            .map(|project| (project.id, project.path.clone()))
    }

    fn h_flex() -> gpui::Div {
        div().flex().flex_row()
    }
    fn v_flex() -> gpui::Div {
        div().flex().flex_col()
    }
}

impl Render for ServicesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((project, root)) = self.active_project(cx) else {
            return Self::v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Open a project"),
                );
        };

        // Make sure a scan is running / results are fresh.
        self.services
            .update(cx, |state, cx| state.ensure_scanned(project, root, cx));

        let state = self.services.read(cx);
        let sub_apps = state.services_for(project).cloned().unwrap_or_default();
        let mode = state.mode(project);
        let effective = state
            .selected_source(project)
            .cloned()
            .or_else(|| sub_apps.first().map(|s| s.rel_path.clone()));

        Self::v_flex()
            .size_full()
            .bg(crate::ui::design::nav(cx))
            .child(self.render_panel_title(project, mode, cx))
            .child(
                Self::v_flex()
                    .id("services-source-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .when(mode == ServicesMode::Services, |list| {
                        list.child(self.render_lane("Sources", cx))
                            .children(sub_apps.iter().map(|sub| {
                                self.render_service_source_row(
                                    project,
                                    sub,
                                    effective.as_deref(),
                                    cx,
                                )
                            }))
                    })
                    .when(mode == ServicesMode::Env, |list| {
                        list.children(sub_apps.iter().filter(|sub| !sub.env_files.is_empty()).map(
                            |sub| {
                                Self::v_flex()
                                    .w_full()
                                    .child(self.render_lane(&sub.name, cx))
                                    .children(sub.env_files.iter().map(|name| {
                                        self.render_env_file_row(project, sub, name, cx)
                                    }))
                            },
                        ))
                    }),
            )
    }
}

impl ServicesPanel {
    fn render_panel_title(
        &self,
        project: ProjectId,
        mode: ServicesMode,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        crate::ui::design::header::panel_bar(cx)
            .child(crate::ui::design::header::panel_identity(
                None,
                if mode == ServicesMode::Services {
                    "Services"
                } else {
                    "Env"
                },
                cx,
            ))
            .child(div().flex_1())
            .child(
                crate::ui::style::context_panel_action_button(
                    "services-mode-flip",
                    if mode == ServicesMode::Services {
                        gpui_component::IconName::File
                    } else {
                        gpui_component::IconName::Building2
                    },
                    if mode == ServicesMode::Services {
                        "Env"
                    } else {
                        "Services"
                    },
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let next = if mode == ServicesMode::Services {
                        ServicesMode::Env
                    } else {
                        ServicesMode::Services
                    };
                    this.services
                        .update(cx, |state, cx| state.set_mode(project, next, cx));
                })),
            )
            .into_any_element()
    }

    fn render_lane(&self, label: &str, cx: &App) -> gpui::AnyElement {
        div()
            .px(crate::ui::design::context_panel_lane_pad_x())
            .pt(crate::ui::design::context_panel_lane_pad_top())
            .pb(crate::ui::design::context_panel_lane_pad_bottom())
            .text_size(crate::ui::design::text_label())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(crate::ui::design::t4(cx))
            .child(SharedString::from(label.to_uppercase()))
            .into_any_element()
    }

    fn render_service_source_row(
        &self,
        project: ProjectId,
        sub: &SubAppServices,
        effective: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = effective == Some(sub.rel_path.as_str());
        let count = sub.services.len();
        let rel = sub.rel_path.clone();

        Self::h_flex()
            .id(SharedString::from(format!("svc-src-{}", sub.rel_path)))
            .items_center()
            .gap(crate::ui::design::context_panel_row_gap())
            .px(crate::ui::design::context_panel_row_pad_x())
            .py(crate::ui::design::context_panel_row_pad_y())
            .cursor_pointer()
            .when(active, |row| {
                row.bg(crate::ui::design::surface_2(cx))
                    .text_color(crate::ui::design::t1(cx))
            })
            .when(!active, |row| {
                row.hover(|r| r.bg(crate::ui::design::surface_2(cx)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let rel = rel.clone();
                this.services
                    .update(cx, |state, cx| state.set_selected_source(project, rel, cx));
            }))
            .child(crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::Server,
                crate::ui::design::t3(cx),
                crate::ui::design::icon_md(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_head())
                    .text_color(if active {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .truncate()
                    .child(SharedString::from(sub.name.clone())),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t4(cx))
                    .child(format!("{count}")),
            )
            .into_any_element()
    }

    fn render_env_file_row(
        &self,
        project: ProjectId,
        sub: &SubAppServices,
        name: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected_source = self.services.read(cx).selected_source(project);
        let selected_file = self
            .services
            .read(cx)
            .selected_env_file(project, &sub.rel_path);
        let active = selected_source.map(String::as_str) == Some(sub.rel_path.as_str())
            && selected_file.map(String::as_str).unwrap_or_else(|| {
                sub.env_files
                    .first()
                    .map(String::as_str)
                    .unwrap_or_default()
            }) == name;
        let rel = sub.rel_path.clone();
        let picked = name.to_string();

        Self::h_flex()
            .id(SharedString::from(format!(
                "svc-env-{}-{name}",
                sub.rel_path
            )))
            .items_center()
            .gap(crate::ui::design::context_panel_row_gap())
            .px(crate::ui::design::context_panel_row_pad_x())
            .py(crate::ui::design::context_panel_row_pad_y())
            .cursor_pointer()
            .when(active, |row| {
                row.bg(crate::ui::design::surface_2(cx))
                    .text_color(crate::ui::design::t1(cx))
            })
            .when(!active, |row| {
                row.hover(|r| r.bg(crate::ui::design::surface_2(cx)))
            })
            .child(
                Icon::new(IconName::File)
                    .size(crate::ui::design::icon_md())
                    .text_color(crate::ui::design::t3(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_head())
                    .text_color(if active {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .truncate()
                    .child(SharedString::from(name.to_string())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.services.update(cx, |state, cx| {
                    state.set_selected_source(project, rel.clone(), cx);
                    state.set_selected_env_file(project, &rel, picked.clone(), cx);
                });
            }))
            .into_any_element()
    }
}
