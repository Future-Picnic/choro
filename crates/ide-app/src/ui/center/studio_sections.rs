//! Studio sections: grouping edits, selection, and the native section inspector.
//! Every grouping change is one saved, undoable design edit. An unsaved screen
//! editor is flushed successfully first and stays open afterwards.
use super::*;
use gpui::Focusable as _;
use ide_core::studio::*;
use serde_json::json;

/// Personal canvas consequences of a grouping edit, applied only on success.
#[derive(Default)]
pub(super) struct GroupingEffect {
    pub select_section: Option<Uuid>,
    pub clear_section: bool,
    /// Newly unsectioned screens leave the board.
    pub unsectioned: Vec<Uuid>,
    /// A screen dragged out of a section keeps its drop position.
    pub drop: Option<(Uuid, StudioCanvasPoint)>,
    /// Correlated canvas interaction: (canvas session, request).
    pub canvas_request: Option<(Uuid, Uuid)>,
}
pub(super) struct PendingGrouping {
    pub request: Uuid,
    operations: Vec<StudioOperation>,
    effect: GroupingEffect,
    expected: Option<(u64, String)>,
    before_flush: Option<StudioDesignManifest>,
    editor_save: Option<(u64, String)>,
}
impl PendingGrouping {
    pub(super) fn record_editor_save(&mut self, design: &StudioDesign) {
        self.editor_save = Some((design.manifest.revision, design.fingerprint.clone()));
    }
    /// Only our one acknowledged document save may advance a gesture's base.
    /// Every manifest field must remain identical, including grouping and sizes.
    fn advance_after_flush(&mut self, design: &StudioDesign) {
        let Some(base) = self.before_flush.as_ref() else { return; };
        let saved = (design.manifest.revision, design.fingerprint.clone());
        if self.editor_save.as_ref() != Some(&saved) || design.manifest.revision != base.revision.saturating_add(1) {
            return;
        }
        let mut metadata = base.clone();
        metadata.revision = design.manifest.revision;
        if metadata == design.manifest { self.expected = Some(saved); }
    }
}

const GAP_STEP: u32 = 16;

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(store: &StudioStore, design: &StudioDesign, operations: Vec<StudioOperation>, expected: Option<(u64, String)>) -> anyhow::Result<StudioDesign> {
        let scope = StudioTurnScope::whole_design(design);
        let (expected_revision, expected_fingerprint) = expected.unwrap_or((design.manifest.revision, design.fingerprint.clone()));
        store.apply(&scope, &StudioTransaction { id: Uuid::new_v4(), scope_id: scope.id,
            design_id: scope.design_id, expected_revision, expected_fingerprint, operations })
    }
    fn pending(design: &StudioDesign, operation: StudioOperation) -> PendingGrouping {
        PendingGrouping { request: Uuid::new_v4(), operations: vec![operation], effect: Default::default(),
            expected: Some((design.manifest.revision, design.fingerprint.clone())),
            before_flush: Some(design.manifest.clone()), editor_save: None }
    }
    #[test]
    fn studio_dirty_editor_drop_uses_its_own_save_and_keeps_peer_changes_conflicting() {
        let root = std::env::temp_dir().join(format!("choro-studio-drop-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let initial = store.create("Dirty drop").unwrap();
        let section = StudioSection::new("Add post");
        let design = commit(&store, &initial, vec![StudioOperation::CreateSection { section: section.clone() }], None).unwrap();
        let id = design.manifest.screens[0].id;
        let move_screen = || StudioOperation::MoveScreenToSection { screen_id: id, section_id: Some(section.id), before_screen_id: None };
        let mut drop = pending(&design, move_screen());
        let mut document = design.documents[&id].clone();
        document.html = "<h1>Saved editor content</h1>".into();
        let saved = commit(&store, &design, vec![StudioOperation::WriteScreen { screen_id: id, document }], None).unwrap();
        drop.advance_after_flush(&saved);
        assert_eq!(drop.expected.as_ref().unwrap().0, design.manifest.revision, "An unacknowledged save cannot advance a gesture");
        drop.record_editor_save(&saved);
        drop.advance_after_flush(&saved);
        let moved = commit(&store, &saved, drop.operations, drop.expected).unwrap();
        assert_eq!(moved.manifest.section(section.id).unwrap().screen_ids, [id]);
        assert_eq!(moved.documents, saved.documents);
        let undone = store.undo_latest(moved.manifest.id).unwrap();
        assert!(undone.manifest.section(section.id).unwrap().screen_ids.is_empty());
        assert_eq!(undone.documents, saved.documents, "Undoing grouping preserves the flushed editor content");

        let mut stale = pending(&undone, move_screen());
        let mut document = undone.documents[&id].clone();
        document.html.push_str("<p>More saved content</p>");
        let saved = commit(&store, &undone, vec![StudioOperation::WriteScreen { screen_id: id, document }], None).unwrap();
        stale.record_editor_save(&saved);
        let peer = commit(&store, &saved, vec![StudioOperation::UpdateSection { section_id: section.id,
            name: Some("Peer's flow".into()), direction: None, gap: None, title_style: None, header_alignment: None }], None).unwrap();
        stale.advance_after_flush(&peer);
        assert!(commit(&store, &peer, stale.operations, stale.expected).unwrap_err().to_string().contains("conflict"));
        assert_eq!(store.load(peer.manifest.id).unwrap().manifest.sections, peer.manifest.sections);
    }
}

impl CenterArea {
    /// Apply grouping operations as one saved edit. `expected` is the
    /// interaction-start revision for canvas gestures.
    pub(super) fn studio_group(
        &mut self,
        operations: Vec<StudioOperation>,
        effect: GroupingEffect,
        expected: Option<(u64, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.studio.as_mut() else { return; };
        if s.grouping.is_some() || (s.saving && !s.dirty) {
            let reason = "Wait for the current edit to save.";
            s.error = Some(reason.into());
            self.studio_canvas_move_result(effect.canvas_request, Some(reason.into()), cx);
            cx.notify();
            return;
        }
        let before_flush = expected.as_ref()
            .filter(|(revision, fingerprint)| *revision == s.design.manifest.revision && fingerprint == &s.design.fingerprint)
            .map(|_| s.design.manifest.clone());
        let pending = PendingGrouping { request: Uuid::new_v4(), operations, effect, expected, before_flush, editor_save: None };
        if s.editing_screen().is_some() && (s.dirty || s.saving) {
            // Grouping waits for a successful save; a failed save cancels it.
            let reply = json!({"session":s.editor_session,"type":"flush","request_id":pending.request});
            s.grouping = Some(pending);
            self.web_host.read(cx).studio_reply(&reply);
            cx.notify();
            return;
        }
        self.studio_group_apply(pending, cx);
    }
    /// Called for the editor's `flushed` reply; true when it belonged to grouping.
    pub(super) fn studio_grouping_flushed(&mut self, request: Option<Uuid>, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.studio.as_mut() else { return false; };
        if request.is_none() || s.grouping.as_ref().map(|g| g.request) != request {
            return false;
        }
        s.dirty = false;
        if let Some(mut pending) = s.grouping.take() {
            pending.advance_after_flush(&s.design);
            self.studio_group_apply(pending, cx);
        }
        true
    }
    pub(super) fn studio_grouping_cancel(&mut self, reason: &str, cx: &mut Context<Self>) {
        let Some(pending) = self.studio.as_mut().and_then(|s| s.grouping.take()) else { return; };
        let reason = format!("{reason} The grouping change was not applied.");
        if let Some(s) = self.studio.as_mut() { s.error = Some(reason.clone()); }
        self.studio_canvas_move_result(pending.effect.canvas_request, Some(reason), cx);
    }
    fn studio_group_apply(&mut self, pending: PendingGrouping, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        let PendingGrouping { operations, effect, expected, .. } = pending;
        let (revision, fingerprint) = expected
            .unwrap_or_else(|| (s.design.manifest.revision, s.design.fingerprint.clone()));
        let scope = StudioTurnScope::whole_design(&s.design);
        let tx = StudioTransaction {
            id: effect.canvas_request.map(|(_, request)| request).unwrap_or_else(Uuid::new_v4),
            scope_id: scope.id,
            design_id: scope.design_id,
            expected_revision: revision,
            expected_fingerprint: fingerprint,
            operations,
        };
        let store = s.store.clone();
        let design_id = scope.design_id;
        s.saving = true;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let result = store.apply(&scope, &tx);
                let latest = result.is_err().then(|| store.load(design_id));
                (result, latest)
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == design_id) else { return; };
                s.saving = false;
                let error = match result {
                    (Ok(design), _) => {
                        s.redo = false;
                        s.design = design;
                        s.error = None;
                        let layout = &mut s.canvas.layout;
                        if !effect.unsectioned.is_empty() {
                            layout.place_outside_board(&s.design.manifest, &effect.unsectioned);
                        }
                        if let Some((id, point)) = effect.drop {
                            layout.positions.insert(id, point);
                        }
                        if effect.select_section.is_some() {
                            layout.select_section(effect.select_section);
                        } else if effect.clear_section {
                            layout.select_section(None);
                        }
                        layout.reconcile_design(&s.design.manifest);
                        if !s.canvas.corrupt {
                            if let Err(e) = s.store.save_canvas_state(s.design.manifest.id, &s.canvas.layout) {
                                s.error = Some(format!("Could not save canvas layout: {e}"));
                            }
                        }
                        None
                    }
                    (Err(error), latest) => {
                        // Roll back to authority; never merge a stale grouping edit.
                        if let Some(Ok(design)) = latest { s.design = design; }
                        let message = format!("{error:#}");
                        s.error = Some(message.clone());
                        Some(message)
                    }
                };
                this.studio_after_grouping(cx);
                this.studio_canvas_move_result(effect.canvas_request, error, cx);
                cx.notify();
            });
        }).detach();
    }
    /// Organization-only edits keep the live editor, its caret and undo stack.
    fn studio_after_grouping(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut() else { return; };
        if s.editing_screen().is_some() {
            let reply = json!({"session":s.editor_session,"type":"screens","screens":s.design.manifest.screens});
            self.web_host.read(cx).studio_reply(&reply);
        }
        if let Some(s) = self.studio.as_mut() { s.canvas.invalidate_metadata(); }
        self.refresh_studio_canvas(cx);
    }
    pub(super) fn studio_canvas_move_result(&mut self, request: Option<(Uuid, Uuid)>, error: Option<String>, cx: &mut Context<Self>) {
        let Some((session, request)) = request else { return; };
        let Some(s) = self.studio.as_ref().filter(|s| s.canvas.session == session) else { return; };
        let reply = super::studio_canvas::move_result(s, request, error);
        self.web_host.read(cx).canvas_reply(&reply);
    }

    pub(super) fn studio_select_section(&mut self, section: Option<Uuid>, cx: &mut Context<Self>) {
        if section.is_some() && self.studio.as_ref().is_some_and(|s| s.canvas.layout.overview_mode == StudioOverviewMode::Focus) {
            self.studio_overview_mode(StudioOverviewMode::Canvas, cx);
        }
        let Some(s) = self.studio.as_mut() else { return; };
        let section = section.filter(|id| s.design.manifest.section(*id).is_some());
        if s.inline_screen.is_some() && section.is_some() {
            self.studio_inline_select(None, cx);
        }
        let Some(s) = self.studio.as_mut() else { return; };
        if s.canvas.layout.selected_section_id == section && section.is_some() { return; }
        s.canvas.layout.select_section(section);
        s.section_name_input = None;
        if !s.canvas.corrupt {
            if let Err(e) = s.store.save_canvas_state(s.design.manifest.id, &s.canvas.layout) {
                s.error = Some(e.to_string());
            }
        }
        self.studio_canvas_selection(cx);
        cx.notify();
    }
    /// Sections can only be fitted on the canvas; Grid switches to it first.
    pub(super) fn studio_fit_section(&mut self, section: Uuid, cx: &mut Context<Self>) {
        let canvas = self.studio_canvas_active()
            && self.studio.as_ref().is_some_and(|s| s.canvas.layout.overview_mode == StudioOverviewMode::Canvas);
        if let Some(s) = self.studio.as_mut() { s.canvas.pending_fit_section = Some(section); }
        if !canvas {
            self.studio_overview_mode(StudioOverviewMode::Canvas, cx);
        }
        self.studio_canvas_fit_pending(cx);
    }
    pub(super) fn studio_canvas_fit_pending(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_mut().filter(|s| s.canvas.ready) else { return; };
        let Some(section) = s.canvas.pending_fit_section.take() else { return; };
        let value = json!({"session":s.canvas.session,"type":"command","command":"fit-section","section_id":section});
        self.web_host.read(cx).canvas_reply(&value);
    }

    pub(super) fn studio_create_section(&mut self, name: String, screen: Option<Uuid>, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_ref() else { return; };
        let section = StudioSection {
            screen_ids: screen.into_iter().collect(),
            ..StudioSection::new(name.trim())
        };
        let id = section.id;
        let manifest = &s.design.manifest;
        let mut operations = vec![StudioOperation::CreateSection { section }];
        if manifest.section_layout.origin.is_none() {
            // Freeze the board below today's screens so later edits never move it.
            operations.push(StudioOperation::SetSectionLayout {
                direction: manifest.section_layout.direction,
                origin: Some(s.canvas.layout.board_origin(manifest)),
            });
        }
        self.studio_group(operations, GroupingEffect { select_section: Some(id), ..Default::default() }, None, cx);
    }
    pub(super) fn studio_move_screen(&mut self, screen: Uuid, section: Option<Uuid>, before: Option<Uuid>, cx: &mut Context<Self>) {
        let effect = GroupingEffect { unsectioned: if section.is_none() { vec![screen] } else { vec![] }, ..Default::default() };
        self.studio_group(vec![StudioOperation::MoveScreenToSection { screen_id: screen, section_id: section, before_screen_id: before }], effect, None, cx);
    }
    pub(super) fn studio_set_arrangement(&mut self, direction: StudioSectionArrangement, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_ref() else { return; };
        let manifest = &s.design.manifest;
        if manifest.section_layout.direction == direction { return; }
        let origin = manifest.section_layout.origin.unwrap_or_else(|| s.canvas.layout.board_origin(manifest));
        self.studio_group(vec![StudioOperation::SetSectionLayout { direction, origin: Some(origin) }], GroupingEffect::default(), None, cx);
    }
    pub(super) fn studio_update_section(&mut self, id: Uuid, update: impl FnOnce(&StudioSection) -> StudioOperation, cx: &mut Context<Self>) {
        let Some(section) = self.studio.as_ref().and_then(|s| s.design.manifest.section(id)).cloned() else { return; };
        self.studio_group(vec![update(&section)], GroupingEffect::default(), None, cx);
    }
    pub(super) fn studio_section_menu_action(&mut self, id: Uuid, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_ref() else { return; };
        let manifest = &s.design.manifest;
        let Some(section) = manifest.section(id).cloned() else { return; };
        match action {
            "rename" => self.studio_section_name_dialog(Some(id), &section.name, window, cx),
            "add-screen" => self.studio_add_screen_dialog(Some(id), window, cx),
            "fit" => self.studio_fit_section(id, cx),
            "edit" => {
                self.studio_select_section(Some(id), cx);
                if let Some(s) = self.studio.as_mut() { s.tab = super::studio::StudioTab::Agent; s.sidebar_collapsed = false; }
                cx.notify();
            }
            "ungroup" => {
                let effect = GroupingEffect {
                    unsectioned: section.screen_ids.clone(),
                    clear_section: s.canvas.layout.selected_section_id == Some(id),
                    ..Default::default()
                };
                self.studio_group(vec![StudioOperation::UngroupSection { section_id: id }], effect, None, cx);
            }
            "earlier" | "later" => {
                let mut order: Vec<Uuid> = manifest.sections.iter().map(|s| s.id).collect();
                let index = order.iter().position(|s| *s == id).unwrap_or_default();
                let target = if action == "earlier" { index.checked_sub(1) } else { (index + 1 < order.len()).then_some(index + 1) };
                let Some(target) = target else { return; };
                order.swap(index, target);
                self.studio_group(vec![StudioOperation::ReorderSections { section_ids: order }], GroupingEffect::default(), None, cx);
            }
            _ => {}
        }
    }
    /// Earlier/later within the screen's current group, and grouped duplicates.
    pub(super) fn studio_screen_group_action(&mut self, id: Uuid, action: &str, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.studio.as_ref() else { return true; };
        let manifest = &s.design.manifest;
        let Some(screen) = manifest.screens.iter().find(|p| p.id == id).cloned() else { return true; };
        let section = manifest.section_of(id).cloned();
        let operation = match action {
            "duplicate" => {
                let copy = StudioScreen { id: Uuid::new_v4(), name: format!("{} copy", screen.name), ..screen.clone() };
                let mut operations = vec![StudioOperation::CreateScreen {
                    screen: copy.clone(),
                    document: s.design.documents[&id].clone(),
                    section_id: Some(section.as_ref().map(|s| s.id)),
                }];
                // The duplicate lands immediately after its source.
                if let Some(section) = &section {
                    let mut order = section.screen_ids.clone();
                    let index = order.iter().position(|v| *v == id).unwrap_or_default();
                    order.insert(index + 1, copy.id);
                    operations.push(StudioOperation::ReorderSectionScreens { section_id: section.id, screen_ids: order });
                } else {
                    let mut order: Vec<Uuid> = manifest.screens.iter().map(|s| s.id).collect();
                    let index = order.iter().position(|v| *v == id).unwrap_or_default();
                    order.insert(index + 1, copy.id);
                    operations.push(StudioOperation::Reorder { screen_ids: order });
                }
                self.studio_group(operations, GroupingEffect::default(), None, cx);
                return true;
            }
            "up" | "down" if screen.archived => return false,
            "up" | "down" => {
                let earlier = action == "up";
                if let Some(section) = section {
                    let mut order = section.screen_ids.clone();
                    let index = order.iter().position(|v| *v == id).unwrap_or_default();
                    let target = if earlier { index.checked_sub(1) } else { (index + 1 < order.len()).then_some(index + 1) };
                    let Some(target) = target else { return true; };
                    order.swap(index, target);
                    StudioOperation::ReorderSectionScreens { section_id: section.id, screen_ids: order }
                } else {
                    let group: Vec<Uuid> = manifest.screens.iter()
                        .filter(|p| !p.archived && manifest.section_of(p.id).is_none()).map(|p| p.id).collect();
                    let index = group.iter().position(|v| *v == id).unwrap_or_default();
                    let neighbor = if earlier { index.checked_sub(1) } else { (index + 1 < group.len()).then_some(index + 1) };
                    let Some(neighbor) = neighbor.map(|i| group[i]) else { return true; };
                    let mut order: Vec<Uuid> = manifest.screens.iter().map(|s| s.id).collect();
                    let (a, b) = (order.iter().position(|v| *v == id), order.iter().position(|v| *v == neighbor));
                    if let (Some(a), Some(b)) = (a, b) { order.swap(a, b); }
                    StudioOperation::Reorder { screen_ids: order }
                }
            }
            _ => return false,
        };
        self.studio_group(vec![operation], GroupingEffect::default(), None, cx);
        true
    }

    /// Native inspector for a selected section. It replaces the screen inspector.
    pub(super) fn render_studio_section_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let s = self.studio.as_ref()?;
        if s.prototype || s.comment_mode || s.editing_screen().is_some() || s.design.manifest.system_workspace { return None; }
        let section = s.design.manifest.section(s.canvas.layout.selected_section_id?)?.clone();
        let busy = s.saving || s.grouping.is_some();
        let id = section.id;
        let input = self.studio_section_name_input(&section, window, cx);
        let active = self.studio.as_ref()?.design.manifest.screens.iter()
            .filter(|p| !p.archived && section.screen_ids.contains(&p.id)).count();
        let label = |text: &'static str| div().text_size(crate::ui::design::text_label())
            .font_weight(gpui::FontWeight::SEMIBOLD).text_color(crate::ui::design::t3(cx)).child(text);
        let field = |title: &'static str, control: gpui::AnyElement| v_flex().gap_1p5().child(label(title)).child(control);
        let choice = |key: &'static str, text: &'static str, selected: bool, hint: &'static str| {
            style::stage_bar_choice(SharedString::from(format!("studio-section-{key}")), text, selected, hint, cx).disabled(busy)
        };
        let direction = style::stage_bar_choices(cx)
            .child(choice("horizontal", "Horizontal", section.direction == StudioSectionDirection::Horizontal, "Screens run left to right")
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.direction = Some(StudioSectionDirection::Horizontal)), cx))))
            .child(choice("vertical", "Vertical", section.direction == StudioSectionDirection::Vertical, "Screens run top to bottom")
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.direction = Some(StudioSectionDirection::Vertical)), cx))));
        let gap = section.gap;
        let spacing = h_flex().items_center().gap_1()
            .child(style::header_icon_button("studio-section-gap-less", IconName::Minus, cx).tooltip("Less space between screens")
                .disabled(busy || gap == 0)
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |s| section_update(id, |op| op.gap = Some(s.gap.saturating_sub(GAP_STEP))), cx))))
            .child(style::stage_bar_readout(format!("{gap}"), cx).w(px(44.)).flex().justify_center())
            .child(style::header_icon_button("studio-section-gap-more", IconName::Plus, cx).tooltip("More space between screens")
                .disabled(busy || gap >= MAX_SECTION_GAP)
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |s| section_update(id, |op| op.gap = Some((s.gap + GAP_STEP).min(MAX_SECTION_GAP))), cx))));
        let header = section.title_style == StudioSectionTitleStyle::FullWidthHeader;
        let title = style::stage_bar_choices(cx)
            .child(choice("left-title", "Left title", !header, "A title above the section's left edge")
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.title_style = Some(StudioSectionTitleStyle::LeftTitle)), cx))))
            .child(choice("full-header", "Full-width header", header, "A header bar across the whole section")
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.title_style = Some(StudioSectionTitleStyle::FullWidthHeader)), cx))));
        let centered = section.header_alignment == StudioSectionHeaderAlignment::Center;
        let alignment = style::stage_bar_choices(cx)
            .child(choice("align-left", "Left", !centered, "Align the header title left").disabled(busy || !header)
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.header_alignment = Some(StudioSectionHeaderAlignment::Left)), cx))))
            .child(choice("align-center", "Center", centered, "Center the header title").disabled(busy || !header)
                .on_click(cx.listener(move |this, _, _, cx| this.studio_update_section(id, |_| section_update(id, |op| op.header_alignment = Some(StudioSectionHeaderAlignment::Center)), cx))));
        Some(v_flex()
            .id("studio-section-inspector")
            .w(px(272.)).h_full().flex_none().min_h(px(0.)).overflow_y_scroll()
            .bg(crate::ui::design::base(cx)).border_l_1().border_color(crate::ui::design::line(cx))
            .px_3().py_3().gap_4()
            .child(h_flex().items_center().gap_2()
                .child(v_flex().flex_1().min_w(px(0.)).gap(px(2.))
                    .child(div().text_size(crate::ui::design::text_ui()).font_weight(gpui::FontWeight::MEDIUM).child("Section"))
                    .child(div().text_size(crate::ui::design::text_label()).text_color(crate::ui::design::t3(cx))
                        .child(format!("{active} {}", if active == 1 { "screen" } else { "screens" }))))
                .child(style::header_icon_button("studio-section-close", IconName::Close, cx).tooltip("Close section inspector")
                    .on_click(cx.listener(|this, _, _, cx| this.studio_select_section(None, cx)))))
            .child(field("Name", Input::new(&input).disabled(busy).into_any_element()))
            .child(field("Screen direction", direction.into_any_element()))
            .child(field("Space between screens", spacing.into_any_element()))
            .child(field("Title", title.into_any_element()))
            .child(field("Header alignment", alignment.into_any_element()))
            .child(h_flex().gap_2().flex_wrap()
                .child(style::ghost_button_compact("studio-section-fit", "Fit section")
                    .on_click(cx.listener(move |this, _, _, cx| this.studio_fit_section(id, cx))))
                .child(style::ghost_button_compact("studio-section-edit", "Edit section").tooltip("Ask the agent about this section")
                    .on_click(cx.listener(move |this, _, window, cx| this.studio_section_menu_action(id, "edit", window, cx)))))
            .into_any_element())
    }
    fn studio_section_name_input(&mut self, section: &StudioSection, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let id = section.id;
        if let Some((_, input)) = self.studio.as_ref().and_then(|s| s.section_name_input.as_ref()).filter(|(section, _)| *section == id) {
            // Follow renames made elsewhere unless the field is being edited.
            let stale = input.read(cx).value().trim() != section.name && !input.read(cx).focus_handle(cx).is_focused(window);
            if !stale { return input.clone(); }
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Section name").default_value(section.name.clone()));
        cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, _, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) { return; }
            let value = input.read(cx).value().trim().to_string();
            let current = this.studio.as_ref().and_then(|s| s.design.manifest.section(id)).map(|s| s.name.clone());
            if value.is_empty() || current.as_deref().is_none_or(|name| name == value) { return; }
            this.studio_update_section(id, |_| section_update(id, |op| op.name = Some(value)), cx);
        }).detach();
        if let Some(s) = self.studio.as_mut() { s.section_name_input = Some((id, input.clone())); }
        input
    }
    pub(super) fn studio_section_name_dialog(&mut self, section: Option<Uuid>, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let title = if section.is_some() { "Rename section" } else { "New section" };
        self.studio_name_dialog(title, value, super::studio::NameAction::Section(section), window, cx);
    }
    pub(super) fn studio_add_screen_dialog(&mut self, section: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let action = match section { Some(id) => super::studio::NameAction::ScreenIn(id), None => super::studio::NameAction::Screen(None) };
        self.studio_name_dialog("New screen", "New screen", action, window, cx);
    }
    pub(super) fn studio_new_section_with(&mut self, screen: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.studio_name_dialog("New section", "New section", super::studio::NameAction::SectionWithScreen(screen), window, cx);
    }
}

/// A partial `update_section` operation for one property.
fn section_update(id: Uuid, set: impl FnOnce(&mut SectionUpdate)) -> StudioOperation {
    let mut update = SectionUpdate::default();
    set(&mut update);
    StudioOperation::UpdateSection {
        section_id: id,
        name: update.name,
        direction: update.direction,
        gap: update.gap,
        title_style: update.title_style,
        header_alignment: update.header_alignment,
    }
}
#[derive(Default)]
struct SectionUpdate {
    name: Option<String>,
    direction: Option<StudioSectionDirection>,
    gap: Option<u32>,
    title_style: Option<StudioSectionTitleStyle>,
    header_alignment: Option<StudioSectionHeaderAlignment>,
}
