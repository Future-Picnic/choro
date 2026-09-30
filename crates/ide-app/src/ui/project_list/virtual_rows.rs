//! Stable row identities and variable-height virtualization. The list shell is
//! deliberately uncached: a dirty row must not invalidate cached siblings.
use super::*;
use crate::state::delegation::display::DelegatedTaskRow;
use gpui::{ListAlignment, ListOffset, ListState};
use std::time::Instant;

#[cfg(test)]
thread_local! {
    pub(super) static RENDERED_AGENTS: std::cell::RefCell<Vec<Uuid>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Project,
    Pinned,
    Attention,
}

#[derive(Clone, PartialEq, Eq)]
enum Item {
    Section(SidebarSection),
    Attention,
    Pinned,
    Project(RowInfo),
    Agent(Lane, ProjectId, SharedString, String, SidebarAgent),
    Assignment(Lane, ProjectId, Uuid, DelegatedTaskRow),
    More(ProjectId, usize, bool),
    Gap(String, u16),
    Empty,
}

impl Item {
    fn key(&self) -> String {
        match self {
            Self::Section(s) => format!("section:{:?}", s.kind),
            Self::Attention => "attention".into(),
            Self::Pinned => "pinned".into(),
            Self::Project(p) => format!("project:{}", p.id.0),
            Self::Agent(lane, _, _, _, a) => format!("{lane:?}:{}", a.id),
            Self::Assignment(lane, _, parent, a) => format!("{lane:?}:{parent}:{}", a.task_id),
            Self::More(id, _, _) => format!("more:{}", id.0),
            Self::Gap(id, _) => format!("gap:{id}"),
            Self::Empty => "empty".into(),
        }
    }

    fn height(&self) -> f32 {
        match self {
            Self::Gap(_, height) => *height as f32,
            Self::Project(p) => {
                if p.scripts.is_empty() {
                    32.
                } else {
                    56.
                }
            }
            Self::Assignment(..) => 28.,
            Self::More(..) => 28.,
            Self::Empty => 80.,
            _ => 32.,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Interaction {
    selected: bool,
    hovered: bool,
    menu: bool,
    expanded: bool,
    pinned: bool,
    rename_epoch: Option<u64>,
}

pub(super) struct SidebarRows {
    pub dirty: bool,
    pub list: ListState,
    pub visible: std::rc::Rc<Vec<Row>>,
    pub received_at: Option<Instant>,
    keys: Vec<String>,
    views: HashMap<String, Entity<RowView>>,
}

#[derive(Clone)]
pub(super) struct Row {
    pub view: Entity<RowView>,
    pub anchors: ActivityAnchors,
    pub height: f32,
}

impl SidebarRows {
    pub fn new() -> Self {
        Self {
            dirty: true,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            visible: std::rc::Rc::new(vec![]),
            received_at: None,
            keys: vec![],
            views: HashMap::new(),
        }
    }
}

pub(super) struct RowView {
    owner: WeakEntity<ProjectList>,
    item: Item,
    interaction: Interaction,
    anchors: ActivityAnchors,
    received_at: Option<Instant>,
}

impl Render for RowView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _probe = crate::ui::performance::UiProbe::new("sidebar.row");
        #[cfg(test)]
        if let Item::Agent(_, _, _, _, agent) = &self.item {
            RENDERED_AGENTS.with(|ids| ids.borrow_mut().push(agent.id));
        }
        self.anchors.clear();
        let result = self
            .owner
            .update(cx, |owner, cx| {
                let previous = std::mem::replace(&mut owner.activity_anchors, self.anchors.clone());
                let result = match &self.item {
                    Item::Section(section) => owner.render_section_header(section, cx),
                    Item::Attention => owner
                        .render_attention_section(true, cx)
                        .unwrap_or_else(|| div().into_any_element()),
                    Item::Pinned => owner
                        .render_pinned_section(true, cx)
                        .unwrap_or_else(|| div().into_any_element()),
                    Item::Project(project) => owner
                        .render_row(project.clone(), true, cx)
                        .into_any_element(),
                    Item::Agent(lane, project, name, icon, agent) => match lane {
                        Lane::Project => owner.render_project_agent(0, 0, *project, agent, cx),
                        Lane::Pinned => {
                            owner.render_pinned_agent(0, *project, name.clone(), icon, agent, cx)
                        }
                        Lane::Attention => {
                            owner.render_attention_agent(0, *project, name.clone(), icon, agent, cx)
                        }
                    },
                    Item::Assignment(lane, project, parent, task) => div()
                        .w_full()
                        .pl(px(if *lane == Lane::Project { 34. } else { 12. }))
                        .pr_1()
                        .child(owner.render_delegated_task_row(*project, *parent, task, cx))
                        .into_any_element(),
                    Item::More(project, hidden, expanded) => {
                        let project = *project;
                        style::sidebar_more_agents_button(
                            ("show-agents", project.0.as_u128() as u64),
                            *hidden,
                            *expanded,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_agent_lists.remove(&project) {
                                this.expanded_agent_lists.insert(project);
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                    }
                    Item::Gap(..) => div().into_any_element(),
                    Item::Empty => div()
                        .px_2()
                        .py_4()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No projects yet.\nAdd a project to get started.")
                        .into_any_element(),
                };
                owner.activity_anchors = previous;
                result
            })
            .unwrap_or_else(|_| div().into_any_element());
        let received_at = self.received_at.take();
        div().size_full().child(result).child(
            gpui::canvas(
                |_, _, _| (),
                move |_, _, _, _| {
                    #[cfg(any(test, feature = "ui-performance"))]
                    if let Some(at) = received_at {
                        crate::ui::performance::probes::record(
                            "sidebar.event_to_paint",
                            at.elapsed(),
                        );
                    }
                    #[cfg(not(any(test, feature = "ui-performance")))]
                    let _ = received_at;
                },
            )
            .absolute()
            .size_0(),
        )
    }
}

impl ProjectList {
    fn push_agent_item(
        &self,
        items: &mut Vec<Item>,
        lane: Lane,
        project: ProjectId,
        name: SharedString,
        icon: String,
        agent: SidebarAgent,
        cx: &App,
    ) {
        let id = agent.id;
        items.push(Item::Agent(lane, project, name, icon, agent));
        if lane != Lane::Attention && self.expanded_delegations.contains(&id) {
            if let Some(tasks) = self.model.read(cx).assignments.get(&id) {
                items.extend(
                    tasks
                        .iter()
                        .cloned()
                        .map(|task| Item::Assignment(lane, project, id, task)),
                );
            }
        }
    }

    pub(super) fn reconcile_virtual_rows(&mut self, cx: &mut Context<Self>) {
        let _probe = crate::ui::performance::UiProbe::new("sidebar.projection");
        self.agent_list_view.active_work = self.workspace.read(cx).sidebar_active_work;
        let mut items = Vec::new();
        let attention = self.collect_attention_agents(cx);
        if !attention.is_empty() {
            items.push(Item::Attention);
            if !self.workspace.read(cx).attention_collapsed {
                for (project, name, icon, agent) in attention {
                    self.push_agent_item(
                        &mut items,
                        Lane::Attention,
                        project,
                        name,
                        icon,
                        agent,
                        cx,
                    );
                }
            }
            items.push(Item::Gap("attention".into(), 12));
        }
        let pinned = self.collect_pinned_agents(cx);
        if !pinned.is_empty() {
            items.push(Item::Pinned);
            if !self.workspace.read(cx).pinned_agents_collapsed {
                for (project, name, icon, agent) in pinned {
                    self.push_agent_item(&mut items, Lane::Pinned, project, name, icon, agent, cx);
                }
            }
            items.push(Item::Gap("pinned".into(), 12));
        }
        let rows = self.collect_rows(cx);
        let workspace = self.workspace.read(cx);
        let mut after_rows = false;
        for (ix, group) in ide_core::agent_navigation::project_groups(
            &workspace.projects,
            &workspace.project_sections,
        )
        .into_iter()
        .enumerate()
        {
            let (kind, collapsed) = if group.id == "favorites" {
                (SidebarSectionKind::Favorites, workspace.favorites_collapsed)
            } else if group.id == "projects" {
                (SidebarSectionKind::Projects, workspace.projects_collapsed)
            } else {
                let section = workspace
                    .project_sections
                    .iter()
                    .find(|s| s.id.0.to_string() == group.id)
                    .unwrap();
                (SidebarSectionKind::Custom(section.id), section.collapsed)
            };
            if ix > 0 {
                items.push(Item::Gap(group.id.clone(), if after_rows { 16 } else { 4 }));
            }
            items.push(Item::Section(SidebarSection {
                ix,
                label: group.name.into(),
                kind,
                collapsed,
                rows: vec![],
            }));
            after_rows = false;
            if collapsed {
                continue;
            }
            for row in rows
                .iter()
                .filter(|row| group.project_ids.contains(&row.id))
            {
                after_rows = true;
                let mut header = row.clone();
                header.in_progress_agents.clear();
                items.push(Item::Project(header));
                let expanded = self.agent_list_view.active_work
                    || workspace.expanded_projects.contains(&row.id);
                if !expanded {
                    continue;
                }
                let all = self.expanded_agent_lists.contains(&row.id);
                for agent in row
                    .in_progress_agents
                    .iter()
                    .take(if all { usize::MAX } else { 5 })
                {
                    self.push_agent_item(
                        &mut items,
                        Lane::Project,
                        row.id,
                        row.name.clone(),
                        row.icon.clone(),
                        agent.clone(),
                        cx,
                    );
                }
                if row.in_progress_agents.len() > 5 {
                    items.push(Item::More(row.id, row.in_progress_agents.len() - 5, all));
                }
            }
        }
        if rows.is_empty() {
            items.push(Item::Empty);
        }
        let keys = items.iter().map(Item::key).collect::<Vec<_>>();
        let old_offset = self.virtual_rows.list.logical_scroll_top();
        let next_offset = preserve_scroll(&self.virtual_rows.keys, &keys, old_offset);
        if self.virtual_rows.keys != keys {
            self.virtual_rows
                .list
                .splice(0..self.virtual_rows.keys.len(), keys.len());
            self.virtual_rows.list.scroll_to(next_offset);
        }
        let owner = cx.entity().downgrade();
        let mut visible = Vec::with_capacity(items.len());
        for (index, item) in items.into_iter().enumerate() {
            let mut interaction = Interaction::default();
            match &item {
                Item::Agent(lane, project, _, _, agent) => {
                    interaction.selected = if *lane == Lane::Attention {
                        self.attention_pinned == Some(agent.id)
                    } else {
                        self.workspace.read(cx).active == Some(*project)
                            && self.agents.read(cx).explicitly_selected_agent_id(*project)
                                == Some(agent.id)
                    };
                    interaction.hovered = self.hovered_agent
                        == Some(match lane {
                            Lane::Project => HoveredAgentRow::Project(agent.id),
                            Lane::Pinned => HoveredAgentRow::Pinned(agent.id),
                            Lane::Attention => HoveredAgentRow::Attention(agent.id),
                        });
                    interaction.pinned = self.workspace.read(cx).is_agent_pinned(agent.id);
                    interaction.expanded = self.expanded_delegations.contains(&agent.id);
                    interaction.rename_epoch =
                        self.agent_title_animation_epochs.get(&agent.id).copied();
                }
                Item::Project(row) => {
                    interaction.hovered = self.hovered_project == Some(row.id);
                    interaction.menu = self.menu_project == Some(row.id);
                    interaction.expanded = if self.agent_list_view.active_work {
                        self.agent_list_view.expanded_project == Some(row.id)
                    } else {
                        self.workspace.read(cx).expanded_projects.contains(&row.id)
                    };
                }
                Item::Attention => {
                    interaction.expanded = !self.workspace.read(cx).attention_collapsed
                }
                Item::Pinned => {
                    interaction.expanded = !self.workspace.read(cx).pinned_agents_collapsed
                }
                Item::Section(section) => {
                    if let SidebarSectionKind::Custom(id) = section.kind {
                        interaction.hovered = self.hovered_section == Some(id);
                        interaction.menu = self.menu_section == Some(id);
                    }
                }
                _ => {}
            }
            let key = &keys[index];
            let height = item.height();
            let view = if let Some(view) = self.virtual_rows.views.get(key) {
                view.clone()
            } else {
                let view = cx.new(|_| RowView {
                    owner: owner.clone(),
                    item: item.clone(),
                    interaction: interaction.clone(),
                    anchors: ActivityAnchors::default(),
                    received_at: None,
                });
                self.virtual_rows.views.insert(key.clone(), view.clone());
                view
            };
            let (changed, old_height, anchors) = {
                let previous = view.read(cx);
                (
                    previous.item != item || previous.interaction != interaction,
                    previous.item.height(),
                    previous.anchors.clone(),
                )
            };
            if old_height != height {
                self.virtual_rows.list.splice(index..index + 1, 1);
                self.virtual_rows.list.scroll_to(next_offset);
            }
            if changed {
                // Off-screen changes have no paint deadline. A later user scroll
                // must not be counted as event-processing latency.
                let received_at = self.virtual_rows.received_at.filter(|_| {
                    self.virtual_rows
                        .list
                        .bounds_for_item(index)
                        .is_some_and(|bounds| {
                            let visible =
                                bounds.intersect(&self.virtual_rows.list.viewport_bounds());
                            visible.size.height > px(0.) && visible.size.width > px(0.)
                        })
                });
                view.update(cx, |view, cx| {
                    view.item = item;
                    view.interaction = interaction;
                    view.received_at = received_at;
                    cx.notify();
                });
            }
            visible.push(Row {
                view,
                anchors,
                height,
            });
        }
        let current_keys = keys.iter().collect::<HashSet<_>>();
        self.virtual_rows
            .views
            .retain(|key, _| current_keys.contains(key));
        self.virtual_rows.keys = keys;
        self.virtual_rows.visible = std::rc::Rc::new(visible);
        self.virtual_rows.received_at = None;
        self.virtual_rows.dirty = false;
    }
}

fn preserve_scroll(old: &[String], new: &[String], offset: ListOffset) -> ListOffset {
    let key = old.get(offset.item_ix);
    if let Some(ix) = key.and_then(|key| new.iter().position(|candidate| candidate == key)) {
        return ListOffset {
            item_ix: ix,
            offset_in_item: offset.offset_in_item,
        };
    }
    // If the anchor disappears, use the next surviving row, then the previous.
    let neighbor = old
        .iter()
        .skip(offset.item_ix)
        .chain(old.iter().take(offset.item_ix).rev())
        .find_map(|key| new.iter().position(|candidate| candidate == key));
    ListOffset {
        item_ix: neighbor.unwrap_or(0),
        offset_in_item: px(0.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_scroll_survives_insert_remove_and_collapse() {
        let keys = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let old = keys(&["a", "b", "c"]);
        let offset = ListOffset {
            item_ix: 1,
            offset_in_item: px(7.),
        };
        let next = preserve_scroll(&old, &keys(&["x", "a", "b", "c"]), offset);
        assert_eq!((next.item_ix, next.offset_in_item), (2, px(7.)));
        let next = preserve_scroll(&old, &keys(&["a", "c"]), offset);
        assert_eq!((next.item_ix, next.offset_in_item), (1, px(0.)));
        assert_eq!(preserve_scroll(&old, &[], offset).item_ix, 0);
    }
}
