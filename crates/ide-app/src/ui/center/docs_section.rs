use super::*;

/// Native WebKit children sit above GPUI's popovers. Preserve the live editor
/// but hide its native view for the menu's lifetime, including dismissal by
/// Escape, outside click, or navigation away from the document.
fn suspend_doc_webview_for_menu(
    host: Entity<web_preview::WebPreviewHost>,
    cx: &mut Context<gpui_component::menu::PopupMenu>,
) {
    web_preview::suspend_for_menu(host, cx);
}

impl CenterArea {
    pub(super) fn render_docs_section(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(doc) = self.docs.read(cx).selected_doc(project) else {
            let docs = self.docs.clone();
            let center = cx.entity().downgrade();
            let action_error = self.doc_action_error.clone();
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
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
                                crate::ui::illustrations::Illustration::Docs,
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
                                .child("Create your first doc"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    "Capture plans, decisions, and project knowledge in one place.",
                                ),
                        ),
                )
                .child(
                    style::primary_button("empty-create-doc", "New Doc", cx)
                        .icon(IconName::Plus)
                        .dropdown_caret(true)
                        .dropdown_menu(move |menu, window, cx| {
                            crate::ui::doc_templates::doc_template_menu(
                                menu,
                                docs.clone(),
                                center.clone(),
                                project,
                                window,
                                cx,
                            )
                        }),
                )
                .when_some(action_error, |view, error| {
                    view.child(
                        div()
                            .max_w(px(460.))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(error),
                    )
                })
                .into_any_element();
        };
        let path = doc.path.clone();
        let is_template = doc.is_template;
        let opened_document = match self
            .docs
            .update(cx, |docs, _| docs.web_document_for_path(&path))
        {
            Ok(document) => document,
            Err(error) => {
                return v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(crate::ui::design::rose(cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .child("Could not open doc"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .child(error.to_string()),
                    )
                    .into_any_element();
            }
        };
        let status_error = match self.docs.read(cx).save_status(&path, cx) {
            DocSaveStatus::Error(error) => Some(error),
            _ => self.doc_action_error.clone(),
        };
        let current_label = self.docs.read(cx).doc_label(project, &doc.relative_path);
        let current_doc_status = self.docs.read(cx).doc_status(project, &doc.relative_path);
        let label_options = self.docs.read(cx).labels_for_project(project);
        let label_input = self.doc_label_input(project, window, cx);
        let implementors = if is_template {
            Vec::new()
        } else {
            self.doc_implementors(project, &doc.relative_path, cx)
        };
        let active_implementor = implementors.first().cloned();
        let title_edit_input = self
            .doc_title_edit
            .as_ref()
            .filter(|edit| edit.project == project && edit.path == path)
            .map(|edit| edit.input.clone());
        let title_hovered = self.hovered_doc_title;
        let label_button_text = current_label.clone().unwrap_or_else(|| "Label".to_string());
        let label_button_accent = current_label
            .as_deref()
            .map(doc_label_accent)
            .unwrap_or_else(|| crate::ui::design::t3(cx));
        let label_picker = crate::ui::style::header_dropdown_button("doc-label-picker", cx)
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(crate::ui::design::indicator::dot(label_button_accent))
                    .child(
                        div()
                            .line_height(gpui::relative(1.))
                            .child(label_button_text),
                    ),
            )
            .dropdown_menu({
                let center = cx.entity().clone();
                let input = label_input.clone();
                let relative = doc.relative_path.clone();
                let current_label = current_label.clone();
                let label_options = label_options.clone();
                let web_host = self.web_host.clone();
                move |mut menu, window, cx| {
                    suspend_doc_webview_for_menu(web_host.clone(), cx);
                    let query = input.read(cx).value().to_string();
                    let clean_query = clean_doc_label(&query);
                    menu = menu.item(PopupMenuItem::element({
                        let input = input.clone();
                        move |_, _| {
                            div()
                                .w(px(220.))
                                .p_2()
                                .child(Input::new(&input).small().h(px(30.)))
                        }
                    }));
                    menu = menu.item(PopupMenuItem::separator());
                    menu = menu.item(
                        PopupMenuItem::new("No label")
                            .checked(current_label.is_none())
                            .on_click(window.listener_for(&center, {
                                let input = input.clone();
                                let relative = relative.clone();
                                move |this: &mut Self, _, window, cx| {
                                    if let Err(error) = this.docs.update(cx, |docs, cx| {
                                        docs.set_doc_label(project, &relative, None, cx)
                                    }) {
                                        eprintln!("failed to clear doc label: {error:#}");
                                    }
                                    input.update(cx, |input, cx| input.set_value("", window, cx));
                                }
                            })),
                    );
                    let query_lower = clean_query
                        .as_deref()
                        .map(str::to_ascii_lowercase)
                        .unwrap_or_default();
                    let matching = label_options
                        .iter()
                        .filter(|label| {
                            query_lower.is_empty()
                                || label.to_ascii_lowercase().contains(&query_lower)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    for label in matching {
                        let row_label = label.clone();
                        let selected = current_label
                            .as_deref()
                            .is_some_and(|current| current.eq_ignore_ascii_case(&label));
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| doc_label_menu_row(&row_label, cx))
                                .checked(selected)
                                .on_click(window.listener_for(&center, {
                                    let input = input.clone();
                                    let relative = relative.clone();
                                    move |this: &mut Self, _, window, cx| {
                                        if let Err(error) = this.docs.update(cx, |docs, cx| {
                                            docs.set_doc_label(
                                                project,
                                                &relative,
                                                Some(label.clone()),
                                                cx,
                                            )
                                        }) {
                                            eprintln!("failed to set doc label: {error:#}");
                                        }
                                        input.update(cx, |input, cx| {
                                            input.set_value("", window, cx)
                                        });
                                    }
                                })),
                        );
                    }
                    if let Some(label) = clean_query {
                        let exists = label_options
                            .iter()
                            .any(|existing| existing.eq_ignore_ascii_case(&label));
                        if !exists {
                            menu = menu.item(PopupMenuItem::separator());
                            menu = menu.item(
                                PopupMenuItem::new(format!("Create \"{label}\"")).on_click(
                                    window.listener_for(&center, {
                                        let input = input.clone();
                                        let relative = relative.clone();
                                        move |this: &mut Self, _, window, cx| {
                                            if let Err(error) = this.docs.update(cx, |docs, cx| {
                                                docs.set_doc_label(
                                                    project,
                                                    &relative,
                                                    Some(label.clone()),
                                                    cx,
                                                )
                                            }) {
                                                eprintln!("failed to create doc label: {error:#}");
                                            }
                                            input.update(cx, |input, cx| {
                                                input.set_value("", window, cx)
                                            });
                                        }
                                    }),
                                ),
                            );
                        }
                    }
                    menu
                }
            });
        let doc_status_accent = status_accent(current_doc_status, cx);
        let doc_status_picker = crate::ui::style::header_dropdown_button("doc-status-picker", cx)
            .child(crate::ui::design::indicator::status(
                current_doc_status.label(),
                doc_status_accent,
                cx,
            ))
            .dropdown_menu({
                let center = cx.entity().clone();
                let relative = doc.relative_path.clone();
                let web_host = self.web_host.clone();
                move |mut menu, window, cx| {
                    suspend_doc_webview_for_menu(web_host.clone(), cx);
                    for status in AgentStatus::ALL {
                        let relative = relative.clone();
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| status_menu_row(status, cx))
                                .checked(status == current_doc_status)
                                .on_click(window.listener_for(
                                    &center,
                                    move |this: &mut Self, _, _, cx| {
                                        this.doc_action_error = this
                                            .docs
                                            .update(cx, |docs, cx| {
                                                docs.set_doc_status(project, &relative, status, cx)
                                            })
                                            .err()
                                            .map(|error| {
                                                format!(
                                                    "Could not change document status: {error:#}"
                                                )
                                            });
                                        cx.notify();
                                    },
                                )),
                        );
                    }
                    menu
                }
            });
        let implementor_indicator = active_implementor.as_ref().map(|agent| {
            let agent_id = agent.id;
            let short_id = agent_id
                .simple()
                .to_string()
                .chars()
                .take(6)
                .collect::<String>();
            let label = SharedString::from(format!("Agent {short_id}"));
            let tooltip = SharedString::from(format!("{} — open", agent.title));
            let color = crate::ui::agent_status_style::implement_status_color(agent.status, cx);
            if implementors.len() == 1 {
                crate::ui::design::indicator::subline_link(
                    ("doc-linked-agent", agent_id.as_u128() as u64),
                    IconName::Bot,
                    label,
                    color,
                    cx,
                )
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_agent(agent_id, window, cx);
                }))
                .into_any_element()
            } else {
                let center = cx.entity().clone();
                let history = implementors.clone();
                let web_host = self.web_host.clone();
                crate::ui::style::header_dropdown_button(
                    ("doc-linked-agent-history", agent_id.as_u128() as u64),
                    cx,
                )
                .p_0()
                .child(crate::ui::design::indicator::subline_indicator(
                    IconName::Bot,
                    label,
                    color,
                    cx,
                ))
                .tooltip("Implementation agents")
                .dropdown_menu(move |mut menu, window, menu_cx| {
                    suspend_doc_webview_for_menu(web_host.clone(), menu_cx);
                    for (index, implementation) in history.iter().cloned().enumerate() {
                        let implementation_id = implementation.id;
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                implementation_agent_history_row(&implementation, index == 0, cx)
                            })
                            .checked(index == 0)
                            .on_click(window.listener_for(
                                &center,
                                move |this: &mut Self, _, window, cx| {
                                    this.open_agent(implementation_id, window, cx);
                                },
                            )),
                        );
                    }
                    menu
                })
                .into_any_element()
            }
        });
        let pocketcomet_indicator = opened_document
            .origin
            .as_ref()
            .and_then(pocketcomet_document_url)
            .map(|url| {
                let tooltip = SharedString::from(format!("Open “{}” in PocketComet", doc.title));
                crate::ui::design::indicator::subline_link_with_icon(
                    "doc-pocketcomet-origin",
                    crate::ui::design::indicator::pocketcomet_icon(
                        crate::ui::design::accent(cx),
                        crate::ui::design::icon_ind(),
                    ),
                    SharedString::from("PocketComet"),
                    cx,
                )
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url))
                .into_any_element()
            });
        let mut design_indicators = Vec::new();
        if !is_template {
            for design in self.studio_designs_for_doc(project, &doc.relative_path) {
                design_indicators.push(self.render_linked_studio_indicator("doc-linked-studio", project, design, cx));
            }
        }
        let implement_action = (!is_template).then(|| {
            let label = if active_implementor.is_some() {
                "Reimplement"
            } else {
                "Implement"
            };
            div()
                .relative()
                .flex_none()
                .child(
                    crate::ui::style::implement_button("implement-doc", label, cx).on_click({
                        let relative_doc_path = doc.relative_path.clone();
                        let doc_title = doc.title.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.open_implementation_agent_for_doc(
                                project,
                                relative_doc_path.clone(),
                                doc_title.clone(),
                                window,
                                cx,
                            );
                        })
                    }),
                )
                .child(crate::ui::onboarding::target_marker(
                    crate::ui::onboarding::SpotlightTarget::DocImplement,
                    cx,
                ))
                .into_any_element()
        });
        let design_action = (!is_template).then(|| {
            let creating = self.studio_creating.contains(&project);
            let title = doc.title.clone();
            let relative_doc_path = doc.relative_path.clone();
            style::accent_button_compact("design-doc", if creating { "Creating…" } else { "Design" }, cx)
                .icon(crate::ui::design::design_icon()).disabled(creating)
                .tooltip("Create a Studio design from this document")
                .on_click(cx.listener(move |this, _, _, cx| this.create_studio_for_doc(project, title.clone(), relative_doc_path.clone(), cx)))
        });
        let assistant_open = !is_template && self.docs_focus_mode == DocsFocusMode::Assistant;
        let assistant_toggle = (!is_template).then(|| {
            style::accent_button_compact(
                "open-doc-assistant",
                if assistant_open {
                    "Hide Assistant"
                } else {
                    "Assistant"
                },
                cx,
            )
            .icon(IconName::Asterisk)
            .selected(assistant_open)
            .on_click(cx.listener(|this, _, _, cx| {
                this.set_docs_assistant_open(this.docs_focus_mode != DocsFocusMode::Assistant, cx);
                cx.notify();
            }))
        });
        let doc_web_host = self.web_host.clone();
        let doc_editor_ready = self.web_host.read(cx).doc_editor_ready_for(&path);
        let editor_shell =
            v_flex()
                .size_full()
                .bg(crate::ui::design::base(cx))
                .child(
                    crate::ui::design::header::bar(cx)
                        .child(
                            crate::ui::design::header::title_col(cx)
                                .child(match title_edit_input {
                                    Some(input) => {
                                        let save_path = path.clone();
                                        h_flex()
                                            .w_full()
                                            .max_w(crate::ui::design::element_title_edit_max_w())
                                            .gap_1()
                                            .items_center()
                                            .child(
                                                div().flex_1().min_w(px(0.)).child(
                                                    Input::new(&input)
                                                        .small()
                                                        .h(crate::ui::design::control_h()),
                                                ),
                                            )
                                            .child(
                                                style::header_icon_button(
                                                    "cancel-doc-title-edit",
                                                    IconName::Close,
                                                    cx,
                                                )
                                                .h(crate::ui::design::control_h_xs())
                                                .tooltip("Cancel")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.doc_title_edit = None;
                                                    cx.notify();
                                                })),
                                            )
                                            .child(
                                                style::header_icon_button(
                                                    "save-doc-title-edit",
                                                    IconName::Check,
                                                    cx,
                                                )
                                                .h(crate::ui::design::control_h_xs())
                                                .tooltip("Save")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let Some(edit) = this.doc_title_edit.as_ref()
                                                    else {
                                                        return;
                                                    };
                                                    let title = edit.input.read(cx).value();
                                                    let previous_relative = this
                                                        .docs
                                                        .read(cx)
                                                        .relative_path_for(project, &save_path, cx);
                                                    let result =
                                                        this.docs.update(cx, |docs, cx| {
                                                            docs.rename_doc_to_title(
                                                                project, &save_path, &title, cx,
                                                            )
                                                        });
                                                    match result {
                                                        Ok(next_path) => {
                                                            if let (
                                                                Some(previous_relative),
                                                                Some(next_relative),
                                                            ) = (
                                                                previous_relative,
                                                                this.docs
                                                                    .read(cx)
                                                                    .relative_path_for(
                                                                        project, &next_path, cx,
                                                                    ),
                                                            ) {
                                                                this.agents.update(
                                                                    cx,
                                                                    |agents, cx| {
                                                                        agents.move_doc_reference(
                                                                            project,
                                                                            &previous_relative,
                                                                            &next_relative,
                                                                            cx,
                                                                        );
                                                                    },
                                                                );
                                                                this.doc_assistants.update(
                                                                    cx,
                                                                    |assistants, cx| {
                                                                        assistants
                                                                            .move_doc_reference(
                                                                                project,
                                                                                &previous_relative,
                                                                                next_relative
                                                                                    .clone(),
                                                                                cx,
                                                                            );
                                                                    },
                                                                );
                                                                this.move_studio_doc_links(project, previous_relative.clone(), next_relative.clone(), cx);
                                                            }
                                                        }
                                                        Err(error) => {
                                                            eprintln!(
                                                                "failed to rename doc: {error:#}"
                                                            );
                                                        }
                                                    }
                                                    this.doc_title_edit = None;
                                                    cx.notify();
                                                })),
                                            )
                                            .into_any_element()
                                    }
                                    None => crate::ui::design::header::title_row()
                                        .id("doc-title-hover")
                                        .on_hover(cx.listener(|this, hovered, _, cx| {
                                            this.hovered_doc_title = *hovered;
                                            cx.notify();
                                        }))
                                        .child(
                                            crate::ui::design::header::title(
                                                SharedString::from(doc.title.clone()),
                                                cx,
                                            )
                                            .max_w(crate::ui::design::element_title_edit_max_w())
                                            .min_w(px(0.)),
                                        )
                                        .when(title_hovered, |row| {
                                            row.child(
                                                crate::ui::style::header_svg_button(
                                                    "rename-doc-title",
                                                    svg()
                                                        .path("agent-icons/pencil.svg")
                                                        .size(crate::ui::design::icon_ind())
                                                        .text_color(crate::ui::design::t3(cx)),
                                                    cx,
                                                )
                                                .h(crate::ui::design::control_h_xs())
                                                .w(crate::ui::design::control_h_xs())
                                                .tooltip("Rename")
                                                .on_click({
                                                    let path = path.clone();
                                                    let title = doc.title.clone();
                                                    cx.listener(move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        let input = cx.new(|cx| {
                                                            InputState::new(window, cx)
                                                                .default_value(title.clone())
                                                        });
                                                        input.update(cx, |input, cx| {
                                                            input.focus(window, cx);
                                                        });
                                                        this.doc_title_edit = Some(DocTitleEdit {
                                                            project,
                                                            path: path.clone(),
                                                            input,
                                                        });
                                                        cx.notify();
                                                    })
                                                }),
                                            )
                                        })
                                        .into_any_element(),
                                })
                                .when(is_template, |column| {
                                    column.child(
                                        crate::ui::design::header::subline().child(
                                            h_flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(
                                                    Icon::new(IconName::GalleryVerticalEnd)
                                                        .size(crate::ui::design::icon_sm())
                                                        .text_color(crate::ui::design::accent(cx)),
                                                )
                                                .child("Project template"),
                                        ),
                                    )
                                })
                                .when(
                                    !design_indicators.is_empty()
                                        || implementor_indicator.is_some()
                                        || pocketcomet_indicator.is_some(),
                                    |column| {
                                        column.child(
                                            crate::ui::design::header::subline()
                                                .children(pocketcomet_indicator)
                                                .children(design_indicators)
                                                .children(implementor_indicator),
                                        )
                                    },
                                ),
                        )
                        .child(
                            crate::ui::design::header::actions()
                                .when(is_template, |actions| {
                                    let template_path = path.clone();
                                    actions.child(
                                        style::primary_button_compact(
                                            "create-doc-from-project-template",
                                            "Create Doc",
                                            cx,
                                        )
                                        .icon(IconName::Plus)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                let source =
                                                    crate::state::docs::DocTemplateSource::Project(
                                                        template_path.clone(),
                                                    );
                                                this.create_doc_from_template(project, source, cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(!is_template, |actions| actions.child(doc_status_picker))
                                .when(!is_template, |actions| actions.child(label_picker))
                                .children(design_action)
                                .children(implement_action)
                                .children(assistant_toggle),
                        ),
                )
                .when_some(status_error, |view, error| {
                    view.child(
                        h_flex()
                            .w_full()
                            .px_3()
                            .py_2()
                            .gap_2()
                            .items_center()
                            .border_b_1()
                            .border_color(crate::ui::design::rose(cx).opacity(0.2))
                            .bg(crate::ui::design::rose(cx).opacity(0.08))
                            .child(
                                gpui_component::Icon::new(IconName::TriangleAlert)
                                    .size(crate::ui::design::icon())
                                    .text_color(crate::ui::design::rose(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .truncate()
                                    .child(error),
                            ),
                    )
                })
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h(px(0.))
                        .child(
                            div()
                                .relative()
                                .size_full()
                                .bg(crate::ui::design::base(cx))
                                .when(!doc_editor_ready, |view| {
                                    view.child(
                                        v_flex()
                                            .size_full()
                                            .items_center()
                                            .justify_center()
                                            .gap_2()
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(logo_spinner(
                                                18.,
                                                "doc-editor-loading",
                                                0,
                                                crate::ui::design::t3(cx),
                                            ))
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .child("Loading document editor…"),
                                            ),
                                    )
                                })
                                .child(
                                    canvas(
                                        move |bounds, window, cx| {
                                            doc_web_host
                                                .update(cx, |host, _| host.place(bounds, window));
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                ),
                        )
                        // The doc itself, without its header, toolbar, or Assistant.
                        // The tour's "read this" step lit the whole work area, which
                        // put a dozen buttons beside a card asking you to just read.
                        .child(crate::ui::onboarding::target_marker(
                            crate::ui::onboarding::SpotlightTarget::DocBody,
                            cx,
                        )),
                )
                .into_any_element();
        let editor_panel = h_flex()
            .size_full()
            .min_h(px(0.))
            .child(div().flex_1().min_w(px(0.)).size_full().child(editor_shell))
            .into_any_element();
        let panel_width = self.doc_assistant_panel_width;
        let assistant_panel =
            assistant_open.then(|| self.render_selected_doc_assistant_panel(window, cx));
        div()
            .relative()
            .size_full()
            .child(
                h_flex()
                    .size_full()
                    .min_h(px(0.))
                    .child(div().flex_1().min_w(px(0.)).size_full().child(editor_panel))
                    .when_some(assistant_panel, |row, panel| {
                        row.child(
                            div()
                                .relative()
                                .flex_none()
                                .w(px(panel_width))
                                .h_full()
                                .border_l_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.34))
                                .bg(crate::ui::design::nav(cx))
                                .overflow_hidden()
                                .child(panel)
                                .child(self.doc_assistant_resize_handle(cx)),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn implementation_agent_history_row(
    agent: &AgentRecord,
    latest: bool,
    cx: &App,
) -> gpui::AnyElement {
    let color = crate::ui::agent_status_style::implement_status_color(agent.status, cx);
    let when = super::time::branch_relative_time(agent.created_at as i64);
    let meta = if when.is_empty() {
        format!("{} · {}", agent.model_short_label(), agent.status.label())
    } else {
        format!(
            "{} · {} · {when}",
            agent.model_short_label(),
            agent.status.label()
        )
    };
    h_flex()
        .w(px(280.))
        .min_w(px(0.))
        .items_center()
        .gap_2()
        .child(
            Icon::new(IconName::Bot)
                .size(crate::ui::design::icon_md())
                .text_color(color),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(1.))
                .child(
                    div()
                        .truncate()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t1(cx))
                        .child(agent.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(meta),
                ),
        )
        .when(latest, |row| {
            row.child(
                div()
                    .flex_none()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::accent(cx))
                    .child("Latest"),
            )
        })
        .into_any_element()
}

fn pocketcomet_document_url(origin: &crate::state::docs::ChoroDocumentOrigin) -> Option<String> {
    let crate::state::docs::ChoroDocumentOrigin::PocketComet {
        workspace_id,
        project_id,
        document_id,
    } = origin;
    let mut url = url::Url::parse("pocketcomet://document").ok()?;
    url.path_segments_mut().ok()?.push(document_id);
    url.query_pairs_mut()
        .append_pair("workspace_id", workspace_id)
        .append_pair("project_id", project_id);
    Some(url.into())
}

#[cfg(test)]
mod pocketcomet_document_link_tests {
    use super::*;

    #[test]
    fn document_link_encodes_the_pocketcomet_page_identity() {
        let origin = crate::state::docs::ChoroDocumentOrigin::PocketComet {
            workspace_id: "workspace one".into(),
            project_id: "project/one".into(),
            document_id: "document/one".into(),
        };

        let url = url::Url::parse(&pocketcomet_document_url(&origin).unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("document"));
        assert_eq!(url.path(), "/document%2Fone");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "workspace_id")
                .unwrap()
                .1,
            "workspace one"
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "project_id")
                .unwrap()
                .1,
            "project/one"
        );
    }
}
