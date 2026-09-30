use super::*;
use ide_core::studio::{
    StudioDesignManifest, StudioHandoff, StudioOperation, StudioSource, StudioStore,
    StudioTransaction, StudioTurnScope,
};

/// Lightweight presentation data from the same frozen snapshot sent to the agent.
/// Rendering a composer must not reopen the design or load its full asset bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StudioAttachment {
    pub revision: u64,
    pub screen_names: Vec<String>,
    pub system_name: Option<String>,
    pub preview: Option<std::sync::Arc<gpui::Image>>,
}

impl StudioAttachment {
    fn from_handoff(handoff: &StudioHandoff) -> Self {
        Self {
            revision: handoff.design.manifest.revision,
            screen_names: handoff
                .design
                .manifest
                .screens
                .iter()
                .map(|screen| screen.name.clone())
                .collect(),
            system_name: handoff
                .design_system_context
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            // Reuse one selected screen's frozen thumbnail; never render live
            // HTML or load a whole design just to decorate its context tag.
            preview: handoff
                .design
                .manifest
                .screens
                .iter()
                .filter_map(|screen| handoff.thumbnails.get(&screen.id))
                .find_map(|bytes| {
                    if bytes.len() > 1024 * 1024 {
                        return None;
                    }
                    let (width, height) = image::ImageReader::with_format(
                        std::io::Cursor::new(bytes),
                        image::ImageFormat::Png,
                    )
                    .into_dimensions()
                    .ok()?;
                    if u64::from(width) * u64::from(height) > 1024 * 1024 {
                        return None;
                    }
                    Some(std::sync::Arc::new(gpui::Image::from_bytes(
                        gpui::ImageFormat::Png,
                        bytes.clone(),
                    )))
                }),
        }
    }

    pub(super) fn screen_label(&self) -> String {
        match self.screen_names.len() {
            1 => "1 screen".into(),
            count => format!("{count} screens"),
        }
    }

    pub(super) fn system_label(&self) -> String {
        self.system_name
            .as_ref()
            .map(|name| format!("Design system: {name}"))
            .unwrap_or_else(|| "No design system selected".into())
    }
}

pub(super) fn studio_implementation_request(handoff: &StudioHandoff) -> String {
    if let Some(name) = handoff
        .design_system_context
        .get("name")
        .and_then(serde_json::Value::as_str)
    {
        format!("Implement this design using the {name} design system.")
    } else {
        "Implement this design using its screen styles.".into()
    }
}

fn source_design_name(title: &str) -> String {
    let mut name = String::new();
    for character in title.trim().chars().filter(|c| !c.is_control()) {
        if name.len() + character.len_utf8() > 153 {
            break;
        }
        name.push(character);
    }
    if name.is_empty() {
        "New design".into()
    } else {
        format!("{} Design", name.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_snapshot_mentions_keep_durable_identity_without_browser_setup() {
        let temp = tempfile::tempdir().unwrap();
        let store = StudioStore::new(temp.path(), temp.path().join("cache")).unwrap();
        let design = store.create("Checkout").unwrap();
        let snapshot = store.handoff(design.manifest.id, None).unwrap();
        let token = ComposerMentionToken::studio_design(&snapshot);
        assert_eq!(token.studio_handoff_id(), Some(snapshot.id));
        assert_eq!(token.chip_label(), "Checkout");
        let tags = composer_message_tags(None, &[token.clone()], false);
        assert_eq!(tags.len(), 1);
        let submission = composer_mentions_submission_text("Implement checkout", &[token], &[]);
        assert!(submission.contains("studio_handoff_read"));
        assert!(submission.contains(&snapshot.id.to_string()));
        assert!(!submission.contains("penpot"));
        assert!(!submission.contains("default browser"));
        let agent = Uuid::new_v4();
        store.link_implementation_agent(snapshot.id, agent).unwrap();
        assert_eq!(
            store.implementation_agents(design.manifest.id).unwrap(),
            vec![agent]
        );
    }

    #[test]
    fn task_and_doc_titles_produce_valid_studio_names() {
        assert_eq!(source_design_name("Checkout"), "Checkout Design");
        let title = "ע".repeat(200);
        let name = source_design_name(&title);
        assert!(name.len() <= 160);
        assert!(name.ends_with(" Design"));
        assert!(!source_design_name("Billing\nflow").contains('\n'));
    }

    #[test]
    fn implementation_keeps_system_instructions_behind_readable_context() {
        let temp = tempfile::tempdir().unwrap();
        let store = StudioStore::new(temp.path(), temp.path().join("cache")).unwrap();
        let system = store.create_system("Choro", "Desktop", None).unwrap();
        let draft = store.load(system.id).unwrap();
        let scope = ide_core::studio::scope_for_request(&draft, Some(system.id), None);
        let draft = store
            .apply(
                &scope,
                &StudioTransaction {
                    id: Uuid::new_v4(),
                    scope_id: scope.id,
                    design_id: system.id,
                    expected_revision: draft.manifest.revision,
                    expected_fingerprint: draft.fingerprint,
                    operations: vec![StudioOperation::SetSystem {
                        system: ide_core::studio::StudioDesignSystem::default(),
                        expected_system_revision: draft.system.revision,
                    }],
                },
            )
            .unwrap();
        store
            .publish_system(system.id, draft.manifest.revision, &draft.fingerprint, &[])
            .unwrap();
        store.set_default_system(Some(system.id)).unwrap();
        let design = store.create("Calorie counter").unwrap();
        let snapshot = store.handoff(design.manifest.id, None).unwrap();
        let token = ComposerMentionToken::studio_design(&snapshot);
        let request = studio_implementation_request(&snapshot);
        assert_eq!(
            request,
            "Implement this design using the Choro design system."
        );
        let attachment = token.studio_attachment.as_ref().unwrap();
        assert_eq!(attachment.system_name.as_deref(), Some("Choro"));
        assert_eq!(attachment.screen_names.len(), snapshot.screen_ids.len());
        // The user can rewrite their request without dropping the design-system contract.
        let edited = "Build this and connect the calorie totals to our API.";
        assert_eq!(
            composer_message_display_text(edited, None, &[token.clone()]),
            edited
        );
        let submitted = composer_mentions_submission_text(edited, &[token.clone()], &[]);
        assert!(submitted.contains(&format!("handoff_id \"{}\"", snapshot.id)));
        assert!(submitted.contains("section=\"tokens\""));
        assert!(submitted.contains("Use the applied design system \"Choro\""));
        assert!(submitted.contains("per-design overrides"));
        assert!(submitted.ends_with(edited));
        assert!(composer_message_tags(None, &[token], false)[0]
            .detail
            .as_ref()
            .unwrap()
            .contains("Design system: Choro"));
        assert!(!request.contains(&snapshot.id.to_string()));
        assert!(!request.contains("studio_handoff_read"));
        assert_eq!(composer_mentions_submission_text(edited, &[], &[]), edited);
    }

    #[test]
    fn implementation_without_a_system_uses_local_styles_and_a_bounded_snapshot_preview() {
        let temp = tempfile::tempdir().unwrap();
        let store = StudioStore::new(temp.path(), temp.path().join("cache")).unwrap();
        let design = store.create("Local styles").unwrap();
        let mut snapshot = store.handoff(design.manifest.id, None).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let screen = snapshot.screen_ids[0];
        snapshot.thumbnails.insert(screen, png.into_inner());
        let token = ComposerMentionToken::studio_design(&snapshot);
        let attachment = token.studio_attachment.as_ref().unwrap();
        assert_eq!(attachment.system_name, None);
        assert_eq!(attachment.system_label(), "No design system selected");
        assert_eq!(
            attachment.preview.as_ref().unwrap().bytes,
            snapshot.thumbnails[&screen]
        );
        assert!(token
            .context
            .as_ref()
            .unwrap()
            .contains("effective local tokens and screen styles"));
        assert_eq!(
            studio_implementation_request(&snapshot),
            "Implement this design using its screen styles."
        );
        // Never show a thumbnail belonging to an unselected screen.
        snapshot.thumbnails = [(Uuid::new_v4(), snapshot.thumbnails[&screen].clone())].into();
        assert!(StudioAttachment::from_handoff(&snapshot).preview.is_none());
        let mut oversized = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(1025, 1024)
            .write_to(&mut oversized, image::ImageFormat::Png)
            .unwrap();
        snapshot.thumbnails.insert(screen, oversized.into_inner());
        assert!(StudioAttachment::from_handoff(&snapshot).preview.is_none());
    }
}

impl ComposerMentionToken {
    pub(super) fn studio_design(handoff: &StudioHandoff) -> Self {
        let attachment = StudioAttachment::from_handoff(handoff);
        let system_instruction = if let Some(name) = attachment.system_name.as_ref() {
            format!("Use the applied design system {name:?} captured in this snapshot: its effective tokens, component recipes, typography/font assets, and per-design overrides are the visual source of truth. Reuse compatible repository components and map the captured system into the project's framework. Do not substitute a generic theme, fetch a newer system revision, or modify the Studio design system.")
        } else {
            "No design system is selected for this snapshot. Use its effective local tokens and screen styles with the repository's existing components; do not attach an unrelated project system automatically.".into()
        };
        Self {
            kind: ComposerMentionKind::StudioDesign,
            title: handoff.design.manifest.name.clone(),
            path_label: format!("Studio · {} · {} · version {}", attachment.screen_label(), attachment.system_label(), attachment.revision),
            context: Some(format!(
                "@@studio-handoff:{}\nRead studio_handoff_read with handoff_id \"{}\" before implementing this design. Read the manifest, section=\"tokens\" (design system, effective tokens, recipes, and fonts), section=\"assets\", and each selected screen. This immutable Studio snapshot includes the selected screens and source requirements. {} {}\n",
                handoff.id, handoff.id, system_instruction, handoff.instruction
            )),
            project_id: None,
            studio_attachment: Some(attachment),
        }
    }

    pub(super) fn studio_handoff_id(&self) -> Option<Uuid> {
        if self.kind != ComposerMentionKind::StudioDesign {
            return None;
        }
        self.context
            .as_ref()?
            .lines()
            .next()?
            .strip_prefix("@@studio-handoff:")?
            .parse()
            .ok()
    }
}

impl NewAgentComposer {
    pub(super) fn remove_context_mention(&mut self, index: usize) {
        if index >= self.selected_mentions.len() {
            return;
        }
        let removed = self.selected_mentions.remove(index);
        if removed.kind == ComposerMentionKind::Doc {
            self.linked_docs
                .retain(|path| path.to_string_lossy() != removed.path_label);
        }
        if removed
            .studio_handoff_id()
            .is_some_and(|id| self.implementation_target == Some(ImplementationTarget::Studio(id)))
        {
            self.implementation_target = None;
        }
        self.error = None;
    }

    /// An explicit source implementation replaces an older restored draft's
    /// design target. In particular, Studio must never inherit a Penpot launch.
    pub(super) fn reset_source_implementation(&mut self) {
        self.implementation_target = None;
        self.studio_attachment_error = None;
        self.source_doc = None;
        self.source_task = None;
        self.linked_docs.clear();
        self.linked_tasks.clear();
        self.selected_mentions.retain(|mention| {
            !matches!(
                mention.kind,
                ComposerMentionKind::StudioDesign
            )
        });
    }
}

impl CenterArea {
    pub(super) fn render_studio_composer_attachment(
        &self,
        index: usize,
        mention: &ComposerMentionToken,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(attachment) = mention.studio_attachment.as_ref() else {
            return div().into_any_element();
        };
        let preview = if let Some(image) = attachment.preview.as_ref() {
            img(image.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui_component::Icon::new(crate::ui::design::design_icon())
                        .size(crate::ui::design::icon_lg())
                        .text_color(crate::ui::design::accent(cx)),
                )
                .into_any_element()
        };
        let title = SharedString::from(mention.title.clone());
        let details = SharedString::from(format!(
            "Saved version {}\n{}\n{}",
            attachment.revision,
            attachment.screen_names.join(", "),
            attachment.system_label()
        ));
        let system_label = SharedString::from(attachment.system_label());
        let system_tooltip = system_label.clone();
        h_flex()
            .id(("studio-implementation-context", index))
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_3()
            .pb_3()
            .border_b_1()
            .border_color(crate::ui::design::line(cx))
            .child(
                div()
                    .w(px(56.))
                    .h(px(44.))
                    .flex_none()
                    .overflow_hidden()
                    .rounded(crate::ui::design::r_sm())
                    .bg(crate::ui::design::base(cx))
                    .child(preview),
            )
            .child(
                v_flex()
                    .id(("studio-implementation-details", index))
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        div()
                            .id(("studio-implementation-title", index))
                            .w_full()
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title.clone())
                            .tooltip(move |window, cx| {
                                Tooltip::new(title.clone()).build(window, cx)
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w(px(0.))
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(attachment.screen_label()),
                            )
                            .child(
                                h_flex()
                                    .id(("studio-implementation-system", index))
                                    .min_w(px(0.))
                                    .gap_1()
                                    .items_center()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(crate::ui::design::r_xs())
                                    .bg(crate::ui::design::surface(cx))
                                    .child(
                                        gpui_component::Icon::new(IconName::Palette)
                                            .flex_none()
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t2(cx)),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_size(crate::ui::design::text_label())
                                            .text_color(crate::ui::design::t2(cx))
                                            .child(system_label),
                                    )
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(system_tooltip.clone()).build(window, cx)
                                    }),
                            ),
                    )
                    .tooltip(move |window, cx| Tooltip::new(details.clone()).build(window, cx)),
            )
            .child(
                style::attachment_remove_button(
                    ("remove-studio-implementation-context", index),
                    cx,
                )
                .flex_none()
                .tooltip("Remove design context")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(composer) = this.new_agent_composer.as_mut() {
                        composer.remove_context_mention(index);
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    pub(super) fn studio_design_references(
        &self,
        project: ProjectId,
        query: &str,
    ) -> Vec<ProjectReference> {
        self.studio_designs(project)
            .into_iter()
            .filter(|design| query.is_empty() || design.name.to_ascii_lowercase().contains(query))
            .map(|design| ProjectReference {
                id: design.id,
                project_id: project,
                kind: ide_core::ProjectReferenceKind::Url,
                title: design.name,
                source: "Studio".into(),
                preview_relative_path: None,
                notes: String::new(),
                metadata_json: serde_json::json!({"provider":"studio","design_id":design.id})
                    .to_string(),
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .collect()
    }

    pub(super) fn studio_reference_token(
        &self,
        reference: &ProjectReference,
        cx: &App,
    ) -> anyhow::Result<Option<ComposerMentionToken>> {
        let metadata: serde_json::Value =
            serde_json::from_str(&reference.metadata_json).unwrap_or_default();
        if metadata["provider"] != "studio" {
            return Ok(None);
        }
        let (_, root) = self
            .project_by_id(reference.project_id, cx)
            .ok_or_else(|| anyhow::anyhow!("Studio project is unavailable"))?;
        let handoff = StudioStore::for_project(root)?.handoff(reference.id, None)?;
        Ok(Some(ComposerMentionToken::studio_design(&handoff)))
    }

    pub(super) fn link_studio_mentions_to_agent(
        &mut self,
        agent: &AgentRecord,
        mentions: &[ComposerMentionToken],
        cx: &mut Context<Self>,
    ) {
        let handoffs = mentions
            .iter()
            .filter_map(ComposerMentionToken::studio_handoff_id)
            .collect::<Vec<_>>();
        if handoffs.is_empty() {
            return;
        }
        let project = agent.project_id;
        let agent_id = agent.id;
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = StudioStore::for_project(root)?;
                    for handoff in handoffs {
                        store.link_implementation_agent(handoff, agent_id)?;
                    }
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.agent_start_errors.insert(
                        agent_id,
                        format!("Could not save the Studio design link: {error:#}"),
                    );
                }
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn studio_designs_for_doc(
        &self,
        project: ProjectId,
        path: &Path,
    ) -> Vec<StudioDesignManifest> {
        self.studio_designs(project)
            .into_iter()
            .filter(|d| d.links_doc(path))
            .collect()
    }

    pub(super) fn studio_designs_for_task(
        &self,
        project: ProjectId,
        task: &TaskRef,
    ) -> Vec<StudioDesignManifest> {
        self.studio_designs(project)
            .into_iter()
            .filter(|d| d.links_task(task))
            .collect()
    }

    pub(super) fn studio_designs_for_agent(
        &self,
        agent: &AgentRecord,
    ) -> Vec<StudioDesignManifest> {
        self.studio_designs(agent.project_id)
            .into_iter()
            .filter(|design| {
                self.studio_catalog_implementors
                    .get(&agent.project_id)
                    .and_then(|ids| ids.get(&design.id))
                    .is_some_and(|ids| ids.contains(&agent.id))
                    || agent
                        .source_doc
                        .as_deref()
                        .is_some_and(|path| design.links_doc(path))
                    || agent.linked_docs.iter().any(|path| design.links_doc(path))
                    || agent
                        .source_task
                        .as_ref()
                        .is_some_and(|task| design.links_task(task))
                    || agent
                        .linked_tasks
                        .iter()
                        .any(|task| design.links_task(task))
            })
            .collect()
    }

    pub(super) fn render_linked_studio_indicator(
        &self,
        key: &'static str,
        project: ProjectId,
        design: StudioDesignManifest,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = design.id;
        let tooltip = SharedString::from(format!("{} — open in Studio", design.name));
        crate::ui::design::indicator::subline_link(
            (key, id.as_u128() as u64),
            crate::ui::design::design_icon(),
            SharedString::from(super::design_workspace::short_design_chip_label(&design.name)),
            crate::ui::design::accent(cx),
            cx,
        )
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.open_studio(project, id, cx)))
        .into_any_element()
    }

    pub(super) fn create_studio_for_doc(
        &mut self,
        project: ProjectId,
        title: String,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let source = self
            .docs
            .update(cx, |docs, _| docs.web_document_for_path(&root.join(&path)))
            .and_then(|document| Ok(serde_json::to_string_pretty(&document)?));
        let content = match source {
            Ok(content) => content,
            Err(error) => {
                self.doc_action_error =
                    Some(format!("Could not read the source document: {error:#}"));
                cx.notify();
                return;
            }
        };
        let reference = path.to_string_lossy().to_string();
        let prompt = format!(
            "Design the product experience described in @@{reference}. Read the document first, then use the Studio tools to create the important screens, states, hierarchy, and interactions in this design. Keep the design grounded in the document and ask about ambiguity before inventing major product behavior.\n\n{}",
            ide_core::studio::codebase_context_instruction()
        );
        self.create_studio_from_source(
            project,
            source_design_name(&title),
            Some(StudioSource {
                reference,
                content,
                task_ref: None,
            }),
            None,
            prompt,
            cx,
        );
    }

    pub(super) fn create_studio_for_task(
        &mut self,
        project: ProjectId,
        summary: TaskSummary,
        detail: Option<TaskDetail>,
        cx: &mut Context<Self>,
    ) {
        let prompt = super::tasks::prompt::task_design_prompt(&summary, detail.as_ref());
        let name = if summary.reference.title.trim().is_empty() {
            &summary.reference.issue_key
        } else {
            &summary.reference.title
        };
        let task = StudioSource {
            reference: format!(
                "{} {}",
                summary.reference.issue_key, summary.reference.issue_url
            ),
            content: prompt.clone(),
            task_ref: Some(summary.reference.clone()),
        };
        self.create_studio_from_source(
            project,
            source_design_name(name),
            None,
            Some(task),
            prompt,
            cx,
        );
    }

    fn create_studio_from_source(
        &mut self,
        project: ProjectId,
        name: String,
        document: Option<StudioSource>,
        task: Option<StudioSource>,
        prompt: String,
        cx: &mut Context<Self>,
    ) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        if !self.studio_creating.insert(project) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = StudioStore::for_project(root)?;
                    let design = store.create(&name)?;
                    let scope = StudioTurnScope::whole_design(&design);
                    store.apply(
                        &scope,
                        &StudioTransaction {
                            id: Uuid::new_v4(),
                            scope_id: scope.id,
                            design_id: design.manifest.id,
                            expected_revision: design.manifest.revision,
                            expected_fingerprint: design.fingerprint,
                            operations: vec![StudioOperation::SetSource { document, task }],
                        },
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.studio_creating.remove(&project);
                match result {
                    Ok(design) => {
                        this.pending_studio_drafts
                            .insert(design.manifest.id, prompt);
                        this.refresh_studio_catalog(project, cx);
                        this.open_studio(project, design.manifest.id, cx);
                    }
                    Err(error) => {
                        this.design_hub_error =
                            Some(format!("Could not create Studio design: {error:#}"));
                        this.show_design_hub(cx);
                        this.set_view_mode(CenterMode::Design, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read source associations from disk on the explicit action, so opening an
    /// implementation before the background catalog finishes still includes it.
    pub(super) fn attach_studio_source_designs(
        &mut self,
        project: ProjectId,
        doc: Option<&Path>,
        task: Option<&TaskRef>,
        cx: &mut Context<Self>,
    ) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        let result = (|| -> anyhow::Result<Vec<StudioHandoff>> {
            let store = StudioStore::for_project(root)?;
            store
                .list()?
                .into_iter()
                .filter(|d| {
                    doc.is_some_and(|p| d.links_doc(p)) || task.is_some_and(|t| d.links_task(t))
                })
                .map(|d| store.handoff(d.id, None))
                .collect()
        })();
        if let Some(composer) = self.new_agent_composer.as_mut() {
            match result {
                Ok(handoffs) => composer
                    .selected_mentions
                    .extend(handoffs.iter().map(ComposerMentionToken::studio_design)),
                Err(error) => {
                    let message = format!(
                        "Could not attach the linked Studio design: {error:#}. Reopen Implement to retry."
                    );
                    composer.error = Some(message.clone());
                    composer.studio_attachment_error = Some(message);
                }
            }
        }
    }

    pub(super) fn move_studio_doc_links(
        &mut self,
        project: ProjectId,
        old: PathBuf,
        new: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = StudioStore::for_project(root)?;
                    for manifest in store.list()?.into_iter().filter(|d| d.links_doc(&old)) {
                        let design = store.load(manifest.id)?;
                        let mut document = design
                            .manifest
                            .source_context
                            .get("document")
                            .cloned()
                            .unwrap_or(StudioSource {
                                reference: String::new(),
                                content: String::new(),
                                task_ref: None,
                            });
                        document.reference = new.to_string_lossy().into_owned();
                        let scope = StudioTurnScope::whole_design(&design);
                        store.apply(
                            &scope,
                            &StudioTransaction {
                                id: Uuid::new_v4(),
                                scope_id: scope.id,
                                design_id: manifest.id,
                                expected_revision: design.manifest.revision,
                                expected_fingerprint: design.fingerprint,
                                operations: vec![StudioOperation::SetSource {
                                    document: Some(document),
                                    task: design.manifest.source_context.get("task").cloned(),
                                }],
                            },
                        )?;
                    }
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.doc_action_error = Some(format!(
                        "Document moved; could not update its Studio link: {error:#}"
                    ));
                }
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
