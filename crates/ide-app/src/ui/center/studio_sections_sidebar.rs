//! Screens tab: ordered section groups, Unsectioned and Archived, with
//! drag-and-drop and context menus. Sidebar order is canvas section order.
use super::*;
use gpui_component::menu::PopupMenu;
use ide_core::studio::*;

#[derive(Clone)]
pub(super) struct DraggedStudioScreen {
    id: Uuid,
    name: SharedString,
}
#[derive(Clone)]
pub(super) struct DraggedStudioSection {
    id: Uuid,
    name: SharedString,
}
fn drag_chip(name: SharedString, cx: &App) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .rounded(crate::ui::design::r_xs())
        .bg(crate::ui::design::surface(cx))
        .border_1()
        .border_color(crate::ui::design::accent(cx))
        .text_size(crate::ui::design::text_ui())
        .text_color(crate::ui::design::t1(cx))
        .child(name)
}
impl Render for DraggedStudioScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_chip(self.name.clone(), cx)
    }
}
impl Render for DraggedStudioSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_chip(self.name.clone(), cx)
    }
}

/// Escape cancels an in-flight sidebar drag; nothing changes until a drop.
pub(super) fn drag_escape(cx: &mut App) -> gpui::Subscription {
    cx.observe_keystrokes(|event, window, cx| {
        if event.keystroke.key == "escape" && cx.has_active_drag() {
            cx.stop_active_drag(window);
        }
    })
}

/// One Grid group: an optional section heading and its visible screens.
pub(super) struct GridGroup {
    pub section: Option<(Uuid, String)>,
    pub screens: Vec<StudioScreen>,
}
pub(super) fn grid_groups(manifest: &StudioDesignManifest) -> Vec<GridGroup> {
    let visible = |id: &Uuid| manifest.screens.iter().find(|s| s.id == *id && !s.archived).cloned();
    let mut groups: Vec<GridGroup> = manifest
        .sections
        .iter()
        .map(|section| GridGroup {
            section: Some((section.id, section.name.clone())),
            screens: section.screen_ids.iter().filter_map(visible).collect(),
        })
        .filter(|group| !group.screens.is_empty())
        .collect();
    let free: Vec<StudioScreen> = manifest
        .screens
        .iter()
        .filter(|s| !s.archived && manifest.section_of(s.id).is_none())
        .cloned()
        .collect();
    if !free.is_empty() || groups.is_empty() {
        let label = (!manifest.sections.is_empty()).then(|| (Uuid::nil(), "Unsectioned".to_string()));
        groups.push(GridGroup { section: label, screens: free });
    }
    groups
}

#[derive(Clone)]
struct MenuContext {
    center: Entity<CenterArea>,
    host: Entity<web_preview::WebPreviewHost>,
    sections: Vec<(Uuid, String)>,
}

impl CenterArea {
    fn studio_menu_context(&self, cx: &mut Context<Self>) -> MenuContext {
        let sections = self
            .studio
            .as_ref()
            .map(|s| s.design.manifest.sections.iter().map(|s| (s.id, s.name.clone())).collect())
            .unwrap_or_default();
        MenuContext { center: cx.entity(), host: self.web_host.clone(), sections }
    }

    pub(super) fn render_studio_screens_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(studio) = self.studio.as_ref() else { return div().into_any_element(); };
        let manifest = studio.design.manifest.clone();
        let dirty = studio.dirty || studio.saving || studio.grouping.is_some() || studio.comment_mode;
        let redo = studio.redo;
        let selected_screen = studio.editing_screen().or(studio.canvas.layout.selected_screen_id);
        let selected_section = studio.canvas.layout.selected_section_id;
        let folded = studio.folded_sections.clone();
        let context = self.studio_menu_context(cx);
        let mut panel = v_flex().py_2().child(h_flex().px_1().pt_1().child(self.studio_screen_actions(dirty, redo, cx)))
            .child(div().h(px(1.)).my_2().bg(crate::ui::design::line(cx)));
        let active = manifest.screens.iter().filter(|s| !s.archived).count();
        let open = !folded.contains("Screens");
        panel = panel.child(
            h_flex().items_center().pr_2()
                .child(self.studio_section_fold("Screens", active, cx).flex_1())
                .child(style::header_icon_button("studio-new-section", IconName::Frame, cx)
                    .tooltip("New section").disabled(dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.studio_section_name_dialog(None, "New section", window, cx))))
                .child(style::header_icon_button("studio-add-screen", IconName::Plus, cx)
                    .tooltip(if selected_section.is_some() { "Add screen to the selected section" } else { "Add screen" })
                    .disabled(dirty)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let section = this.studio.as_ref().and_then(|s| s.canvas.layout.selected_section_id);
                        this.studio_add_screen_dialog(section, window, cx);
                    }))),
        );
        if open {
            if !manifest.sections.is_empty() {
                let side = manifest.section_layout.direction == StudioSectionArrangement::SideBySide;
                panel = panel.child(
                    h_flex().px_3().pb_2().gap_2().items_center()
                        .child(div().flex_1().text_size(crate::ui::design::text_label()).text_color(crate::ui::design::t3(cx)).child("Sections"))
                        .child(style::stage_bar_choices(cx)
                            .child(style::stage_bar_choice("studio-sections-stacked", "Stacked", !side, "Place sections below one another", cx)
                                .disabled(dirty)
                                .on_click(cx.listener(|this, _, _, cx| this.studio_set_arrangement(StudioSectionArrangement::Stacked, cx))))
                            .child(style::stage_bar_choice("studio-sections-side", "Side by side", side, "Place sections beside one another", cx)
                                .disabled(dirty)
                                .on_click(cx.listener(|this, _, _, cx| this.studio_set_arrangement(StudioSectionArrangement::SideBySide, cx))))),
                );
            }
            for (index, section) in manifest.sections.iter().enumerate() {
                let key = format!("section:{}", section.id);
                let expanded = !folded.contains(&key);
                panel = panel.child(self.studio_section_group_row(section, index, &manifest, expanded, selected_section == Some(section.id), dirty, &context, cx));
                if !expanded { continue; }
                let members: Vec<&StudioScreen> = section.screen_ids.iter()
                    .filter_map(|id| manifest.screens.iter().find(|s| s.id == *id && !s.archived)).collect();
                if members.is_empty() {
                    panel = panel.child(self.studio_empty_section_row(section.id, index, cx));
                }
                for screen in members {
                    panel = panel.child(self.studio_screen_row(screen, Some(section.id), selected_screen == Some(screen.id), dirty, &context, cx));
                }
            }
            let free: Vec<&StudioScreen> = manifest.screens.iter()
                .filter(|s| !s.archived && manifest.section_of(s.id).is_none()).collect();
            if !manifest.sections.is_empty() {
                panel = panel.child(self.studio_unsectioned_row(free.len(), folded.contains("Unsectioned"), cx));
            }
            if manifest.sections.is_empty() || !folded.contains("Unsectioned") {
                for screen in free {
                    panel = panel.child(self.studio_screen_row(screen, None, selected_screen == Some(screen.id), dirty, &context, cx));
                }
            }
        }
        let archived: Vec<&StudioScreen> = manifest.screens.iter().filter(|s| s.archived).collect();
        if !archived.is_empty() {
            panel = panel.child(self.studio_section_fold("Archived", archived.len(), cx));
            if !folded.contains("Archived") {
                for screen in archived {
                    panel = panel.child(self.studio_screen_row(screen, None, selected_screen == Some(screen.id), dirty, &context, cx));
                }
            }
        }
        panel.into_any_element()
    }

    fn studio_section_fold(&self, label: &'static str, count: usize, cx: &mut Context<Self>) -> gpui_component::button::Button {
        let open = self.studio.as_ref().is_some_and(|s| !s.folded_sections.contains(label));
        style::design_sidebar_section(SharedString::from(format!("studio-section-{label}")), label, open, cx)
            .child(div().flex_1())
            .child(div().text_color(crate::ui::design::t3(cx)).child(count.to_string()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = this.studio.as_mut() {
                    if !s.folded_sections.remove(label) { s.folded_sections.insert(label.to_string()); }
                }
                cx.notify();
            }))
    }

    fn studio_screen_actions(&self, dirty: bool, redo: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let center = cx.entity();
        let host = self.web_host.clone();
        style::ghost_button_compact("studio-screen-actions", "Screen actions")
            .text_color(crate::ui::design::t2(cx))
            .dropdown_caret(true)
            .dropdown_menu(move |menu, window, cx| {
                web_preview::suspend_for_menu(host.clone(), cx);
                menu.item(PopupMenuItem::new("Refresh previews").on_click(window.listener_for(&center, |this: &mut Self, _, _, cx| {
                    if let Some(studio) = this.studio.as_mut() { studio.thumbnail_revision = None; }
                    if this.studio_canvas_active() { this.studio_canvas_command("refresh", cx); } else { this.queue_studio_thumbnails(cx); }
                })))
                .item(PopupMenuItem::new("Undo saved edit").disabled(dirty)
                    .on_click(window.listener_for(&center, |this: &mut Self, _, _, cx| this.studio_undo(false, cx))))
                .item(PopupMenuItem::new("Redo saved edit").disabled(dirty || !redo)
                    .on_click(window.listener_for(&center, |this: &mut Self, _, _, cx| this.studio_undo(true, cx))))
                .item(PopupMenuItem::new("Implement selected screens…")
                    .on_click(window.listener_for(&center, |this: &mut Self, _, window, cx| this.studio_choose_implementation(window, cx))))
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn studio_section_group_row(
        &self,
        section: &StudioSection,
        index: usize,
        manifest: &StudioDesignManifest,
        expanded: bool,
        selected: bool,
        dirty: bool,
        context: &MenuContext,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = section.id;
        let count = section.screen_ids.iter().filter(|m| manifest.screens.iter().any(|s| s.id == **m && !s.archived)).count();
        let key = format!("section:{id}");
        let name: SharedString = section.name.clone().into();
        let first = index == 0;
        let last = index + 1 == manifest.sections.len();
        let menu_context = context.clone();
        let right_click = context.clone();
        let row = h_flex()
            .id(("studio-section-group", index))
            .w_full().min_w(px(0.)).items_center().pl_1().pr_1()
            .bg(if selected { crate::ui::design::surface_2(cx) } else { gpui::transparent_black() })
            .child(style::header_icon_button(("studio-section-fold", index), if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, cx)
                .tooltip(if expanded { "Collapse section" } else { "Expand section" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(s) = this.studio.as_mut() {
                        if !s.folded_sections.remove(&key) { s.folded_sections.insert(key.clone()); }
                    }
                    cx.notify();
                })))
            .child(style::design_sidebar_row(("studio-section-row", index), selected, cx)
                .px_1().flex_1().min_w(px(0.))
                .child(div().flex_1().min_w(px(0.)).truncate().font_weight(gpui::FontWeight::SEMIBOLD).child(name.clone()))
                .child(div().flex_none().text_color(crate::ui::design::t3(cx)).child(count.to_string()))
                .tooltip(format!("{} · {count} {} · double-click to fit", section.name, if count == 1 { "screen" } else { "screens" }))
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                    if matches!(event, gpui::ClickEvent::Mouse(mouse) if mouse.up.click_count >= 2) {
                        this.studio_fit_section(id, cx);
                    } else {
                        this.studio_select_section(Some(id), cx);
                    }
                })))
            .child(style::header_icon_button(("studio-section-menu", index), IconName::Ellipsis, cx)
                .tooltip("Section actions").disabled(dirty)
                .dropdown_menu(move |menu, window, cx| section_menu(menu, window, cx, &menu_context, id, first, last)))
            .when(!dirty, |row| row
                .on_drag(DraggedStudioSection { id, name: name.clone() }, |drag, _, _, cx| { cx.stop_propagation(); cx.new(|_| drag.clone()) })
                .hover(|style| style)
                .drag_over::<DraggedStudioScreen>(|style, _, _, cx| style.bg(crate::ui::design::surface_2(cx)).border_1().border_color(crate::ui::design::accent(cx)))
                .drag_over::<DraggedStudioSection>(move |style, drag, _, cx| if drag.id == id { style } else { style.border_t_2().border_color(crate::ui::design::accent(cx)) })
                .on_drop(cx.listener(move |this, drag: &DraggedStudioScreen, _, cx| this.studio_move_screen(drag.id, Some(id), None, cx)))
                .on_drop(cx.listener(move |this, drag: &DraggedStudioSection, _, cx| this.studio_drop_section_before(drag.id, Some(id), cx))));
        row.context_menu(move |menu, window, cx| section_menu(menu, window, cx, &right_click, id, first, last)).into_any_element()
    }

    fn studio_empty_section_row(&self, section: Uuid, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        div().id(("studio-section-empty", index)).w_full().h(px(28.)).pl(px(40.)).flex().items_center()
            .text_size(crate::ui::design::text_label()).text_color(crate::ui::design::t3(cx))
            .child("Drag screens here")
            .hover(|style| style)
            .drag_over::<DraggedStudioScreen>(|style, _, _, cx| style.bg(crate::ui::design::surface_2(cx)).border_1().border_color(crate::ui::design::accent(cx)))
            .on_drop(cx.listener(move |this, drag: &DraggedStudioScreen, _, cx| this.studio_move_screen(drag.id, Some(section), None, cx)))
            .into_any_element()
    }

    fn studio_unsectioned_row(&self, count: usize, folded: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        div().id("studio-unsectioned-group").w_full()
            .child(style::design_sidebar_section("studio-section-unsectioned", "Unsectioned", !folded, cx)
                .child(div().flex_1())
                .child(div().text_color(crate::ui::design::t3(cx)).child(count.to_string()))
                .tooltip("Screens outside every section")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(s) = this.studio.as_mut() {
                        if !s.folded_sections.remove("Unsectioned") { s.folded_sections.insert("Unsectioned".into()); }
                    }
                    cx.notify();
                })))
            .drag_over::<DraggedStudioScreen>(|style, _, _, cx| style.bg(crate::ui::design::surface_2(cx)).border_1().border_color(crate::ui::design::accent(cx)))
            .hover(|style| style)
            .drag_over::<DraggedStudioSection>(|style, _, _, cx| style.border_t_2().border_color(crate::ui::design::accent(cx)))
            .on_drop(cx.listener(|this, drag: &DraggedStudioScreen, _, cx| this.studio_move_screen(drag.id, None, None, cx)))
            .on_drop(cx.listener(|this, drag: &DraggedStudioSection, _, cx| this.studio_drop_section_before(drag.id, None, cx)))
            .into_any_element()
    }

    /// Reorder by dropping a section before another, or at the end (`None`).
    fn studio_drop_section_before(&mut self, dragged: Uuid, before: Option<Uuid>, cx: &mut Context<Self>) {
        let Some(s) = self.studio.as_ref() else { return; };
        if before == Some(dragged) { return; }
        let mut order: Vec<Uuid> = s.design.manifest.sections.iter().map(|s| s.id).filter(|id| *id != dragged).collect();
        let index = before.and_then(|b| order.iter().position(|id| *id == b)).unwrap_or(order.len());
        order.insert(index, dragged);
        if order == s.design.manifest.sections.iter().map(|s| s.id).collect::<Vec<_>>() { return; }
        self.studio_group(vec![StudioOperation::ReorderSections { section_ids: order }], Default::default(), None, cx);
    }
    /// Insert a dragged screen before `target` within the target's group.
    fn studio_drop_screen_before(&mut self, dragged: Uuid, target: Uuid, cx: &mut Context<Self>) {
        if dragged == target { return; }
        let Some(s) = self.studio.as_ref() else { return; };
        let section = s.design.manifest.section_of(target).map(|s| s.id);
        self.studio_move_screen(dragged, section, Some(target), cx);
    }

    fn studio_screen_row(
        &self,
        screen: &StudioScreen,
        section: Option<Uuid>,
        selected: bool,
        dirty: bool,
        context: &MenuContext,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = screen.id;
        let name: SharedString = screen.name.clone().into();
        let element = id.as_u128() as u64;
        let indent = section.is_some();
        let row = style::design_sidebar_row(("studio-screen", element), selected, cx)
            .icon(IconName::File)
            .flex_1()
            .min_w(px(0.))
            .when(indent, |row| row.pl(px(30.)))
            .child(div().flex_1().min_w(px(0.)).truncate().child(name.clone()))
            .tooltip(format!("{} · {} × {}", screen.name, screen.width, screen.height))
            .on_click(cx.listener(move |this, _, _, cx| this.studio_select_screen(Some(id), cx)));
        let menu_context = context.clone();
        let right_click = context.clone();
        let archived = screen.archived;
        let menu_name = screen.name.clone();
        let context_name = screen.name.clone();
        h_flex()
            .id(("studio-screen-item", element))
            .w_full().min_w(px(0.)).pr_1()
            .bg(if selected { crate::ui::design::surface_2(cx) } else { gpui::transparent_black() })
            .child(row)
            .child(style::header_icon_button(("studio-screen-menu", element), IconName::Ellipsis, cx)
                .disabled(dirty)
                .dropdown_menu(move |menu, window, cx| screen_menu(menu, window, cx, &menu_context, id, &menu_name, section, archived)))
            .when(!archived && !dirty, |row| row
                .on_drag(DraggedStudioScreen { id, name: name.clone() }, |drag, _, _, cx| { cx.stop_propagation(); cx.new(|_| drag.clone()) })
                .hover(|style| style)
                .drag_over::<DraggedStudioScreen>(move |style, drag, _, cx| if drag.id == id { style } else { style.border_t_2().border_color(crate::ui::design::accent(cx)) })
                .on_drop(cx.listener(move |this, drag: &DraggedStudioScreen, _, cx| this.studio_drop_screen_before(drag.id, id, cx))))
            .context_menu(move |menu, window, cx| screen_menu(menu, window, cx, &right_click, id, &context_name, section, archived))
            .into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn screen_menu(
    mut menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
    id: Uuid,
    name: &str,
    section: Option<Uuid>,
    archived: bool,
) -> PopupMenu {
    web_preview::suspend_for_menu(context.host.clone(), cx);
    let center = context.center.clone();
    let rename = name.to_string();
    menu = menu.item(PopupMenuItem::new("Rename").on_click(window.listener_for(&center, move |this: &mut CenterArea, _, window, cx| {
        this.studio_name_dialog("Rename screen", &rename, super::studio::NameAction::Screen(Some(id)), window, cx)
    })));
    menu = menu.item(PopupMenuItem::new("Duplicate").on_click(window.listener_for(&center, move |this: &mut CenterArea, _, _, cx| {
        this.studio_screen_menu_action(id, "duplicate", cx)
    })));
    if !archived {
        let sections = context.sections.clone();
        let submenu_center = center.clone();
        menu = menu.submenu("Move to section", window, cx, move |mut submenu, window, _| {
            for (target, label) in &sections {
                let target = *target;
                submenu = submenu.item(PopupMenuItem::new(label.clone()).checked(section == Some(target)).disabled(section == Some(target))
                    .on_click(window.listener_for(&submenu_center, move |this: &mut CenterArea, _, _, cx| this.studio_move_screen(id, Some(target), None, cx))));
            }
            if !sections.is_empty() { submenu = submenu.separator(); }
            submenu.item(PopupMenuItem::new("New section…").on_click(window.listener_for(&submenu_center, move |this: &mut CenterArea, _, window, cx| this.studio_new_section_with(id, window, cx))))
                .item(PopupMenuItem::new("Unsectioned").checked(section.is_none()).disabled(section.is_none())
                    .on_click(window.listener_for(&submenu_center, move |this: &mut CenterArea, _, _, cx| this.studio_move_screen(id, None, None, cx))))
        });
    }
    for (label, action) in [("Move earlier", "up"), ("Move later", "down"), ("Archive / restore", "archive"), ("Mobile viewport", "mobile"), ("Desktop viewport", "desktop")] {
        menu = menu.item(PopupMenuItem::new(label).on_click(window.listener_for(&center, move |this: &mut CenterArea, _, _, cx| this.studio_screen_menu_action(id, action, cx))));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "ui-layout-tests")]
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[cfg(feature = "ui-layout-tests")]
    struct DropRows { highlights: Arc<AtomicUsize>, drops: Arc<AtomicUsize> }
    #[cfg(feature = "ui-layout-tests")]
    impl Render for DropRows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let highlights = self.highlights.clone();
            let drops = self.drops.clone();
            v_flex().size_full()
                .child(div().id("drag-source").w_full().h(px(48.))
                    .on_drag(DraggedStudioScreen { id: Uuid::nil(), name: "Screen".into() }, |drag, _, _, cx| cx.new(|_| drag.clone())))
                .child(div().id("drop-section").w_full().h(px(48.)).hover(|style| style)
                    .drag_over::<DraggedStudioScreen>(move |style, _, _, _| {
                        highlights.fetch_add(1, Ordering::Relaxed);
                        style.border_1()
                    })
                    .on_drop(move |_: &DraggedStudioScreen, _, _| { drops.fetch_add(1, Ordering::Relaxed); }))
        }
    }
    #[cfg(feature = "ui-layout-tests")]
    struct CachedRows(gpui::Entity<DropRows>);
    #[cfg(feature = "ui-layout-tests")]
    impl Render for CachedRows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            gpui::AnyView::from(self.0.clone()).cached(gpui::StyleRefinement::default().size_full())
        }
    }
    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn studio_cached_sidebar_keeps_drop_feedback_and_escape_cancels(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let _escape = cx.update(drag_escape);
        let highlights = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let (_, cx) = cx.add_window_view(|window, cx| {
            let rows = cx.new(|_| DropRows { highlights: highlights.clone(), drops: drops.clone() });
            let view = cx.new(|_| CachedRows(rows));
            gpui_component::Root::new(view, window, cx)
        });
        cx.simulate_resize(gpui::size(px(320.), px(200.)));
        cx.run_until_parked();
        let source = gpui::point(px(40.), px(24.));
        let target = gpui::point(px(40.), px(72.));
        let modifiers = gpui::Modifiers::default();
        cx.simulate_mouse_down(source, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_move(target, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_move(target, gpui::MouseButton::Left, modifiers);
        cx.run_until_parked();
        assert!(highlights.load(Ordering::Relaxed) > 0, "Cached targets show drag feedback");
        cx.simulate_keystrokes("escape");
        cx.simulate_mouse_up(target, gpui::MouseButton::Left, modifiers);
        cx.run_until_parked();
        assert_eq!(drops.load(Ordering::Relaxed), 0, "Escape cancels without a drop");
        cx.simulate_mouse_down(source, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_move(target, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_move(target, gpui::MouseButton::Left, modifiers);
        cx.simulate_mouse_up(target, gpui::MouseButton::Left, modifiers);
        cx.run_until_parked();
        assert_eq!(drops.load(Ordering::Relaxed), 1, "One accepted drop invokes one mutation");
    }
    fn screen(name: &str, archived: bool) -> StudioScreen {
        StudioScreen { id: Uuid::new_v4(), name: name.into(), width: 390, height: 844, archived, files: Default::default() }
    }
    #[test]
    fn grid_groups_follow_section_order_and_hide_archived_and_empty_groups() {
        let screens = vec![screen("Feed", false), screen("Old", true), screen("Composer", false), screen("Loose", false)];
        let mut manifest: StudioDesignManifest = serde_json::from_value(serde_json::json!({
            "schema_version":1,"id":Uuid::new_v4(),"name":"Grid","revision":0,"design_system":"",
            "screens":screens,"source_doc":null,"source_task":null
        })).unwrap();
        assert_eq!(grid_groups(&manifest).len(), 1);
        assert!(grid_groups(&manifest)[0].section.is_none(), "No headings without sections");
        manifest.sections = vec![
            StudioSection { screen_ids: vec![screens[2].id, screens[1].id, screens[0].id], ..StudioSection::new("Add post") },
            StudioSection::new("Empty"),
        ];
        let groups = grid_groups(&manifest);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].section.as_ref().unwrap().1, "Add post");
        assert_eq!(groups[0].screens.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Composer", "Feed"]);
        assert_eq!(groups[1].section.as_ref().unwrap().1, "Unsectioned");
        assert_eq!(groups[1].screens.len(), 1);
    }
}

fn section_menu(
    mut menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    context: &MenuContext,
    id: Uuid,
    first: bool,
    last: bool,
) -> PopupMenu {
    web_preview::suspend_for_menu(context.host.clone(), cx);
    let center = context.center.clone();
    for (label, action, disabled) in [
        ("Rename", "rename", false),
        ("Add screen", "add-screen", false),
        ("Move earlier", "earlier", first),
        ("Move later", "later", last),
        ("Fit section", "fit", false),
        ("Ungroup section", "ungroup", false),
    ] {
        menu = menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(window.listener_for(&center, move |this: &mut CenterArea, _, window, cx| {
            this.studio_section_menu_action(id, action, window, cx)
        })));
    }
    menu
}
