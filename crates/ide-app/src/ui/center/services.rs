use super::*;

use ide_core::{DetectedService, ServiceCategory, SubAppServices};

use crate::state::ServicesMode;

impl CenterArea {
    /// The Services center: renders the source chosen in the right sidebar, in
    /// the chosen mode (its detected services, or its env files). Detection and
    /// env reads are read-only; env values are masked until you reveal them.
    pub(super) fn render_services_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let root = self
            .workspace
            .read(cx)
            .active_project()
            .map(|p| p.path.clone());

        if let Some(root) = root.clone() {
            self.services
                .update(cx, |state, cx| state.ensure_scanned(project, root, cx));
        }

        let sub_apps = self
            .services
            .read(cx)
            .services_for(project)
            .cloned()
            .unwrap_or_default();
        let scanning = self.services.read(cx).is_scanning(project);
        let mode = self.services.read(cx).mode(project);
        let effective = self
            .services
            .read(cx)
            .selected_source(project)
            .cloned()
            .or_else(|| sub_apps.first().map(|s| s.rel_path.clone()));

        if sub_apps.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(crate::ui::design::services_empty_gap())
                .when(scanning, |col| {
                    col.child(logo_spinner(
                        crate::ui::design::SERVICES_SPINNER_SIZE,
                        "services-scan",
                        project.0.as_u128() as usize,
                        crate::ui::design::surface_2(cx),
                    ))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Scanning the project…"),
                    )
                })
                .when(!scanning, |col| {
                    col.child(
                        v_flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(160.))
                                    .h(px(110.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        crate::ui::illustrations::illustration(
                                            crate::ui::illustrations::Illustration::Services,
                                            cx,
                                        )
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_title())
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child("No services detected"),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("Nothing recognizable in this project's package.json, deploy configs, or platform folders yet."),
                                    ),
                            ),
                        )
                })
                .into_any_element();
        }

        let Some(sub) = effective
            .as_ref()
            .and_then(|rel| sub_apps.iter().find(|s| &s.rel_path == rel))
            .cloned()
        else {
            return div().size_full().into_any_element();
        };

        let root = root.unwrap_or_default();
        let body = match mode {
            ServicesMode::Services => self.render_services_grid(&sub, cx),
            ServicesMode::Env => self.render_services_env(project, &sub, &root, cx),
        };

        v_flex()
            .size_full()
            .child(self.render_services_header(project, &sub, mode, cx))
            .child(
                v_flex()
                    .id("services-center-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .items_center()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            // Match the chat column: use the available center-pane
                            // width, then stay centered once the readable cap is hit.
                            .max_w(crate::ui::design::agent_chat_content_max_w())
                            .mx_auto()
                            .px(crate::ui::design::agent_chat_gutter_x())
                            .py(crate::ui::design::center_column_pad_y())
                            .child(body),
                    ),
            )
            .into_any_element()
    }

    fn render_services_header(
        &self,
        project: ProjectId,
        sub: &SubAppServices,
        mode: ServicesMode,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let subtitle = match mode {
            ServicesMode::Services => format!(
                "{} service{}",
                sub.services.len(),
                if sub.services.len() == 1 { "" } else { "s" }
            ),
            ServicesMode::Env => format!(
                "{} env file{}",
                sub.env_files.len(),
                if sub.env_files.len() == 1 { "" } else { "s" }
            ),
        };
        let reveal = self.services_reveal;
        use crate::ui::design::header;
        header::bar(cx)
            .child(
                header::title_col(cx)
                    .child(
                        h_flex()
                            .items_center()
                            .gap(crate::ui::design::services_header_title_gap())
                            .child(header::title(SharedString::from(sub.name.clone()), cx))
                            .when(sub.rel_path != ".", |row| {
                                row.child(header::title_meta(
                                    SharedString::from(sub.rel_path.clone()),
                                    cx,
                                ))
                            }),
                    )
                    .child(header::subtitle(SharedString::from(subtitle), cx)),
            )
            .child(
                header::actions()
                    .when(
                        mode == ServicesMode::Env && !sub.env_files.is_empty(),
                        |row| {
                            row.child(
                                style::ghost_button_compact(
                                    "services-env-reveal",
                                    if reveal {
                                        "Hide values"
                                    } else {
                                        "Reveal values"
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.services_reveal = !this.services_reveal;
                                        cx.notify();
                                    },
                                )),
                            )
                        },
                    )
                    .child(
                        style::ghost_button_compact("services-rescan", "Rescan").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.services
                                    .update(cx, |state, cx| state.invalidate(project, cx));
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_services_grid(
        &self,
        sub: &SubAppServices,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Services arrive pre-sorted by category, so group by walking runs of the
        // same category into their own labelled section.
        let mut groups: Vec<(ServiceCategory, Vec<&DetectedService>)> = Vec::new();
        for svc in &sub.services {
            match groups.last_mut() {
                Some((cat, items)) if *cat == svc.category => items.push(svc),
                _ => groups.push((svc.category, vec![svc])),
            }
        }

        v_flex()
            .w_full()
            .gap(crate::ui::design::services_group_gap())
            .children(groups.into_iter().map(|(category, items)| {
                style::chat_card(cx)
                    .child(
                        style::chat_card_head(cx)
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Server,
                                crate::ui::design::t3(cx),
                                crate::ui::design::icon_md(),
                            ))
                            .child(SharedString::from(category.label().to_string()))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(format!("{}", items.len())),
                            ),
                    )
                    .children(
                        items
                            .into_iter()
                            .enumerate()
                            .map(|(index, service)| render_service_row(service, index, cx)),
                    )
            }))
            .into_any_element()
    }

    fn render_services_env(
        &mut self,
        project: ProjectId,
        sub: &SubAppServices,
        root: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if sub.env_files.is_empty() {
            return style::empty_state(
                IconName::File,
                "No env files here",
                "This source has no .env files to show.",
                cx,
            )
            .into_any_element();
        }

        let dir = if sub.rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(&sub.rel_path)
        };
        let names = sub.env_files.clone();
        let rel = sub.rel_path.clone();
        let files = self.services.update(cx, |state, _cx| {
            state.env_files(project, &rel, dir.clone(), &names)
        });

        let selected_name = self
            .services
            .read(cx)
            .selected_env_file(project, &rel)
            .cloned()
            .filter(|n| names.contains(n))
            .unwrap_or_else(|| names.first().cloned().unwrap_or_default());
        let selected_file = files.iter().find(|f| f.name == selected_name).cloned();

        let body: gpui::AnyElement = match selected_file {
            Some(file) if !file.entries.is_empty() => {
                let path = dir.join(&file.name);
                let reveal = self.services_reveal;
                let file_name = file.name.clone();
                let (badge, badge_color) = env_badge(&file_name, cx);
                let rows: Vec<gpui::AnyElement> = file
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(ix, entry)| self.render_env_row(&path, entry, reveal, ix, cx))
                    .collect();
                style::chat_card(cx)
                    .child(
                        style::chat_card_head(cx)
                            .child(
                                gpui_component::Icon::new(IconName::File)
                                    .size(crate::ui::design::icon_md())
                                    .text_color(crate::ui::design::t3(cx)),
                            )
                            .child(
                                div()
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .child(SharedString::from(file_name)),
                            )
                            .when_some(badge, |header, badge| {
                                header.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(badge_color)
                                        .child(SharedString::from(badge)),
                                )
                            }),
                    )
                    .children(rows)
                    .into_any_element()
            }
            _ => style::chat_card(cx)
                .child(
                    style::chat_card_head(cx)
                        .child(
                            gpui_component::Icon::new(IconName::File)
                                .size(crate::ui::design::icon_md())
                                .text_color(crate::ui::design::t3(cx)),
                        )
                        .child(SharedString::from(selected_name)),
                )
                .child(
                    style::chat_card_row(cx)
                        .text_color(crate::ui::design::t3(cx))
                        .child("No variables in this file."),
                )
                .into_any_element(),
        };

        body
    }

    fn render_env_row(
        &self,
        path: &std::path::Path,
        entry: &ide_core::EnvEntry,
        reveal: bool,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let editing = self
            .services_env_edit
            .as_ref()
            .filter(|e| e.path == path && e.key == entry.key);

        let row = style::chat_card_row(cx)
            .when(ix > 0, |r| {
                r.border_t_1().border_color(crate::ui::design::line(cx))
            })
            .child(
                div()
                    .w(crate::ui::design::services_env_key_w())
                    .flex_none()
                    .font_family(crate::ui::design::FONT_MONO)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t2(cx))
                    .truncate()
                    .child(SharedString::from(entry.key.clone())),
            );

        if let Some(edit) = editing {
            row.child(div().flex_1().min_w(px(0.)).child(Input::new(&edit.input)))
                .child(
                    style::primary_button_compact("env-save", "Save", cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.save_env_edit(cx);
                        },
                    )),
                )
                .child(
                    style::ghost_button_compact("env-cancel", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.cancel_env_edit(cx);
                        },
                    )),
                )
                .into_any_element()
        } else {
            let shown = if reveal {
                entry.value.clone()
            } else {
                "•".repeat(entry.value.chars().count().clamp(6, 16))
            };
            let path = path.to_path_buf();
            let key = entry.key.clone();
            let value = entry.value.clone();
            row.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(crate::ui::design::FONT_MONO)
                    .truncate()
                    .text_color(if reveal {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(SharedString::from(shown)),
            )
            .child(
                style::ghost_button_compact(
                    SharedString::from(format!("env-edit-{}", entry.key)),
                    "Edit",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_env_edit(path.clone(), key.clone(), value.clone(), window, cx);
                })),
            )
            .into_any_element()
        }
    }

    pub(super) fn begin_env_edit(
        &mut self,
        path: std::path::PathBuf,
        key: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        self.services_env_edit = Some(ServicesEnvEdit { path, key, input });
        cx.notify();
    }

    pub(super) fn save_env_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.services_env_edit.take() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        if let Err(error) = ide_core::set_env_value(&edit.path, &edit.key, &value) {
            eprintln!("services: failed to write env value: {error}");
        }
        self.services
            .update(cx, |state, cx| state.clear_env_cache(cx));
        cx.notify();
    }

    pub(super) fn cancel_env_edit(&mut self, cx: &mut Context<Self>) {
        if self.services_env_edit.is_some() {
            self.services_env_edit = None;
            cx.notify();
        }
    }
}

/// The env value currently being edited inline.
pub(super) struct ServicesEnvEdit {
    pub path: std::path::PathBuf,
    pub key: String,
    pub input: gpui::Entity<InputState>,
}

fn env_badge(name: &str, cx: &App) -> (Option<String>, gpui::Hsla) {
    if name.ends_with(".production") {
        (Some("prod".into()), crate::ui::design::rose(cx))
    } else if name.ends_with(".development") {
        (Some("dev".into()), crate::ui::design::sky(cx))
    } else if name.ends_with(".local") {
        (Some("local".into()), crate::ui::design::t3(cx))
    } else if name.ends_with(".example") {
        (Some("template".into()), crate::ui::design::t3(cx))
    } else {
        (None, crate::ui::design::t3(cx))
    }
}

fn render_service_row(
    svc: &DetectedService,
    index: usize,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let dash_url = svc.dashboard_url.clone().or_else(|| svc.docs_url.clone());
    let evidence = svc.evidence.first().cloned();
    v_flex()
        .w_full()
        .when(index > 0, |row| {
            row.border_t_1().border_color(crate::ui::design::line(cx))
        })
        .child(
            style::chat_card_row(cx)
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::Server,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_md(),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(crate::ui::design::services_fact_gap())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_head())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t2(cx))
                                .truncate()
                                .child(SharedString::from(svc.name.clone())),
                        )
                        .when_some(evidence, |column, evidence| {
                            column.child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_color(crate::ui::design::t4(cx))
                                    .truncate()
                                    .child(SharedString::from(evidence)),
                            )
                        }),
                )
                .when_some(dash_url, |row, url| {
                    row.child(
                        div()
                            .id(SharedString::from(format!("svc-dash-{}", svc.id)))
                            .flex_none()
                            .h(crate::ui::design::control_h_xs())
                            .w(crate::ui::design::control_h_xs())
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|button| button.bg(crate::ui::design::surface_2(cx)))
                            .child(
                                gpui_component::Icon::new(IconName::ExternalLink)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t4(cx)),
                            )
                            .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url)),
                    )
                }),
        )
        .when(!svc.facts.is_empty(), |card| {
            card.child(
                v_flex().w_full().children(
                    svc.facts
                        .iter()
                        .enumerate()
                        .map(|(ix, fact)| render_service_fact(&svc.id, ix, fact, cx)),
                ),
            )
        })
        .into_any_element()
}

fn render_service_fact(
    service_id: &str,
    ix: usize,
    fact: &ide_core::ServiceFact,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let row = h_flex()
        .w_full()
        .items_center()
        .gap(crate::ui::design::services_fact_gap())
        .px(crate::ui::design::chat_card_row_pad_x())
        .pb(crate::ui::design::chat_card_row_pad_y())
        .when(!fact.label.is_empty(), |r| {
            r.child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t4(cx))
                    .child(SharedString::from(fact.label.clone())),
            )
        });
    match &fact.url {
        Some(url) => {
            let url = url.clone();
            let link = crate::ui::design::sky(cx);
            row.child(
                h_flex()
                    .id(SharedString::from(format!("svc-fact-{service_id}-{ix}")))
                    .flex_1()
                    .min_w(px(0.))
                    .items_center()
                    .gap(crate::ui::design::services_fact_gap())
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .hover(|r| r.bg(link.opacity(0.12)))
                    .child(
                        gpui_component::Icon::new(IconName::ExternalLink)
                            .size(crate::ui::design::icon_sm())
                            .text_color(link),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_color(link)
                            .truncate()
                            .child(SharedString::from(fact.value.clone())),
                    )
                    .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url)),
            )
            .into_any_element()
        }
        None => row
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_ui())
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_color(crate::ui::design::t3(cx))
                    .truncate()
                    .child(SharedString::from(fact.value.clone())),
            )
            .into_any_element(),
    }
}
