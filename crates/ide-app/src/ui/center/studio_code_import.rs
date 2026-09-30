//! Import discovery uses the existing Studio chat; draft creation stays native.
use super::*;
use gpui_component::Colorize;
use ide_core::studio::{StudioCodeImport, StudioStore};
use std::collections::BTreeSet;

pub(super) struct StudioCodeImportUi {
    pub visible: bool,
    result: StudioCodeImport,
    selected: BTreeSet<Uuid>,
    polling: bool,
    needs_refresh: bool,
    creating: bool,
    error: Option<String>,
}

fn import_chat_path(id: Uuid) -> PathBuf {
    PathBuf::from(format!(".choro/assistants/studio-imports/{id}/{id}"))
}

impl CenterArea {
    pub(super) fn open_code_import(
        &mut self,
        project: ProjectId,
        new_scan: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !new_scan {
            if let Some(import) = self.studio_code_imports.get_mut(&project) {
                import.visible = true;
                import.needs_refresh = true;
                cx.notify();
                return;
            }
        }
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let result = StudioStore::for_project(root).and_then(|store| {
            if !new_scan {
                if let Some(import) = store.latest_code_import()? {
                    return Ok((import, false));
                }
            }
            store.create_code_import().map(|import| (import, true))
        });
        match result {
            Ok((result, start)) => {
                let selected = result
                    .candidates
                    .iter()
                    .filter(|candidate| {
                        candidate.definition.existing_system_id.is_none()
                            && !result.created.contains(&candidate.id)
                    })
                    .map(|candidate| candidate.id)
                    .collect();
                self.studio_code_imports.insert(
                    project,
                    StudioCodeImportUi {
                        visible: true,
                        result,
                        selected,
                        polling: false,
                        needs_refresh: true,
                        creating: false,
                        error: None,
                    },
                );
                if start {
                    self.start_code_import_analysis(project, window, cx);
                }
            }
            Err(error) => {
                self.design_hub_error = Some(format!("Could not start code analysis: {error:#}"))
            }
        }
        cx.notify();
    }

    fn start_code_import_analysis(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(import) = self.studio_code_imports.get(&project) else {
            return;
        };
        let path = import_chat_path(import.result.id);
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, path.clone(), cx)
        });
        self.hydrate_doc_assistant_chat_session(&record, &root, cx);
        let agent = Self::doc_assistant_agent_record(&record, root);
        let input =
            self.agent_chat_input(&agent, "Refine the analysis or specify an app", window, cx);
        if input.read(cx).value().trim().is_empty() {
            input.update(cx, |input, cx| input.set_value("Find the distinct design systems used in this project and prepare drafts from the existing code. Inspect shared UI packages and each app. Keep shared systems together and separate products with different visual foundations. Show which files support each system.", window, cx));
        }
        self.submit_agent_chat_message_for_surface(
            &agent,
            input,
            false,
            &AgentChatSurface::Document {
                project,
                relative_doc_path: path,
            },
            window,
            cx,
        );
    }

    pub(super) fn prepare_code_import_turn(
        &mut self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(context) = agent.studio_context.as_ref() else {
            return false;
        };
        let Some(import) = self
            .studio_code_imports
            .get_mut(&agent.project_id)
            .filter(|import| import.result.id == context.design_id)
        else {
            return false;
        };
        let result = StudioStore::for_project(&agent.project_path)
            .and_then(|store| store.prepare_code_import(agent.id, context));
        import.error = result
            .as_ref()
            .err()
            .map(|error| format!("Could not analyze project: {error:#}"));
        import.needs_refresh = true;
        cx.notify();
        result.is_ok()
    }

    fn poll_code_import(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let Some(import) = self
            .studio_code_imports
            .get_mut(&project)
            .filter(|import| !import.polling)
        else {
            return;
        };
        import.polling = true;
        import.needs_refresh = false;
        let id = import.result.id;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move { StudioStore::for_project(root)?.code_import(id) })
                .await;
            this.update(cx, |this, cx| {
                let Some(import) = this
                    .studio_code_imports
                    .get_mut(&project)
                    .filter(|import| import.result.id == id)
                else {
                    return;
                };
                import.polling = false;
                match result {
                    Ok(result) => {
                        if result.proposal_id != import.result.proposal_id {
                            import.selected = result
                                .candidates
                                .iter()
                                .filter(|candidate| {
                                    candidate.definition.existing_system_id.is_none()
                                        && !result.created.contains(&candidate.id)
                                })
                                .map(|candidate| candidate.id)
                                .collect();
                        }
                        import.selected.retain(|id| !result.created.contains(id));
                        import.result = result;
                    }
                    Err(error) => {
                        import.error = Some(format!("Could not read analysis: {error:#}"))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn create_imported_systems(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let Some(import) = self
            .studio_code_imports
            .get_mut(&project)
            .filter(|import| !import.creating && !import.selected.is_empty())
        else {
            return;
        };
        import.creating = true;
        import.error = None;
        let id = import.result.id;
        let proposal = import.result.proposal_id;
        let selected = import.selected.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    StudioStore::for_project(root)?
                        .create_code_system_drafts(id, proposal, &selected)
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(import) = this
                    .studio_code_imports
                    .get_mut(&project)
                    .filter(|import| import.result.id == id)
                else {
                    return;
                };
                import.creating = false;
                match result {
                    Ok(_) => {
                        import.selected.clear();
                        import.needs_refresh = true;
                        this.open_system_library(project, cx);
                    }
                    Err(error) => {
                        import.error = Some(format!("Could not create drafts: {error:#}"))
                    }
                }
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_code_import(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return div().into_any_element();
        };
        let Some(import) = self.studio_code_imports.get(&project) else {
            return div().into_any_element();
        };
        let path = import_chat_path(import.result.id);
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, path.clone(), cx)
        });
        self.hydrate_doc_assistant_chat_session(&record, &root, cx);
        let agent = Self::doc_assistant_agent_record(&record, root);
        let busy = self
            .agent_chats
            .read(cx)
            .session(agent.id)
            .is_some_and(|session| {
                matches!(
                    session.status,
                    AgentChatStatus::Running | AgentChatStatus::Cancelling
                )
            });
        if busy || self.studio_code_imports[&project].needs_refresh {
            self.poll_code_import(project, cx);
        }
        let import = &self.studio_code_imports[&project];
        let result = import.result.clone();
        let selected = import.selected.clone();
        let creating = import.creating;
        let error = import.error.clone();
        let mut findings = v_flex().w_full().min_w(px(0.)).gap_3();
        if result.proposed {
            findings = findings.child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t2(cx))
                    .child(result.summary),
            );
            for (index, candidate) in result.candidates.iter().enumerate() {
                let id = candidate.id;
                let definition = &candidate.definition;
                let existing =
                    definition.existing_system_id.is_some() || result.created.contains(&id);
                let existing_id = definition
                    .existing_system_id
                    .or_else(|| result.created.contains(&id).then_some(id));
                let colors = definition
                    .system
                    .tokens
                    .iter()
                    .filter(|(name, _)| name.contains("color"))
                    .filter_map(|(_, value)| gpui::Hsla::parse_hex(value).ok())
                    .take(6)
                    .collect::<Vec<_>>();
                findings = findings.child(
                    v_flex()
                        .w_full()
                        .min_w(px(0.))
                        .py_3()
                        .gap_2()
                        .border_b_1()
                        .border_color(crate::ui::design::line(cx))
                        .child(
                            style::ghost_button_compact(
                                ("import-system-select", index),
                                definition.name.clone(),
                            )
                            .w_full()
                            .justify_start()
                            .overflow_hidden()
                            .icon(if selected.contains(&id) {
                                IconName::Check
                            } else {
                                IconName::Plus
                            })
                            .selected(selected.contains(&id))
                            .disabled(busy || creating || existing)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(import) = this.studio_code_imports.get_mut(&project)
                                    {
                                        if !import.selected.remove(&id) {
                                            import.selected.insert(id);
                                        }
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child(if existing {
                                    format!("{} · Already in the library", definition.platform)
                                } else {
                                    format!(
                                        "{} · {} tokens · {} source files",
                                        definition.platform,
                                        definition.system.tokens.len(),
                                        definition.sources.len()
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child(definition.description.clone()),
                        )
                        .when(!colors.is_empty(), |row| {
                            row.child(h_flex().gap_1().h(px(16.)).children(colors.into_iter().map(
                                |color| {
                                    div()
                                        .w(px(24.))
                                        .h_full()
                                        .rounded(crate::ui::design::r_xs())
                                        .bg(color)
                                },
                            )))
                        })
                        .when_some(existing_id, |row, id| {
                            row.child(
                                style::ghost_button_compact(
                                    ("code-import-open-existing", index),
                                    "Open system",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.open_studio(project, id, cx),
                                )),
                            )
                        })
                        .children(definition.sources.keys().take(3).map(|source| {
                            div()
                                .w_full()
                                .truncate()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child(source.clone())
                        })),
                );
            }
        } else {
            findings = findings.child(div().text_size(crate::ui::design::text_body()).child(if busy { "Finding your design systems…" } else { "Analyze your project" }))
                .child(div().text_size(crate::ui::design::text_ui()).text_color(crate::ui::design::t3(cx)).child("Studio reads your apps, shared components, themes, and fonts. Distinct systems appear here for you to select."));
        }
        let chat = self.render_agent_chat_body_for_surface(
            &agent,
            AgentChatSurface::Document {
                project,
                relative_doc_path: path,
            },
            window,
            cx,
        );
        v_flex().size_full().min_w(px(0.)).min_h(px(0.))
            .child(crate::ui::design::header::workspace_bar(cx)
                .child(style::header_icon_button("code-import-back", IconName::ArrowLeft, cx).tooltip("Back to design systems")
                    .on_click(cx.listener(move |this, _, _, cx| this.open_system_library(project, cx))))
                .child(crate::ui::design::header::title_col(cx).child(crate::ui::design::header::title("Design systems from code", cx)))
                .child(crate::ui::design::header::actions()
                    .child(style::ghost_button_compact("code-import-new", "New analysis").disabled(busy || creating)
                        .on_click(cx.listener(move |this, _, window, cx| this.open_code_import(project, true, window, cx))))
                    .child(style::primary_button_compact("code-import-create", if creating { "Creating drafts…".into() } else { match selected.len() { 0 => "Create drafts".into(), 1 => "Create draft".into(), count => format!("Create {count} drafts") } }, cx)
                        .disabled(busy || creating || !result.proposed || selected.is_empty())
                        .on_click(cx.listener(move |this, _, _, cx| this.create_imported_systems(project, cx))))))
            .when_some(error, |page, error| page.child(div().px_4().py_2().text_color(crate::ui::design::rose(cx)).child(error)))
            .child(h_flex().flex_1().min_h(px(0.)).min_w(px(0.))
                .child(div().flex_1().min_w(px(0.)).h_full().child(chat))
                .child(v_flex().w(px(340.)).min_w(px(220.)).max_w(gpui::relative(0.45)).h_full().border_l_1().border_color(crate::ui::design::line(cx))
                    .child(div().id("code-import-findings").flex_1().min_h(px(0.)).overflow_y_scroll().p_4().child(findings))
                    .child(div().p_4().border_t_1().border_color(crate::ui::design::line(cx)).text_size(crate::ui::design::text_label()).text_color(crate::ui::design::t3(cx))
                        .child("Selected systems become drafts. Review their specimens before applying them to a design."))))
            .into_any_element()
    }
}
