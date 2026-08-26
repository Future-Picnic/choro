use super::*;

impl Render for CenterArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_voice_composer_actions(window, cx);
        // Ask History is a global destination: it remains useful before a
        // project is open and never leaves a native preview layered above it.
        if self.view_mode == CenterMode::QuickAskHistory {
            let web_preview_torn_down = self.web_host.update(cx, |host, _| host.set_intent(None));
            let compare_preview_torn_down = self
                .compare_web_host
                .update(cx, |host, _| host.set_intent(None));
            if web_preview_torn_down || compare_preview_torn_down {
                web_preview::restore_focus(window);
            }
            return self.render_quick_ask_history(window, cx);
        }
        let Some((project, _)) = self.active_project(cx) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(
                    gpui_component::Icon::new(IconName::FolderOpen)
                        .size_8()
                        .text_color(crate::ui::design::t3(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Open a project to get started"),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .child("⌘O or the button in the sidebar"),
                )
                .into_any_element();
        };
        self.terminals
            .update(cx, |terminals, cx| terminals.sync_theme(cx));
        let doc_messages = self
            .web_host
            .update(cx, |host, _| host.take_doc_editor_messages());
        for message in doc_messages {
            match message {
                web_preview::DocEditorMessage::Change { path, document } => {
                    if let Err(error) = self
                        .docs
                        .update(cx, |docs, cx| docs.apply_web_document(path, document, cx))
                    {
                        eprintln!("failed to apply document editor change: {error:#}");
                    }
                }
                web_preview::DocEditorMessage::OpenReference { path, target } => {
                    self.pending_reference_open = Some((path, target));
                }
                _ => {}
            }
        }
        let penpot_messages = self
            .web_host
            .update(cx, |host, _| host.take_penpot_messages());
        for message in penpot_messages {
            match message {
                web_preview::PenpotMessage::OpenAssistant => {
                    self.penpot_assistant_open = true;
                }
                web_preview::PenpotMessage::McpStatus {
                    connected, file_id, ..
                } => {
                    self.handle_design_mcp_status(connected, file_id, cx);
                }
                web_preview::PenpotMessage::ExportFinished { success, file_name } => {
                    let message = if success {
                        format!("Exported {file_name} to Downloads")
                    } else {
                        format!("Could not export {file_name}")
                    };
                    let notification = if success {
                        Notification::success(message)
                    } else {
                        Notification::error(message)
                    };
                    window.push_notification(notification, cx);
                }
            }
        }
        self.handle_project_preview_messages(window, cx);
        self.apply_pending_reference_open(window, cx);

        let has_terminals = self.has_terminal_content(project, cx);
        let effective_mode = self.effective_code_mode(project, has_terminals);

        // Reconcile the single in-app web preview: a URL is intended only when the
        // Assets context is visible and a web-URL reference is selected. Any other
        // state (different kind, Docs, another tab) tears the page down. `place`
        // (from the reserved region's canvas) builds/positions it.
        //
        // The WKWebView is a native view layered ABOVE all GPUI content, so while
        // a dialog or sheet is open it would cover that overlay and steal keyboard
        // first-responder (breaking typing until app restart). Suppress it then so
        // dialogs stay interactive.
        let overlay_open = window.has_active_dialog(cx) || window.has_active_sheet(cx);
        let penpot_compare_active = !overlay_open
            && effective_mode == CenterMode::Design
            && self.penpot_compare_open
            && self
                .penpot_open_design
                .is_some_and(|(open_project, _)| open_project == project);
        let project_preview_intent = if !overlay_open {
            self.project_preview_intent(project, cx)
        } else {
            None
        };
        let compare_preview_intent = if penpot_compare_active {
            project_preview_intent.clone()
        } else {
            None
        };
        let web_preview_intent = if !penpot_compare_active && project_preview_intent.is_some() {
            project_preview_intent
        } else if !overlay_open
            && effective_mode == CenterMode::Docs
            && self.context_mode == ContextMode::Docs
        {
            let selected_doc = self.docs.read(cx).selected_doc(project);
            selected_doc.and_then(|doc| {
                let document = self
                    .docs
                    .update(cx, |docs, _| docs.web_document_for_path(&doc.path))
                    .ok()?;
                let files = self
                    .workspace_file_entries(project, &doc.project_path, cx)
                    .into_iter()
                    .map(|entry| web_preview::DocEditorMention {
                        label: entry.name,
                        target: format!("ref:file:{}", entry.relative_label),
                        detail: entry.relative_label,
                        kind: "file".to_string(),
                        badge: "File".to_string(),
                        preview_url: None,
                        preview_path: None,
                    })
                    .collect::<Vec<_>>();
                let assets = self
                    .designs
                    .read(cx)
                    .references_for_project(project)
                    .into_iter()
                    .map(|reference| {
                        let badge =
                            crate::ui::designs_panel::design_kind_label(reference.kind).to_string();
                        let preview_path =
                            crate::state::designs::reference_absolute_preview_path(&reference)
                                .filter(|path| path.is_file());
                        let preview_url = preview_path
                            .as_ref()
                            .map(|_| format!("choro-reference://localhost/{}", reference.id));
                        web_preview::DocEditorMention {
                            label: reference.title,
                            target: format!("ref:design:{}", reference.id),
                            detail: reference.source,
                            kind: "asset".to_string(),
                            badge,
                            preview_url,
                            preview_path,
                        }
                    })
                    .collect::<Vec<_>>();
                Some(web_preview::WebPreviewIntent::DocEditor {
                    path: doc.path,
                    document,
                    files,
                    assets,
                    theme: web_preview::DocEditorTheme::from_app(cx),
                })
            })
        } else if !overlay_open
            && effective_mode == CenterMode::Docs
            && self.context_mode == ContextMode::Designs
        {
            self.designs
                .read(cx)
                .selected_reference(project)
                .and_then(|reference| designs::design_web_preview_url(&reference))
                .map(web_preview::WebPreviewIntent::Url)
        } else if !overlay_open
            && effective_mode == CenterMode::Design
            && self
                .figma_open_design
                .is_some_and(|(open_project, _)| open_project == project)
        {
            self.figma_open_design
                .and_then(|(_, reference_id)| {
                    self.designs.read(cx).reference(project, reference_id)
                })
                .and_then(|reference| designs::design_web_preview_url(&reference))
                .map(web_preview::WebPreviewIntent::Url)
        } else if !overlay_open
            && effective_mode == CenterMode::Design
            && self
                .penpot_open_design
                .is_some_and(|(open_project, _)| open_project == project)
            && self.penpot.read(cx).is_configured()
            && !self.penpot_editing_settings
        {
            self.penpot
                .read(cx)
                .selected_design_web_url(project)
                .map(|url| web_preview::WebPreviewIntent::PenpotUrl {
                    url,
                    theme: web_preview::PenpotTheme::from_app(cx),
                })
        } else if !overlay_open && effective_mode == CenterMode::Agents {
            self.agents
                .read(cx)
                .selected_agent(project)
                .and_then(|agent| self.active_chat_visualization_intent(&agent, cx))
        } else {
            None
        };
        let penpot_assistant_open = effective_mode == CenterMode::Design
            && self.penpot_open_design.is_some()
            && self.penpot_assistant_open;
        let composer_has_linked_design = self.new_agent_composer.as_ref().is_some_and(|composer| {
            composer
                .selected_mentions
                .iter()
                .any(|mention| mention.kind == ComposerMentionKind::PenpotDesign)
        });
        let selected_agent_has_linked_design = self
            .agents
            .read(cx)
            .selected_agent(project)
            .is_some_and(|agent| !self.penpot.read(cx).designs_for_agent(&agent).is_empty());
        let external_browser_owns_mcp = self
            .penpot_external_mcp_design
            .is_some_and(|(open_project, _)| open_project == project)
            && (composer_has_linked_design || selected_agent_has_linked_design);
        let dedicated_design_session_active =
            self.design_mcp_readiness
                .iter()
                .any(|(design_id, readiness)| {
                    !matches!(readiness, DesignMcpReadiness::Blocked(_))
                        && self
                            .penpot_open_design
                            .is_some_and(|(_, open_design_id)| open_design_id == *design_id)
                });
        let keep_penpot_connected = dedicated_design_session_active
            || (effective_mode == CenterMode::Agents
                && (composer_has_linked_design || selected_agent_has_linked_design)
                && !external_browser_owns_mcp);
        let web_preview_torn_down = self.web_host.update(cx, |host, _| {
            host.set_penpot_assistant_open(penpot_assistant_open);
            host.set_penpot_compare_open(penpot_compare_active);
            host.set_penpot_keepalive(keep_penpot_connected);
            host.set_intent(web_preview_intent)
        });
        let compare_preview_torn_down = self
            .compare_web_host
            .update(cx, |host, _| host.set_intent(compare_preview_intent));
        if web_preview_torn_down || compare_preview_torn_down {
            // The native webview held the keyboard; hand focus back to GPUI so
            // dialogs / text fields stop rejecting keystrokes.
            web_preview::restore_focus(window);
        }

        let body: gpui::AnyElement = match effective_mode {
            CenterMode::Files => {
                let editors = self.render_editor_section(project, cx);
                editors.unwrap_or_else(|| {
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(IconName::File)
                                .size_8()
                                .text_color(crate::ui::design::t3(cx)),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child("No files open"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child("Pick a file from the Files tab on the right"),
                        )
                        .into_any_element()
                })
            }
            CenterMode::Terminal => self.render_terminal_section(project, cx),
            CenterMode::Agents => self.render_agent_section(project, window, cx),
            CenterMode::PocketComet => self.render_pocketcomet_section(cx),
            CenterMode::Tasks | CenterMode::MyTasks => {
                self.render_tasks_section(project, window, cx)
            }
            CenterMode::QuickAskHistory => self.render_quick_ask_history(window, cx),
            CenterMode::Db => self.render_db_section(project, cx),
            CenterMode::Services => self.render_services_section(project, cx),
            CenterMode::Docs => self.render_context_section(project, window, cx),
            CenterMode::Design => self.render_penpot_section(project, window, cx),
            CenterMode::Split => {
                let editor_section = self.render_editor_section(project, cx);
                let terminal_section = self.render_terminal_section(project, cx);
                match (editor_section, has_terminals) {
                    // Files open: editors on top, terminals in a resizable bottom section.
                    (Some(editors), true) => v_resizable("editor-terminal-split")
                        .child(div().size_full().child(editors).into_any_element())
                        .child(
                            resizable_panel()
                                .size(px(300.))
                                .size_range(px(120.)..px(800.))
                                .child(
                                    div().size_full().child(terminal_section).into_any_element(),
                                ),
                        )
                        .into_any_element(),
                    // No terminals open: files get the whole center.
                    (Some(editors), false) => editors,
                    // No files open: terminals get the whole center.
                    (None, _) => terminal_section,
                }
            }
        };

        let show_project_preview = self.is_project_preview_open(project);
        let project_preview_ratio = self
            .project_preview_panel_ratio
            .clamp(0.0, PROJECT_PREVIEW_PANEL_MAX_RATIO);
        let preview_layout_view = cx.entity().clone();
        let project_preview_panel =
            show_project_preview.then(|| self.render_project_preview_panel(project, window, cx));

        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &SaveFile, window, cx| {
                this.save_active(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.close_selected(window, cx);
            }))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        h_flex()
                            .size_full()
                            .min_h(px(0.))
                            .child(div().flex_1().min_w(px(0.)).size_full().child(body))
                            .when_some(project_preview_panel, |row, panel| {
                                row.child(
                                    div()
                                        .relative()
                                        .flex_none()
                                        .w(gpui::relative(project_preview_ratio))
                                        .h_full()
                                        .border_l_1()
                                        .border_color(crate::ui::design::line(cx).opacity(0.34))
                                        // Keep a slim GPUI-owned strip between the
                                        // center workspace and the native WebView.
                                        // The WKWebView can never cover this gutter,
                                        // so resizing works for the panel's full height.
                                        .bg(crate::ui::design::base(cx))
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .absolute()
                                                .top(px(0.))
                                                .right(px(0.))
                                                .bottom(px(0.))
                                                .left(px(PROJECT_PREVIEW_RESIZE_GUTTER))
                                                .child(panel),
                                        )
                                        .child(self.project_preview_resize_handle(cx)),
                                )
                            }),
                    )
                    .when(show_project_preview, |layout| {
                        layout.child(
                            canvas(
                                move |bounds, _, cx| {
                                    let available_width = bounds.size.width.as_f32();
                                    preview_layout_view.update(cx, |this, cx| {
                                        if (this.project_preview_available_width - available_width)
                                            .abs()
                                            > 0.5
                                        {
                                            this.project_preview_available_width = available_width;
                                            cx.notify();
                                        }
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                    }),
            )
            .into_any_element()
    }
}
