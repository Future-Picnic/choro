#![allow(dead_code, reason = "retained document-assistant drawer")]

use super::*;

impl CenterArea {
    pub(super) fn doc_assistant_input(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.doc_assistant_inputs.get(key) {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Ask about this doc"));
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.doc_assistant_inputs
            .insert(key.to_string(), input.clone());
        input
    }

    pub(super) fn send_doc_assistant_message(
        &mut self,
        project: ProjectId,
        project_path: PathBuf,
        relative_doc_path: PathBuf,
        message: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let message = message.trim().to_string();
        if message.is_empty() {
            return false;
        }
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, relative_doc_path.clone(), cx)
        });
        let key = record.key();
        let prompt = doc_assistant::user_prompt(&relative_doc_path, &message);
        if !self.ensure_doc_assistant_chat_backend(&record, project_path, cx) {
            return false;
        }
        self.doc_assistant_errors.remove(&key);
        self.dispatch_agent_chat_submission(
            record.chat_agent_id,
            prompt,
            AgentInteractionMode::Default,
            cx,
        );
        cx.notify();
        true
    }

    pub(super) fn stop_doc_assistant_chat(
        &mut self,
        record: &DocAssistantRecord,
        cx: &mut Context<Self>,
    ) {
        self.sync_doc_assistant_chat_session_ids(cx);
        self.agent_chats.update(cx, |chats, cx| {
            chats.stop_backend(record.chat_agent_id, cx);
        });
        cx.notify();
    }

    pub(super) fn reset_doc_assistant_chat(
        &mut self,
        project: ProjectId,
        relative_doc_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if let Some(record) = self
            .doc_assistants
            .read(cx)
            .record_for(project, &relative_doc_path)
        {
            self.agent_chats.update(cx, |chats, cx| {
                chats.reset_session(record.chat_agent_id, cx);
            });
            let key = record.key();
            self.doc_assistant_list_states.remove(&key);
            self.doc_assistant_errors.remove(&key);
            self.agent_start_errors.remove(&record.chat_agent_id);
            self.agent_chat_inputs.remove(&record.chat_agent_id);
            self.agent_chat_attached_files.remove(&record.chat_agent_id);
            self.agent_chat_pasted_text_blocks
                .remove(&record.chat_agent_id);
            self.agent_chat_selected_commands
                .remove(&record.chat_agent_id);
            self.agent_chat_selected_mentions
                .remove(&record.chat_agent_id);
        }
        self.doc_assistants.update(cx, |assistants, cx| {
            assistants.reset_record(project, relative_doc_path, cx);
        });
        cx.notify();
    }

    pub(super) fn doc_assistant_list_state(&mut self, key: &str, row_count: usize) -> ListState {
        let state = self
            .doc_assistant_list_states
            .entry(key.to_string())
            .or_insert_with(|| ListState::new(row_count, ListAlignment::Bottom, px(420.)))
            .clone();
        let old_count = state.item_count();
        match old_count.cmp(&row_count) {
            std::cmp::Ordering::Less => state.splice(old_count..old_count, row_count - old_count),
            std::cmp::Ordering::Greater => state.splice(row_count..old_count, 0),
            std::cmp::Ordering::Equal => {}
        }
        state
    }

    pub(super) fn render_doc_assistant_panel(
        &mut self,
        project: ProjectId,
        doc: &WorkspaceDocEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let relative_doc_path = doc.relative_path.clone();
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, relative_doc_path.clone(), cx)
        });
        let key = record.key();
        self.hydrate_doc_assistant_chat_session(&record, &doc.project_path, cx);
        let agent = Self::doc_assistant_agent_record(&record, doc.project_path.clone());
        let error = self
            .doc_assistant_errors
            .get(&key)
            .cloned()
            .or_else(|| self.agent_start_errors.get(&record.chat_agent_id).cloned());
        let surface = AgentChatSurface::Document {
            project,
            relative_doc_path: relative_doc_path.clone(),
        };

        v_flex()
            .size_full()
            .min_w(px(0.))
            .bg(crate::ui::design::base(cx))
            // WKWebView is a native AppKit child view and can remain the
            // window's first responder after the user clicks back into GPUI.
            // Hand native keyboard ownership to GPUI before the clicked chat
            // input applies its normal FocusHandle, otherwise keystrokes keep
            // going into the BlockNote editor on the left.
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                web_preview::restore_focus(window);
            })
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.42))
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .child("Doc Assistant"),
                    )
                    .child(
                        crate::ui::style::ghost_button_compact("reset-doc-assistant-chat", "Reset")
                            .on_click({
                                let relative = relative_doc_path.clone();
                                cx.listener(move |this, _, _, cx| {
                                    this.reset_doc_assistant_chat(project, relative.clone(), cx);
                                })
                            }),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            "close-doc-assistant-panel",
                            IconName::Close,
                            cx,
                        )
                        .tooltip("Close assistant")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.docs_focus_mode = DocsFocusMode::Doc;
                            cx.notify();
                        })),
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

    /// The `@`/`@@` mention picker for the doc editor. `@` lists project files;
    /// `@@` lists docs + designs. Selecting inserts a Markdown link (or, for a
    /// design with a preview, an inline image) at the typed mention via velotype's
    /// paste pipeline.
    /// Compute the `@`/`@@` mention results for the editor's current query and
    /// hand them to velotype, which renders the caret-anchored overlay and
    /// drives keyboard navigation. Called every render; pushes an empty list
    /// when no mention is active so the overlay clears.
    pub(super) fn sync_doc_mention_candidates(
        &mut self,
        project: ProjectId,
        editor: Entity<VelotypeEditor>,
        cx: &mut Context<Self>,
    ) {
        let Some(mention) = editor.read(cx).embedded_mention_query(cx) else {
            editor.update(cx, |editor, cx| {
                editor.embedded_set_mention_candidates(Vec::new(), cx);
            });
            return;
        };

        let query = mention.query.to_ascii_lowercase();
        let mut candidates: Vec<velotype::MentionCandidate> = Vec::new();

        match mention.trigger {
            velotype::MentionTrigger::File => {
                if let Some((_, root)) = self.project_by_id(project, cx) {
                    // Filter on the typed text, then rank like the chat `@`
                    // picker: name-prefix matches first, then by path, capped.
                    let mut matches = self
                        .workspace_file_entries(project, &root)
                        .into_iter()
                        .filter(|file| {
                            query.is_empty()
                                || file.name.to_ascii_lowercase().contains(&query)
                                || file.relative_label.to_ascii_lowercase().contains(&query)
                        })
                        .collect::<Vec<_>>();
                    matches.sort_by(|left, right| {
                        let lhs = left.name.to_ascii_lowercase().starts_with(&query);
                        let rhs = right.name.to_ascii_lowercase().starts_with(&query);
                        rhs.cmp(&lhs).then_with(|| {
                            left.relative_label
                                .to_ascii_lowercase()
                                .cmp(&right.relative_label.to_ascii_lowercase())
                        })
                    });
                    for file in matches.into_iter().take(8) {
                        candidates.push(velotype::MentionCandidate {
                            label: file.name.clone(),
                            sublabel: file.relative_label.clone(),
                            badge: "File".to_string(),
                            markdown: format!("[{}](ref:file:{})", file.name, file.relative_label),
                            is_block: false,
                        });
                    }
                }
            }
            velotype::MentionTrigger::DocOrDesign => {
                for doc in self.docs.read(cx).docs_for_project(project) {
                    if !query.is_empty() && !doc.title.to_ascii_lowercase().contains(&query) {
                        continue;
                    }
                    let relative = doc.relative_path.to_string_lossy().to_string();
                    let status = self.docs.read(cx).doc_status(project, &doc.relative_path);
                    let mut tags = Vec::new();
                    if let Some(label) = self.docs.read(cx).doc_label(project, &doc.relative_path) {
                        tags.push(label);
                    }
                    let data = velotype::ReferenceData {
                        kind: "doc".to_string(),
                        title: doc.title.clone(),
                        target: format!("ref:doc:{}", relative),
                        subtitle: Some(relative.clone()),
                        preview: None,
                        badge: Some("Doc".to_string()),
                        status: (status != AgentStatus::Backlog)
                            .then(|| status.label().to_string()),
                        tags,
                        open_label: Some("Open doc".to_string()),
                    };
                    candidates.push(velotype::MentionCandidate {
                        label: doc.title.clone(),
                        sublabel: relative,
                        badge: "Doc".to_string(),
                        markdown: velotype::reference_fence_markdown(&data),
                        is_block: true,
                    });
                    if candidates.len() >= 6 {
                        break;
                    }
                }
                let mut design_count = 0;
                for reference in self.designs.read(cx).references_for_project(project) {
                    if !query.is_empty()
                        && !reference.title.to_ascii_lowercase().contains(&query)
                        && !reference.source.to_ascii_lowercase().contains(&query)
                    {
                        continue;
                    }
                    let preview =
                        crate::state::designs::reference_absolute_preview_path(&reference)
                            .filter(|path| path.is_file())
                            .map(|path| path.to_string_lossy().to_string());
                    let kind_label = crate::ui::designs_panel::design_kind_label(reference.kind);
                    let mut tags = Vec::new();
                    if let Some(folder) = crate::state::designs::reference_folder(&reference) {
                        tags.push(folder);
                    }
                    // A plain uploaded image has nothing external to open, so its
                    // primary target is the in-app image modal. Figma/URL designs
                    // keep their source as the navigate target.
                    let is_pure_image = reference.kind == ide_core::ProjectReferenceKind::Image;
                    let target = match (&preview, is_pure_image) {
                        (Some(path), true) => format!("ref:image:{path}"),
                        _ => format!("ref:design:{}", reference.id),
                    };
                    let data = velotype::ReferenceData {
                        kind: "design".to_string(),
                        title: reference.title.clone(),
                        target,
                        subtitle: Some(reference.source.clone()),
                        preview,
                        badge: Some(kind_label.to_string()),
                        status: None,
                        tags,
                        open_label: Some(format!("Open in {kind_label}")),
                    };
                    candidates.push(velotype::MentionCandidate {
                        label: reference.title.clone(),
                        sublabel: reference.source.clone(),
                        badge: kind_label.to_string(),
                        markdown: velotype::reference_fence_markdown(&data),
                        is_block: true,
                    });
                    design_count += 1;
                    if design_count >= 6 {
                        break;
                    }
                }
                if design_count < 6 {
                    let penpot = self.penpot.read(cx);
                    for design in penpot.designs_for_project(project) {
                        let Some(source) = penpot.design_url(&design) else {
                            continue;
                        };
                        if !query.is_empty()
                            && !design.name.to_ascii_lowercase().contains(&query)
                            && !source.to_ascii_lowercase().contains(&query)
                        {
                            continue;
                        }
                        let data = velotype::ReferenceData {
                            kind: "penpot_design".to_string(),
                            title: design.name.clone(),
                            target: format!("ref:penpot:{}", design.id),
                            subtitle: Some(source.clone()),
                            preview: None,
                            badge: Some("Design".to_string()),
                            status: None,
                            tags: vec![format!("File {}", design.penpot_file_id)],
                            open_label: Some("Open Design".to_string()),
                        };
                        candidates.push(velotype::MentionCandidate {
                            label: design.name.clone(),
                            sublabel: source,
                            badge: "Design".to_string(),
                            markdown: velotype::reference_fence_markdown(&data),
                            is_block: true,
                        });
                        design_count += 1;
                        if design_count >= 6 {
                            break;
                        }
                    }
                }
            }
        }

        editor.update(cx, |editor, cx| {
            editor.embedded_set_mention_candidates(candidates, cx);
        });
    }
}
