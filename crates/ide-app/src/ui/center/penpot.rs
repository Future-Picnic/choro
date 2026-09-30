use super::*;
use crate::state::PenpotDesign;

// Must stay aligned with `--choro-penpot-left-sidebar-width` in the hosted
// Penpot overrides so switching modes never moves the canvas.
const PENPOT_LEFT_SIDEBAR_WIDTH: f32 = 318.0;

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

struct DesignBrowserOpenChoice {
    dont_show_again: bool,
}

impl Render for DesignBrowserOpenChoice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let checked = self.dont_show_again;
        v_flex()
            .gap_4()
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        "To let the implementation agent inspect the live design, Choro will open this design in your default browser when the agent starts. Keep that browser tab open while the agent works; you can continue using Choro normally.",
                    ),
            )
            .child(
                h_flex()
                    .id("design-browser-dont-show-again")
                    .w_full()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .child(crate::ui::style::checkbox(
                        "design-browser-dont-show-again-checkbox",
                        checked,
                        cx,
                    ))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child("Don’t show this again"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dont_show_again = !this.dont_show_again;
                        window.refresh();
                        cx.notify();
                    })),
            )
    }
}

impl CenterArea {
    const DESIGN_MCP_DISCONNECT_GRACE: Duration = Duration::from_millis(1_500);

    pub(super) fn reconnect_penpot(&mut self, cx: &mut Context<Self>) {
        let configured = self.penpot.read(cx).is_configured();
        self.penpot.update(cx, |penpot, cx| {
            if configured {
                penpot.test_connection(cx);
            } else {
                penpot.ensure_auto_provisioned(cx);
            }
        });
        cx.notify();
    }

    pub(super) fn open_penpot_design(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.open_penpot_design_surface(project, design_id, false, cx);
    }

    pub(super) fn open_penpot_design_surface(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        assistant_open: bool,
        cx: &mut Context<Self>,
    ) {
        self.close_penpot_compare(cx);
        if let Some((previous_project, previous_design_id)) = self.penpot_open_design {
            if previous_design_id != design_id {
                if let Some(conversation) =
                    self.penpot.read(cx).current_conversation(previous_project)
                {
                    self.agent_chats.update(cx, |chats, cx| {
                        chats.force_stop_backend(conversation.agent_id, cx)
                    });
                }
                self.design_mcp_readiness.remove(&previous_design_id);
                self.design_mcp_disconnect_tokens
                    .remove(&previous_design_id);
                let has_pending = self
                    .pending_design_assistant_submissions
                    .get(&previous_design_id)
                    .is_some_and(|queue| !queue.is_empty());
                if !has_pending {
                    self.penpot.update(cx, |penpot, cx| {
                        penpot.set_assistant_busy(previous_project, false, cx)
                    });
                }
            }
        }
        self.penpot.update(cx, |penpot, cx| {
            penpot.select_design(project, design_id, cx);
            penpot.test_connection(cx);
        });
        self.figma_open_design = None;
        self.penpot_external_mcp_design = None;
        self.penpot_open_design = Some((project, design_id));
        self.design_mcp_readiness
            .insert(design_id, DesignMcpReadiness::Connecting);
        self.penpot_assistant_open = assistant_open;
        self.web_host
            .update(cx, |host, _| host.set_penpot_assistant_open(assistant_open));
        self.set_view_mode(CenterMode::Design, cx);
        cx.notify();
    }

    pub(super) fn handle_design_mcp_status(
        &mut self,
        connected: bool,
        file_id: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        let Some((project, design_id)) = self.penpot_open_design else {
            return;
        };
        let Some(design) = self.penpot.read(cx).design(project, design_id) else {
            return;
        };
        let conversation_agent_id = self
            .penpot
            .read(cx)
            .current_conversation(project)
            .map(|conversation| conversation.agent_id);

        if !connected {
            let was_ready = matches!(
                self.design_mcp_readiness.get(&design_id),
                Some(DesignMcpReadiness::Ready { .. })
            );
            self.design_mcp_readiness
                .insert(design_id, DesignMcpReadiness::Connecting);
            if was_ready {
                let token = Uuid::new_v4();
                self.design_mcp_disconnect_tokens.insert(design_id, token);
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Self::DESIGN_MCP_DISCONNECT_GRACE)
                        .await;
                    let Some(center) = this.upgrade() else {
                        return;
                    };
                    center
                        .update(cx, |this, cx| {
                            let still_disconnected = this
                                .design_mcp_disconnect_tokens
                                .get(&design_id)
                                .is_some_and(|active| *active == token)
                                && matches!(
                                    this.design_mcp_readiness.get(&design_id),
                                    Some(DesignMcpReadiness::Connecting)
                                );
                            if !still_disconnected {
                                return;
                            }
                            this.design_mcp_disconnect_tokens.remove(&design_id);
                            if let Some(agent_id) = conversation_agent_id {
                                this.agent_chats.update(cx, |chats, cx| {
                                    chats.force_stop_backend(agent_id, cx)
                                });
                                this.agent_start_errors.insert(
                                    agent_id,
                                    "The exact Design connection was lost. The assistant was stopped before it could target another canvas."
                                        .to_string(),
                                );
                            }
                            cx.notify();
                        })
                        .ok();
                })
                .detach();
            }
            return;
        }

        self.design_mcp_disconnect_tokens.remove(&design_id);
        if file_id != Some(design.penpot_file_id) {
            let actual = file_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let error = format!(
                "The live Design connection targets file {actual}, not “{}”. The assistant was not started.",
                design.name
            );
            self.design_mcp_readiness
                .insert(design_id, DesignMcpReadiness::Blocked(error.clone()));
            if let Some(agent_id) = conversation_agent_id {
                self.agent_chats
                    .update(cx, |chats, cx| chats.force_stop_backend(agent_id, cx));
                self.agent_start_errors.insert(agent_id, error);
            }
            cx.notify();
            return;
        }

        self.design_mcp_readiness.insert(
            design_id,
            DesignMcpReadiness::Ready {
                file_id: design.penpot_file_id,
            },
        );
        if let Some(agent_id) = conversation_agent_id {
            self.agent_start_errors.remove(&agent_id);
        }
        let pending = self
            .pending_design_assistant_submissions
            .remove(&design_id)
            .unwrap_or_default();
        for submission in pending {
            if design_mcp_submission_expired(submission.queued_at, Instant::now()) {
                self.agent_start_errors.insert(
                    submission.agent.id,
                    "Design connection timed out before the assistant could start. Send the message again."
                        .to_string(),
                );
                continue;
            }
            self.dispatch_agent_chat_submission_with_agent(
                &submission.agent,
                submission.text,
                submission.display_text,
                submission.tags,
                submission.mode,
                cx,
            );
        }
        cx.notify();
    }

    fn show_penpot_hub(&mut self, cx: &mut Context<Self>) {
        let project = self
            .penpot_open_design
            .or(self.figma_open_design)
            .map(|(project, _)| project);
        if let Some((open_project, design_id)) = self.penpot_open_design {
            if let Some(conversation) = self.penpot.read(cx).current_conversation(open_project) {
                self.agent_chats.update(cx, |chats, cx| {
                    chats.force_stop_backend(conversation.agent_id, cx)
                });
            }
            self.design_mcp_readiness.remove(&design_id);
            self.design_mcp_disconnect_tokens.remove(&design_id);
            let has_pending = self
                .pending_design_assistant_submissions
                .get(&design_id)
                .is_some_and(|queue| !queue.is_empty());
            if !has_pending {
                self.penpot.update(cx, |penpot, cx| {
                    penpot.set_assistant_busy(open_project, false, cx)
                });
            }
        }
        self.close_penpot_compare(cx);
        self.penpot_open_design = None;
        self.figma_open_design = None;
        self.penpot_assistant_open = false;
        if let Some(project) = project {
            self.penpot.update(cx, |penpot, cx| {
                penpot.refresh_design_thumbnails(project, cx)
            });
        }
        self.set_view_mode(CenterMode::Design, cx);
        cx.notify();
    }

    pub(super) fn set_penpot_compare_open(
        &mut self,
        project: ProjectId,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        if !open {
            self.close_penpot_compare(cx);
            return;
        }
        self.penpot_compare_open = true;
        // Compare reviews belong to the design's dedicated conversation. Keep
        // that assistant beside the design and live result instead of routing
        // the user away to an unrelated project agent.
        self.penpot_assistant_open = true;
        self.project_preview_panel_ratio = 0.5;
        let ui = self.project_preview_ui.entry(project).or_default();
        ui.open = true;
        ui.status = Some("Compare · choose the live result to review".to_string());
        self.project_preview_inspecting = None;
        self.web_host.update(cx, |host, _| {
            host.set_penpot_assistant_open(true);
            host.set_penpot_compare_open(true);
        });
        self.reconcile_project_preview_for_selected_agent(project, cx);
        cx.notify();
    }

    pub(super) fn close_penpot_compare(&mut self, cx: &mut Context<Self>) {
        if !self.penpot_compare_open {
            return;
        }
        self.penpot_compare_open = false;
        self.project_preview_inspecting = None;
        if let Some((project, _)) = self.penpot_open_design {
            let ui = self.project_preview_ui.entry(project).or_default();
            ui.open = false;
            ui.status = None;
        }
        self.web_host
            .update(cx, |host, _| host.set_penpot_compare_open(false));
        self.compare_web_host.update(cx, |host, _| {
            let _ = host.set_project_preview_inspecting(false);
        });
        cx.notify();
    }

    pub(super) fn open_penpot_design_request(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        initial_draft: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let draft = initial_draft.filter(|prompt| !prompt.trim().is_empty());
        if let Some(draft) = draft.as_ref() {
            self.pending_design_assistant_drafts
                .insert(design_id, draft.clone());
        }
        self.open_penpot_design_surface(project, design_id, draft.is_some(), cx);
    }

    pub(super) fn create_penpot_design_for_doc(
        &mut self,
        project: ProjectId,
        title: String,
        relative_doc_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let name = format!("{title} Design");
        let path = relative_doc_path.to_string_lossy();
        let prompt = format!(
            "Design the product experience described in @@{path}.\n\nRead the document first, then use the connected Design MCP tools to create the design in this file. Cover the important screens, states, hierarchy, and interactions. Keep the design grounded in the document and call out any ambiguity before inventing major product behavior.\n\n{}",
            penpot_assistant::codebase_context_instruction()
        );
        self.penpot.update(cx, |penpot, cx| {
            penpot.create_design(
                project,
                name,
                Some(PenpotDesignSource::Document(relative_doc_path)),
                Some(prompt),
                cx,
            )
        });
    }

    pub(super) fn create_penpot_design_for_task(
        &mut self,
        project: ProjectId,
        summary: TaskSummary,
        detail: Option<TaskDetail>,
        cx: &mut Context<Self>,
    ) {
        let source_title = summary.reference.title.trim();
        let source_title = if source_title.is_empty() {
            summary.reference.issue_key.as_str()
        } else {
            source_title
        };
        let name = format!("{source_title} Design");
        let prompt = super::tasks::prompt::task_design_prompt(&summary, detail.as_ref());
        self.penpot.update(cx, |penpot, cx| {
            penpot.create_design(
                project,
                name,
                Some(PenpotDesignSource::Task(summary.reference)),
                Some(prompt),
                cx,
            )
        });
    }

    pub(super) fn render_linked_design_indicator(
        &mut self,
        key: (&'static str, u64),
        design: PenpotDesign,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let project = design.project_id;
        let design_id = design.id;
        let tooltip = SharedString::from(format!("{} — open design", design.name));
        let label = SharedString::from(short_design_chip_label(&design.name));
        crate::ui::design::indicator::subline_link(
            key,
            crate::ui::design::design_icon(),
            label,
            crate::ui::design::accent(cx),
            cx,
        )
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.open_penpot_design(project, design_id, cx);
        }))
        .into_any_element()
    }

    pub(super) fn add_design_mentions_to_new_agent(
        &mut self,
        designs: Vec<PenpotDesign>,
        cx: &mut Context<Self>,
    ) {
        let tokens = designs
            .into_iter()
            .filter_map(|design| {
                let source = self.penpot.read(cx).design_url(&design)?;
                let reference = penpot_design_reference(&design, source);
                ComposerMentionToken::penpot_design(&reference)
            })
            .collect::<Vec<_>>();
        let Some(composer) = self.new_agent_composer.as_mut() else {
            return;
        };
        for token in tokens {
            if !composer.selected_mentions.contains(&token) {
                composer.selected_mentions.push(token);
            }
        }
    }

    fn open_implementation_agent_for_design(
        &mut self,
        design: PenpotDesign,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = design.project_id;
        self.penpot_external_mcp_design = None;
        let prompt = if let Some(relative_doc_path) = design.source_doc.as_ref() {
            let doc_path = relative_doc_path.to_string_lossy();
            format!(
                "Implement the attached design “{}” for the work described in @@{doc_path}.\n\nRead the document and inspect the exact linked design with the connected Design MCP tools before changing code. Treat the document as the product-requirements source of truth and the design as the visual and interaction source of truth. Implement the relevant screens, states, responsive behavior, and interactions; update or add focused tests where they matter. If the document, design, and codebase conflict, stop and ask before making broad assumptions.",
                design.name
            )
        } else if let Some(task) = design.source_task.as_ref() {
            let provider = task.provider.label();
            let task_url = (!task.issue_url.trim().is_empty())
                .then(|| format!("\nTask URL: {}", task.issue_url))
                .unwrap_or_default();
            format!(
                "Implement the attached design “{}” for {provider} task {}: {}.\n\nFor the freshest task details, call the `task_read` tool with `{}`.{task_url}\n\nInspect the exact linked design with the connected Design MCP tools before changing code. Use the task as the product-requirements source of truth and the design as the visual and interaction source of truth. Implement the relevant screens, states, responsive behavior, and interactions; update or add focused tests where they matter. If the task, design, and codebase conflict, stop and ask before making broad assumptions.",
                design.name, task.issue_key, task.title, task.issue_key
            )
        } else {
            format!(
                "Implement the attached design “{}”.\n\nInspect the exact linked design with the connected Design MCP tools before changing code. Treat its screens, states, responsive behavior, and interactions as the implementation source of truth. Make the needed code changes and update or add focused tests where they matter. If the design is ambiguous or conflicts with the codebase, stop and ask before making broad assumptions.",
                design.name
            )
        };

        self.open_new_agent_composer_for_project(project, window, cx);

        {
            let Some(composer) = self.new_agent_composer.as_mut() else {
                return;
            };
            composer.prompt.update(cx, |input, cx| {
                input.set_value(prompt.clone(), window, cx);
                input.set_cursor_position(
                    input_position_for_byte_offset(&prompt, prompt.len()),
                    window,
                    cx,
                );
                input.focus(window, cx);
            });
            composer.linked_docs.clear();
            composer.linked_tasks.clear();
            composer.source_doc = design.source_doc.clone();
            composer.source_task = design.source_task.clone();
            composer.implementation_design = Some(design.id);
            composer.design_browser_open_confirmed = false;
            if let Some(source_doc) = design.source_doc.clone() {
                composer.linked_docs.push(source_doc);
                if crate::ui::onboarding::defaults_doc_agent_to_plan(cx) {
                    composer.interaction_mode = AgentInteractionMode::Plan;
                }
            }
            if let Some(source_task) = design.source_task.clone() {
                composer.linked_tasks.push(source_task);
            }
            composer.selected_mentions.clear();
            composer.error = None;
            composer.doc_mention_selected = 0;
            composer.doc_mention_dismissed_query = None;
            composer.file_mention_selected = 0;
            composer.file_mention_dismissed_query = None;
        }
        self.add_design_mentions_to_new_agent(vec![design], cx);
        cx.notify();
    }

    pub(super) fn confirm_design_browser_open(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        design_name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let choice = cx.new(|_| DesignBrowserOpenChoice {
            dont_show_again: false,
        });
        let center = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let choice_for_footer = choice.clone();
            let center_for_confirm = center.clone();
            dialog
                .w(px(480.))
                .overlay_closable(true)
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(crate::ui::confirm::icon_badge(
                            IconName::ExternalLink,
                            crate::ui::design::accent(cx),
                            cx,
                        ))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Open design for this agent?"),
                        ),
                )
                .child(
                    v_flex().gap_3().child(choice.clone()).child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::design::line(cx))
                            .bg(crate::ui::design::surface(cx))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .truncate()
                            .child(design_name.clone()),
                    ),
                )
                .footer(move |_, _, _, cx| {
                    let choice = choice_for_footer.clone();
                    let center = center_for_confirm.clone();
                    vec![
                        crate::ui::style::dialog_neutral_button(
                            "design-browser-cancel",
                            "Cancel",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                        crate::ui::style::primary_button_compact(
                            "design-browser-open-and-start",
                            "Open & Start",
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            let dont_show_again = choice.read(cx).dont_show_again;
                            window.close_dialog(cx);
                            let _ = center.update(cx, |this, cx| {
                                let matches_pending_design =
                                    this.new_agent_composer.as_ref().is_some_and(|composer| {
                                        composer.project == project
                                            && composer.implementation_design == Some(design_id)
                                    });
                                if !matches_pending_design {
                                    return;
                                }
                                if dont_show_again {
                                    this.workspace.update(cx, |workspace, cx| {
                                        workspace.dismiss_design_browser_open_prompt(cx);
                                    });
                                }
                                if let Some(composer) = this.new_agent_composer.as_mut() {
                                    composer.design_browser_open_confirmed = true;
                                }
                                this.start_new_agent_composer(window, cx);
                            });
                        }),
                    ]
                })
        });
    }

    pub(super) fn open_design_browser_for_agent(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let url = self
            .penpot
            .read(cx)
            .design(project, design_id)
            .and_then(|design| self.penpot.read(cx).external_mcp_design_url(&design));
        if let Some(url) = url {
            // The ordinary browser owns the live Design/MCP surface while the
            // implementation agent runs. This keeps Choro responsive and
            // avoids parking a second native WebView for the same MCP token.
            crate::ui::git::git_panel::open_url(&url);
            self.penpot_external_mcp_design = Some((project, design_id));
        } else {
            self.penpot_external_mcp_design = None;
        }
    }

    fn penpot_design_implementors(
        &mut self,
        design: &PenpotDesign,
        cx: &mut Context<Self>,
    ) -> Vec<AgentRecord> {
        let mut implementors = self
            .agents
            .read(cx)
            .records_for_project(design.project_id)
            .into_iter()
            .filter(|agent| {
                !agent.hidden_doc_assistant
                    && (agent.started_at.is_some()
                        || agent.cli_session_id.is_some()
                        || agent.chat_session_id.is_some())
                    && self
                        .penpot
                        .read(cx)
                        .designs_for_agent(agent)
                        .iter()
                        .any(|linked| linked.id == design.id)
            })
            .collect::<Vec<_>>();
        implementors.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.started_at.cmp(&left.started_at))
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        implementors
    }

    pub(super) fn penpot_design_assistant_agent(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> Option<AgentRecord> {
        let design_id = self
            .penpot_open_design
            .and_then(|(open_project, design_id)| (open_project == project).then_some(design_id))?;
        let design = self.penpot.read(cx).design(project, design_id)?;
        let conversation = self.penpot.read(cx).current_conversation(project)?;
        let (_, project_path) = self.project_by_id(project, cx)?;
        let relative = penpot_assistant::conversation_record_path(design.id, conversation.id);
        let record = penpot_conversation_record(project, relative, &conversation);
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.upsert_external_record(record, cx)
        });
        self.hydrate_doc_assistant_chat_session(&record, &project_path, cx);
        let design_url = self
            .penpot
            .read(cx)
            .selected_design_url(project)
            .unwrap_or_default();
        Some(Self::penpot_assistant_agent_record(
            &record,
            project_path,
            &design,
            &design_url,
        ))
    }

    fn penpot_design_source_indicator(
        &mut self,
        design: &PenpotDesign,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let project = design.project_id;
        if let Some(relative) = design.source_doc.clone() {
            let (_, project_path) = self.project_by_id(project, cx)?;
            let absolute = project_path.join(&relative);
            return Some(
                crate::ui::design::indicator::subline_link(
                    ("design-source-doc", design.id.as_u128() as u64),
                    crate::ui::design::docs_icon(),
                    SharedString::from(short_doc_chip_label(&relative)),
                    crate::ui::design::sky(cx),
                    cx,
                )
                .tooltip(|window, cx| Tooltip::new("Open source document").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_doc(project, absolute.clone(), cx);
                }))
                .into_any_element(),
            );
        }
        design.source_task.clone().map(|task| {
            let label = SharedString::from(task.issue_key.clone());
            crate::ui::design::indicator::subline_link(
                ("design-source-task", design.id.as_u128() as u64),
                crate::ui::design::tasks_icon(),
                label,
                crate::ui::design::accent(cx),
                cx,
            )
            .tooltip(|window, cx| Tooltip::new("Open source task").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_task(project, task.clone(), cx);
            }))
            .into_any_element()
        })
    }

    fn penpot_design_agent_indicator(
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

    fn penpot_design_pull_request(
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

    pub(crate) fn create_penpot_design_from_hub(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) {
        let Some(project_name) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|candidate| candidate.id == project)
            .map(|project| project.name.clone())
        else {
            return;
        };
        let existing = self.penpot.read(cx).designs_for_project(project).len();
        let name = if existing == 0 {
            format!("{project_name} Design")
        } else {
            format!("{project_name} Design {}", existing + 1)
        };
        self.penpot.update(cx, |penpot, cx| {
            penpot.create_design(project, name, None, None, cx)
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
        let configured = self.penpot.read(cx).is_configured();
        let busy =
            self.penpot.read(cx).creating_design() || self.penpot.read(cx).assistant_busy(project);
        let center = cx.entity().clone();
        style::primary_button_compact(id, label, cx)
            .icon(IconName::Plus)
            .dropdown_menu(move |menu, window, _| {
                let native_center = center.clone();
                let figma_center = center.clone();
                menu.item(
                    PopupMenuItem::new("New Choro Design")
                        .icon(crate::ui::design::design_icon())
                        .disabled(!configured || busy)
                        .on_click(window.listener_for(&native_center, move |this, _, _, cx| {
                            this.create_penpot_design_from_hub(project, cx);
                        })),
                )
                .item(PopupMenuItem::new("Figma").icon(IconName::Globe).on_click(
                    window.listener_for(&figma_center, move |this, _, window, cx| {
                        this.open_figma_design_dialog(project, window, cx);
                    }),
                ))
            })
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
                        style::ghost_button_compact("add-figma-design-cancel", "Cancel")
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
        self.close_penpot_compare(cx);
        self.penpot_open_design = None;
        self.figma_open_design = Some((project, reference_id));
        self.penpot_assistant_open = false;
        self.web_host
            .update(cx, |host, _| host.set_penpot_assistant_open(false));
        self.set_view_mode(CenterMode::Design, cx);
        cx.notify();
    }

    fn confirm_delete_penpot_design(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let center = cx.entity().clone();
        crate::ui::confirm::ConfirmDialog::new(
            "Delete design?",
            "This design and its Design Assistant conversations will be deleted. This cannot be undone.",
        )
        .icon(IconName::Delete)
        .detail(title)
        .confirm_label("Delete")
        .confirm_id("delete-penpot-design-confirm")
        .on_confirm(move |_, cx| {
            center.update(cx, |center, cx| {
                center.design_hub_error = None;
                center.penpot.update(cx, |penpot, cx| {
                    penpot.delete_design(project, design_id, cx);
                });
                cx.notify();
            });
        })
        .open(window, cx);
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

    fn render_penpot_hub_card(
        &mut self,
        project: ProjectId,
        design: PenpotDesign,
        key: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let design_id = design.id;
        let title = SharedString::from(design.name.clone());
        let delete_title = title.clone();
        let center = cx.entity().clone();
        let source = if let Some(path) = design.source_doc.as_ref() {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Document");
            format!("Doc · {name}")
        } else if let Some(task) = design.source_task.as_ref() {
            format!("Task · {}", task.issue_key)
        } else {
            "Project design".to_string()
        };
        let updated =
            super::time::branch_relative_time(design.updated_at.min(i64::MAX as u64) as i64);
        let updated = if updated.is_empty() {
            "Recently updated".to_string()
        } else {
            format!("Updated {updated}")
        };
        let thumbnail = self.penpot.read(cx).thumbnail_path(design_id);
        let preview: gpui::AnyElement = match thumbnail {
            Some(path) => img(path)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .relative()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .left(px(22.))
                        .top(px(20.))
                        .size(px(76.))
                        .rounded(crate::ui::design::r_lg())
                        .bg(crate::ui::design::accent(cx).opacity(0.10)),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(24.))
                        .bottom(px(18.))
                        .size(px(62.))
                        .rounded_full()
                        .bg(crate::ui::design::sky(cx).opacity(0.10)),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_component::Icon::new(crate::ui::design::design_icon())
                                .size_8()
                                .text_color(crate::ui::design::accent(cx)),
                        ),
                )
                .into_any_element(),
        };

        v_flex()
            .id(("penpot-hub-card", key))
            .w(px(252.))
            .h(px(190.))
            .flex_none()
            .overflow_hidden()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::surface(cx))
            .cursor_pointer()
            .hover(|card| {
                card.bg(crate::ui::design::surface_2(cx))
                    .border_color(crate::ui::design::accent(cx).opacity(0.42))
            })
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
                    .child(preview)
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .bottom(px(9.))
                            .max_w(px(190.))
                            .truncate()
                            .rounded(crate::ui::design::r_xs())
                            .bg(crate::ui::design::surface(cx).opacity(0.92))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(source),
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
                this.open_penpot_design(project, design_id, cx);
            }))
            .context_menu(move |menu, _, _| {
                let center = center.clone();
                let delete_title = delete_title.clone();
                menu.item(
                    PopupMenuItem::new("Delete")
                        .icon(IconName::Delete)
                        .on_click(move |_, window, cx| {
                            center.update(cx, |center, cx| {
                                center.confirm_delete_penpot_design(
                                    project,
                                    design_id,
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

        v_flex()
            .id(("figma-hub-card", key))
            .w(px(252.))
            .h(px(190.))
            .flex_none()
            .overflow_hidden()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::surface(cx))
            .cursor_pointer()
            .hover(|card| {
                card.bg(crate::ui::design::surface_2(cx))
                    .border_color(crate::ui::design::accent(cx).opacity(0.42))
            })
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
            return self.render_penpot_hub(project, cx);
        };
        let title = reference.title.clone();
        let source = reference.source.clone();
        let back_to_designs =
            style::header_icon_button("figma-back-to-designs", IconName::ArrowLeft, cx)
                .tooltip("Back to project designs")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_penpot_hub(cx);
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

    fn render_penpot_hub(
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
        let mut designs = self.penpot.read(cx).designs_for_project(project);
        designs.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.name.cmp(&right.name))
        });
        let creating = self.penpot.read(cx).creating_design();
        let connection_pending = self.penpot.read(cx).connection_pending();
        let connection_needs_attention = self.penpot.read(cx).connection_needs_attention();
        let mut figma_designs = self.designs.read(cx).design_hub_references(project);
        figma_designs.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.title.cmp(&right.title))
        });
        let design_count = designs.len() + figma_designs.len();
        let subtitle = match design_count {
            0 => format!("{project_name} · No designs yet"),
            1 => format!("{project_name} · 1 design"),
            count => format!("{project_name} · {count} designs"),
        };
        let mut cards = designs
            .into_iter()
            .enumerate()
            .map(|(key, design)| self.render_penpot_hub_card(project, design, key, cx))
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
        let new_design = self.new_design_dropdown(
            "penpot-hub-new-design",
            if creating {
                "Creating…"
            } else {
                "New Design"
            },
            project,
            cx,
        );
        let reconnect = connection_needs_attention.then(|| {
            style::refresh_button("penpot-hub-reconnect", "Reconnect", cx)
                .disabled(connection_pending)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reconnect_penpot(cx);
                }))
        });

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
                                    .child(crate::ui::design::header::subtitle(subtitle, cx))
                                    .when(connection_needs_attention, |subline| {
                                        subline.child(
                                            crate::ui::design::indicator::subline_indicator(
                                                IconName::Globe,
                                                "Not connected",
                                                crate::ui::design::rose(cx),
                                                cx,
                                            ),
                                        )
                                    })
                                    .when(connection_pending, |subline| {
                                        subline.child(
                                            crate::ui::design::indicator::subline_indicator(
                                                IconName::Globe,
                                                "Connecting",
                                                crate::ui::design::amber(cx),
                                                cx,
                                            ),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        crate::ui::design::header::actions()
                            .children(reconnect)
                            .child(new_design),
                    ),
            )
            .when_some(
                self.design_hub_error
                    .clone()
                    .or_else(|| self.penpot.read(cx).last_error().map(str::to_string)),
                |hub, error| {
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
                },
            )
            .when(design_count == 0 && !creating, |hub| {
                hub.child(
                    style::empty_state(
                        crate::ui::design::design_icon(),
                        "No designs yet",
                        "Create the first design for this project.",
                        cx,
                    )
                    .child(self.new_design_dropdown(
                        "penpot-hub-empty-new-design",
                        "New Design",
                        project,
                        cx,
                    )),
                )
            })
            .when(creating && design_count == 0, |hub| {
                hub.child(style::loading_state(
                    "Creating design",
                    "Preparing a new design file for this project…",
                    0,
                    cx,
                ))
            })
            .when(!cards.is_empty(), |hub| {
                hub.child(
                    div()
                        .id("penpot-hub-scroll")
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

    pub(super) fn render_penpot_section(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if let Some(reference_id) =
            self.figma_open_design
                .and_then(|(open_project, reference_id)| {
                    (open_project == project).then_some(reference_id)
                })
        {
            return self.render_figma_design_section(project, reference_id, cx);
        }

        if !self.penpot.read(cx).is_configured() || self.penpot_editing_settings {
            let connection_pending = self.penpot.read(cx).connection_pending();
            let connection_needs_attention = self.penpot.read(cx).connection_needs_attention();
            let open_design = self
                .penpot_open_design
                .and_then(|(open_project, design_id)| {
                    (open_project == project)
                        .then(|| self.penpot.read(cx).design(project, design_id))
                        .flatten()
                });
            let title = open_design
                .as_ref()
                .map(|design| design.name.clone())
                .unwrap_or_else(|| "Designs".to_string());
            let back_to_designs = open_design.as_ref().map(|_| {
                style::header_icon_button("penpot-setup-back-to-designs", IconName::ArrowLeft, cx)
                    .tooltip("Back to project designs")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_penpot_hub(cx);
                    }))
            });
            let reconnect = connection_needs_attention.then(|| {
                style::refresh_button("penpot-setup-reconnect", "Reconnect", cx)
                    .disabled(connection_pending)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reconnect_penpot(cx);
                    }))
            });
            return v_flex()
                .size_full()
                .min_h(px(0.))
                .bg(crate::ui::design::base(cx))
                .child(
                    crate::ui::design::header::workspace_bar(cx)
                        .children(back_to_designs)
                        .child(
                            crate::ui::design::header::title_col(cx)
                                .child(crate::ui::design::header::title(title, cx))
                                .child(crate::ui::design::header::subline().child(
                                    crate::ui::design::indicator::subline_indicator(
                                        IconName::Globe,
                                        if connection_pending {
                                            "Connecting"
                                        } else {
                                            "Not connected"
                                        },
                                        if connection_pending {
                                            crate::ui::design::amber(cx)
                                        } else {
                                            crate::ui::design::rose(cx)
                                        },
                                        cx,
                                    ),
                                )),
                        )
                        .child(crate::ui::design::header::actions().children(reconnect)),
                )
                .child(self.render_penpot_setup(window, cx))
                .into_any_element();
        }

        let open_design_id = self
            .penpot_open_design
            .and_then(|(open_project, design_id)| (open_project == project).then_some(design_id));
        let Some(open_design_id) = open_design_id else {
            return self.render_penpot_hub(project, cx);
        };
        let selected_design = self.penpot.read(cx).design(project, open_design_id);
        if selected_design.is_none() {
            self.penpot_open_design = None;
            return self.render_penpot_hub(project, cx);
        }
        let design_implementors = selected_design
            .as_ref()
            .map(|design| self.penpot_design_implementors(design, cx))
            .unwrap_or_default();
        let active_implementor = design_implementors.first().cloned();
        let design_pr = self.penpot_design_pull_request(active_implementor.as_ref(), cx);
        let source_indicator = selected_design
            .as_ref()
            .and_then(|design| self.penpot_design_source_indicator(design, cx));
        let agent_indicator = selected_design.as_ref().and_then(|design| {
            active_implementor
                .as_ref()
                .map(|agent| self.penpot_design_agent_indicator(agent, design.id, cx))
        });
        let assistant = (self.penpot_assistant_open && selected_design.is_some()).then(|| {
            self.render_penpot_assistant(
                project,
                selected_design.as_ref().expect("selected design").clone(),
                window,
                cx,
            )
        });
        let connection_pending = self.penpot.read(cx).connection_pending();
        let connection_needs_attention = self.penpot.read(cx).connection_needs_attention();
        let title = selected_design
            .as_ref()
            .map(|design| design.name.clone())
            .unwrap_or_else(|| "Design".to_string());
        let mut header_indicators = Vec::new();
        if connection_needs_attention {
            header_indicators.push(
                crate::ui::design::indicator::subline_indicator(
                    IconName::Globe,
                    "Not connected",
                    crate::ui::design::rose(cx),
                    cx,
                )
                .into_any_element(),
            );
        } else if connection_pending {
            header_indicators.push(
                crate::ui::design::indicator::subline_indicator(
                    IconName::Globe,
                    "Connecting",
                    crate::ui::design::amber(cx),
                    cx,
                )
                .into_any_element(),
            );
        }
        header_indicators.extend(source_indicator);
        header_indicators.extend(agent_indicator);
        if let Some(pr) = design_pr.as_ref() {
            header_indicators.push(self.render_agent_ship_pr_indicator(pr, cx));
        }
        let back_to_designs =
            style::header_icon_button("penpot-back-to-designs", IconName::ArrowLeft, cx)
                .tooltip("Back to project designs")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_penpot_hub(cx);
                }));
        let implement_action = selected_design.as_ref().map(|design| {
            let design = design.clone();
            style::implement_button(
                ("implement-penpot-design", design.id.as_u128() as u64),
                if active_implementor.is_some() {
                    "Reimplement"
                } else {
                    "Implement"
                },
                cx,
            )
            .tooltip("Implement this design")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_implementation_agent_for_design(design.clone(), window, cx);
            }))
        });
        let reconnect_action = connection_needs_attention.then(|| {
            style::refresh_button("penpot-design-reconnect", "Reconnect", cx)
                .disabled(connection_pending)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reconnect_penpot(cx);
                }))
        });
        let browser_url = self.penpot.read(cx).selected_design_web_url(project);
        let open_in_browser_action = browser_url.as_ref().map(|url| {
            let url = url.clone();
            style::secondary_button_compact("penpot-open-in-browser", "Open in Browser")
                .icon(IconName::ExternalLink)
                .tooltip("Open this authenticated design in your default browser")
                .on_click(move |_, _, _| {
                    crate::ui::git::git_panel::open_url(&url);
                })
        });
        let copy_browser_link_action = browser_url.map(|url| {
            style::ghost_button_compact("penpot-copy-browser-link", "Copy Link")
                .icon(IconName::Copy)
                .tooltip("Copy the authenticated design link for another browser")
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                    window.push_notification(
                        Notification::success("Authenticated Design link copied"),
                        cx,
                    );
                })
        });
        let compare_action = if self.penpot_compare_open {
            style::accent_button_compact("penpot-close-compare", "Close Compare", cx)
                .icon(IconName::Replace)
                .tooltip("Return to the full design canvas")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_penpot_compare(cx);
                }))
        } else {
            style::context_panel_action_button(
                "penpot-open-compare",
                IconName::Replace,
                "Compare",
                cx,
            )
            .tooltip("Compare this design with the live project result")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_penpot_compare_open(project, true, cx);
            }))
        };
        let design_body = if selected_design.is_some() {
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
                            "penpot-loading",
                            0,
                            crate::ui::design::t3(cx),
                        ))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child("Loading design…"),
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
                })
                .into_any_element()
        } else {
            style::empty_state(
                crate::ui::design::design_icon(),
                "No design selected",
                "Use + New Design in the right panel, then select it here.",
                cx,
            )
            .into_any_element()
        };

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
                                crate::ui::design::header::subline().children(header_indicators),
                            ),
                    )
                    .child(
                        crate::ui::design::header::actions()
                            .children(reconnect_action)
                            .children(open_in_browser_action)
                            .children(copy_browser_link_action)
                            .child(compare_action)
                            .children(implement_action),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .when_some(assistant, |row, assistant| {
                        row.child(
                            div()
                                .flex_none()
                                .w(px(PENPOT_LEFT_SIDEBAR_WIDTH))
                                .h_full()
                                .child(assistant),
                        )
                        .child(
                            div()
                                .flex_none()
                                .w(px(1.))
                                .h_full()
                                .bg(crate::ui::design::line(cx).opacity(0.82)),
                        )
                    })
                    .child(design_body),
            )
            .into_any_element()
    }

    fn render_penpot_setup(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !self.penpot_editing_settings {
            let status = self.penpot.read(cx).status().clone();
            let error = match &status {
                PenpotConnectionStatus::Error(error) => Some(error.clone()),
                _ => None,
            };
            let message = match &status {
                PenpotConnectionStatus::NotChecked => {
                    "Design is ready to set up for this Choro installation."
                }
                PenpotConnectionStatus::Provisioning => {
                    "Creating and securely connecting your Design workspace…"
                }
                PenpotConnectionStatus::Checking => "Checking the Design connection…",
                PenpotConnectionStatus::Reachable => "Design is connected.",
                PenpotConnectionStatus::Error(_) => {
                    "Choro could not finish Design setup. Your projects are unchanged."
                }
            };
            return v_flex()
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .items_center()
                .justify_center()
                .p_6()
                .bg(crate::ui::design::base(cx))
                .child(
                    v_flex()
                        .w_full()
                        .max_w(px(520.))
                        .items_center()
                        .gap_3()
                        .p_5()
                        .rounded(crate::ui::design::r_lg())
                        .border_1()
                        .border_color(crate::ui::design::line(cx))
                        .bg(crate::ui::design::surface(cx))
                        .child(logo_spinner(
                            22.,
                            "penpot-provisioning",
                            0,
                            crate::ui::design::accent(cx),
                        ))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child("Preparing Design"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .text_center()
                                .child(message),
                        )
                        .when_some(error, |card, error| {
                            card.child(
                                div()
                                    .w_full()
                                    .rounded(crate::ui::design::r_sm())
                                    .border_1()
                                    .border_color(crate::ui::design::rose(cx).opacity(0.3))
                                    .bg(crate::ui::design::rose(cx).opacity(0.08))
                                    .p_2()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(error),
                            )
                            .child(
                                style::primary_button_compact(
                                    "penpot-provisioning-retry",
                                    "Try Again",
                                    cx,
                                )
                                .on_click({
                                    let penpot = self.penpot.clone();
                                    move |_, _, cx| {
                                        penpot.update(cx, |penpot, cx| {
                                            penpot.ensure_auto_provisioned(cx)
                                        });
                                    }
                                }),
                            )
                        }),
                )
                .into_any_element();
        }
        let has_stored_key = self.penpot.read(cx).has_key();
        let has_stored_access_token = self.penpot.read(cx).has_access_token();
        let status = self.penpot.read(cx).status().clone();
        let status_message = match &status {
            PenpotConnectionStatus::NotChecked => {
                "Save these values, then Choro will verify the Design connection.".to_string()
            }
            PenpotConnectionStatus::Provisioning => {
                "Preparing the managed Design connection…".to_string()
            }
            PenpotConnectionStatus::Checking => "Checking the Design connection…".to_string(),
            PenpotConnectionStatus::Reachable => {
                "Design is connected. New designs can now be created from Choro.".to_string()
            }
            PenpotConnectionStatus::Error(error) => error.clone(),
        };
        let instance_input = self.penpot_instance_input.clone();
        let mcp_input = self.penpot_mcp_input.clone();
        let key_input = self.penpot_key_input.clone();
        let access_token_input = self.penpot_access_token_input.clone();
        let docs_url = "https://help.penpot.app/mcp/".to_string();
        let account_url = self
            .penpot_instance_input
            .read(cx)
            .value()
            .trim()
            .to_string();

        v_flex()
            .w_full()
            .flex_1()
            .min_h(px(0.))
            .items_center()
            .justify_center()
            .p_6()
            .bg(crate::ui::design::base(cx))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(620.))
                    .gap_4()
                    .p_5()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Connect Design"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "The MCP key lets the Design Assistant edit. An access token lets Choro create and open new designs.",
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(penpot_field_label("Design service URL", cx))
                            .child(Input::new(&self.penpot_instance_input).w_full()),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(penpot_field_label("MCP server URL", cx))
                            .child(Input::new(&self.penpot_mcp_input).w_full())
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(
                                        "You may paste the full copied MCP URL; Choro separates and secures its userToken.",
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(penpot_field_label("Personal access token", cx))
                            .child(
                                Input::new(&self.penpot_access_token_input)
                                    .w_full()
                                    .mask_toggle(),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(if has_stored_access_token {
                                        "An access token is stored for the current Design host. Enter a token when changing hosts."
                                    } else {
                                        "Generate this under Your account → Access tokens. It is stored in Keychain."
                                    }),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(penpot_field_label("MCP key", cx))
                            .child(Input::new(&self.penpot_key_input).w_full().mask_toggle())
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(if has_stored_key {
                                        "A key is already stored in macOS Keychain. Leave blank to keep it."
                                    } else {
                                        "Stored in macOS Keychain, never in the project or agent history."
                                    }),
                            ),
                    )
                    .when_some(self.penpot_setup_error.clone(), |card, error| {
                        card.child(
                            div()
                                .rounded(crate::ui::design::r_sm())
                                .border_1()
                                .border_color(crate::ui::design::rose(cx).opacity(0.3))
                                .bg(crate::ui::design::rose(cx).opacity(0.08))
                                .p_2()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::rose(cx))
                                .child(error),
                        )
                    })
                    .when(self.penpot_setup_error.is_none(), |card| {
                        card.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    crate::ui::design::indicator::dot(match &status {
                                        PenpotConnectionStatus::Reachable => {
                                            crate::ui::design::sage(cx)
                                        }
                                        PenpotConnectionStatus::Error(_) => {
                                            crate::ui::design::rose(cx)
                                        }
                                        PenpotConnectionStatus::Provisioning => {
                                            crate::ui::design::amber(cx)
                                        }
                                        PenpotConnectionStatus::Checking => {
                                            crate::ui::design::amber(cx)
                                        }
                                        PenpotConnectionStatus::NotChecked => {
                                            crate::ui::design::t4(cx)
                                        }
                                    }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(status_message),
                                ),
                        )
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        style::ghost_button_compact(
                                            "penpot-open-account",
                                            "Open Design service",
                                        )
                                        .icon(IconName::ExternalLink)
                                        .on_click(move |_, _, _| {
                                            crate::ui::git::git_panel::open_url(&account_url)
                                        }),
                                    )
                                    .child(
                                        style::ghost_button_compact(
                                            "penpot-open-mcp-guide",
                                            "Setup guide",
                                        )
                                        .icon(IconName::BookOpen)
                                        .on_click(move |_, _, _| {
                                            crate::ui::git::git_panel::open_url(&docs_url)
                                        }),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .when(self.penpot_editing_settings, |actions| {
                                        actions.child(
                                            style::dialog_neutral_button(
                                                "penpot-settings-cancel",
                                                "Cancel",
                                                cx,
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.penpot_editing_settings = false;
                                                this.penpot_setup_error = None;
                                                cx.notify();
                                            })),
                                        )
                                    })
                                    .child(
                                        style::primary_button_compact(
                                            "penpot-save-connect",
                                            "Save & Connect",
                                            cx,
                                        )
                                        .icon(IconName::Globe)
                                        .disabled(matches!(
                                            self.penpot.read(cx).status(),
                                            PenpotConnectionStatus::Provisioning
                                                | PenpotConnectionStatus::Checking
                                        ))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let instance =
                                                instance_input.read(cx).value().to_string();
                                            let mcp = mcp_input.read(cx).value().to_string();
                                            let key = key_input.read(cx).value().to_string();
                                            let access_token =
                                                access_token_input.read(cx).value().to_string();
                                            let result = this.penpot.update(cx, |penpot, cx| {
                                                penpot.save_connection(
                                                    &instance,
                                                    &mcp,
                                                    &key,
                                                    &access_token,
                                                    cx,
                                                )
                                            });
                                            match result {
                                                Ok(()) => {
                                                    this.penpot_setup_error = None;
                                                    this.penpot_editing_settings = false;
                                                    let saved =
                                                        this.penpot.read(cx).config().clone();
                                                    instance_input.update(cx, |input, cx| {
                                                        input.set_value(
                                                            saved.instance_url,
                                                            window,
                                                            cx,
                                                        )
                                                    });
                                                    mcp_input.update(cx, |input, cx| {
                                                        input.set_value(
                                                            saved.mcp_url,
                                                            window,
                                                            cx,
                                                        )
                                                    });
                                                    key_input.update(cx, |input, cx| {
                                                        input.set_value("", window, cx)
                                                    });
                                                    access_token_input.update(
                                                        cx,
                                                        |input, cx| {
                                                            input.set_value("", window, cx)
                                                        },
                                                    );
                                                    this.penpot.update(cx, |penpot, cx| {
                                                        penpot.test_connection(cx)
                                                    });
                                                }
                                                Err(error) => {
                                                    this.penpot_setup_error =
                                                        Some(error.to_string());
                                                }
                                            }
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_penpot_assistant(
        &mut self,
        project: ProjectId,
        design: PenpotDesign,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((_, project_path)) = self.project_by_id(project, cx) else {
            return div().into_any_element();
        };
        let Some(conversation) = self.penpot.read(cx).current_conversation(project) else {
            return style::empty_state(
                crate::ui::design::design_icon(),
                "Could not open Design Assistant",
                "The conversation record could not be loaded.",
                cx,
            )
            .into_any_element();
        };
        let relative = penpot_assistant::conversation_record_path(design.id, conversation.id);
        let record = penpot_conversation_record(project, relative.clone(), &conversation);
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.upsert_external_record(record, cx)
        });
        self.hydrate_doc_assistant_chat_session(&record, &project_path, cx);
        let design_url = self
            .penpot
            .read(cx)
            .selected_design_url(project)
            .unwrap_or_default();
        let agent =
            Self::penpot_assistant_agent_record(&record, project_path, &design, &design_url);
        let is_running = self
            .agent_chats
            .read(cx)
            .session(record.chat_agent_id)
            .is_some_and(|session| {
                matches!(
                    session.status,
                    AgentChatStatus::Running | AgentChatStatus::Cancelling
                )
            });
        self.penpot.update(cx, |penpot, cx| {
            penpot.set_assistant_busy(project, is_running, cx)
        });
        let key = record.key();
        let error = self
            .doc_assistant_errors
            .get(&key)
            .cloned()
            .or_else(|| self.agent_start_errors.get(&record.chat_agent_id).cloned());
        let surface = AgentChatSurface::Design {
            project,
            design_id: design.id,
            conversation_id: conversation.id,
            relative_doc_path: relative.clone(),
        };
        if let Some(draft) = self.pending_design_assistant_drafts.remove(&design.id) {
            let input = self.agent_chat_input(&agent, surface.input_placeholder(), window, cx);
            if input.read(cx).value().trim().is_empty() {
                input.update(cx, |input, cx| {
                    input.set_value(draft.clone(), window, cx);
                    input.set_cursor_position(
                        input_position_for_byte_offset(&draft, draft.len()),
                        window,
                        cx,
                    );
                    input.focus(window, cx);
                });
            }
        }
        let conversations = self
            .penpot
            .read(cx)
            .conversations_for_selected_design(project);
        let center = cx.entity();
        let history_button =
            style::ghost_button_compact("design-assistant-history", &conversation.title)
                .w(px(190.))
                .justify_start()
                .dropdown_caret(true)
                .disabled(is_running)
                .dropdown_menu({
                    let center = center.clone();
                    move |mut menu, window, _| {
                        menu = menu.min_w(px(190.)).max_w(px(190.));
                        for item in conversations.clone() {
                            let center = center.clone();
                            let label = item.title.clone();
                            let checked = item.id == conversation.id;
                            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                                window.listener_for(&center, move |this: &mut Self, _, _, cx| {
                                    this.select_penpot_conversation(
                                        project, design.id, item.id, cx,
                                    );
                                }),
                            ));
                        }
                        menu
                    }
                });
        let close_assistant =
            style::sidebar_mode_icon_tab("close-design-assistant", IconName::PanelLeftClose, cx)
                .tooltip("Collapse design sidebar")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.web_host
                        .update(cx, |host, _| host.collapse_penpot_left_sidebar());
                    this.penpot_assistant_open = false;
                    cx.notify();
                }));
        let sidebar_tabs = style::sidebar_mode_tabs(cx)
            .font_family(crate::theme::UI_FONT_FAMILY)
            .rounded_tr(px(0.))
            .rounded_br(px(0.))
            .child(style::sidebar_mode_tab(
                "design-sidebar-agent",
                "AGENT",
                true,
                cx,
            ))
            .child(
                style::sidebar_mode_tab("design-sidebar-layers", "LAYERS", false, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.open_penpot_sidebar_tab(web_preview::PenpotSidebarTab::Layers, cx);
                    }),
                ),
            )
            .child(
                style::sidebar_mode_tab("design-sidebar-assets", "ASSETS", false, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.open_penpot_sidebar_tab(web_preview::PenpotSidebarTab::Assets, cx);
                    }),
                ),
            )
            .child(
                style::sidebar_mode_tab("design-sidebar-tokens", "TOKENS", false, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.open_penpot_sidebar_tab(web_preview::PenpotSidebarTab::Tokens, cx);
                    }),
                ),
            )
            .child(close_assistant);

        v_flex()
            .size_full()
            .min_w(px(0.))
            .bg(crate::ui::design::base(cx))
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                web_preview::restore_focus(window);
            })
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .pt_2()
                    .pb_0()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.42))
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w(px(0.))
                            .relative()
                            .top(px(-8.))
                            .left(px(5.))
                            .items_center()
                            .child(sidebar_tabs),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.42))
                    .bg(crate::ui::design::nav(cx))
                    .child(div().flex_1().min_w(px(0.)).child(history_button))
                    .child(
                        style::header_icon_button(
                            "new-design-assistant-conversation",
                            IconName::Plus,
                            cx,
                        )
                        .disabled(is_running)
                        .tooltip(if is_running {
                            "Wait for the current turn to finish"
                        } else {
                            "Start a fresh conversation for this design"
                        })
                        .on_click({
                            let design_id = design.id;
                            cx.listener(move |this, _, _, cx| {
                                this.create_new_penpot_conversation(
                                    project,
                                    design_id,
                                    record.chat_agent_id,
                                    cx,
                                );
                            })
                        }),
                    ),
            )
            .when_some(error, |panel, error| {
                panel.child(
                    div()
                        .mx_3()
                        .mt_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.28))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_agent_chat_body_for_surface(&agent, surface, window, cx)),
            )
            .into_any_element()
    }

    fn open_penpot_sidebar_tab(
        &mut self,
        tab: web_preview::PenpotSidebarTab,
        cx: &mut Context<Self>,
    ) {
        self.penpot_assistant_open = false;
        self.web_host
            .update(cx, |host, _| host.select_penpot_sidebar_tab(tab));
        cx.notify();
    }

    fn penpot_assistant_agent_record(
        record: &DocAssistantRecord,
        project_path: PathBuf,
        design: &PenpotDesign,
        design_url: &str,
    ) -> AgentRecord {
        let mut agent = AgentRecord::new(
            record.project_id,
            project_path,
            "Design Assistant",
            penpot_assistant::system_prompt_for_design(
                &design.name,
                design.id,
                design.penpot_file_id,
                design_url,
            ),
            record.provider,
            record.model,
            record.effort,
            record.access_mode,
        );
        agent.id = record.chat_agent_id;
        agent.runtime = AgentRuntimeKind::Chat;
        if record.provider == AgentKind::OpenCode {
            if let (Some(model_id), Some(model_label)) = (
                record.external_model_id.as_deref(),
                record.external_model_label.as_deref(),
            ) {
                agent.set_external_model(
                    model_id,
                    model_label,
                    record.external_model_variants.clone(),
                );
                agent.effort = record.effort;
            }
        }
        agent.hidden_doc_assistant = true;
        agent.design_context = Some(ide_core::AgentDesignContext {
            design_id: design.id,
            file_id: design.penpot_file_id,
        });
        agent.chat_session_id = record.chat_session_id.clone().or_else(|| {
            (record.provider == AgentKind::Codex)
                .then(|| record.cli_session_id.clone())
                .flatten()
        });
        agent.cli_session_id = record.cli_session_id.clone();
        agent
    }

    fn create_new_penpot_conversation(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        current_agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let running = self
            .agent_chats
            .read(cx)
            .session(current_agent_id)
            .is_some_and(|session| {
                matches!(
                    session.status,
                    AgentChatStatus::Running | AgentChatStatus::Cancelling
                )
            });
        if running {
            return;
        }
        match ide_core::local_store::LocalStore::open_default()
            .and_then(|store| store.create_penpot_conversation(design_id))
        {
            Ok(_) => {
                let refreshed = self.penpot.update(cx, |penpot, cx| {
                    penpot.set_assistant_busy(project, false, cx);
                    penpot.refresh_conversations(design_id, cx)
                });
                if let Err(error) = refreshed {
                    self.penpot_setup_error = Some(format!(
                        "Could not refresh the new design conversation: {error:#}"
                    ));
                }
                cx.notify();
            }
            Err(error) => {
                self.penpot_setup_error = Some(format!(
                    "Could not start a new design conversation: {error:#}"
                ));
                cx.notify();
            }
        }
    }

    fn select_penpot_conversation(
        &mut self,
        project: ProjectId,
        design_id: Uuid,
        conversation_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if self.penpot.read(cx).assistant_busy(project) {
            return;
        }
        if let Err(error) = ide_core::local_store::LocalStore::open_default()
            .and_then(|store| store.select_penpot_conversation(design_id, conversation_id))
        {
            self.penpot_setup_error =
                Some(format!("Could not open the design conversation: {error:#}"));
        } else {
            let refreshed = self
                .penpot
                .update(cx, |penpot, cx| penpot.refresh_conversations(design_id, cx));
            if let Err(error) = refreshed {
                self.penpot_setup_error = Some(format!(
                    "Could not refresh the design conversation: {error:#}"
                ));
            }
        }
        cx.notify();
    }
}

fn penpot_conversation_record(
    project: ProjectId,
    relative_doc_path: PathBuf,
    conversation: &ide_core::local_store::StoredPenpotDesignConversation,
) -> DocAssistantRecord {
    DocAssistantRecord {
        chat_agent_id: conversation.agent_id,
        project_id: project,
        relative_doc_path,
        provider: conversation.provider,
        model: conversation.model.clone(),
        external_model_id: conversation.external_model_id.clone(),
        external_model_label: conversation.external_model_label.clone(),
        external_model_variants: conversation.external_model_variants.clone(),
        effort: conversation.effort,
        access_mode: conversation.access_mode,
        chat_session_id: conversation.chat_session_id.clone(),
        cli_session_id: conversation.cli_session_id.clone(),
        last_transcript_path: conversation.last_transcript_path.clone(),
        pending_proposal: None,
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
    }
}

fn penpot_field_label(label: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(crate::ui::design::t2(cx))
        .child(label)
        .into_any_element()
}

fn short_design_chip_label(name: &str) -> String {
    let mut chars = name.chars();
    let prefix = chars.by_ref().take(10).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}
