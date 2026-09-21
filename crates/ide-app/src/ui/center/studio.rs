use super::*;
use gpui_component::Colorize;
use ide_core::studio::*;
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StudioTab {
    Agent,
    Screens,
    System,
}
pub(super) struct StudioWorkspace {
    pub project: ProjectId,
    pub store: StudioStore,
    pub design: StudioDesign,
    pub canvas: super::studio_canvas::Runtime,
    pub screen: Option<Uuid>,
    pub inline_screen: Option<Uuid>,
    pub inline_flush: Option<(Uuid, Option<Uuid>)>,
    pub prototype: bool,
    pub prototype_history: Vec<Uuid>,
    pub preview_mode: bool,
    pub viewing_size: Option<(u32, u32)>,
    pub export_flush: Option<(Uuid, Uuid)>,
    pub exporting: bool,
    pub tab: StudioTab,
    pub sidebar_collapsed: bool,
    pub folded_sections: HashSet<String>,
    pub sidebar_scroll: [gpui::ScrollHandle; 3],
    pub editor_session: Uuid,
    pub editor_html: Option<String>,
    pub dirty: bool,
    pub saving: bool,
    pub pending_implementation: Option<Option<Vec<Uuid>>>,
    pub pending_implementation_picker: bool,
    pub implementation_flush: Option<Uuid>,
    pub pending_screen: Option<Option<Uuid>>,
    pub selected_element: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub preparing_comparison: bool,
    pub relative_chat: PathBuf,
    pub conversations: StudioConversations,
    pub implementation_agents: Vec<Uuid>,
    pub refreshing: bool,
    pub thumbnails_busy: bool,
    pub thumbnail_revision: Option<String>,
    pub redo: bool,
    pub pending_mode: Option<CenterMode>,
    pub pending_navigation: Option<Box<dyn FnOnce(&mut CenterArea, &mut Context<CenterArea>)>>,
    pub poll_id: Uuid,
    pub overview_scroll: gpui::UniformListScrollHandle,
}
impl StudioWorkspace {
    pub fn editing_screen(&self) -> Option<Uuid> { self.screen.or(self.inline_screen) }
}
#[derive(Clone)]
enum NameAction {
    Design(ProjectId, Option<Uuid>),
    Screen(Option<Uuid>),
    Token(String, bool),
    NewToken,
}

impl CenterArea {
    pub(super) fn render_studio_hub_card(
        &self,
        project: ProjectId,
        design: StudioDesignManifest,
        key: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = design.id;
        let count = design
            .screens
            .iter()
            .filter(|screen| !screen.archived)
            .count();
        let caption = match count {
            0 => "No screens yet · Local".to_string(),
            1 => "1 screen · Local".to_string(),
            n => format!("{n} screens · Local"),
        };
        let preview = self
            .studio_catalog_previews
            .get(&project)
            .and_then(|previews| previews.get(&id));
        let preview = if let Some(path) = preview {
            img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    gpui_component::Icon::new(crate::ui::design::design_icon())
                        .size_8()
                        .text_color(crate::ui::design::accent(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Open to start designing"),
                )
                .into_any_element()
        };
        style::design_hub_card(("studio-hub-card", key), cx)
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
                            .rounded(crate::ui::design::r_xs())
                            .bg(crate::ui::design::surface(cx).opacity(0.92))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Studio"),
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
                            .child(design.name),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(caption),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_studio(project, id, cx)))
            .into_any_element()
    }

    pub(crate) fn studio_designs(&self, project: ProjectId) -> Vec<StudioDesignManifest> {
        self.studio_catalog
            .get(&project)
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn refresh_studio_catalog(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = StudioStore::for_project(root)?;
                    let designs = store.list()?;
                    let systems = store.systems()?;
                    let mut previews = designs
                        .iter()
                        .filter_map(|design| {
                            design
                                .screens
                                .iter()
                                .filter(|screen| !screen.archived)
                                .find_map(|screen| store.last_thumbnail(screen.id))
                                .map(|path| (design.id, path))
                        })
                        .collect::<HashMap<_, _>>();
                    for system in &systems {
                        if let Some(path) = store.last_thumbnail(system.id) {
                            previews.insert(system.id, path);
                        }
                    }
                    Ok::<_, anyhow::Error>((designs, previews, systems))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((designs, previews, systems)) => {
                        this.studio_catalog.insert(project, designs);
                        this.studio_system_catalog.insert(project, systems);
                        this.studio_catalog_previews.insert(project, previews);
                    }
                    Err(error) => this.design_hub_error = Some(format!("Studio: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn create_studio_from_hub(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.studio_name_dialog(
            "New Studio design",
            "",
            NameAction::Design(project, None),
            window,
            cx,
        );
    }
    pub(super) fn defer_studio_navigation(
        &mut self,
        action: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.studio_canvas_active(){return self.defer_canvas_navigation(action,cx);}
        let Some(studio) = self.studio.as_mut().filter(|s| s.dirty || s.saving) else {
            return false;
        };
        studio.pending_navigation = Some(Box::new(action));
        let reply = json!({"session":studio.editor_session,"type":"flush"});
        self.web_host
            .update(cx, |host, _| host.studio_reply(&reply));
        true
    }
    pub(crate) fn open_studio(&mut self, project: ProjectId, id: Uuid, cx: &mut Context<Self>) {
        if self.defer_studio_navigation(move |this, cx| this.open_studio(project, id, cx), cx) {
            return;
        }
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = StudioStore::for_project(root)?;
                    let design = store.load(id)?;
                    let conversations = store.conversations(id)?;
                    let implementation_agents = store.implementation_agents(id)?;
                    Ok::<_, anyhow::Error>((store, design, conversations, implementation_agents))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((store, design, conversations, implementation_agents)) => {
                        this.studio_system_library = None;
                        this.penpot_open_design = None;
                        let system_workspace = design.manifest.system_workspace;
                        this.figma_open_design = None;
                        this.penpot_compare_open = false;
                        let selection = fs::read(store.cache.join(format!("selection-{id}.json")))
                            .ok()
                            .and_then(|bytes| {
                                serde_json::from_slice::<serde_json::Value>(&bytes).ok()
                            });
                        let screen = selection
                            .as_ref()
                            .and_then(|v| v["screen"].as_str())
                            .and_then(|s| s.parse::<Uuid>().ok())
                            .filter(|id| design.documents.contains_key(id))
                            .or_else(|| system_workspace.then_some(id));
                        let (canvas,canvas_notice)=super::studio_canvas::Runtime::new(&store,&design);
                        this.flush_studio_canvas();
                        this.studio = Some(StudioWorkspace {
                            canvas,
                            project,
                            store,
                            design,
                            screen,
                            inline_screen: None,
                            inline_flush: None,
                            prototype: false,
                            prototype_history: vec![],
                            preview_mode: system_workspace
                                || selection.as_ref().is_some_and(|v| v["preview"] == true),
                            viewing_size: None,
                            export_flush: None,
                            exporting: false,
                            tab: if system_workspace {
                                StudioTab::Agent
                            } else {
                                StudioTab::Screens
                            },
                            sidebar_collapsed: false,
                            sidebar_scroll: std::array::from_fn(|_| gpui::ScrollHandle::new()),
                            folded_sections: ["Archived", "Recipes", "Screen styles", "Other"]
                                .into_iter()
                                .map(str::to_string)
                                .collect(),
                            editor_session: Uuid::new_v4(),
                            editor_html: None,
                            dirty: false,
                            saving: false,
                            pending_implementation: None,
                            pending_implementation_picker: false,
                            implementation_flush: None,
                            pending_screen: None,
                            selected_element: None,
                            error: None,
                            notice: canvas_notice,
                            preparing_comparison: false,
                            relative_chat: conversation_path_for(
                                id,
                                conversations.selected,
                                system_workspace,
                            ),
                            conversations,
                            implementation_agents,
                            refreshing: false,
                            thumbnails_busy: false,
                            thumbnail_revision: None,
                            redo: false,
                            pending_mode: None,
                            pending_navigation: None,
                            poll_id: Uuid::new_v4(),
                            overview_scroll: gpui::UniformListScrollHandle::new(),
                        });
                        this.set_view_mode(CenterMode::Design, cx);
                        this.rebuild_studio_editor(cx);
                        this.start_studio_poll(id, cx);
                        this.refresh_studio_catalog(project, cx);
                    }
                    Err(error) => {
                        this.design_hub_error = Some(format!("Could not open Studio: {error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn studio_workspace_visible(&self, cx: &App) -> bool {
        self.view_mode == CenterMode::Design
            && !self.web_host.read(cx).is_route_suspended()
            && self.studio.as_ref().is_some_and(|studio| {
                self.active_project(cx)
                    .is_some_and(|(project, _)| project == studio.project)
            })
    }

    fn start_studio_poll(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(poll_id) = self.studio.as_ref().map(|s| s.poll_id) else {
            return;
        };
        cx.spawn(async move |this, cx| loop {
            smol::Timer::after(Duration::from_secs(1)).await;
            let keep = this
                .update(cx, |this, cx| {
                    if !this
                        .studio
                        .as_ref()
                        .is_some_and(|s| s.design.manifest.id == id && s.poll_id == poll_id)
                    {
                        return false;
                    }
                    if this.studio_workspace_visible(cx) {
                        this.refresh_studio(cx);
                        cx.notify();
                    } else {
                        // A running design agent still needs its explicitly
                        // requested review image after the user navigates away.
                        this.queue_studio_thumbnails(cx);
                    }
                    true
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
        self.refresh_studio(cx);
    }
    fn refresh_studio(&mut self, cx: &mut Context<Self>) {
        if !self.studio_workspace_visible(cx) {
            return;
        }
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.refreshing {
            return;
        }
        studio.refreshing = true;
        let store = studio.store.clone();
        let id = studio.design.manifest.id;
        cx.spawn(async move|this,cx|{
            let result=cx.background_executor().spawn(async move{store.load(id)}).await;
            let _=this.update(cx,|this,cx|{
                let Some(studio)=this.studio.as_mut().filter(|s|s.design.manifest.id==id) else{return;};studio.refreshing=false;
                match result{
                    Ok(design) if design.fingerprint!=studio.design.fingerprint=>{
                        let active_unchanged = studio.editing_screen().is_some_and(|screen|
                            studio.design.documents.get(&screen) == design.documents.get(&screen)
                            && studio.design.manifest.screens.iter().find(|s| s.id == screen) == design.manifest.screens.iter().find(|s| s.id == screen)
                            && studio.design.system == design.system
                            && studio.design.asset_fingerprint == design.asset_fingerprint
                            && studio.design.overrides.tokens == design.overrides.tokens);
                        if active_unchanged {
                            // Keep the live editor and caret while another screen is saved.
                            // Its original revision remains valid for an independent save.
                            let reply = json!({"session":studio.editor_session,"type":"screens","screens":design.manifest.screens});
                            studio.design=design;
                            this.web_host.update(cx, |host, _| host.studio_reply(&reply));
                        } else if studio.dirty||studio.saving {
                            studio.error=Some("This screen changed while you were editing. Your draft is preserved; save will check for overlapping changes.".into());
                        } else {studio.redo=false;studio.design=design;this.rebuild_studio_editor(cx);}
                    }
                    Err(error)=>studio.error=Some(format!("Could not refresh Studio: {error:#}")),_=>{}
                }
                this.queue_studio_thumbnails(cx);cx.notify();
            });
        }).detach();
    }
    fn queue_studio_thumbnails(&mut self, cx: &mut Context<Self>) {
        let visible = self.studio_workspace_visible(cx);
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.thumbnails_busy
            || ((!visible
                || studio.thumbnail_revision.as_ref() == Some(&studio.design.fingerprint))
                && studio
                    .store
                    .has_requested_thumbnails()
                    .map(|pending| !pending)
                    .unwrap_or(false))
        {
            return;
        }
        studio.thumbnails_busy = true;
        if visible {
            studio.thumbnail_revision = Some(studio.design.fingerprint.clone());
        }
        let store = studio.store.clone();
        let mut design = studio.design.clone();
        let canvas_overview=studio.screen.is_none()&&!studio.design.manifest.system_workspace&&studio.canvas.active();
        let project = studio.project;
        let id = design.manifest.id;
        if let Some(selected) = studio.screen {
            design.manifest.screens.sort_by_key(|s| s.id != selected);
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    for revision in store.requested_thumbnails()? {
                        super::studio_editor::render_thumbnails(store.clone(), revision)?;
                    }
                    if visible && !canvas_overview {
                        super::studio_editor::render_thumbnails(store, design)?;
                    }
                    Ok::<(), anyhow::Error>(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == id) {
                    s.thumbnails_busy = false;
                    if let Err(error) = result {
                        s.error = Some(format!("Thumbnails: {error:#}"));
                    }
                }
                // A batch can finish after Back. Refresh the hub's cached image
                // without reopening the editor or starting another renderer.
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn rebuild_studio_editor(&mut self, cx: &App) {
        if self.studio_canvas_active(){
            if let Some(s)=self.studio.as_mut(){
                if s.inline_screen.is_some_and(|id|!s.design.manifest.screens.iter().any(|p|p.id==id&&!p.archived)) && !s.dirty && !s.saving {
                    s.inline_screen=None;
                    super::studio_editor::revoke_inline(s.canvas.session);
                    self.web_host.read(cx).canvas_reply(&json!({"session":s.canvas.session,"type":"editor","screen_id":null}));
                }
                s.export_flush=None;s.selected_element=None;s.editor_session=Uuid::new_v4();s.editor_html=None;
            }
            self.refresh_studio_canvas(cx);
            self.refresh_studio_bootstrap(cx);
            if let Some(s)=self.studio.as_ref().filter(|s|s.inline_screen.is_some()) {
                let reply=json!({"session":s.canvas.session,"type":"editor","screen_id":s.inline_screen,"editor_session":s.editor_session,"document":s.editor_html});
                self.web_host.read(cx).canvas_reply(&reply);
            }
            self.focus_studio_canvas_screen(cx);
            return;
        }
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        studio.export_flush = None;
        studio.editor_session = Uuid::new_v4();
        studio.selected_element = None;
        self.refresh_studio_bootstrap(cx);
    }
    fn refresh_studio_bootstrap(&mut self, cx: &App) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        studio.editor_html=studio.editing_screen().and_then(|screen|{
            let theme=super::studio_editor::web_theme(cx);
            let mut rendered = studio.design.clone();
            if let Some((width, height)) = studio.viewing_size { if let Some(s) = rendered.manifest.screens.iter_mut().find(|s| s.id == screen) { s.width = width; s.height = height; } }
            use super::studio_editor::{document_for_surface,EditorSurface};
            let surface=if studio.inline_screen.is_some(){EditorSurface::Canvas}else if studio.prototype{EditorSurface::Prototype}else{EditorSurface::Screen};
            match document_for_surface(&studio.store,&rendered,screen,studio.editor_session,false,studio.preview_mode && studio.inline_screen.is_none(),theme,surface){Ok(html)=>Some(html),Err(error)=>{studio.error=Some(format!("Could not load screen: {error:#}"));None}}

        });
        if studio.inline_screen.is_some() {
            if studio.editor_html.is_some() { super::studio_editor::register_inline(studio.canvas.session,studio.editor_session); }
        }
    }
    pub(super) fn studio_inline_select(&mut self, screen: Option<Uuid>, cx: &mut Context<Self>) {
        let Some(s)=self.studio.as_mut() else{return;};
        if screen.is_some_and(|id|!s.design.manifest.screens.iter().any(|p|p.id==id&&!p.archived)) {return;}
        if s.inline_screen==screen || s.canvas.navigation_pending() {return;}
        if s.saving && s.inline_screen.is_none() {s.error=Some("Wait for the current screen change to save.".into());return;}
        if s.inline_screen.is_some() {
            let request=Uuid::new_v4();s.inline_flush=Some((request,screen));
            let reply=json!({"session":s.editor_session,"type":"flush","request_id":request});
            self.web_host.read(cx).studio_reply(&reply);
            return;
        }
        self.studio_finish_inline(screen,cx);
    }
    pub(super) fn studio_finish_inline(&mut self, screen: Option<Uuid>, cx: &mut Context<Self>) {
        let Some(s)=self.studio.as_mut() else{return;};
        super::studio_editor::revoke_inline(s.canvas.session);
        s.inline_screen=screen;s.inline_flush=None;s.dirty=false;s.saving=false;s.selected_element=None;
        s.editor_html=None;s.editor_session=Uuid::new_v4();
        if screen.is_some(){
            s.canvas.layout.selected_screen_id=screen;
            if !s.canvas.corrupt {if let Err(error)=s.store.save_canvas_state(s.design.manifest.id,&s.canvas.layout){s.error=Some(format!("Could not save canvas selection: {error}"));}}
        }
        s.canvas.invalidate_metadata();
        if screen.is_none(){
            self.web_host.read(cx).canvas_reply(&json!({"session":s.canvas.session,"type":"editor","screen_id":null}));
        }
        self.rebuild_studio_editor(cx);
        cx.notify();
    }
    pub(super) fn studio_start_prototype(&mut self, cx: &mut Context<Self>) {
        if self.defer_studio_navigation(|this,cx|this.studio_start_prototype(cx),cx){return;}
        let Some(s)=self.studio.as_mut() else{return;};
        let target=s.editing_screen().or(s.canvas.layout.selected_screen_id)
            .filter(|id|s.design.manifest.screens.iter().any(|p|p.id==*id&&!p.archived))
            .or_else(||s.design.manifest.screens.iter().find(|p|!p.archived).map(|p|p.id));
        let Some(target)=target else{return;};
        s.prototype=true;s.preview_mode=true;s.prototype_history.clear();s.inline_screen=None;
        self.studio_select_screen(Some(target),cx);
    }
    fn studio_prototype_back(&mut self, cx: &mut Context<Self>) {
        let target=self.studio.as_mut().and_then(|s|s.prototype_history.pop());
        if let Some(id)=target {self.studio_select_screen(Some(id),cx);}
    }
    pub(super) fn studio_select_screen(&mut self, screen: Option<Uuid>, cx: &mut Context<Self>) {
        if self.studio_canvas_active() && !self.studio.as_ref().is_some_and(|s|s.prototype) {
            if let Some(s) = self.studio.as_mut() {
                s.canvas.focus_screen = screen.filter(|id| s.design.manifest.screens.iter().any(|p| p.id == *id && !p.archived));
            }
            self.studio_inline_select(screen,cx);
            self.focus_studio_canvas_screen(cx);
            return;
        }
        if screen.is_some()&&self.defer_canvas_navigation(move|this,cx|this.studio_select_screen(screen,cx),cx){return;}
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.dirty || (studio.saving && studio.screen.is_some()) {
            studio.pending_screen = Some(screen);
            let reply = json!({"session":studio.editor_session,"type":"flush"});
            self.web_host
                .update(cx, |host, _| host.studio_reply(&reply));
            return;
        }
        if screen.is_some(){
            if let Some(id)=screen{studio.canvas.layout.selected_screen_id=Some(id);}
            if !studio.canvas.corrupt {if let Err(e)=studio.store.save_canvas_state(studio.design.manifest.id,&studio.canvas.layout){studio.error=Some(e.to_string());}}
            studio.canvas.leave();
        }
        studio.screen = screen;
        if screen.is_none() { studio.prototype=false;studio.prototype_history.clear(); }
        studio.viewing_size = None;
        if let Err(error) = atomic(
            &studio
                .store
                .cache
                .join(format!("selection-{}.json", studio.design.manifest.id)),
            &serde_json::to_vec(&json!({"screen":screen,"preview":studio.preview_mode}))
                .unwrap_or_default(),
        ) {
            studio.error = Some(format!("Could not remember screen: {error:#}"));
        }
        self.rebuild_studio_editor(cx);
        cx.notify();
    }
    pub(super) fn process_studio_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let messages = super::studio_editor::drain();
        if messages.is_empty() {
            return;
        }
        for message in messages {
            let Some(studio) = self.studio.as_mut() else {
                continue;
            };
            if message["session"]
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok())
                != Some(studio.editor_session)
            {
                continue;
            }
            match message["type"].as_str().unwrap_or("") {
                "mode" => {
                    let Some(mode) = message["mode"]
                        .as_str()
                        .filter(|mode| matches!(*mode, "edit" | "preview"))
                    else {
                        continue;
                    };
                    studio.preview_mode = mode == "preview";
                    if let Err(error) = atomic(
                        &studio
                            .store
                            .cache
                            .join(format!("selection-{}.json", studio.design.manifest.id)),
                        &serde_json::to_vec(
                            &json!({"screen":studio.screen,"preview":studio.preview_mode}),
                        )
                        .unwrap_or_default(),
                    ) {
                        studio.error = Some(format!("Could not remember editor mode: {error:#}"));
                    }
                    self.refresh_studio_bootstrap(cx);
                }

                "system-token" if studio.design.manifest.system_workspace => {
                    if let Some(token) = message["token"].as_str() {
                        if let Some(value) = studio.design.system.tokens.get(token).cloned() {
                            studio.selected_element = Some(format!("token:{token}"));
                            self.studio_override_dialog(Some(token.into()), value, window, cx);
                        }
                    }
                }
                "system-recipe" if studio.design.manifest.system_workspace => {
                    if let Some(recipe) = message["recipe"].as_str().map(str::to_string) {
                        self.studio_recipe_dialog(recipe, window, cx);
                    }
                }
                "render-error" => {
                    studio.error = Some(
                        message["error"]
                            .as_str()
                            .unwrap_or("The editor could not complete this action")
                            .to_string(),
                    );
                }
                "asset" => {
                    use base64::Engine;
                    let extension = message["extension"].as_str().unwrap_or("");
                    if !["png", "jpg", "jpeg", "gif", "webp", "svg"].contains(&extension) {
                        continue;
                    }
                    if let Some(data) = message["data_url"]
                        .as_str()
                        .and_then(|s| s.split_once(',').map(|(_, data)| data))
                    {
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                            let mut operations = Vec::new();
                            let name = if message.get("document").is_some() {
                                // Import and assign together; never apply a file-read result
                                // to a document that changed while the picker was open.
                                if message["revision"].as_u64()
                                    != Some(studio.design.manifest.revision)
                                    || message["fingerprint"].as_str()
                                        != Some(studio.design.fingerprint.as_str())
                                {
                                    studio.error =
                                        Some("The screen changed. Choose the image again.".into());
                                    continue;
                                }
                                let Some(name) = message["name"].as_str() else {
                                    continue;
                                };
                                let Some(stem) = name.strip_suffix(&format!(".{extension}")) else {
                                    continue;
                                };
                                if stem.parse::<Uuid>().is_err() {
                                    continue;
                                }
                                let Some(screen_id) = studio.editing_screen() else {
                                    continue;
                                };
                                let Ok(document) = serde_json::from_value::<StudioDocument>(
                                    message["document"].clone(),
                                ) else {
                                    continue;
                                };
                                operations.push(StudioOperation::WriteScreen {
                                    screen_id,
                                    document,
                                });
                                name.to_string()
                            } else {
                                format!("{}.{extension}", Uuid::new_v4())
                            };
                            operations.insert(0, StudioOperation::AddAsset { name, bytes });
                            self.studio_apply_ui(operations, false, cx);
                        }
                    }
                }
                "dirty" => {
                    studio.dirty = message["dirty"].as_bool().unwrap_or(true);
                    if let Some(screen) = studio.editing_screen() {
                        if let Err(error) = atomic(
                            &studio
                                .store
                                .cache
                                .join("drafts")
                                .join(format!("{screen}.json")),
                            &serde_json::to_vec(&message).unwrap_or_default(),
                        ) {
                            studio.error = Some(format!("Could not preserve draft: {error:#}"));
                        }
                    }
                }
                "recover" => {
                    studio.inline_flush=None;studio.pending_navigation=None;studio.pending_screen=None;studio.pending_mode=None;
                    studio.canvas.cancel_navigation();
                    if let Some(screen) = studio.editing_screen() {
                        let path = studio
                            .store
                            .cache
                            .join("drafts")
                            .join(format!("{screen}.json"));
                        if let Ok(bytes) = fs::read(&path) {
                            if let Ok(mut draft) =
                                serde_json::from_slice::<serde_json::Value>(&bytes)
                            {
                                draft["dirty"] = json!(false);
                                draft["recoverable"] = json!(true);
                                let _ =
                                    atomic(&path, &serde_json::to_vec(&draft).unwrap_or_default());
                            }
                        }
                    }
                    studio.dirty = false;
                    studio.saving = false;
                    match studio.store.load(studio.design.manifest.id) {
                        Ok(design) => {
                            studio.design = design;
                            studio.error=Some("Draft preserved. Ask the Studio agent to reconcile the recovered draft with the current screen.".into());
                        }
                        Err(error) => studio.error = Some(format!("Could not reload: {error:#}")),
                    }
                    self.rebuild_studio_editor(cx);
                }
                "selection" => {
                    studio.selected_element = message["element"].as_str().map(str::to_string)
                }
                "navigate" => {
                    let id = message["screen_id"].as_str().and_then(|s| s.parse().ok());
                    if id.is_some_and(|id| studio.design.manifest.screens.iter().any(|s|s.id==id&&!s.archived)) {
                        if studio.prototype && id!=studio.screen { if let Some(previous)=studio.screen { studio.prototype_history.push(previous); } }
                        self.studio_select_screen(id, cx);
                    }
                }
                "flushed" => {
                    studio.dirty = false;
                    if let Some((request,next))=studio.inline_flush {
                        if message["request_id"].as_str().and_then(|id|id.parse::<Uuid>().ok())==Some(request) {
                            studio.inline_flush=None;
                            self.studio_finish_inline(next,cx);
                            continue;
                        }
                    }
                    let matching_flush = studio.implementation_flush.is_some()
                        && message["request_id"]
                            .as_str()
                            .and_then(|s| s.parse::<Uuid>().ok())
                            == studio.implementation_flush;
                    let implementation = if matching_flush {
                        studio.pending_implementation.take()
                    } else {
                        None
                    };
                    let picker =
                        matching_flush && std::mem::take(&mut studio.pending_implementation_picker);
                    if matching_flush {
                        studio.implementation_flush = None;
                    }
                    let exported_screen = studio
                        .export_flush
                        .filter(|(request, _)| {
                            message["request_id"]
                                .as_str()
                                .and_then(|s| s.parse::<Uuid>().ok())
                                == Some(*request)
                        })
                        .map(|(_, screen)| screen);
                    if exported_screen.is_some() {
                        studio.export_flush = None;
                    }
                    // Capture the saved screen before any queued navigation changes it.
                    if let Some(screen) = exported_screen {
                        self.studio_finish_export(screen, cx);
                    }
                    let Some(studio) = self.studio.as_mut() else {
                        continue;
                    };
                    let mode = studio.pending_mode.take();
                    let next = studio.pending_screen.take();
                    let navigation = if studio.inline_screen.is_none() { studio.pending_navigation.take() } else { None };
                    if let Some(next) = next {
                        self.studio_select_screen(next, cx);
                    }
                    if let Some(mode) = mode {
                        self.set_view_mode(mode, cx);
                    }
                    if let Some(navigation) = navigation {
                        navigation(self, cx);
                    }
                    if let Some(subset) = implementation {
                        self.studio_finish_implement(subset, window, cx);
                    } else if picker {
                        self.studio_finish_implementation_picker(window, cx);
                    }
                }
                "save" => {
                    let Some(screen) = studio.editing_screen() else {
                        continue;
                    };
                    if studio.saving {
                        continue;
                    }
                    let parsed = (|| -> anyhow::Result<_> {
                        Ok((
                            message["id"]
                                .as_str()
                                .ok_or_else(|| anyhow::anyhow!("Missing edit ID"))?
                                .parse::<Uuid>()?,
                            serde_json::from_value::<StudioDocument>(message["document"].clone())?,
                        ))
                    })();
                    let Ok((id, document)) = parsed else {
                        continue;
                    };
                    let mut scope = StudioTurnScope::screen(studio.design.manifest.id, screen);
                    let mut operations = vec![StudioOperation::WriteScreen {
                        screen_id: screen,
                        document,
                    }];
                    if let Some(tokens) = message["document"].get("overrides").and_then(|v| {
                        serde_json::from_value::<std::collections::BTreeMap<String, String>>(
                            v.clone(),
                        )
                        .ok()
                    }) {
                        if tokens != studio.design.overrides.tokens {
                            scope.allow_design_overrides = true;
                            operations.push(StudioOperation::SetOverrides {
                                overrides: StudioOverrides {
                                    tokens,
                                    ..Default::default()
                                },
                            });
                        }
                    }
                    let tx = StudioTransaction {
                        id,
                        scope_id: scope.id,
                        design_id: scope.design_id,
                        expected_revision: message["revision"].as_u64().unwrap_or(u64::MAX),
                        expected_fingerprint: message["fingerprint"].as_str().unwrap_or("").into(),
                        operations,
                    };
                    studio.saving = true;
                    let store = studio.store.clone();
                    let session = studio.editor_session;
                    cx.spawn(async move|this,cx|{
                        let result=cx.background_executor().spawn(async move{store.apply(&scope,&tx)}).await;
                        let _=this.update(cx,|this,cx|{
                            let Some(studio)=this.studio.as_mut().filter(|s|s.editor_session==session) else{return;};studio.saving=false;
                            let reply=match result{Ok(design)=>{let reply=json!({"session":session,"id":id,"revision":design.manifest.revision,"fingerprint":design.fingerprint});studio.redo=false;studio.design=design;studio.error=None;reply},Err(error)=>{studio.error=Some(format!("{error:#}"));json!({"session":session,"id":id,"error":format!("{error:#}")})}};
                            this.web_host.update(cx,|host,_|host.studio_reply(&reply));this.refresh_studio_bootstrap(cx);this.queue_studio_thumbnails(cx);cx.notify();
                        });
                    }).detach();
                }
                _ => {}
            }
        }
        self.refresh_studio_bootstrap(cx);
    }
    pub(super) fn studio_apply_ui(
        &mut self,
        operations: Vec<StudioOperation>,
        shared: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.dirty || studio.saving {
            studio.error = Some("Wait for the current edit to save.".into());
            return;
        }
        let mut scope = StudioTurnScope::whole_design(&studio.design);
        scope.allow_shared_system = shared;
        scope.allow_system_binding = operations
            .iter()
            .any(|op| matches!(op, StudioOperation::BindSystem { .. }));
        let tx = StudioTransaction {
            id: Uuid::new_v4(),
            scope_id: scope.id,
            design_id: scope.design_id,
            expected_revision: studio.design.manifest.revision,
            expected_fingerprint: studio.design.fingerprint.clone(),
            operations,
        };
        let store = studio.store.clone();
        let id = scope.design_id;
        studio.saving = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.apply(&scope, &tx) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(studio) = this.studio.as_mut().filter(|s| s.design.manifest.id == id)
                else {
                    return;
                };
                studio.saving = false;
                match result {
                    Ok(design) => {
                        studio.redo = false;
                        studio.design = design;
                        studio.error = None;
                        this.rebuild_studio_editor(cx);
                    }
                    Err(error) => studio.error = Some(format!("{error:#}")),
                }
                this.queue_studio_thumbnails(cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn studio_override_dialog(
        &mut self,
        token: Option<String>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = token
            .as_deref()
            .unwrap_or("New local token · name: value")
            .to_string();
        self.studio_name_dialog(
            &title,
            &value,
            token
                .map(|name| NameAction::Token(name, false))
                .unwrap_or(NameAction::NewToken),
            window,
            cx,
        );
    }
    fn studio_name_dialog(
        &mut self,
        title: &str,
        value: &str,
        action: NameAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.studio.as_ref().is_some_and(|s| s.dirty || s.saving) {
            return;
        }
        let creating = matches!(action, NameAction::Design(_, _));
        let shared = matches!(action, NameAction::Token(_, true));
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value.to_string())
                .placeholder(if creating {
                    "e.g. Customer app"
                } else {
                    "Enter a value"
                })
        });
        let title = title.to_string();
        let center = cx.entity();
        let expected = (!creating)
            .then(|| self.studio.as_ref().map(|s| s.design.fingerprint.clone()))
            .flatten();
        let project = match &action {
            NameAction::Design(project, _) => Some(*project),
            _ => None,
        };
        let store = project
            .and_then(|project| self.project_by_id(project, cx))
            .and_then(|(_, root)| StudioStore::for_project(root).ok());
        let systems = store
            .as_ref()
            .and_then(|s| s.systems().ok())
            .unwrap_or_default();
        let default = store
            .as_ref()
            .and_then(|s| s.default_system_id().ok())
            .flatten()
            .filter(|id| {
                systems
                    .iter()
                    .any(|s| s.id == *id && !s.archived && s.applied.is_some())
            });
        let chosen_system = std::rc::Rc::new(std::cell::Cell::new(default));
        let required = std::rc::Rc::new(std::cell::Cell::new(false));
        let affected = self
            .studio
            .as_ref()
            .map(|s| {
                self.studio_designs(s.project)
                    .iter()
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        // Both Enter and the primary action submit the current field and selection.
        let submit: std::rc::Rc<dyn Fn(&mut Window, &mut App) -> bool> = std::rc::Rc::new({
            let input = input.clone();
            let selected = chosen_system.clone();
            let required = required.clone();
            move |window, cx| {
                let value = input.read(cx).value().to_string();
                if value.trim().is_empty() {
                    required.set(true);
                    input.update(cx, |input, cx| input.focus(window, cx));
                    window.refresh();
                    return false;
                }
                let action = match action.clone() {
                    NameAction::Design(project, _) => NameAction::Design(project, selected.get()),
                    other => other,
                };
                center.update(cx, |this, cx| {
                    if expected.as_ref().is_some_and(|expected| this.studio.as_ref()
                        .is_none_or(|s| &s.design.fingerprint != expected)) {
                        if let Some(s) = this.studio.as_mut() {
                            s.error = Some("The design changed while this dialog was open. Reopen the value and try again.".into());
                        }
                        cx.notify();
                        return false;
                    }
                    this.studio_named_action(action, value, cx);
                    true
                })
            }
        });
        let focus_input = input.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let mut form = v_flex().w_full().gap_5().pt_1();
            form = form.child(v_flex().w_full().gap_2()
                .when(creating, |field| field.child(div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::MEDIUM).child("Design name")))
                .child(Input::new(&input).w_full())
                .when(required.get() && input.read(cx).value().trim().is_empty(), |field| {
                    field.child(div().text_size(crate::ui::design::text_label())
                        .child(if creating { "Enter a name for your design." } else { "Enter a value." }))
                }));
            if creating {
                let picked = chosen_system.clone();
                let systems = systems.clone();
                let label = systems.iter().find(|s| Some(s.id) == picked.get())
                    .map(|s| s.name.clone()).unwrap_or_else(|| "No system".into());
                let has_drafts = systems.iter().any(|s| !s.archived && s.applied.is_none());
                form = form.child(v_flex().w_full().gap_2()
                    .child(div().text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::MEDIUM).child("Design system"))
                    .child(style::sidebar_selector_button("studio-create-system", label.clone(), px(430.), cx)
                        .tooltip(label)
                        .dropdown_menu(move |mut menu, _, _| {
                            let none = picked.clone();
                            menu = menu.item(PopupMenuItem::new("No system").checked(picked.get().is_none())
                                .on_click(move |_, window, _| { none.set(None); window.refresh(); }));
                            for system in systems.iter().filter(|s| !s.archived) {
                                let select = picked.clone();
                                let id = system.id;
                                let draft = system.applied.is_none();
                                let label = if draft { format!("{} (Draft — apply first)", system.name) }
                                    else { system.name.clone() };
                                menu = menu.item(PopupMenuItem::new(label)
                                    .checked(picked.get() == Some(id)).disabled(draft)
                                    .on_click(move |_, window, _| { select.set(Some(id)); window.refresh(); }));
                            }
                            menu
                        }))
                    .child(div().text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(if has_drafts { "Apply draft systems in the library before using them." }
                            else { "You can change this later." })));
            }
            if shared {
                form = form.child(div().text_size(crate::ui::design::text_ui())
                    .child(format!("This updates the shared project system used by these Studio designs: {affected}. Design overrides remain in place.")));
            }
            let enter = submit.clone();
            let submit = submit.clone();
            dialog.title(title.clone()).w(px(480.)).child(form)
                .on_ok(move |_, window, cx| enter(window, cx))
                .footer(move |_, _, _, cx| {
                    let submit = submit.clone();
                    vec![
                        style::dialog_neutral_button("studio-name-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("studio-name-save", if creating { "Create design" } else { "Save" }, cx)
                            .on_click(move |_, window, cx| {
                                if submit(window, cx) { window.close_dialog(cx); }
                            }),
                    ]
                })
        });
        focus_input.update(cx, |input, cx| input.focus(window, cx));
    }
    fn studio_named_action(&mut self, action: NameAction, value: String, cx: &mut Context<Self>) {
        match action {
            NameAction::Design(project, system) => {
                let Some((_, root)) = self.project_by_id(project, cx) else {
                    return;
                };
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let store = StudioStore::for_project(root)?;
                            let design = store.create(&value)?;
                            let mut scope = StudioTurnScope::whole_design(&design);
                            scope.allow_system_binding = true;
                            store.apply(
                                &scope,
                                &StudioTransaction {
                                    id: Uuid::new_v4(),
                                    scope_id: scope.id,
                                    design_id: design.manifest.id,
                                    expected_revision: design.manifest.revision,
                                    expected_fingerprint: design.fingerprint.clone(),
                                    operations: vec![StudioOperation::BindSystem {
                                        system_id: system,
                                        expected_system_fingerprint: system
                                            .map(|id| store.system_binding_fingerprint(id))
                                            .transpose()?,
                                    }],
                                },
                            )
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        match result {
                            Ok(design) => this.open_studio(project, design.manifest.id, cx),
                            Err(error) => {
                                this.design_hub_error =
                                    Some(format!("Could not create Studio: {error:#}"))
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            NameAction::Screen(id) => {
                let Some(studio) = self.studio.as_ref() else {
                    return;
                };
                let op = if let Some(id) = id {
                    let Some(mut screen) = studio
                        .design
                        .manifest
                        .screens
                        .iter()
                        .find(|s| s.id == id)
                        .cloned()
                    else {
                        return;
                    };
                    screen.name = value;
                    StudioOperation::UpdateScreen { screen }
                } else {
                    StudioOperation::CreateScreen {
                        screen: StudioScreen {
                            id: Uuid::new_v4(),
                            name: value,
                            width: 1440,
                            height: 960,
                            archived: false,
                            files: StudioScreenFiles::default(),
                        },
                        document: starter_document(),
                    }
                };
                self.studio_apply_ui(vec![op], false, cx);
            }
            NameAction::NewToken => {
                let Some((name, value)) = value.split_once(':') else {
                    if let Some(s) = self.studio.as_mut() {
                        s.error = Some("Enter token-name: value".into());
                    }
                    return;
                };
                self.studio_named_action(
                    NameAction::Token(name.trim().into(), false),
                    value.trim().into(),
                    cx,
                );
            }
            NameAction::Token(name, shared) => {
                let Some(studio) = self.studio.as_ref() else {
                    return;
                };
                let shared = shared || studio.design.manifest.system_workspace;
                let op = if shared {
                    let mut system = studio.design.system.clone();
                    system.tokens.insert(name, value);
                    StudioOperation::SetSystem {
                        system,
                        expected_system_revision: studio.design.system.revision,
                    }
                } else {
                    let mut overrides = studio.design.overrides.clone();
                    overrides.tokens.insert(name, value);
                    StudioOperation::SetOverrides { overrides }
                };
                self.studio_apply_ui(vec![op], shared, cx);
            }
        }
    }
    fn studio_screen_menu_action(&mut self, id: Uuid, action: &str, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        let Some(mut screen) = studio
            .design
            .manifest
            .screens
            .iter()
            .find(|s| s.id == id)
            .cloned()
        else {
            return;
        };
        let op = match action {
            "duplicate" => {
                screen.id = Uuid::new_v4();
                screen.name = format!("{} copy", screen.name);
                StudioOperation::CreateScreen {
                    screen,
                    document: studio.design.documents[&id].clone(),
                }
            }
            "mobile" => {
                screen.width = 390;
                screen.height = 844;
                StudioOperation::UpdateScreen { screen }
            }
            "desktop" => {
                screen.width = 1440;
                screen.height = 960;
                StudioOperation::UpdateScreen { screen }
            }
            "archive" => {
                screen.archived = !screen.archived;
                StudioOperation::UpdateScreen { screen }
            }
            _ => {
                let mut ids = studio
                    .design
                    .manifest
                    .screens
                    .iter()
                    .map(|s| s.id)
                    .collect::<Vec<_>>();
                let index = ids.iter().position(|v| *v == id).unwrap();
                let target = if action == "up" {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(ids.len() - 1)
                };
                ids.swap(index, target);
                StudioOperation::Reorder { screen_ids: ids }
            }
        };
        self.studio_apply_ui(vec![op], false, cx);
    }
    fn select_studio_conversation(
        &mut self,
        design_id: Uuid,
        conversation: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self
            .studio
            .as_ref()
            .filter(|s| s.design.manifest.id == design_id)
        else {
            return;
        };
        let previous = self
            .doc_assistants
            .read(cx)
            .record_for(studio.project, &studio.relative_chat);
        if previous.as_ref().is_some_and(|record| {
            self.agent_chats
                .read(cx)
                .session(record.chat_agent_id)
                .is_some_and(|session| {
                    matches!(
                        session.status,
                        AgentChatStatus::Running | AgentChatStatus::Cancelling
                    )
                })
        }) {
            return;
        }
        let result = studio.store.select_conversation(design_id, conversation);
        let studio = self.studio.as_mut().unwrap();
        match result {
            Ok(history) => {
                let path = conversation_path_for(
                    design_id,
                    history.selected,
                    studio.design.manifest.system_workspace,
                );
                if conversation.is_none() {
                    let mut record = DocAssistantRecord::new(studio.project, path.clone());
                    if let Some(previous) = previous {
                        record.provider = previous.provider;
                        record.model = previous.model;
                        record.effort = previous.effort;
                        record.access_mode = previous.access_mode;
                        record.external_model_id = previous.external_model_id;
                        record.external_model_label = previous.external_model_label;
                        record.external_model_variants = previous.external_model_variants;
                    }
                    self.doc_assistants.update(cx, |assistants, cx| {
                        assistants.upsert_external_record(record, cx)
                    });
                }
                studio.relative_chat = path;
                studio.conversations = history;
                studio.error = None;
                self.composer_model_expanded = false;
            }
            Err(error) => studio.error = Some(format!("Could not open conversation: {error:#}")),
        }
        cx.notify();
    }
    pub(super) fn prepare_studio_turn(
        &mut self,
        agent: &AgentRecord,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(context) = agent.studio_context.as_ref() else {
            return true;
        };
        let Some(studio) = self
            .studio
            .as_mut()
            .filter(|s| s.design.manifest.id == context.design_id)
        else {
            return false;
        };
        let result = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                !studio.dirty && !studio.saving,
                "Wait for the current edit to save before sending."
            );
            let design = studio.store.load(context.design_id)?;
            let scope = scope_for_request(&design, studio.editing_screen().or(studio.canvas.layout.selected_screen_id), studio.selected_element.clone());
            studio.store.save_scope(agent.id, &scope)?;
            studio.conversations = studio.store.title_conversation(
                context.design_id,
                context.conversation_id,
                text,
            )?;
            atomic(
                &studio
                    .store
                    .cache
                    .join("roles")
                    .join(format!("{}.json", agent.id)),
                &serde_json::to_vec(context)?,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            studio.error = Some(format!("{error:#}"));
            cx.notify();
            false
        } else {
            studio.error = None;
            true
        }
    }
    pub(super) fn studio_undo(&mut self, redo: bool, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.dirty || studio.saving {
            return;
        }
        let current = studio.design.clone();
        let result = if redo {
            studio.store.redo_latest(current.manifest.id)
        } else {
            studio.store.undo_latest(current.manifest.id)
        };
        match result {
            Ok(design) => {
                studio.redo = !redo;
                studio.design = design;
                self.rebuild_studio_editor(cx);
            }
            Err(error) => studio.error = Some(format!("{error:#}")),
        }
        cx.notify();
    }
    fn studio_choose_implementation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .studio
            .as_ref()
            .is_some_and(|s| s.export_flush.is_some())
        {
            return;
        }
        if let Some(studio) = self.studio.as_mut().filter(|s| s.editing_screen().is_some()) {
            studio.pending_implementation_picker = true;
            studio.implementation_flush = Some(Uuid::new_v4());
            let reply = json!({"session":studio.editor_session,"type":"flush","request_id":studio.implementation_flush});
            self.web_host
                .update(cx, |host, _| host.studio_reply(&reply));
            return;
        }
        self.studio_finish_implementation_picker(window, cx);
    }
    fn studio_finish_implementation_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        let screens = studio
            .design
            .manifest
            .screens
            .iter()
            .filter(|s| !s.archived)
            .cloned()
            .collect::<Vec<_>>();
        let selected = std::rc::Rc::new(std::cell::RefCell::new(
            screens
                .iter()
                .map(|s| s.id)
                .collect::<std::collections::BTreeSet<_>>(),
        ));
        let center = cx.entity();
        window.open_dialog(cx, move |dialog, _, _cx| {
            let mut list = v_flex().gap_1();
            for (index, screen) in screens.iter().enumerate() {
                let chosen = selected.clone();
                let id = screen.id;
                list = list.child(
                    style::ghost_button_compact(
                        ("studio-handoff-screen", index),
                        screen.name.clone(),
                    )
                    .selected(selected.borrow().contains(&id))
                    .justify_start()
                    .on_click(move |_, window, _| {
                        let mut values = chosen.borrow_mut();
                        if !values.remove(&id) {
                            values.insert(id);
                        }
                        window.refresh();
                    }),
                );
            }
            let selected = selected.clone();
            let center = center.clone();
            dialog
                .title("Choose screens to implement")
                .child(
                    div()
                        .id("studio-subset-list")
                        .max_h(px(420.))
                        .overflow_y_scroll()
                        .child(list),
                )
                .footer(move |_, _, _, cx| {
                    let selected = selected.clone();
                    let center = center.clone();
                    let empty = selected.borrow().is_empty();
                    vec![
                        style::dialog_neutral_button("studio-subset-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact(
                            "studio-subset-implement",
                            "Implement selected",
                            cx,
                        )
                        .disabled(empty)
                        .on_click(move |_, window, cx| {
                            let ids = selected.borrow().iter().copied().collect();
                            window.close_dialog(cx);
                            center.update(cx, |this, cx| {
                                this.studio_finish_implement(Some(ids), window, cx)
                            });
                        }),
                    ]
                })
        });
    }
    fn studio_set_viewing_size(&mut self, mobile: bool, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        let Some(screen) = studio
            .design
            .manifest
            .screens
            .iter()
            .find(|s| Some(s.id) == studio.screen)
        else {
            return;
        };
        let size = if (screen.width < 600) == mobile {
            (screen.width, screen.height)
        } else if mobile {
            (390, 844)
        } else {
            (1440, 960)
        };
        studio.viewing_size = Some(size);
        let reply = json!({"session":studio.editor_session,"type":"viewport","width":size.0,"height":size.1});
        self.web_host
            .update(cx, |host, _| host.studio_reply(&reply));
        self.refresh_studio_bootstrap(cx);
        cx.notify();
    }

    fn studio_export_png(&mut self, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        let Some(screen) = studio.editing_screen() else {
            return;
        };
        if studio.exporting
            || studio.export_flush.is_some()
            || studio.implementation_flush.is_some()
        {
            return;
        }
        let request = Uuid::new_v4();
        studio.export_flush = Some((request, screen));
        let reply = json!({"session":studio.editor_session,"type":"flush","request_id":request});
        self.web_host
            .update(cx, |host, _| host.studio_reply(&reply));
        cx.notify();
    }

    fn studio_finish_export(&mut self, screen: Uuid, cx: &mut Context<Self>) {
        let Some(studio) = self.studio.as_mut() else {
            return;
        };
        if studio.dirty || studio.saving {
            return;
        }
        let mut design = studio.design.clone();
        let Some(page) = design.manifest.screens.iter_mut().find(|s| s.id == screen) else {
            return;
        };
        if let Some((width, height)) = studio.viewing_size {
            page.width = width;
            page.height = height;
        }
        let name: String = page
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                    c
                } else {
                    '_'
                }
            })
            .take(100)
            .collect();
        let name = format!(
            "{}.png",
            if name.trim().is_empty() {
                "Screen"
            } else {
                name.trim()
            }
        );
        let store = studio.store.clone();
        let design_id = design.manifest.id;
        let destination = cx.prompt_for_new_path(&store.project, Some(&name));
        studio.exporting = true;
        studio.notice = Some("Exporting screen…".into());
        cx.spawn(async move |this, cx| {
            let result = match destination.await {
                Ok(Ok(Some(path))) => {
                    cx.background_executor()
                        .spawn(async move {
                            let bytes = super::studio_editor::export_png(store, design, screen)?;
                            atomic(&path, &bytes)?;
                            Ok::<_, anyhow::Error>(Some(path))
                        })
                        .await
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error),
                Err(error) => Err(anyhow::anyhow!("Save dialog closed: {error}")),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(studio) = this
                    .studio
                    .as_mut()
                    .filter(|s| s.design.manifest.id == design_id)
                {
                    studio.exporting = false;
                    studio.notice = None;
                    match result {
                        Ok(Some(path)) => {
                            studio.notice = Some(format!("Exported {}", path.display()))
                        }
                        Ok(None) => {}
                        Err(error) => {
                            studio.error = Some(format!("Could not export screen: {error:#}"))
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn studio_implement(
        &mut self,
        subset: Option<Vec<Uuid>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .studio
            .as_ref()
            .is_some_and(|s| s.export_flush.is_some())
        {
            return;
        }
        if let Some(studio) = self.studio.as_mut().filter(|s| s.editing_screen().is_some()) {
            studio.pending_implementation = Some(subset);
            studio.implementation_flush = Some(Uuid::new_v4());
            let reply = json!({"session":studio.editor_session,"type":"flush","request_id":studio.implementation_flush});
            self.web_host
                .update(cx, |host, _| host.studio_reply(&reply));
            return;
        }
        self.studio_finish_implement(subset, window, cx);
    }
    fn studio_finish_implement(
        &mut self,
        subset: Option<Vec<Uuid>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        if studio.dirty || studio.saving {
            return;
        }
        let project = studio.project;
        let result = studio.store.handoff(studio.design.manifest.id, subset);
        match result {
            Ok(handoff) => {
                self.open_new_agent_composer_for_project(project, window, cx);
                if let Some(composer) = self.new_agent_composer.as_mut() {
                    let prompt=format!("Implement Studio design ‘{}’. First call studio_handoff_read with handoff_id ‘{}’. Read its manifest, tokens, assets, and each selected screen before changing code. The snapshot is immutable, revision {}. {}",handoff.design.manifest.name,handoff.id,handoff.design.manifest.revision,handoff.instruction);
                    composer
                        .prompt
                        .update(cx, |input, cx| input.set_value(prompt, window, cx));
                    if let Some(source) = handoff.design.manifest.source_doc.as_ref() {
                        let path = PathBuf::from(source);
                        composer.source_doc = Some(path.clone());
                        composer.linked_docs.push(path);
                    }
                    composer.implementation_target = Some(ImplementationTarget::Studio(handoff.id));
                    composer.design_browser_open_confirmed = true;
                }
            }
            Err(error) => {
                if let Some(s) = self.studio.as_mut() {
                    s.error = Some(format!("Could not prepare implementation: {error:#}"));
                }
            }
        }
        cx.notify();
    }
    fn studio_section(
        &self,
        section: &str,
        count: usize,
        cx: &mut Context<Self>,
    ) -> gpui_component::button::Button {
        let name = section.to_string();
        let open = self
            .studio
            .as_ref()
            .is_some_and(|s| !s.folded_sections.contains(section));
        style::design_sidebar_section(
            SharedString::from(format!("studio-section-{section}")),
            section.to_string(),
            open,
            cx,
        )
        .child(div().flex_1())
        .child(
            div()
                .text_color(crate::ui::design::t3(cx))
                .child(count.to_string()),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(s) = this.studio.as_mut() {
                if !s.folded_sections.remove(&name) {
                    s.folded_sections.insert(name.clone());
                }
                if s.design.manifest.system_workspace {
                    let section = match name.as_str() {
                        "Colors" => "colors",
                        "Typography" => "typography",
                        "Recipes" => "recipes",
                        _ => "foundations",
                    };
                    let reply = json!({"session":s.editor_session,"type":"system-section","section":section});
                    this.web_host.update(cx, |host, _| host.studio_reply(&reply));
                }
            }
            cx.notify();
        }))
    }

    fn render_studio_system(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let studio = self.studio.as_ref().unwrap();
        if !studio.design.manifest.system_workspace {
            return self.render_system_binding(cx);
        }
        let tokens = studio.design.tokens();
        let dirty = studio.dirty || studio.saving;
        let mut panel = v_flex().py_2().min_w(px(0.)).child(
            h_flex()
                .items_center()
                .px_3()
                .pb_2()
                .gap_2()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child("Design system"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Draft foundations · select a token to edit"),
                        ),
                )
                .child(
                    style::header_icon_button("studio-add-token", IconName::Plus, cx)
                        .tooltip("Add design token")
                        .disabled(dirty)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.studio_name_dialog(
                                "New design token · name: value",
                                "color-accent: #8057d9",
                                NameAction::NewToken,
                                window,
                                cx,
                            );
                        })),
                ),
        );
        for group in [
            "Colors",
            "Typography",
            "Spacing",
            "Corners",
            "Shadows",
            "Other",
        ] {
            let entries = tokens
                .iter()
                .filter(|(name, _)| studio_token_group(name) == group)
                .collect::<Vec<_>>();
            if entries.is_empty() {
                continue;
            }
            panel = panel.child(
                div()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.6))
                    .child(self.studio_section(group, entries.len(), cx)),
            );
            if studio.folded_sections.contains(group) {
                continue;
            }
            let mut rows = v_flex().px_2().pb_2().gap(px(2.));
            for (name, value) in entries {
                rows = rows.child(self.studio_token_row(name.clone(), value.clone(), group, cx));
            }
            panel = panel.child(rows);
        }
        panel = panel.child(
            div()
                .border_t_1()
                .border_color(crate::ui::design::line(cx).opacity(0.6))
                .child(self.studio_section("Recipes", studio.design.system.recipes.len(), cx)),
        );
        if !studio.folded_sections.contains("Recipes") {
            for (name, properties) in &studio.design.system.recipes {
                let key = format!("recipe:{name}");
                let open = studio.folded_sections.contains(&key);
                let action_key = key.clone();
                panel = panel.child(
                    style::design_sidebar_section(
                        SharedString::from(key.clone()),
                        studio_token_label(name),
                        open,
                        cx,
                    )
                    .tooltip(format!("Reusable ds-{name} style"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = this.studio.as_mut() {
                            if !s.folded_sections.remove(&action_key) {
                                s.folded_sections.insert(action_key.clone());
                            }
                        }
                        cx.notify();
                    })),
                );
                if open {
                    let mut details = v_flex().px_3().pb_3().gap_2();
                    for (property, value) in properties {
                        details = details.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(studio_token_label(property)),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(value.clone()),
                                ),
                        );
                    }
                    panel = panel.child(details);
                }
            }
        }
        let screens = studio
            .design
            .manifest
            .screens
            .iter()
            .filter(|s| !s.archived)
            .collect::<Vec<_>>();
        panel = panel.child(
            div()
                .border_t_1()
                .border_color(crate::ui::design::line(cx).opacity(0.6))
                .child(self.studio_section("Screen styles", screens.len(), cx)),
        );
        if !studio.folded_sections.contains("Screen styles") {
            for screen in screens {
                let id = screen.id;
                panel = panel.child(
                    style::design_sidebar_row(
                        SharedString::from(format!("studio-screen-styles-{id}")),
                        false,
                        cx,
                    )
                    .icon(IconName::File)
                    .label(screen.name.clone())
                    .tooltip("Open this screen’s local styles")
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.studio_select_screen(Some(id), cx)),
                    ),
                );
            }
        }
        panel.into_any_element()
    }

    fn studio_token_row(
        &self,
        name: String,
        value: String,
        group: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let studio = self.studio.as_ref().unwrap();
        let dirty = studio.dirty || studio.saving;
        let local_override = studio.design.overrides.tokens.contains_key(&name);
        let center = cx.entity();
        let menu_host = self.web_host.clone();
        let label = name.clone();
        let token = name.clone();
        let swatch = gpui::Hsla::parse_hex(&value).ok();
        let preview = div()
            .size(px(28.))
            .flex_none()
            .rounded(crate::ui::design::r_xs())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .flex()
            .items_center()
            .justify_center()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .bg(if group == "Colors" {
                swatch.unwrap_or(crate::ui::design::surface_2(cx))
            } else {
                crate::ui::design::surface_2(cx)
            })
            .when(group != "Colors" || swatch.is_none(), |sample| {
                sample.child(match group {
                    "Typography" => "Aa",
                    "Spacing" => "↔",
                    "Corners" => "⌜",
                    "Shadows" => "◧",
                    _ => "–",
                })
            });
        style::design_property_row(SharedString::from(format!("studio-token-{name}")), cx)
            .disabled(dirty)
            .tooltip(format!(
                "{name}: {value}\n{}",
                if local_override {
                    "Design override"
                } else {
                    "Inherited from project"
                }
            ))
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.))
                    .items_center()
                    .gap_2()
                    .child(preview)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .items_start()
                            .gap(px(2.))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(crate::ui::design::text_ui())
                                    .child(studio_token_label(&name)),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(value.clone()),
                            ),
                    )
                    .when(local_override, |row| {
                        row.child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::accent(cx))
                                .child("Local"),
                        )
                    })
                    .child(
                        gpui_component::Icon::new(IconName::ChevronDown)
                            .size(px(12.))
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
            .dropdown_menu(move |menu, window, cx| {
                web_preview::suspend_for_menu(menu_host.clone(), cx);
                let token = token.clone();
                let value = value.clone();
                let title = label.clone();
                menu.item(
                    PopupMenuItem::new("Edit draft value").on_click(window.listener_for(
                        &center,
                        move |this: &mut Self, _, window, cx| {
                            this.studio_name_dialog(
                                &title,
                                &value,
                                NameAction::Token(token.clone(), false),
                                window,
                                cx,
                            )
                        },
                    )),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_studio(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(studio) = self.studio.as_ref() else {
            return div().into_any_element();
        };
        let system_workspace = studio.design.manifest.system_workspace;
        let tab = studio.tab;
        let sidebar_collapsed = studio.sidebar_collapsed;
        let sidebar_scroll = studio.sidebar_scroll[tab as usize].clone();
        let selected = studio.screen;
        let selected_artboard = studio.editing_screen().or(studio.canvas.layout.selected_screen_id);
        let title = studio.design.manifest.name.clone();
        let design_id = studio.design.manifest.id;
        let source_doc = studio.design.manifest.source_doc.clone();
        let source_task = studio.design.manifest.source_task.clone();
        let project_root = studio.store.project.clone();
        let implementor_ids = studio.implementation_agents.clone();
        let viewport =
            selected.and_then(|id| screens_for_viewport(&studio.design, id, studio.viewing_size));
        let exporting = studio.exporting || studio.export_flush.is_some();
        let export_disabled = exporting || studio.implementation_flush.is_some();
        let dirty = studio.dirty || studio.saving;
        let redo = studio.redo;
        let error = studio.error.clone();
        let notice = studio.notice.clone();
        let screens = studio.design.manifest.screens.clone();
        let chat = studio.relative_chat.clone();
        let tabs = style::sidebar_mode_tabs(cx)
            .font_family(crate::theme::UI_FONT_FAMILY)
            .children(
                [
                    (StudioTab::Agent, "Agent"),
                    (StudioTab::Screens, "Screens"),
                    (
                        StudioTab::System,
                        if system_workspace {
                            "Library"
                        } else {
                            "Design system"
                        },
                    ),
                ]
                .into_iter()
                .filter(|(tab, _)| !system_workspace || *tab != StudioTab::Screens)
                .enumerate()
                .map(|(i, (tab_id, label))| {
                    style::sidebar_named_tab(("studio-tab", i), label, tab == tab_id, cx).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if let Some(s) = this.studio.as_mut() {
                                s.tab = tab_id;
                            }
                            cx.notify();
                        }),
                    )
                }),
            );
        let tabs = h_flex().w_full().gap_1().items_center().child(tabs).child(
            style::sidebar_mode_icon_tab("studio-collapse-sidebar", IconName::PanelLeftClose, cx)
                .flex_none()
                .tooltip("Collapse design sidebar")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(studio) = this.studio.as_mut() {
                        studio.sidebar_collapsed = true;
                    }
                    this.composer_model_expanded = false;
                    cx.notify();
                })),
        );
        let panel = if sidebar_collapsed {
            div().into_any_element()
        } else {
            match tab {
                StudioTab::Screens => {
                    let mut panel = v_flex()
                        .py_2()
                        .child(
                            style::design_sidebar_row("studio-all", selected.is_none(), cx)
                                .icon(IconName::LayoutDashboard)
                                .label("All screens")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.studio_select_screen(None, cx)
                                })),
                        )
                        .child(h_flex().px_1().pt_1().child({
                            let center = cx.entity();
                            let host = self.web_host.clone();
                            style::ghost_button_compact("studio-screen-actions", "Screen actions")
                                .text_color(crate::ui::design::t2(cx))
                                .dropdown_caret(true)
                                .dropdown_menu(move |menu, window, cx| {
                                    web_preview::suspend_for_menu(host.clone(), cx);
                                    menu.item(PopupMenuItem::new("Refresh previews").on_click(
                                        window.listener_for(
                                            &center,
                                            |this: &mut Self, _, _, cx| {
                                                if let Some(studio) = this.studio.as_mut() {
                                                    studio.thumbnail_revision = None;
                                                }
                                                if this.studio_canvas_active(){this.studio_canvas_command("refresh",cx);}else{this.queue_studio_thumbnails(cx);}
                                            },
                                        ),
                                    ))
                                    .item(
                                        PopupMenuItem::new("Undo saved edit")
                                            .disabled(dirty)
                                            .on_click(window.listener_for(
                                                &center,
                                                |this: &mut Self, _, _, cx| {
                                                    this.studio_undo(false, cx)
                                                },
                                            )),
                                    )
                                    .item(
                                        PopupMenuItem::new("Redo saved edit")
                                            .disabled(dirty || !redo)
                                            .on_click(window.listener_for(
                                                &center,
                                                |this: &mut Self, _, _, cx| {
                                                    this.studio_undo(true, cx)
                                                },
                                            )),
                                    )
                                    .item(
                                        PopupMenuItem::new("Implement selected screens…").on_click(
                                            window.listener_for(
                                                &center,
                                                |this: &mut Self, _, window, cx| {
                                                    this.studio_choose_implementation(window, cx)
                                                },
                                            ),
                                        ),
                                    )
                                })
                        }))
                        .child(div().h(px(1.)).my_2().bg(crate::ui::design::line(cx)));
                    for archived in [false, true] {
                        let count = screens
                            .iter()
                            .filter(|screen| screen.archived == archived)
                            .count();
                        if archived && count == 0 {
                            continue;
                        }
                        let section = if archived { "Archived" } else { "Screens" };
                        let open = !self
                            .studio
                            .as_ref()
                            .unwrap()
                            .folded_sections
                            .contains(section);
                        panel = panel.child(
                            h_flex()
                                .items_center()
                                .pr_2()
                                .child(self.studio_section(section, count, cx).flex_1())
                                .when(!archived, |row| {
                                    row.child(
                                        style::header_icon_button(
                                            "studio-add-screen",
                                            IconName::Plus,
                                            cx,
                                        )
                                        .tooltip("Add screen")
                                        .disabled(dirty)
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.studio_name_dialog(
                                                    "New screen",
                                                    "New screen",
                                                    NameAction::Screen(None),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                                }),
                        );
                        if !open {
                            continue;
                        }
                        for (index, screen) in screens
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| s.archived == archived)
                        {
                            let id = screen.id;
                            let name = screen.name.clone();
                            let row = style::design_sidebar_row(
                                ("studio-screen", index),
                                selected_artboard == Some(id),
                                cx,
                            )
                            .icon(IconName::File)
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .child(screen.name.clone()),
                            )
                            .tooltip(format!(
                                "{} · {} × {}",
                                screen.name, screen.width, screen.height
                            ))
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.studio_select_screen(Some(id), cx),
                            ));
                            let center = cx.entity();
                            let menu_host = self.web_host.clone();
                            panel =
                                panel.child(
                                    h_flex()
                                        .w_full()
                                        .min_w(px(0.))
                                        .pr_1()
                                        .bg(if selected_artboard == Some(id) {
                                            crate::ui::design::surface_2(cx)
                                        } else {
                                            gpui::transparent_black()
                                        })
                                        .child(row)
                                        .child(
                                            style::header_icon_button(
                                                ("studio-screen-menu", index),
                                                IconName::Ellipsis,
                                                cx,
                                            )
                                            .disabled(dirty)
                                            .dropdown_menu(move |mut menu, window, cx| {
                                                web_preview::suspend_for_menu(
                                                    menu_host.clone(),
                                                    cx,
                                                );
                                                let name = name.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new("Rename")
                                                        .on_click(window.listener_for(
                                                        &center,
                                                        move |this: &mut Self, _, window, cx| {
                                                            this.studio_name_dialog(
                                                                "Rename screen",
                                                                &name,
                                                                NameAction::Screen(Some(id)),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                                );
                                                for (label, action) in [
                                                    ("Duplicate", "duplicate"),
                                                    ("Move up", "up"),
                                                    ("Move down", "down"),
                                                    ("Archive / restore", "archive"),
                                                    ("Mobile viewport", "mobile"),
                                                    ("Desktop viewport", "desktop"),
                                                ] {
                                                    menu = menu.item(
                                                        PopupMenuItem::new(label).on_click(
                                                            window.listener_for(
                                                                &center,
                                                                move |this: &mut Self, _, _, cx| {
                                                                    this.studio_screen_menu_action(
                                                                        id, action, cx,
                                                                    )
                                                                },
                                                            ),
                                                        ),
                                                    );
                                                }
                                                menu
                                            }),
                                        ),
                                );
                        }
                    }
                    panel.into_any_element()
                }
                StudioTab::System => self.render_studio_system(cx),
                StudioTab::Agent => {
                    let root = self.studio.as_ref().unwrap().store.project.clone();
                    let record = self.doc_assistants.update(cx, |assistants, cx| {
                        assistants.ensure_record(project, chat.clone(), cx)
                    });
                    self.hydrate_doc_assistant_chat_session(&record, &root, cx);
                    let agent = Self::doc_assistant_agent_record(&record, root);
                    let history = self.studio.as_ref().unwrap().conversations.clone();
                    let can_build_system =
                        system_workspace && !history.entries.iter().any(|entry| entry.started);
                    let title = history
                        .entries
                        .iter()
                        .find(|entry| entry.id == history.selected)
                        .map(|entry| entry.title.clone())
                        .unwrap_or_else(|| "Design conversation".into());
                    let busy = self
                        .agent_chats
                        .read(cx)
                        .session(record.chat_agent_id)
                        .is_some_and(|session| {
                            matches!(
                                session.status,
                                AgentChatStatus::Running | AgentChatStatus::Cancelling
                            )
                        });
                    let center = cx.entity();
                    let menu_host = self.web_host.clone();
                    let history_button =
                        style::design_conversation_picker("studio-agent-history", &title)
                            .disabled(busy)
                            .dropdown_menu(move |mut menu, window, cx| {
                                web_preview::suspend_for_menu(menu_host.clone(), cx);
                                menu = menu.min_w(px(190.)).max_w(px(280.));
                                for entry in history.entries.iter().rev() {
                                    let conversation = entry.id;
                                    let design_id = history.design_id;
                                    menu = menu.item(
                                        PopupMenuItem::new(entry.title.clone())
                                            .checked(conversation == history.selected)
                                            .on_click(window.listener_for(
                                                &center,
                                                move |this: &mut Self, _, _, cx| {
                                                    this.select_studio_conversation(
                                                        design_id,
                                                        Some(conversation),
                                                        cx,
                                                    )
                                                },
                                            )),
                                    );
                                }
                                menu
                            });
                    let design_id = self.studio.as_ref().unwrap().design.manifest.id;
                    let new_agent =
                        style::header_icon_button("studio-new-agent", IconName::Plus, cx)
                            .disabled(busy)
                            .tooltip(if busy {
                                "Wait for this agent to finish"
                            } else {
                                "New agent"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_studio_conversation(design_id, None, cx)
                            }));
                    let scope_label = if system_workspace {
                        "Design-system draft"
                    } else if let Some(screen) = selected {
                        screens
                            .iter()
                            .find(|s| s.id == screen)
                            .map(|s| s.name.as_str())
                            .unwrap_or("Select screen")
                    } else {
                        "All screens"
                    };
                    v_flex()
                        .size_full()
                        .min_h(px(0.))
                        .min_w(px(0.))
                        .child(
                            h_flex()
                                .flex_none()
                                .w_full()
                                .px_2()
                                .py_2()
                                .gap_2()
                                .border_b_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.42))
                                .bg(crate::ui::design::nav(cx))
                                .items_center()
                                .child(div().flex_1().min_w(px(0.)).child(history_button))
                                .child(div().flex_none().child(new_agent)),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child(scope_label.to_string()),
                        )
                        .when(can_build_system, |panel| {
                            panel.child(
                                div().px_2().py_2().child(
                                    style::primary_button_compact(
                                        "system-build-agent",
                                        "Build with agent",
                                        cx,
                                    )
                                    .disabled(busy)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.start_system_agent(window, cx),
                                    )),
                                ),
                            )
                        })
                        .child(div().flex_1().min_h(px(0.)).child(
                            self.render_agent_chat_body_for_surface(
                                &agent,
                                AgentChatSurface::Document {
                                    project,
                                    relative_doc_path: chat,
                                },
                                window,
                                cx,
                            ),
                        ))
                        .into_any_element()
                }
            }
        };
        let canvas_overview=self.studio_canvas_active();
        let body = if selected.is_some() || canvas_overview {
            let host = self.web_host.clone();
            div()
                .relative()
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            host.update(cx, |host, _| host.place(bounds, window))
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element()
        } else {
            let studio = self.studio.as_ref().unwrap();
            let store = studio.store.clone();
            let design = studio.design.clone();
            let center = cx.entity();
            let screens = screens
                .into_iter()
                .filter(|s| !s.archived)
                .collect::<Vec<_>>();
            let columns = (((window.viewport_size().width.as_f32()
                - if sidebar_collapsed { 520. } else { 800. })
                / 280.)
                .floor() as usize)
                .clamp(1, 4);
            let rows = screens.len().div_ceil(columns);
            gpui::uniform_list("studio-overview", rows, move |range, window, cx| {
                super::studio_editor::prioritize(
                    design.manifest.id,
                    screens
                        .iter()
                        .skip(range.start * columns)
                        .take(range.len() * columns)
                        .map(|s| s.id)
                        .collect(),
                );
                range
                    .map(|row| {
                        let mut line = h_flex().h(px(225.)).gap_4().px_4().py_3().items_start();
                        for index in row * columns..((row + 1) * columns).min(screens.len()) {
                            let screen = &screens[index];
                            let id = screen.id;
                            let path = store.thumbnail_path(&design, id);
                            let fresh = path.exists();
                            let path = if fresh {
                                Some(path)
                            } else {
                                store.last_thumbnail(id)
                            };
                            let preview = if let Some(path) = path {
                                img(path)
                                    .w_full()
                                    .h(px(175.))
                                    .object_fit(ObjectFit::Contain)
                                    .into_any_element()
                            } else {
                                div()
                                    .w_full()
                                    .h(px(175.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Rendering screen…")
                                    .into_any_element()
                            };
                            line = line.child(
                                v_flex()
                                    .id(("studio-card", index))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_2()
                                    .cursor_pointer()
                                    .group("studio-card")
                                    .child(
                                        div()
                                            .bg(crate::ui::design::surface(cx))
                                            .border_1()
                                            .border_color(crate::ui::design::line(cx))
                                            .group_hover("studio-card", |tile| {
                                                tile.border_color(crate::ui::design::line_2(cx))
                                            })
                                            .rounded(crate::ui::design::r_sm())
                                            .overflow_hidden()
                                            .child(preview),
                                    )
                                    .child(
                                        div()
                                            .px_0p5()
                                            .truncate()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t2(cx))
                                            .group_hover("studio-card", |name| {
                                                name.text_color(crate::ui::design::t1(cx))
                                            })
                                            .child(if fresh {
                                                screen.name.clone()
                                            } else {
                                                format!("{} · Updating", screen.name)
                                            }),
                                    )
                                    .on_click(window.listener_for(
                                        &center,
                                        move |this: &mut Self, _, _, cx| {
                                            this.studio_select_screen(Some(id), cx)
                                        },
                                    )),
                            );
                        }
                        line
                    })
                    .collect::<Vec<_>>()
            })
            .track_scroll(studio.overview_scroll.clone())
            .flex_1()
            .h_full()
            .into_any_element()
        };
        // A stage bar carries transient notices beside the action that caused them.
        // Workspaces without a bar (named systems) keep the row under the header.
        let has_stage_bar =
            !system_workspace && (selected.is_none() || viewport.is_some());
        let bar_notice = notice.clone().filter(|_| has_stage_bar);
        let notice = notice.filter(|_| !has_stage_bar);
        let body = if selected.is_none() && !system_workspace {
            let zoom=self.studio.as_ref().map(|s|s.canvas.layout.viewport.zoom).unwrap_or(1.);
            let toolbar = style::stage_bar(cx).h_auto().min_h(px(44.)).flex_wrap().py_1()
                .child(style::ghost_button_compact("studio-prototype", "Prototype")
                    .disabled(self.studio.as_ref().is_none_or(|s|!s.design.manifest.screens.iter().any(|p|!p.archived)))
                    .tooltip("Play the selected screen and follow its screen links")
                    .on_click(cx.listener(|this,_,_,cx|this.studio_start_prototype(cx))))
                .when(self.studio.as_ref().is_some_and(|s|s.inline_screen.is_some()), |row|row.child(
                    style::ghost_button_compact("studio-inline-done", "Done editing")
                        .on_click(cx.listener(|this,_,_,cx|this.studio_inline_select(None,cx)))))
                .child(
                    style::stage_bar_choices(cx)
                        .child(
                            style::stage_bar_choice(
                                "studio-canvas-mode",
                                "Canvas",
                                canvas_overview,
                                "Arrange screens on a free canvas",
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.studio_overview_mode(StudioOverviewMode::Canvas, cx)
                            })),
                        )
                        .child(
                            style::stage_bar_choice(
                                "studio-grid-mode",
                                "Grid",
                                !canvas_overview,
                                "Show screens in a grid",
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.studio_overview_mode(StudioOverviewMode::Grid, cx)
                            })),
                        ),
                )
                .child(style::stage_bar_notice(bar_notice.clone(), cx))
                .when(canvas_overview, |row| {
                    row.child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .child(
                                style::header_icon_button(
                                    "studio-canvas-zoom-out",
                                    IconName::Minus,
                                    cx,
                                )
                                .tooltip("Zoom out")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.studio_canvas_command("zoom-out", cx)
                                })),
                            )
                            .child(
                                style::stage_bar_readout(format!("{:.0}%", zoom * 100.), cx)
                                    .w(px(44.))
                                    .flex()
                                    .justify_center(),
                            )
                            .child(
                                style::header_icon_button(
                                    "studio-canvas-zoom-in",
                                    IconName::Plus,
                                    cx,
                                )
                                .tooltip("Zoom in")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.studio_canvas_command("zoom-in", cx)
                                })),
                            ),
                    )
                    .child(style::stage_bar_rule(cx))
                    .child(
                        style::ghost_button_compact("studio-canvas-fit", "Fit all").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.studio_canvas_command("fit-all", cx)
                            }),
                        ),
                    )
                    .child(
                        style::ghost_button_compact("studio-canvas-fit-selected", "Fit selected")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.studio_canvas_command("fit-selected", cx)
                            })),
                    )
                    .child(
                        style::ghost_button_compact("studio-canvas-arrange", "Arrange").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.studio_canvas_command("arrange", cx)
                            }),
                        ),
                    )
                });
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .bg(crate::ui::design::stage(cx))
                .child(body)
                .child(toolbar)
                .into_any_element()
        } else if let Some((width, height)) = viewport.filter(|_| !system_workspace) {
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .child(body)
                .child(
                    style::stage_bar(cx)
                        .h_auto()
                        .min_h(px(44.))
                        .flex_wrap()
                        .py_1()
                        .child(
                            style::ghost_button_compact("studio-editor-all-screens", "All screens")
                                .tooltip("Return to all screens")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    // Flush even a clean editor: this commits its camera and
                                    // finishes any active inline edit before navigation.
                                    if let Some(studio) = this.studio.as_mut() {
                                        studio.pending_screen = Some(None);
                                        let reply = json!({"session":studio.editor_session,"type":"flush"});
                                        this.web_host.update(cx, |host, _| host.studio_reply(&reply));
                                    }
                                })),
                        )
                        .when(self.studio.as_ref().is_some_and(|s|s.prototype), |row|row.child(
                            style::ghost_button_compact("studio-prototype-back", "Back")
                                .disabled(self.studio.as_ref().is_none_or(|s|s.prototype_history.is_empty()))
                                .on_click(cx.listener(|this,_,_,cx|this.studio_prototype_back(cx)))))
                        .child(
                            style::stage_bar_choices(cx)
                                .child(
                                    style::stage_bar_choice(
                                        "studio-view-desktop",
                                        "Desktop",
                                        width >= 600,
                                        "View this screen at desktop width",
                                        cx,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.studio_set_viewing_size(false, cx)
                                    })),
                                )
                                .child(
                                    style::stage_bar_choice(
                                        "studio-view-mobile",
                                        "Mobile",
                                        width < 600,
                                        "View this screen at mobile width; layout follows its CSS",
                                        cx,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.studio_set_viewing_size(true, cx)
                                    })),
                                ),
                        )
                        .child(
                            style::stage_bar_readout(format!("{width} × {height}"), cx).pl_1(),
                        )
                        .child(style::stage_bar_notice(bar_notice.clone(), cx))
                        .child(
                            style::ghost_button_compact(
                                "studio-export-png",
                                if exporting {
                                    "Exporting…"
                                } else {
                                    "Export PNG"
                                },
                            )
                            .disabled(export_disabled)
                            .tooltip("Save this screen as a PNG at the displayed size")
                            .on_click(cx.listener(|this, _, _, cx| this.studio_export_png(cx))),
                        ),
                )
                .into_any_element()
        } else {
            body
        };
        let active_implementor = self
            .agents
            .read(cx)
            .records_for_project(project)
            .into_iter()
            .filter(|agent| {
                implementor_ids.contains(&agent.id)
                    && !agent.hidden_doc_assistant
                    && (agent.started_at.is_some()
                        || agent.cli_session_id.is_some()
                        || agent.chat_session_id.is_some())
            })
            .max_by_key(|agent| {
                (
                    agent.created_at,
                    agent.started_at,
                    agent.updated_at,
                    agent.id,
                )
            });
        let mut indicators = Vec::new();
        if let Some(relative) = source_doc {
            let relative = PathBuf::from(relative);
            let absolute = project_root.join(&relative);
            indicators.push(
                crate::ui::design::indicator::subline_link(
                    ("studio-source-doc", design_id.as_u128() as u64),
                    crate::ui::design::docs_icon(),
                    SharedString::from(short_doc_chip_label(&relative)),
                    crate::ui::design::sky(cx),
                    cx,
                )
                .tooltip(|window, cx| Tooltip::new("Open source document").build(window, cx))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.open_doc(project, absolute.clone(), cx)),
                )
                .into_any_element(),
            );
        }
        if indicators.is_empty() {
            if let Some(reference) = source_task {
                let task = self.tasks.read(cx).board(project).and_then(|board| {
                    board
                        .issues
                        .into_iter()
                        .map(|issue| issue.reference)
                        .find(|task| reference == format!("{} {}", task.issue_key, task.issue_url))
                });
                if let Some(task) = task {
                    indicators.push(
                        crate::ui::design::indicator::subline_link(
                            ("studio-source-task", design_id.as_u128() as u64),
                            crate::ui::design::tasks_icon(),
                            SharedString::from(task.issue_key.clone()),
                            crate::ui::design::accent(cx),
                            cx,
                        )
                        .tooltip(|window, cx| Tooltip::new("Open source task").build(window, cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_task(project, task.clone(), cx)
                        }))
                        .into_any_element(),
                    );
                } else {
                    indicators.push(
                        crate::ui::design::indicator::subline_indicator(
                            crate::ui::design::tasks_icon(),
                            SharedString::from(reference),
                            crate::ui::design::accent(cx),
                            cx,
                        )
                        .into_any_element(),
                    );
                }
            }
        }
        if let Some(agent) = active_implementor.as_ref() {
            indicators.push(self.penpot_design_agent_indicator(agent, design_id, cx));
        }
        if let Some(pr) = self.penpot_design_pull_request(active_implementor.as_ref(), cx) {
            indicators.push(self.render_agent_ship_pr_indicator(&pr, cx));
        }
        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::workspace_bar(cx)
                    .child(
                        style::header_icon_button("studio-back", IconName::ArrowLeft, cx)
                            .tooltip("Back to project designs")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if system_workspace {
                                    this.open_system_library(project, cx);
                                } else {
                                    this.show_penpot_hub(cx);
                                }
                            })),
                    )
                    .child(
                        crate::ui::design::header::title_col(cx)
                            .child(crate::ui::design::header::title(title, cx))
                            .child(crate::ui::design::header::subline().children(indicators)),
                    )
                    .child(if system_workspace {
                        self.system_workspace_actions(window, cx)
                    } else {
                        crate::ui::design::header::actions()
                            .child(
                                style::context_panel_action_button(
                                    "studio-compare",
                                    IconName::Replace,
                                    if self.penpot_compare_open {
                                        "Close Compare"
                                    } else {
                                        "Compare"
                                    },
                                    cx,
                                )
                                .tooltip("Compare this design with the live implementation")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.set_penpot_compare_open(
                                            project,
                                            !this.penpot_compare_open,
                                            cx,
                                        );
                                    },
                                )),
                            )
                            .child(
                                style::implement_button(
                                    "studio-implement",
                                    if active_implementor.is_some() {
                                        "Reimplement"
                                    } else {
                                        "Implement"
                                    },
                                    cx,
                                )
                                .disabled(exporting)
                                .tooltip("Implement this design")
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.studio_implement(None, window, cx),
                                )),
                            )
                            .into_any_element()
                    }),
            )
            .when_some(notice, |view, notice| {
                view.child(
                    div()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(crate::ui::design::line(cx))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t2(cx))
                        .child(notice),
                )
            })
            .when_some(error, |view, error| {
                view.child(
                    div()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(crate::ui::design::line(cx))
                        .bg(crate::ui::design::rose_soft(cx))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(if sidebar_collapsed {
                        v_flex()
                            .w(px(38.))
                            .h_full()
                            .flex_none()
                            .items_center()
                            .pt_1()
                            .bg(crate::ui::design::base(cx))
                            .border_r_1()
                            .border_color(crate::ui::design::line(cx))
                            .child(
                                style::sidebar_mode_icon_tab(
                                    "studio-expand-sidebar",
                                    IconName::PanelLeft,
                                    cx,
                                )
                                .tooltip("Expand design sidebar")
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if let Some(s) = this.studio.as_mut() {
                                            s.sidebar_collapsed = false;
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .w(px(318.))
                            .h_full()
                            .min_h(px(0.))
                            .min_w(px(0.))
                            .flex_none()
                            .bg(crate::ui::design::base(cx))
                            .on_mouse_down(MouseButton::Left, |_, window, _| {
                                web_preview::restore_focus(window);
                            })
                            .border_r_1()
                            .border_color(crate::ui::design::line(cx))
                            .child(style::design_sidebar_tabs_header(tabs, cx).pb_2())
                            .child(
                                div()
                                    .id("studio-sidebar-content")
                                    .track_scroll(&sidebar_scroll)
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .min_w(px(0.))
                                    .when(tab != StudioTab::Agent, |panel| {
                                        panel.overflow_y_scroll()
                                    })
                                    .when(tab == StudioTab::Agent, |panel| panel.overflow_hidden())
                                    .child(panel),
                            )
                            .into_any_element()
                    })
                    .child(body),
            )
            .into_any_element()
    }
}
fn conversation_path_for(design: Uuid, conversation: Uuid, system: bool) -> PathBuf {
    if system {
        PathBuf::from(format!(
            ".choro/assistants/studio-systems/{design}/{conversation}"
        ))
    } else {
        ide_core::studio::conversation_path(design, conversation)
    }
}

fn studio_token_group(name: &str) -> &'static str {
    if name.starts_with("color-") {
        "Colors"
    } else if name.starts_with("font-")
        || name.starts_with("line-height")
        || name.starts_with("letter-spacing")
    {
        "Typography"
    } else if name.starts_with("space-") || name.starts_with("spacing-") {
        "Spacing"
    } else if name.starts_with("radius-") {
        "Corners"
    } else if name.starts_with("shadow-") {
        "Shadows"
    } else {
        "Other"
    }
}
fn studio_token_label(name: &str) -> String {
    let name = name
        .strip_prefix("color-")
        .or_else(|| name.strip_prefix("space-"))
        .or_else(|| name.strip_prefix("radius-"))
        .or_else(|| name.strip_prefix("shadow-"))
        .unwrap_or(name);
    let label = name.replace('-', " ");
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => label,
    }
}

fn screens_for_viewport(
    design: &StudioDesign,
    screen: Uuid,
    override_size: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    design
        .manifest
        .screens
        .iter()
        .find(|s| s.id == screen)
        .map(|s| override_size.unwrap_or((s.width, s.height)))
}
