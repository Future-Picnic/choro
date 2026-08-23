use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::{
    menu::{DropdownMenu as _, PopupMenuItem},
    Icon, IconName,
};

use ide_core::{
    local_store::{OrbitBuiltin, OrbitModuleId},
    ProjectId,
};

use crate::state::{OrbitState, Workspace};

/// Orbit's project navigator: a compact module switcher with locked built-ins
/// first, then the project's reusable custom views. All module content and
/// source/file navigation belongs to the center surface.
pub struct ServicesPanel {
    workspace: Entity<Workspace>,
    orbit: Entity<OrbitState>,
}

impl ServicesPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        orbit: Entity<OrbitState>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&orbit, |_, _, cx| cx.notify()).detach();
            Self { workspace, orbit }
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
        let Some((project, _root)) = self.active_project(cx) else {
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

        let orbit_state = self.orbit.read(cx);
        let selected = orbit_state.selected(project);
        let visible = orbit_state.visible_modules(project);
        let hidden_builtins = OrbitBuiltin::ALL
            .into_iter()
            .filter(|builtin| !orbit_state.enabled(project, OrbitModuleId::Builtin(*builtin)))
            .collect::<Vec<_>>();
        let available_custom = orbit_state
            .modules()
            .iter()
            .filter(|module| {
                !module.archived && !orbit_state.enabled(project, OrbitModuleId::Custom(module.id))
            })
            .map(|module| (module.id, module.name.clone()))
            .collect::<Vec<_>>();
        let loading = orbit_state.loading();
        let (visible_builtins, visible_custom): (Vec<_>, Vec<_>) = visible
            .into_iter()
            .partition(|module| matches!(module, OrbitModuleId::Builtin(_)));
        let orbit = self.orbit.clone();
        let builtins_count = visible_builtins.len();
        let custom_count = visible_custom.len();

        Self::v_flex()
            .size_full()
            .bg(crate::ui::design::nav(cx))
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(crate::ui::design::header::panel_identity(None, "Orbit", cx))
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::context_panel_action_button(
                            "orbit-add",
                            IconName::Plus,
                            "Add",
                            cx,
                        )
                        .tooltip("Add to Orbit")
                        .dropdown_menu(move |mut menu, _, _| {
                            menu = menu.item(PopupMenuItem::label("ADD TO ORBIT"));
                            for builtin in &hidden_builtins {
                                let module = OrbitModuleId::Builtin(*builtin);
                                let label = builtin.label();
                                let orbit = orbit.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _, cx| {
                                        orbit.update(cx, |orbit, cx| {
                                            orbit.set_enabled(project, module, true, cx)
                                        });
                                    },
                                ));
                            }
                            for (module_id, module_name) in &available_custom {
                                let orbit = orbit.clone();
                                let module_id = *module_id;
                                menu = menu.item(PopupMenuItem::new(module_name.clone()).on_click(
                                    move |_, _, cx| {
                                        orbit.update(cx, |orbit, cx| {
                                            orbit.set_enabled(
                                                project,
                                                OrbitModuleId::Custom(module_id),
                                                true,
                                                cx,
                                            )
                                        });
                                    },
                                ));
                            }
                            if hidden_builtins.is_empty() && available_custom.is_empty() {
                                menu = menu.item(
                                    PopupMenuItem::new("Everything available is already here")
                                        .disabled(true),
                                );
                            }
                            menu.item(PopupMenuItem::new("Open Settings → Orbit").on_click(
                                move |_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::actions::OpenOrbitSettings),
                                        cx,
                                    );
                                },
                            ))
                        }),
                    ),
            )
            .child(
                Self::v_flex()
                    .id("orbit-module-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px_1()
                    .pt(crate::ui::design::context_panel_title_pad_top())
                    .pb_3()
                    .when(loading, |list| {
                        list.child(
                            Self::h_flex()
                                .items_center()
                                .gap_2()
                                .px(crate::ui::design::context_panel_row_pad_x())
                                .py_3()
                                .child(
                                    Icon::new(IconName::LoaderCircle)
                                        .size(crate::ui::design::icon_md()),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Loading Orbit…"),
                                ),
                        )
                    })
                    .when(
                        visible_builtins.is_empty() && visible_custom.is_empty() && !loading,
                        |list| {
                            list.child(
                                div()
                                    .px(crate::ui::design::context_panel_row_pad_x())
                                    .py_3()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "Nothing is visible. Use Add to Orbit to restore a view.",
                                    ),
                            )
                        },
                    )
                    .child(self.render_lane("Built-ins", builtins_count, cx))
                    .children(
                        visible_builtins.iter().map(|module| {
                            self.render_orbit_module_row(project, *module, selected, cx)
                        }),
                    )
                    .child(self.render_lane("Your Orbit", custom_count, cx))
                    .children(
                        visible_custom.iter().map(|module| {
                            self.render_orbit_module_row(project, *module, selected, cx)
                        }),
                    )
                    .when(visible_custom.is_empty(), |list| {
                        list.child(
                            div()
                                .px(crate::ui::design::context_panel_row_pad_x())
                                .py_2()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t4(cx))
                                .child("Create an Orbit view in Settings, then add it here."),
                        )
                    }),
            )
    }
}

impl ServicesPanel {
    fn render_orbit_module_row(
        &self,
        project: ProjectId,
        module: OrbitModuleId,
        selected: Option<OrbitModuleId>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = Some(module) == selected;
        let (name, icon) = match module {
            OrbitModuleId::Builtin(builtin) => (
                builtin.label().to_string(),
                match builtin {
                    OrbitBuiltin::Environment => IconName::File,
                    OrbitBuiltin::Integrations => IconName::Network,
                },
            ),
            OrbitModuleId::Custom(id) => self
                .orbit
                .read(cx)
                .module(id)
                .map(|definition| (definition.name.clone(), IconName::Network))
                .unwrap_or_else(|| ("Module".into(), IconName::Network)),
        };
        let orbit = self.orbit.clone();
        let action_label = if matches!(module, OrbitModuleId::Builtin(_)) {
            "Hide from Orbit"
        } else {
            "Remove from Orbit"
        };
        let group_name = SharedString::from(format!("orbit-module-row-{}", module.storage_key()));

        Self::h_flex()
            .id(SharedString::from(format!(
                "orbit-module-row-{}",
                module.storage_key()
            )))
            .group(group_name.clone())
            .min_w(px(0.))
            .mx_1()
            .min_h(px(36.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .bg(if active {
                crate::ui::design::surface_2(cx).opacity(0.4)
            } else {
                gpui::transparent_black()
            })
            .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.56)))
            .child(
                Icon::new(icon)
                    .size(crate::ui::design::icon())
                    .text_color(if active {
                        crate::ui::design::t2(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_body())
                    .text_color(if active {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .truncate()
                    .child(SharedString::from(name)),
            )
            .child(
                div()
                    .flex_none()
                    .when(!active, |actions| {
                        actions
                            .invisible()
                            .group_hover(group_name, |actions| actions.visible())
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        crate::ui::style::header_icon_button(
                            SharedString::from(format!(
                                "orbit-module-actions-{}",
                                module.storage_key()
                            )),
                            IconName::Ellipsis,
                            cx,
                        )
                        .tooltip("Orbit view actions")
                        .dropdown_menu(move |menu, _, _| {
                            let orbit = orbit.clone();
                            menu.item(
                                PopupMenuItem::new(action_label)
                                    .icon(IconName::EyeOff)
                                    .on_click(move |_, _, cx| {
                                        orbit.update(cx, |orbit, cx| {
                                            orbit.set_enabled(project, module, false, cx)
                                        });
                                    }),
                            )
                        }),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.orbit
                    .update(cx, |orbit, cx| orbit.select(project, module, cx));
            }))
            .into_any_element()
    }

    fn render_lane(&self, label: &str, count: usize, cx: &App) -> gpui::AnyElement {
        Self::h_flex()
            .min_w(px(0.))
            .mx_1()
            .px_2()
            .pt(crate::ui::design::context_panel_lane_pad_top())
            .pb(crate::ui::design::context_panel_lane_pad_bottom())
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t4(cx))
                    .child(SharedString::from(label.to_uppercase())),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t4(cx).opacity(0.7))
                    .child(count.to_string()),
            )
            .into_any_element()
    }
}
