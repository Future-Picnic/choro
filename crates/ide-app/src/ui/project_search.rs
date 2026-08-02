use std::path::{Path, PathBuf};

use gpui::{
    actions, div, prelude::FluentBuilder, px, AnyElement, App, AppContext, Context, Entity,
    FontWeight, InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, Icon, IconName, WindowExt,
};
use ide_core::{AgentRecord, ProjectId, TaskRef};
use uuid::Uuid;

use crate::state::{AgentRecords, DocsState, TasksState, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::{design, palette_ui};

actions!(
    project_search,
    [
        ProjectSearchNext,
        ProjectSearchPrevious,
        ProjectSearchConfirm,
        ProjectSearchCycleScope
    ]
);

const CONTEXT: &str = "ProjectSearch";
const MAX_INDEXED_FILES: usize = 8_000;
const MAX_VISIBLE_RESULTS: usize = 12;

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "__pycache__",
    "coverage",
];

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("down", ProjectSearchNext, Some("ProjectSearch > Input")),
        KeyBinding::new("up", ProjectSearchPrevious, Some("ProjectSearch > Input")),
        KeyBinding::new("enter", ProjectSearchConfirm, Some("ProjectSearch > Input")),
        KeyBinding::new(
            "secondary-enter",
            ProjectSearchConfirm,
            Some("ProjectSearch > Input"),
        ),
        KeyBinding::new(
            "tab",
            ProjectSearchCycleScope,
            Some("ProjectSearch > Input"),
        ),
    ]
}

/// One source lane of the mixed index. `All` searches every lane; Projects
/// only surface there since switching projects is rare enough not to earn a
/// chip of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    All,
    Files,
    Chats,
    Tasks,
    Docs,
}

impl SearchScope {
    const ALL: [Self; 5] = [Self::All, Self::Files, Self::Chats, Self::Tasks, Self::Docs];

    fn title(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Files => "Files",
            Self::Chats => "Chats",
            Self::Tasks => "Tasks",
            Self::Docs => "Docs",
        }
    }

    fn admits(self, kind: &SearchItemKind) -> bool {
        match self {
            Self::All => true,
            Self::Files => matches!(kind, SearchItemKind::File(_)),
            Self::Chats => matches!(kind, SearchItemKind::Agent(_)),
            Self::Tasks => matches!(kind, SearchItemKind::Task(..)),
            Self::Docs => matches!(kind, SearchItemKind::Doc(_)),
        }
    }

    fn next(self) -> Self {
        let position = Self::ALL
            .iter()
            .position(|scope| *scope == self)
            .unwrap_or(0);
        Self::ALL[(position + 1) % Self::ALL.len()]
    }
}

#[derive(Clone)]
struct ProjectFile {
    path: PathBuf,
    name: SharedString,
    parent: SharedString,
    rel_path: String,
}

#[derive(Default)]
struct ProjectFileIndex {
    files: Vec<ProjectFile>,
    truncated: bool,
}

#[derive(Clone)]
enum SearchItemKind {
    File(PathBuf),
    Agent(Uuid),
    Doc(PathBuf),
    Project(ProjectId),
    Task(ProjectId, TaskRef),
}

#[derive(Clone)]
struct SearchItem {
    kind: SearchItemKind,
    title: SharedString,
    subtitle: SharedString,
    score: i32,
    rank_key: String,
}

fn kind_label(kind: &SearchItemKind) -> &'static str {
    match kind {
        SearchItemKind::File(_) => "Files",
        SearchItemKind::Agent(_) => "Chats",
        SearchItemKind::Doc(_) => "Docs",
        SearchItemKind::Project(_) => "Projects",
        SearchItemKind::Task(..) => "Tasks",
    }
}

pub struct ProjectSearch {
    workspace: Entity<Workspace>,
    center: Entity<CenterArea>,
    agents: Entity<AgentRecords>,
    docs: Entity<DocsState>,
    tasks: Entity<TasksState>,
    project: ProjectId,
    project_name: SharedString,
    input: Entity<InputState>,
    files: Vec<ProjectFile>,
    loading_files: bool,
    truncated_files: bool,
    selected: usize,
    scope: SearchScope,
    scroll: ScrollHandle,
}

impl ProjectSearch {
    pub fn open(
        workspace: Entity<Workspace>,
        center: Entity<CenterArea>,
        agents: Entity<AgentRecords>,
        docs: Entity<DocsState>,
        tasks: Entity<TasksState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(project) = workspace.read(cx).active_project().cloned() else {
            eprintln!("project search: no active project");
            return;
        };

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search…")
                .default_value("")
        });
        let project_id = project.id;
        let project_name: SharedString = project.name.clone().into();
        let project_root = project.path.clone();
        tasks.update(cx, |tasks, cx| tasks.refresh_my_tasks(cx));

        let search = cx.new(|cx| {
            cx.observe(&workspace, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&agents, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&docs, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&tasks, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.selected = 0;
                    cx.notify();
                }
            })
            .detach();
            Self {
                workspace,
                center,
                agents,
                docs,
                tasks,
                project: project_id,
                project_name,
                input,
                files: Vec::new(),
                loading_files: true,
                truncated_files: false,
                selected: 0,
                scope: SearchScope::All,
                scroll: ScrollHandle::new(),
            }
        });

        let scan_view = search.clone();
        cx.spawn(async move |cx| {
            let index = cx
                .background_executor()
                .spawn(async move { collect_project_files(project_root) })
                .await;
            scan_view
                .update(cx, |search, cx| {
                    search.files = index.files;
                    search.truncated_files = index.truncated;
                    search.loading_files = false;
                    search.clamp_selection(cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();

        let dialog_search = search.clone();
        palette_ui::soften_overlay(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            palette_ui::styled_dialog(dialog, palette_ui::DIALOG_W, cx).child(dialog_search.clone())
        });

        let input = search.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    fn filtered_items(&self, cx: &App) -> Vec<SearchItem> {
        let query = self.query(cx);
        let mut items = Vec::new();

        for agent in self.agents.read(cx).records_for_project(self.project) {
            if let Some(item) = agent_item(&agent, &query) {
                items.push(item);
            }
        }

        for doc in self.docs.read(cx).docs_for_project(self.project) {
            let searchable = format!("{} {}", doc.title, doc.relative_path.display());
            let Some(score) = (query.is_empty())
                .then_some(700)
                .or_else(|| fuzzy_score(&searchable, &query).map(|score| score + 900))
            else {
                continue;
            };
            items.push(SearchItem {
                kind: SearchItemKind::Doc(doc.path),
                title: doc.title.into(),
                subtitle: doc.relative_path.display().to_string().into(),
                score,
                rank_key: searchable.to_lowercase(),
            });
        }

        for project in &self.workspace.read(cx).projects {
            let Some(score) = (query.is_empty())
                .then_some(500)
                .or_else(|| fuzzy_score(&project.name, &query).map(|score| score + 600))
            else {
                continue;
            };
            items.push(SearchItem {
                kind: SearchItemKind::Project(project.id),
                title: project.name.clone().into(),
                subtitle: SharedString::default(),
                score,
                rank_key: project.name.to_lowercase(),
            });
        }

        for task in self.tasks.read(cx).my_tasks() {
            let reference = task.summary.reference;
            let searchable = format!(
                "{} {} {}",
                reference.issue_key, reference.title, task.project_name
            );
            let Some(score) = (query.is_empty())
                .then_some(650)
                .or_else(|| fuzzy_score(&searchable, &query).map(|score| score + 800))
            else {
                continue;
            };
            items.push(SearchItem {
                kind: SearchItemKind::Task(task.project, reference.clone()),
                title: reference.title.into(),
                subtitle: format!("{} · {}", reference.issue_key, task.project_name).into(),
                score,
                rank_key: searchable.to_lowercase(),
            });
        }

        for file in &self.files {
            if let Some(item) = file_item(file, &query) {
                items.push(item);
            }
        }

        items.retain(|item| self.scope.admits(&item.kind));
        items.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.rank_key.cmp(&b.rank_key))
        });
        items.truncate(MAX_VISIBLE_RESULTS);

        // Regroup the ranked page by source so the list scans in sections.
        // Groups keep first-hit order, so the strongest source still leads.
        let mut groups: Vec<(&'static str, Vec<SearchItem>)> = Vec::new();
        for item in items {
            let label = kind_label(&item.kind);
            if let Some(position) = groups.iter().position(|(key, _)| *key == label) {
                groups[position].1.push(item);
            } else {
                groups.push((label, vec![item]));
            }
        }
        groups.into_iter().flat_map(|(_, bucket)| bucket).collect()
    }

    fn clamp_selection(&mut self, cx: &App) {
        let len = self.filtered_items(cx).len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        let len = self.filtered_items(cx).len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1) % len;
        cx.notify();
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) {
        let len = self.filtered_items(cx).len();
        if len == 0 {
            return;
        }
        self.selected = if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
        cx.notify();
    }

    fn set_scope(&mut self, scope: SearchScope, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.selected = 0;
        cx.notify();
    }

    fn cycle_scope(&mut self, cx: &mut Context<Self>) {
        self.set_scope(self.scope.next(), cx);
    }

    fn confirm_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.filtered_items(cx).get(self.selected).cloned() else {
            return;
        };
        self.open_item(item.kind, window, cx);
    }

    fn open_item(&mut self, kind: SearchItemKind, window: &mut Window, cx: &mut Context<Self>) {
        match kind {
            SearchItemKind::File(path) => {
                self.center.update(cx, |center, cx| {
                    center.open_file(self.project, path, window, cx);
                });
            }
            SearchItemKind::Agent(agent_id) => {
                self.center.update(cx, |center, cx| {
                    center.open_agent(agent_id, window, cx);
                });
            }
            SearchItemKind::Doc(path) => {
                self.docs
                    .update(cx, |docs, cx| docs.select_doc(self.project, path, cx));
                self.center.update(cx, |center, cx| center.show_docs(cx));
            }
            SearchItemKind::Project(project) => {
                self.workspace
                    .update(cx, |workspace, cx| workspace.set_active(project, cx));
            }
            SearchItemKind::Task(project, reference) => {
                self.workspace
                    .update(cx, |workspace, cx| workspace.set_active(project, cx));
                self.tasks
                    .update(cx, |tasks, cx| tasks.select_task(project, reference, cx));
                self.center
                    .update(cx, |center, cx| center.show_task_detail(cx));
            }
        }
        palette_ui::close(window, cx);
    }

    fn render_scope_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .children(SearchScope::ALL.into_iter().enumerate().map(|(ix, scope)| {
                let active = scope == self.scope;
                div()
                    .id(("project-search-scope", ix))
                    .px_2()
                    .py_0p5()
                    .rounded(design::r_sm())
                    .text_size(design::text_ui())
                    .cursor_pointer()
                    .when(active, |chip| {
                        chip.bg(design::control_on(design::focus(cx), cx))
                            .text_color(design::t1(cx))
                    })
                    .when(!active, |chip| {
                        chip.text_color(design::t3(cx))
                            .hover(|chip| chip.text_color(design::t2(cx)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope, cx)))
                    .child(scope.title())
            }))
    }

    fn render_row(
        &self,
        ix: usize,
        item: SearchItem,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = ix == self.selected;
        let icon = match item.kind {
            SearchItemKind::File(_) => IconName::File,
            SearchItemKind::Agent(_) => IconName::Bot,
            SearchItemKind::Doc(_) => IconName::FileText,
            SearchItemKind::Project(_) => IconName::Folder,
            SearchItemKind::Task(_, _) => IconName::CircleCheck,
        };
        let icon_color = if selected {
            design::accent_ink(design::focus(cx), cx)
        } else {
            design::t3(cx)
        };
        let kind = item.kind.clone();

        palette_ui::row(selected, cx)
            .id(("project-search-row", ix))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_item(kind.clone(), window, cx);
            }))
            .child(Icon::new(icon).size(design::icon()).text_color(icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(design::text_body())
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(palette_ui::highlighted_title(item.title, query, cx)),
            )
            .when(!item.subtitle.is_empty(), |row| {
                row.child(
                    div()
                        .max_w(px(260.))
                        .text_size(design::text_ui())
                        .truncate()
                        .text_color(design::t3(cx))
                        .child(item.subtitle),
                )
            })
    }
}

impl Render for ProjectSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.filtered_items(cx);
        if self.selected >= items.len() {
            self.selected = items.len().saturating_sub(1);
        }
        let query = self.query(cx);
        let empty_text = if self.loading_files {
            "Indexing project files..."
        } else if query.is_empty() {
            "No files, chats, tasks, docs, or projects"
        } else {
            "No matches"
        };
        let indexed_label = if self.truncated_files {
            format!("{}+ files", self.files.len())
        } else {
            format!("{} files", self.files.len())
        };

        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_label: Option<&'static str> = None;
        for (ix, item) in items.into_iter().enumerate() {
            let label = kind_label(&item.kind);
            if last_label != Some(label) {
                rows.push(palette_ui::group_label(label, cx).into_any_element());
                last_label = Some(label);
            }
            if ix == self.selected {
                self.scroll.scroll_to_item(rows.len());
            }
            rows.push(self.render_row(ix, item, &query, cx).into_any_element());
        }

        palette_ui::frame(cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &ProjectSearchNext, _, cx| {
                this.select_next(cx);
            }))
            .on_action(cx.listener(|this, _: &ProjectSearchPrevious, _, cx| {
                this.select_previous(cx);
            }))
            .on_action(cx.listener(|this, _: &ProjectSearchConfirm, window, cx| {
                this.confirm_selected(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ProjectSearchCycleScope, _, cx| {
                this.cycle_scope(cx);
            }))
            .child(
                palette_ui::header_band()
                    .child(
                        Icon::new(IconName::Search)
                            .size(design::icon())
                            .text_color(design::t3(cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&self.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false),
                        ),
                    )
                    .child(self.render_scope_chips(cx)),
            )
            .child(
                v_flex()
                    .id("project-search-results")
                    .track_scroll(&self.scroll)
                    .max_h(px(360.))
                    .min_h(px(72.))
                    .px(px(palette_ui::LIST_PAD))
                    .pb(px(palette_ui::LIST_PAD_BOTTOM))
                    .gap_0p5()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .h(px(56.))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(design::text_body())
                                .text_color(design::t3(cx))
                                .child(empty_text),
                        )
                    })
                    .children(rows),
            )
            .child(
                palette_ui::footer_band()
                    .text_size(design::text_ui())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(design::t4(cx))
                            .child(format!("{} · {}", self.project_name, indexed_label)),
                    )
                    .child(palette_ui::key_hint("↑↓", "navigate", cx))
                    .child(palette_ui::key_hint("⇥", "scope", cx))
                    .child(palette_ui::key_hint("↩", "open", cx))
                    .child(palette_ui::key_hint("esc", "close", cx)),
            )
    }
}

fn collect_project_files(root: PathBuf) -> ProjectFileIndex {
    let mut index = ProjectFileIndex::default();
    let mut dirs = vec![root.clone()];

    while let Some(dir) = dirs.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = read.filter_map(|entry| entry.ok()).collect();
        entries.sort_by_key(|entry| entry.file_name().to_ascii_lowercase());

        for entry in entries {
            let name = entry.file_name();
            let name_string = name.to_string_lossy().into_owned();
            if name_string == ".DS_Store" {
                continue;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();
            if file_type.is_dir() {
                if !SKIPPED_DIRS.contains(&name_string.as_str()) {
                    dirs.push(path);
                }
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            if index.files.len() >= MAX_INDEXED_FILES {
                index.truncated = true;
                return index;
            }

            let rel_path = relative_path(&root, &path);
            let parent = rel_path
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_string())
                .unwrap_or_default();
            index.files.push(ProjectFile {
                path,
                name: SharedString::from(name_string),
                parent: SharedString::from(parent),
                rel_path,
            });
        }
    }

    index.files.sort_by_key(|file| file.rel_path.to_lowercase());
    index
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn file_item(file: &ProjectFile, query: &str) -> Option<SearchItem> {
    let score = if query.is_empty() {
        1_000 - file.rel_path.len().min(500) as i32
    } else {
        let name_score = fuzzy_score(file.name.as_ref(), query).map(|score| score + 1_500);
        let path_score = fuzzy_score(&file.rel_path, query);
        name_score.or(path_score)?
    };

    Some(SearchItem {
        kind: SearchItemKind::File(file.path.clone()),
        title: file.name.clone(),
        subtitle: file.parent.clone(),
        score,
        rank_key: file.rel_path.to_lowercase(),
    })
}

fn agent_item(agent: &AgentRecord, query: &str) -> Option<SearchItem> {
    let haystack = format!("{} {} {}", agent.title, agent.doc, agent.meta_label());
    let score = if query.is_empty() {
        1_200
    } else {
        fuzzy_score(&haystack, query).map(|score| score + 1_000)?
    };
    let subtitle = format!("{} · {}", agent.provider_label(), agent.status.label());

    Some(SearchItem {
        kind: SearchItemKind::Agent(agent.id),
        title: SharedString::from(agent.title.clone()),
        subtitle: SharedString::from(subtitle),
        score,
        rank_key: format!("chat/{}", agent.title.to_lowercase()),
    })
}

fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Some(0);
    }

    let haystack = haystack.to_lowercase();
    if haystack == needle {
        return Some(10_000);
    }
    if haystack.starts_with(&needle) {
        return Some(9_000 - haystack.len().min(500) as i32);
    }
    if let Some(ix) = haystack.find(&needle) {
        return Some(8_000 - ix.min(500) as i32);
    }

    let mut score = 0;
    let mut search_start = 0;
    let mut last_match = None;
    for ch in needle.chars() {
        let relative_ix = haystack[search_start..].find(ch)?;
        let ix = search_start + relative_ix;
        let boundary = ix == 0
            || haystack[..ix]
                .chars()
                .last()
                .map(|prev| matches!(prev, '/' | '-' | '_' | '.' | ' '))
                .unwrap_or(false);
        if boundary {
            score += 45;
        }
        if last_match == Some(ix) {
            score += 35;
        }
        score += 12;
        last_match = Some(ix + ch.len_utf8());
        search_start = ix + ch.len_utf8();
    }

    Some(score - haystack.len().min(500) as i32)
}
