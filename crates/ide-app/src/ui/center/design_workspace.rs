use super::*;

struct FigmaDesignDialog {
    title: Entity<InputState>,
    source: Entity<InputState>,
}

impl FigmaDesignDialog {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            title: cx.new(|cx| InputState::new(window, cx).placeholder("Design name")),
            source: cx.new(|cx| InputState::new(window, cx).placeholder("Paste a Figma link")),
        }
    }

    fn collect(&self, cx: &App) -> Option<(String, String)> {
        let source = self.source.read(cx).value().trim().to_string();
        let valid = (source.starts_with("https://") || source.starts_with("http://"))
            && source.to_ascii_lowercase().contains("figma.com/");
        if !valid {
            return None;
        }
        let title = self.title.read(cx).value().trim().to_string();
        Some((
            if title.is_empty() {
                "Figma Design".to_string()
            } else {
                title
            },
            source,
        ))
    }
}

impl Render for FigmaDesignDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("NAME"),
                    )
                    .child(Input::new(&self.title)),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("FIGMA LINK"),
                    )
                    .child(Input::new(&self.source))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child("Paste a link to a Figma file, design, or prototype."),
                    ),
            )
    }
}

impl CenterArea {
    pub(super) fn design_agent_indicator(
        &mut self,
        agent: &AgentRecord,
        design_id: Uuid,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let short_id = agent_id
            .simple()
            .to_string()
            .chars()
            .take(6)
            .collect::<String>();
        let tooltip = SharedString::from(format!("{} — open", agent.title));
        crate::ui::design::indicator::subline_link(
            ("design-linked-agent", design_id.as_u128() as u64),
            IconName::Bot,
            SharedString::from(format!("Agent {short_id}")),
            crate::ui::agent_status_style::implement_status_color(agent.status, cx),
            cx,
        )
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_agent(agent_id, window, cx);
        }))
        .into_any_element()
    }

    pub(super) fn design_pull_request(
        &mut self,
        agent: Option<&AgentRecord>,
        cx: &mut Context<Self>,
    ) -> Option<crate::ui::git::git_panel::BranchPullRequest> {
        let agent = agent?;
        if !self.agent_ship_pr_targets.contains_key(&agent.id) {
            if let (Some(repo_path), Some(branch)) = (
                agent.ship_pr_repo_path.clone(),
                agent.ship_pr_branch.clone(),
            ) {
                self.track_agent_ship_pr_branch(agent.id, repo_path, branch, cx);
            }
        }
        self.sync_agent_ship_pull_request(agent.id, cx);
        self.agent_ship_prs.get(&agent.id).cloned()
    }

    pub(crate) fn open_figma_design_dialog(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = cx.new(|cx| FigmaDesignDialog::new(window, cx));
        let center = cx.entity().clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let content = editor.clone();
            let save_editor = editor.clone();
            let save_center = center.clone();
            dialog
                .w(px(520.))
                .title(SharedString::from("Add Figma design"))
                .child(content)
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let center = save_center.clone();
                    vec![
                        style::dialog_neutral_button("add-figma-design-cancel", "Cancel", cx)
                            .custom(style::dialog_neutral_variant(cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("add-figma-design-save", "Add", cx).on_click(
                            move |_, window, cx| {
                                let Some((title, source)) = editor.read(cx).collect(cx) else {
                                    return;
                                };
                                center.update(cx, |center, cx| {
                                    let result = center.designs.update(cx, |designs, cx| {
                                        designs.create_figma_design_reference(
                                            project, title, source, cx,
                                        )
                                    });
                                    match result {
                                        Ok(reference) => {
                                            center.open_figma_design(project, reference.id, cx);
                                            window.close_dialog(cx);
                                        }
                                        Err(error) => {
                                            eprintln!("failed to add Figma design: {error:#}");
                                        }
                                    }
                                });
                            },
                        ),
                    ]
                })
        });
    }

    fn open_figma_design(
        &mut self,
        project: ProjectId,
        reference_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if self.defer_studio_navigation(
            move |this, cx| this.open_figma_design(project, reference_id, cx),
            cx,
        ) {
            return;
        }
        self.close_design_compare(cx);
        self.studio = None;
        self.studio_system_library = None;
        self.figma_open_design = Some((project, reference_id));
        self.set_view_mode(CenterMode::Design, cx);
        cx.notify();
    }

    fn confirm_delete_figma_design(
        &mut self,
        project: ProjectId,
        reference_id: Uuid,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let center = cx.entity().clone();
        crate::ui::confirm::ConfirmDialog::new(
            "Delete design?",
            "This design link will be removed from Choro. The original Figma file will not be changed.",
        )
        .icon(IconName::Delete)
        .detail(title)
        .confirm_label("Delete")
        .confirm_id("delete-figma-design-confirm")
        .on_confirm(move |_, cx| {
            center.update(cx, |center, cx| {
                let result = center.designs.update(cx, |designs, cx| {
                    designs.delete_reference(project, reference_id, cx)
                });
                center.design_hub_error = result.err().map(|error| error.to_string());
                cx.notify();
            });
        })
        .open(window, cx);
    }

    fn render_figma_hub_card(
        &mut self,
        project: ProjectId,
        reference: ProjectReference,
        key: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let reference_id = reference.id;
        let title = SharedString::from(reference.title.clone());
        let delete_title = title.clone();
        let center = cx.entity().clone();
        let updated =
            super::time::branch_relative_time(reference.updated_at.min(i64::MAX as u64) as i64);
        let updated = if updated.is_empty() {
            "Recently updated".to_string()
        } else {
            format!("Updated {updated}")
        };

        style::design_hub_card(("figma-hub-card", key), cx)
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(124.))
                    .flex_none()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.7))
                    .bg(crate::ui::design::base(cx))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::designs_panel::design_kind_glyph(
                                ide_core::ProjectReferenceKind::Figma,
                                px(46.),
                                crate::ui::design::accent(cx),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .bottom(px(9.))
                            .rounded(crate::ui::design::r_xs())
                            .bg(crate::ui::design::surface(cx).opacity(0.92))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Figma"),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .justify_center()
                    .gap_1()
                    .px_3()
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(updated),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_figma_design(project, reference_id, cx);
            }))
            .context_menu(move |menu, _, _| {
                let center = center.clone();
                let delete_title = delete_title.clone();
                menu.item(
                    PopupMenuItem::new("Delete")
                        .icon(IconName::Delete)
                        .on_click(move |_, window, cx| {
                            center.update(cx, |center, cx| {
                                center.confirm_delete_figma_design(
                                    project,
                                    reference_id,
                                    delete_title.clone(),
                                    window,
                                    cx,
                                );
                            });
                        }),
                )
            })
            .into_any_element()
    }

    fn render_figma_design_section(
        &mut self,
        project: ProjectId,
        reference_id: Uuid,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(reference) = self.designs.read(cx).reference(project, reference_id) else {
            self.figma_open_design = None;
            return self.render_design_hub(project, cx);
        };
        let title = reference.title.clone();
        let source = reference.source.clone();
        let back_to_designs =
            style::header_icon_button("figma-back-to-designs", IconName::ArrowLeft, cx)
                .tooltip("Back to project designs")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_design_hub(cx);
                }));
        let open_source = style::secondary_button_compact("figma-open-source", "Open in Figma")
            .icon(Icon::empty().path("icons/asset-figma.svg"))
            .on_click(move |_, _, _| {
                crate::open_with::open_in(None, &source);
            });

        v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(crate::ui::design::base(cx))
            .child(
                crate::ui::design::header::workspace_bar(cx)
                    .child(back_to_designs)
                    .child(
                        crate::ui::design::header::title_col(cx)
                            .child(crate::ui::design::header::title(title, cx))
                            .child(
                                crate::ui::design::header::subline()
                                    .child(crate::ui::designs_panel::design_kind_glyph(
                                        ide_core::ProjectReferenceKind::Figma,
                                        crate::ui::design::icon_sm(),
                                        crate::ui::design::accent(cx),
                                    ))
                                    .child(crate::ui::design::header::subtitle("Figma", cx)),
                            ),
                    )
                    .child(crate::ui::design::header::actions().child(open_source)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .h_full()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .bg(crate::ui::design::base(cx))
                    .child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .text_color(crate::ui::design::t3(cx))
                            .child(logo_spinner(
                                18.,
                                "figma-design-loading",
                                0,
                                crate::ui::design::t3(cx),
                            ))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .child("Loading Figma design…"),
                            ),
                    )
                    .child({
                        let host = self.web_host.clone();
                        canvas(
                            move |bounds, window, cx| {
                                host.update(cx, |host, _| host.place(bounds, window));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0()
                    }),
            )
            .into_any_element()
    }

    fn render_design_hub(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|candidate| candidate.id == project)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Project".to_string());
        let mut figma_designs = self.designs.read(cx).design_hub_references(project);
        figma_designs.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.title.cmp(&right.title))
        });
        let studio_designs = self.studio_designs(project);
        let design_count = figma_designs.len() + studio_designs.len();
        let subtitle = match design_count {
            0 => format!("{project_name} · No designs yet"),
            1 => format!("{project_name} · 1 design"),
            count => format!("{project_name} · {count} designs"),
        };
        let mut cards = studio_designs
            .into_iter()
            .enumerate()
            .map(|(index, design)| self.render_studio_hub_card(project, design, index, cx))
            .collect::<Vec<_>>();
        let native_count = cards.len();
        cards.extend(
            figma_designs
                .into_iter()
                .enumerate()
                .map(|(key, reference)| {
                    self.render_figma_hub_card(project, reference, native_count + key, cx)
                }),
        );
        let new_design =
            self.new_design_dropdown("design-hub-new-design", "New Design", project, cx);
        v_flex()
            .size_full()
            .min_h(px(0.))
            .bg(crate::ui::design::base(cx))
            .child(
                crate::ui::design::header::workspace_bar(cx)
                    .child(
                        crate::ui::design::header::title_col(cx)
                            .child(crate::ui::design::header::title("Designs", cx))
                            .child(
                                crate::ui::design::header::subline()
                                    .child(crate::ui::design::header::subtitle(subtitle, cx)),
                            ),
                    )
                    .child(
                        crate::ui::design::header::actions()
                            .child(self.studio_trash_menu(project, cx))
                            .child(
                                style::ghost_button_compact("design-hub-systems", "Design systems")
                                    .icon(IconName::Palette)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_system_library(project, cx)
                                    })),
                            )
                            .child(new_design),
                    ),
            )
            .when_some(self.design_hub_error.clone(), |hub, error| {
                hub.child(
                    div()
                        .mx_5()
                        .mb_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.24))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(design_count == 0, |hub| {
                hub.child(
                    style::empty_state(
                        crate::ui::design::design_icon(),
                        "No designs yet",
                        "Create the first design for this project.",
                        cx,
                    )
                    .child(self.new_design_dropdown(
                        "design-hub-empty-new-design",
                        "New Design",
                        project,
                        cx,
                    )),
                )
            })
            .when(!cards.is_empty(), |hub| {
                hub.child(
                    div()
                        .id("design-hub-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .p_5()
                        .child(
                            h_flex()
                                .w_full()
                                .items_start()
                                .gap_4()
                                .flex_wrap()
                                .children(cards),
                        ),
                )
            })
            .into_any_element()
    }

    pub(super) fn show_design_hub(&mut self, cx: &mut Context<Self>) {
        if self.defer_studio_navigation(|this, cx| this.show_design_hub(cx), cx) {
            return;
        }
        self.close_design_compare(cx);
        if let Some(project) = self.studio.as_ref().map(|s| s.project) {
            self.refresh_studio_catalog(project, cx);
        }
        self.studio = None;
        self.studio_system_library = None;
        self.figma_open_design = None;
        self.set_view_mode(CenterMode::Design, cx);
        cx.notify();
    }
    pub(super) fn set_design_compare_open(
        &mut self,
        project: ProjectId,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        if !open {
            self.close_design_compare(cx);
            return;
        }
        self.design_compare_open = true;
        self.project_preview_panel_ratio = 0.5;
        let ui = self.project_preview_ui.entry(project).or_default();
        ui.open = true;
        ui.status = Some("Compare · choose the live result to review".to_string());
        self.project_preview_inspecting = None;
        self.reconcile_project_preview_for_selected_agent(project, cx);
        cx.notify();
    }
    pub(super) fn close_design_compare(&mut self, cx: &mut Context<Self>) {
        if !self.design_compare_open {
            return;
        }
        self.design_compare_open = false;
        self.project_preview_inspecting = None;
        if let Some(project) = self.studio.as_ref().map(|s| s.project) {
            let ui = self.project_preview_ui.entry(project).or_default();
            ui.open = false;
            ui.status = None;
        }
        self.compare_web_host.update(cx, |host, _| {
            let _ = host.set_project_preview_inspecting(false);
        });
        cx.notify();
    }
    fn new_design_dropdown(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let center = cx.entity().clone();
        style::primary_button_compact(id, label, cx)
            .icon(IconName::Plus)
            .dropdown_menu(move |menu, window, _| {
                let studio = center.clone();
                let figma = center.clone();
                menu.item(
                    PopupMenuItem::new("New Studio design")
                        .icon(crate::ui::design::design_icon())
                        .on_click(window.listener_for(&studio, move |this, _, window, cx| {
                            this.create_studio_from_hub(project, window, cx)
                        })),
                )
                .item(
                    PopupMenuItem::new("Add Figma link")
                        .icon(IconName::Globe)
                        .on_click(window.listener_for(&figma, move |this, _, window, cx| {
                            this.open_figma_design_dialog(project, window, cx)
                        })),
                )
            })
    }
    pub(super) fn render_design_section(
        &mut self,
        project: ProjectId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if let Some(id) = self
            .figma_open_design
            .and_then(|(p, id)| (p == project).then_some(id))
        {
            return self.render_figma_design_section(project, id, cx);
        }
        self.render_design_hub(project, cx)
    }
}
pub(super) fn short_design_chip_label(name: &str) -> String {
    let mut chars = name.chars();
    let prefix = chars.by_ref().take(10).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}
