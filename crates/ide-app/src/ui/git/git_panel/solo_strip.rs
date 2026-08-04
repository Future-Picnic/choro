//! Solo scope for the git panel.
//!
//! The panel shows nothing Solo-related — until the agent you have open is a
//! Solo with a live lane. Then a scope flip appears at the top, defaulting to
//! the Solo: the whole panel (changes, staging, commit, history, PRs) reads
//! from the lane's own GitState. Flip to Main and it's the ordinary project
//! panel again. Close the agent and the flip vanishes.

use super::*;

const SOLO_AHEAD_REFRESH: Duration = Duration::from_secs(30);

fn should_scope_to_solo_lane(is_solo: bool, is_rejoined: bool, lane_has_git: bool) -> bool {
    is_solo && !is_rejoined && lane_has_git
}

/// The Solo context the panel is currently scoped around.
pub(super) struct LaneScope {
    pub agent_id: uuid::Uuid,
    pub branch: String,
}

impl GitPanel {
    /// Recompute the Solo context from the selected agent; called every render
    /// before `active_git`. Creates/drops the lane GitState as the selection
    /// changes, and defaults the scope to the Solo when one appears.
    pub(super) fn sync_lane_scope(&mut self, cx: &mut Context<Self>) {
        let context = self
            .workspace
            .read(cx)
            .active
            .and_then(|project| self.agents.read(cx).explicitly_selected_agent(project))
            .and_then(|agent| {
                let lane = agent.lane_path.clone()?;
                should_scope_to_solo_lane(
                    agent.is_solo(),
                    agent.solo_rejoined_branch.is_some(),
                    lane.join(".git").exists(),
                )
                .then_some((agent, lane))
            });

        let Some((agent, lane)) = context else {
            self.scope_agent = None;
            self.lane_git = None;
            self.scope_main = false;
            return;
        };

        if self.scope_agent != Some(agent.id) {
            // A newly focused Solo: scope defaults to it — focus follows you.
            self.scope_agent = Some(agent.id);
            self.scope_main = false;
            self.lane_git = None;
            // A branch picker left open on Main must not survive into lane
            // scope, where switching branches isn't a thing.
            self.branches_expanded = false;
        }
        let stale = self
            .lane_git
            .as_ref()
            .map(|(id, _)| *id != agent.id)
            .unwrap_or(true);
        if stale {
            let git = cx.new(|cx| GitState::new(lane, cx));
            cx.observe(&git, |_, _, cx| cx.notify()).detach();
            self.lane_git = Some((agent.id, git));
        }
    }

    /// The current Solo scope, if the panel is showing one.
    pub(super) fn lane_scope(&self, cx: &App) -> Option<LaneScope> {
        let (agent_id, _) = self.lane_git.as_ref()?;
        let agent_id = *agent_id;
        let project = self.workspace.read(cx).active?;
        let branch = self
            .agents
            .read(cx)
            .explicitly_selected_agent(project)
            .filter(|agent| agent.id == agent_id)
            .and_then(|agent| agent.solo_branch)?;
        Some(LaneScope { agent_id, branch })
    }

    /// True while the panel reads from the lane rather than the project.
    pub(super) fn scoped_to_lane(&self) -> bool {
        self.lane_git.is_some() && !self.scope_main
    }

    /// The scope flip row — rendered only while a Solo is the open agent.
    pub(super) fn render_scope_flip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let scope = self.lane_scope(cx)?;
        let lane_active = self.scoped_to_lane();
        let sky = crate::ui::design::sky(cx);
        let agent_id = scope.agent_id;
        let ahead = self
            .solo_ahead
            .get(&agent_id)
            .map(|(ahead, _)| *ahead)
            .filter(|ahead| *ahead > 0);

        Some(
            h_flex()
                .w_full()
                .px_2()
                .py_1p5()
                .gap_2()
                .items_center()
                .when(lane_active, |row| row.bg(sky.opacity(0.07)))
                .child(
                    crate::ui::style::segmented_container_quiet(cx)
                        .child(
                            crate::ui::style::segment_with_leading(
                                "git-scope-main",
                                crate::ui::design::indicator::lucide_icon(
                                    lucide_icons::Icon::GitBranch,
                                    crate::ui::design::t3(cx),
                                    crate::ui::design::icon_sm(),
                                )
                                .into_any_element(),
                                "Main",
                                !lane_active,
                                cx,
                            )
                            .flex_1()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scope_main = true;
                                cx.notify();
                            })),
                        )
                        .child(
                            crate::ui::style::segment_with_leading(
                                "git-scope-lane",
                                crate::ui::design::indicator::solo_icon(
                                    if lane_active {
                                        sky
                                    } else {
                                        crate::ui::design::t3(cx)
                                    },
                                    crate::ui::design::icon_sm(),
                                )
                                .into_any_element(),
                                {
                                    let slug = crate::ui::style::solo_slug_short(&scope.branch, 14);
                                    match ahead {
                                        Some(ahead) => format!("{slug} ↑{ahead}"),
                                        None => slug,
                                    }
                                },
                                lane_active,
                                cx,
                            )
                            .flex_1()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scope_main = false;
                                cx.notify();
                            })),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Refresh the cached ahead counts off the render path — powers the ↑N in
    /// the flip row. At most one background pass per interval.
    pub(super) fn maybe_refresh_solo_ahead(&mut self, cx: &mut Context<Self>) {
        if self.solo_ahead_fetching {
            return;
        }
        if self
            .solo_ahead_checked_at
            .is_some_and(|at| at.elapsed() < SOLO_AHEAD_REFRESH)
        {
            return;
        }
        let Some(project) = self.workspace.read(cx).active else {
            return;
        };
        let targets: Vec<(uuid::Uuid, PathBuf, String, Option<String>)> = self
            .agents
            .read(cx)
            .records_for_project(project)
            .iter()
            .filter(|agent| agent.is_solo())
            .filter_map(|agent| {
                Some((
                    agent.id,
                    agent.project_path.clone(),
                    agent.solo_branch.clone()?,
                    agent.solo_base_branch.clone(),
                ))
            })
            .collect();
        self.solo_ahead_checked_at = Some(Instant::now());
        if targets.is_empty() {
            self.solo_ahead.clear();
            return;
        }
        self.solo_ahead_fetching = true;
        cx.spawn(async move |this, cx| {
            let counts = cx
                .background_executor()
                .spawn(async move {
                    targets
                        .into_iter()
                        .filter_map(|(id, repo, branch, base)| {
                            let base = base.or_else(|| {
                                ide_core::git::read_head(&repo).ok().and_then(|h| h.branch)
                            })?;
                            let counts = ide_core::git::ahead_behind(&repo, &branch, &base).ok()?;
                            Some((id, counts))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.solo_ahead = counts;
                this.solo_ahead_fetching = false;
                cx.notify();
            })
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::should_scope_to_solo_lane;

    #[test]
    fn rejoined_solo_never_uses_its_leftover_lane_for_git() {
        assert!(!should_scope_to_solo_lane(true, true, true));
        assert!(should_scope_to_solo_lane(true, false, true));
    }
}
