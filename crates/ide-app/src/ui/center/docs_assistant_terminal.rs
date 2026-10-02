#![allow(dead_code, reason = "retained document-assistant terminal UI")]

use super::*;

impl CenterArea {
    pub(super) fn open_doc_assistant_terminal(
        &mut self,
        project: ProjectId,
        project_path: PathBuf,
        relative_doc_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, relative_doc_path.clone(), cx)
        });
        let key = record.key();
        let existing = self
            .terminals
            .read(cx)
            .doc_assistant_session(project, &key)
            .map(|session| (session.id, session.exited));
        match existing {
            Some((_, false)) => {
                self.doc_assistant_errors.remove(&key);
                cx.notify();
                return;
            }
            Some((terminal_id, true)) => {
                self.terminals.update(cx, |terminals, cx| {
                    terminals.close(terminal_id, cx);
                });
            }
            None => {}
        }

        let resume_command = record.resume_command();
        let cli_session_id = if resume_command.is_some() {
            record.cli_session_id.clone()
        } else {
            None
        };
        let command = resume_command.unwrap_or_else(|| {
            let prompt = doc_assistant::system_prompt(&relative_doc_path);
            record.start_command(&prompt)
        });
        let spawned = self.terminals.update(cx, |terminals, cx| {
            terminals.spawn_doc_assistant(
                project,
                project_path,
                key.clone(),
                record.provider,
                record.title(),
                command,
                cli_session_id,
                cx,
            )
        });
        match spawned {
            Ok(_) => {
                self.doc_assistant_errors.remove(&key);
            }
            Err(error) => {
                self.doc_assistant_errors.insert(
                    key,
                    format!("Could not start assistant terminal: {error:#}"),
                );
            }
        }
        cx.notify();
    }

    pub(super) fn render_doc_assistant_terminal_panel(
        &mut self,
        project: ProjectId,
        doc: &WorkspaceDocEntry,
        key: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let record = self
            .doc_assistants
            .read(cx)
            .record_for(project, &doc.relative_path)
            .unwrap_or_else(|| DocAssistantRecord::new(project, doc.relative_path.clone()));
        let session = self
            .terminals
            .read(cx)
            .doc_assistant_session(project, key)
            .map(|session| (session.id, session.view.clone(), session.exited));
        let error = self.doc_assistant_errors.get(key).cloned();
        let can_resume = record.cli_session_id.is_some();
        let button_label = if can_resume { "Resume" } else { "Start" };
        let title = if session.as_ref().is_some_and(|(_, _, exited)| *exited) {
            "ASSISTANT TERMINAL · ENDED"
        } else {
            "ASSISTANT TERMINAL"
        };
        let provider = record.provider;
        let model = record.model;
        let effort = record.effort;
        let supported_efforts = model.efforts();
        let runtime_locked = can_resume || session.as_ref().is_some_and(|(_, _, exited)| !*exited);
        let center = cx.entity().clone();
        let relative_doc_path = doc.relative_path.clone();

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(crate::ui::design::line(cx).opacity(0.28))
            .bg(crate::ui::design::base(cx))
            .child(
                h_flex()
                    .h(px(36.))
                    .px_3()
                    .items_center()
                    .gap_2()
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        gpui_component::Icon::new(IconName::SquareTerminal)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child(title),
                    )
                    .child(
                        Button::new("doc-assistant-terminal-provider")
                            .ghost()
                            .xsmall()
                            .compact()
                            .label(provider.label())
                            .dropdown_caret(true)
                            .disabled(runtime_locked)
                            .dropdown_menu({
                                let center = center.clone();
                                let relative = relative_doc_path.clone();
                                move |menu, window, _| {
                                    menu.item(
                                        PopupMenuItem::new("Claude")
                                            .checked(provider == AgentKind::Claude)
                                            .on_click(window.listener_for(&center, {
                                                let relative = relative.clone();
                                                move |this: &mut Self, _, _, cx| {
                                                    this.doc_assistants.update(
                                                        cx,
                                                        |assistants, cx| {
                                                            assistants.update_runtime(
                                                                project,
                                                                &relative,
                                                                AgentKind::Claude,
                                                                AgentModel::default_for(
                                                                    AgentKind::Claude,
                                                                ),
                                                                effort,
                                                                cx,
                                                            )
                                                        },
                                                    );
                                                }
                                            })),
                                    )
                                    .item(
                                        PopupMenuItem::new("Codex")
                                            .checked(provider == AgentKind::Codex)
                                            .on_click(window.listener_for(&center, {
                                                let relative = relative.clone();
                                                move |this: &mut Self, _, _, cx| {
                                                    this.doc_assistants.update(
                                                        cx,
                                                        |assistants, cx| {
                                                            assistants.update_runtime(
                                                                project,
                                                                &relative,
                                                                AgentKind::Codex,
                                                                AgentModel::default_for(
                                                                    AgentKind::Codex,
                                                                ),
                                                                effort,
                                                                cx,
                                                            )
                                                        },
                                                    );
                                                }
                                            })),
                                    )
                                }
                            }),
                    )
                    .child(
                        crate::ui::style::ghost_button_compact(
                            "doc-assistant-terminal-model",
                            model.label(),
                        )
                        .dropdown_caret(true)
                        .disabled(runtime_locked)
                        .dropdown_menu({
                            let center = center.clone();
                            let relative = relative_doc_path.clone();
                            move |mut menu, window, cx| {
                                let workspace = center.read(cx).workspace.clone();
                                let choices = crate::ui::model_favorites::grouped_choices(
                                    AgentModel::models_for(provider)
                                        .iter()
                                        .copied()
                                        .filter(|_| provider.is_visible_in_picker())
                                        .collect(),
                                    &workspace.read(cx).favorite_models,
                                    |model| {
                                        ide_core::model_favorites::ModelFavorite::new(*model, None)
                                    },
                                );
                                for (heading, candidate, key) in choices {
                                    if let Some(heading) = heading {
                                        menu = menu.item(PopupMenuItem::label(heading));
                                    }
                                    let relative = relative.clone();
                                    menu = menu.item(
                                        crate::ui::model_favorites::model_menu_item(
                                            candidate.label(),
                                            key,
                                            candidate == model,
                                            workspace.clone(),
                                            cx,
                                        )
                                        .on_click(
                                            window.listener_for(
                                                &center,
                                                move |this: &mut Self, _, _, cx| {
                                                    this.doc_assistants.update(
                                                        cx,
                                                        |assistants, cx| {
                                                            assistants.update_runtime(
                                                                project, &relative, provider,
                                                                candidate, effort, cx,
                                                            )
                                                        },
                                                    );
                                                },
                                            ),
                                        ),
                                    );
                                }
                                menu
                            }
                        }),
                    )
                    .when(!supported_efforts.is_empty(), |bar| {
                        bar.child(
                            crate::ui::style::ghost_button_compact(
                                "doc-assistant-terminal-effort",
                                effort.label(),
                            )
                            .dropdown_caret(true)
                            .disabled(runtime_locked)
                            .dropdown_menu({
                                let center = center.clone();
                                let relative = relative_doc_path.clone();
                                let effort_options = supported_efforts.clone();
                                move |mut menu, window, _| {
                                    for candidate in effort_options.iter().copied() {
                                        let relative = relative.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(candidate.label())
                                                .checked(candidate == effort)
                                                .on_click(window.listener_for(
                                                    &center,
                                                    move |this: &mut Self, _, _, cx| {
                                                        this.doc_assistants.update(
                                                            cx,
                                                            |assistants, cx| {
                                                                assistants.update_runtime(
                                                                    project, &relative, provider,
                                                                    model, candidate, cx,
                                                                )
                                                            },
                                                        );
                                                    },
                                                )),
                                        );
                                    }
                                    menu
                                }
                            }),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .truncate()
                            .when(runtime_locked, |meta| {
                                meta.child("Runtime locked for this conversation")
                            }),
                    )
                    .when(
                        session.as_ref().is_none_or(|(_, _, exited)| *exited),
                        |bar| {
                            let project_path = doc.project_path.clone();
                            let relative = doc.relative_path.clone();
                            bar.child(
                                Button::new("start-doc-assistant-terminal")
                                    .outline()
                                    .xsmall()
                                    .compact()
                                    .h(crate::ui::design::control_h())
                                    .px_2()
                                    .label(button_label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_doc_assistant_terminal(
                                            project,
                                            project_path.clone(),
                                            relative.clone(),
                                            cx,
                                        );
                                    })),
                            )
                        },
                    )
                    .when_some(
                        session.as_ref().map(|(id, _, exited)| (*id, *exited)),
                        |bar, (id, exited)| {
                            bar.when(exited, |bar| {
                                bar.child(
                                    Button::new("restart-doc-assistant-terminal")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Redo2)
                                        .tooltip("Restart last command")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Err(error) =
                                                this.terminals.update(cx, |terminals, cx| {
                                                    terminals.restart(id, cx)
                                                })
                                            {
                                                eprintln!("restart failed: {error:#}");
                                            }
                                        })),
                                )
                            })
                            .child(
                                Button::new("close-doc-assistant-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Close terminal")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.terminals.update(cx, |terminals, cx| {
                                            terminals.close(id, cx);
                                        });
                                    })),
                            )
                        },
                    ),
            )
            .when_some(error, |panel, error| {
                panel.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_1()
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
            .child(match session {
                Some((_, view, _)) => div().flex_1().min_h(px(0.)).child(view).into_any_element(),
                None => v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        gpui_component::Icon::new(IconName::Bot).size(crate::ui::design::icon_xl()),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .child("Start the doc assistant terminal"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .child(if can_resume {
                                "Resume the dedicated conversation for this doc."
                            } else {
                                "A dedicated conversation will be created for this doc."
                            }),
                    )
                    .child({
                        let project_path = doc.project_path.clone();
                        let relative = doc.relative_path.clone();
                        Button::new("empty-start-doc-assistant-terminal")
                            .outline()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h())
                            .px_2()
                            .label(button_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_doc_assistant_terminal(
                                    project,
                                    project_path.clone(),
                                    relative.clone(),
                                    cx,
                                );
                            }))
                    })
                    .into_any_element(),
            })
            .into_any_element()
    }

    pub(super) fn render_doc_implementor_terminal_panel(
        &mut self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (terminal_view, terminal_id, terminal_exited) = {
            let manager = self.terminals.read(cx);
            manager
                .agent_record_session(project, agent.id)
                .map(|session| (Some(session.view.clone()), Some(session.id), session.exited))
                .unwrap_or((None, None, false))
        };
        let runtime = self.agent_runtime(agent, project, cx);
        let start_label = match runtime {
            AgentRuntime::NotStarted => "Start",
            AgentRuntime::Working | AgentRuntime::Waiting | AgentRuntime::Open => "Focus",
            AgentRuntime::Idle | AgentRuntime::Ended => "Resume",
        };
        let agent_id = agent.id;
        let current_status = agent.status;
        let agent_status_accent = status_accent(current_status, cx);
        let status_view = cx.entity().clone();

        v_flex()
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(
                h_flex()
                    .h(px(36.))
                    .px_3()
                    .items_center()
                    .gap_2()
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        gpui_component::Icon::new(IconName::SquareTerminal)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("IMPLEMENTOR TERMINAL"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .truncate()
                            .child(SharedString::from(agent.title.clone())),
                    )
                    .child(
                        Button::new(("doc-implementor-status", agent.id.as_u128() as u64))
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h())
                            .px_2()
                            .dropdown_caret(true)
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .color(agent_status_accent.opacity(0.1))
                                    .foreground(crate::ui::design::t1(cx))
                                    .border(agent_status_accent.opacity(0.42))
                                    .hover(agent_status_accent.opacity(0.16))
                                    .active(agent_status_accent.opacity(0.2)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(status_dot(current_status, 6., cx))
                                    .child(
                                        div()
                                            .line_height(gpui::relative(1.))
                                            .child(current_status.label()),
                                    ),
                            )
                            .dropdown_menu(move |mut menu, window, _| {
                                for status in AgentStatus::ALL {
                                    menu =
                                        menu.item(
                                            PopupMenuItem::element(move |_, cx| {
                                                status_menu_row(status, cx)
                                            })
                                            .checked(status == current_status)
                                            .on_click(window.listener_for(
                                                &status_view,
                                                move |this: &mut Self, _, _, cx| {
                                                    this.agents.update(cx, |agents, cx| {
                                                        agents.update_status(agent_id, status, cx);
                                                    });
                                                },
                                            )),
                                        );
                                }
                                menu
                            }),
                    )
                    .when(terminal_view.is_none() || terminal_exited, |bar| {
                        bar.child(
                            Button::new((
                                "start-doc-implementor-terminal",
                                agent.id.as_u128() as u64,
                            ))
                            .outline()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h())
                            .px_2()
                            .label(start_label)
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_doc_implementor_terminal(
                                        project, agent_id, window, cx,
                                    );
                                },
                            )),
                        )
                    })
                    .when_some(
                        terminal_id.map(|id| (id, terminal_exited)),
                        |bar, (id, exited)| {
                            bar.when(exited, |bar| {
                                bar.child(
                                    Button::new("restart-doc-implementor-terminal")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Redo2)
                                        .tooltip("Restart last command")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Err(error) =
                                                this.terminals.update(cx, |terminals, cx| {
                                                    terminals.restart(id, cx)
                                                })
                                            {
                                                eprintln!("restart failed: {error:#}");
                                            }
                                        })),
                                )
                            })
                            .child(
                                Button::new("close-doc-implementor-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Close terminal")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.terminals.update(cx, |terminals, cx| {
                                            terminals.close(id, cx);
                                        });
                                    })),
                            )
                        },
                    ),
            )
            .child(match terminal_view {
                Some(view) => div().flex_1().min_h(px(0.)).child(view).into_any_element(),
                None => v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        gpui_component::Icon::new(IconName::SquareTerminal)
                            .size(crate::ui::design::icon_xl()),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .child("Open the implementor terminal"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .child("This is the project agent created from this doc."),
                    )
                    .child(
                        Button::new(("empty-start-doc-implementor", agent.id.as_u128() as u64))
                            .outline()
                            .small()
                            .icon(IconName::SquareTerminal)
                            .label(start_label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_doc_implementor_terminal(project, agent_id, window, cx);
                            })),
                    )
                    .into_any_element(),
            })
            .into_any_element()
    }

    pub(super) fn render_docs_terminal_panel(
        &mut self,
        project: ProjectId,
        doc: &WorkspaceDocEntry,
        assistant_key: &str,
        implementor: Option<&AgentRecord>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let terminal_mode = if implementor.is_some() {
            self.docs_terminal_mode
        } else {
            DocsTerminalMode::Assistant
        };
        let content = match terminal_mode {
            DocsTerminalMode::Assistant => {
                self.render_doc_assistant_terminal_panel(project, doc, assistant_key, cx)
            }
            DocsTerminalMode::Implementor => self.render_doc_implementor_terminal_panel(
                project,
                implementor.expect("implementor terminal mode requires an implementor"),
                cx,
            ),
        };

        v_flex()
            .size_full()
            .when(implementor.is_some(), |panel| {
                panel.child(self.render_docs_terminal_mode_switch(cx))
            })
            .child(div().flex_1().min_h(px(0.)).child(content))
            .into_any_element()
    }
}
