//! GitHub Actions for a merged Ship. The card belongs to the exact Ship it
//! describes and never changes the conversation's status, queue or composer
//! unless the user asks for an investigation draft.
use super::ship_actions_model::{
    self as model, ActionsTone, ShipActionsInline, ShipActionsPlacement,
};
use super::*;
use crate::state::agent_chat::ShipResult;
use crate::state::ship_actions::{ShipActionsHandle, ShipActionsState};
use ide_core::ship_actions::{GithubActionRun, ShipActionsWatch};

pub(super) fn ship_actions_state(cx: &App) -> Option<Entity<ShipActionsState>> {
    cx.try_global::<ShipActionsHandle>()
        .map(|handle| handle.0.clone())
}

/// Only real GitHub pull requests can be watched; onboarding demos cannot.
pub(super) fn ship_has_github_pr(result: &ShipResult) -> bool {
    result
        .pr_url
        .as_deref()
        .is_some_and(|url| url.starts_with("https://") && url.contains("/pull/"))
}

fn tone_color(tone: ActionsTone, cx: &App) -> gpui::Hsla {
    match tone {
        ActionsTone::Pending => crate::ui::design::amber(cx),
        ActionsTone::Success => crate::ui::design::sage(cx),
        ActionsTone::Failure => crate::ui::design::rose(cx),
        ActionsTone::Muted => crate::ui::design::t3(cx),
    }
}

/// Spinner while live; otherwise a glyph whose shape carries the state.
fn status_glyph(tone: ActionsTone, live: bool, seed: u64, cx: &App) -> gpui::AnyElement {
    let color = tone_color(tone, cx);
    if live {
        return crate::ui::logo_spinner::delegation_spinner(
            12.,
            "ship-actions-status",
            seed as usize,
            color,
        );
    }
    let icon = match tone {
        ActionsTone::Failure => lucide_icons::Icon::CircleX,
        ActionsTone::Success => lucide_icons::Icon::CheckCircle,
        ActionsTone::Pending => lucide_icons::Icon::CircleDashed,
        ActionsTone::Muted => lucide_icons::Icon::CircleMinus,
    };
    crate::ui::design::indicator::lucide_icon(icon, color, crate::ui::design::icon_sm())
        .into_any_element()
}

/// `live` is the watch's health: a cached running run is drawn still once
/// tracking stops, pauses or loses GitHub, so it never looks monitored.
fn run_glyph(run: &GithubActionRun, live: bool, seed: u64, cx: &App) -> gpui::AnyElement {
    let tone = model::run_tone(run);
    let color = tone_color(tone, cx);
    if model::run_spins(run, live) {
        return crate::ui::logo_spinner::delegation_spinner(
            12.,
            "ship-actions-run",
            seed as usize,
            color,
        );
    }
    let icon = match run.outcome() {
        ide_core::ship_actions::ActionsOutcome::Running => lucide_icons::Icon::CircleDot,
        ide_core::ship_actions::ActionsOutcome::Approval => lucide_icons::Icon::Hand,
        ide_core::ship_actions::ActionsOutcome::Queued => lucide_icons::Icon::CircleDashed,
        ide_core::ship_actions::ActionsOutcome::Cancelled => lucide_icons::Icon::CircleSlash,
        _ => match tone {
            ActionsTone::Failure => lucide_icons::Icon::CircleX,
            ActionsTone::Success => lucide_icons::Icon::CheckCircle,
            ActionsTone::Pending => lucide_icons::Icon::CircleDashed,
            ActionsTone::Muted => lucide_icons::Icon::CircleMinus,
        },
    };
    crate::ui::design::indicator::lucide_icon(icon, color, crate::ui::design::icon_sm())
        .into_any_element()
}

fn meta_column(label: &'static str, value: String, mono: bool, cx: &App) -> gpui::Div {
    v_flex()
        .gap_1()
        .min_w(px(0.))
        .child(
            div()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(label),
        )
        .child(
            div()
                .text_size(crate::ui::design::text_body())
                .truncate()
                .when(mono, |value| {
                    value.font_family(crate::ui::design::FONT_MONO)
                })
                .text_color(crate::ui::design::t1(cx))
                .child(SharedString::from(value)),
        )
}

impl CenterArea {
    pub(super) fn ship_actions_watch(
        &self,
        agent_id: Uuid,
        ship_id: &str,
        cx: &App,
    ) -> Option<ShipActionsWatch> {
        ship_actions_state(cx)?
            .read(cx)
            .watch_for_ship(agent_id, ship_id)
            .cloned()
    }

    /// Watcher updates arrive outside any user action, so a card can change
    /// height (runs appear, errors show) without a row being added. Remeasure
    /// only conversations whose Actions geometry changed.
    pub(super) fn sync_ship_actions_layout(&mut self, cx: &mut Context<Self>) {
        let Some(state) = ship_actions_state(cx) else {
            return;
        };
        let state = state.read(cx);
        let mut layouts: HashMap<Uuid, u64> = HashMap::new();
        for watch in state.watches.values() {
            for origin in &watch.origins {
                let entry = layouts.entry(origin.agent_id).or_default();
                *entry = entry.wrapping_add(model::layout_signature(
                    watch,
                    self.ship_actions_expanded.contains(&watch.id),
                    state.error.is_some(),
                ));
            }
        }
        let changed: Vec<Uuid> = layouts
            .iter()
            .filter(|(agent, signature)| self.ship_actions_layout.get(agent) != Some(signature))
            .map(|(agent, _)| *agent)
            .chain(
                self.ship_actions_layout
                    .keys()
                    .filter(|agent| !layouts.contains_key(agent))
                    .copied(),
            )
            .collect();
        for agent in changed {
            self.remeasure_agent_chat_list(agent);
        }
        self.ship_actions_layout = layouts;
    }

    /// Explicit tracking for a Ship created before Actions existed. Repository
    /// identity comes from the Ship's PR URL; the local path only runs `gh`.
    pub(super) fn track_ship_actions(
        &mut self,
        agent_id: Uuid,
        result: &ShipResult,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = ship_actions_state(cx) else {
            return;
        };
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let Some(path) = self.ship_result_repository_path(&agent, result, cx) else {
            return;
        };
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == agent.project_id)
            .map(|project| project.name.clone())
            .unwrap_or_default();
        state.update(cx, |state, cx| {
            state.register(&agent, result, path, project_name, cx)
        });
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    fn update_ship_actions(
        &mut self,
        agent_id: Uuid,
        watch_id: Uuid,
        change: fn(&mut ShipActionsState, Uuid, &mut Context<ShipActionsState>),
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = ship_actions_state(cx) {
            state.update(cx, |state, cx| change(state, watch_id, cx));
        }
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    /// Header control for Ships whose Actions are not tracked yet.
    pub(super) fn render_ship_actions_track_button(
        &self,
        agent: &AgentRecord,
        result: &ShipResult,
        has_repository: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !has_repository || !ship_has_github_pr(result) || ship_actions_state(cx).is_none() {
            return None;
        }
        if self.ship_actions_watch(agent.id, &result.id, cx).is_some() {
            return None;
        }
        let agent_id = agent.id;
        let ship = result.clone();
        Some(
            crate::ui::style::ghost_button_compact(
                ("agent-chat-ship-check-actions", stable_text_key(&result.id)),
                "Check Actions",
            )
            .text_color(crate::ui::design::t2(cx))
            .tooltip("Track GitHub Actions for this pull request's merge")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.track_ship_actions(agent_id, &ship, cx);
            }))
            .into_any_element(),
        )
    }

    /// One subdued line inside the Ship card for states that have no workflow
    /// runs to list, and for earlier Ships that point to the tracked one.
    pub(super) fn render_ship_actions_inline(
        &self,
        agent: &AgentRecord,
        result: &ShipResult,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let watch = self.ship_actions_watch(agent.id, &result.id, cx)?;
        let agent_id = agent.id;
        let key = stable_text_key(&result.id);
        let line = |glyph: gpui::AnyElement, text: SharedString| {
            h_flex()
                .w_full()
                .min_w(px(0.))
                .items_center()
                .gap_1p5()
                .px(crate::ui::design::chat_card_body_pad_x())
                .py_1p5()
                .border_t_1()
                .border_color(crate::ui::design::line(cx))
                .child(glyph)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(text),
                )
        };
        if let ShipActionsPlacement::Superseded { latest_ship_id } =
            model::placement(Some(&watch), agent.id, &result.id)
        {
            return Some(
                line(
                    status_glyph(model::watch_tone(&watch), false, key, cx),
                    "GitHub Actions for this PR are tracked on the latest Ship".into(),
                )
                .child(
                    crate::ui::style::ghost_button_compact(
                        ("ship-actions-go-latest", key),
                        "Go to latest Ship",
                    )
                    .text_color(crate::ui::design::t2(cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reveal_ship(agent_id, latest_ship_id.clone(), cx);
                    })),
                )
                .into_any_element(),
            );
        }
        let watch_id = watch.id;
        // An unmerged PR can wait for days; keep that line still and quiet.
        let awaiting_merge = watch.phase == ide_core::ship_actions::TrackingPhase::AwaitingMerge;
        let live = !awaiting_merge && watch.automatically_polled() && watch.refresh_error.is_none();
        let tone = if awaiting_merge {
            ActionsTone::Muted
        } else {
            model::watch_tone(&watch)
        };
        let glyph = status_glyph(tone, live, key, cx);
        let text = SharedString::from(watch.status_label());
        let row = match model::inline_state(&watch) {
            ShipActionsInline::None => return None,
            ShipActionsInline::Discovering => {
                line(glyph, text).when(model::offers_stop(&watch), |row| {
                    row.child(
                        crate::ui::style::ghost_button_compact(
                            ("ship-actions-inline-stop", key),
                            "Stop tracking",
                        )
                        .text_color(crate::ui::design::t3(cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_ship_actions(
                                agent_id,
                                watch_id,
                                ShipActionsState::stop_tracking,
                                cx,
                            );
                        })),
                    )
                })
            }
            ShipActionsInline::NoRuns
            | ShipActionsInline::Closed
            | ShipActionsInline::Resumable => {
                let label = if model::offers_resume(&watch) {
                    "Resume"
                } else {
                    "Check again"
                };
                line(glyph, text).child(
                    crate::ui::style::ghost_button_compact(
                        ("ship-actions-inline-refresh", key),
                        label,
                    )
                    .text_color(crate::ui::design::t2(cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_ship_actions(agent_id, watch_id, ShipActionsState::refresh, cx);
                    })),
                )
            }
        };
        Some(row.into_any_element())
    }

    /// The Actions card shown directly below the Ship that owns the watch.
    pub(super) fn render_ship_actions_card(
        &self,
        agent: &AgentRecord,
        result: &ShipResult,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let watch = self.ship_actions_watch(agent.id, &result.id, cx)?;
        if model::placement(Some(&watch), agent.id, &result.id) != ShipActionsPlacement::Owner
            || model::inline_state(&watch) != ShipActionsInline::None
        {
            return None;
        }
        let agent_id = agent.id;
        let watch_id = watch.id;
        let key = watch_id.as_u128() as u64;
        let now = unix_now_secs();
        let tone = model::watch_tone(&watch);
        let live = model::watch_live(&watch);
        let expanded = self.ship_actions_expanded.contains(&watch_id);
        let global_error = ship_actions_state(cx).and_then(|state| state.read(cx).error.clone());
        let elapsed = model::watch_elapsed(&watch, now).map(compact_duration);
        let failed = tone == ActionsTone::Failure;
        let approval = watch.outcome() == ide_core::ship_actions::ActionsOutcome::Approval;
        let view = cx.entity();

        let head = crate::ui::style::chat_card_head(cx)
            .child(status_glyph(tone, live, key, cx))
            .child(
                div()
                    .flex_none()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child("GitHub Actions"),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(tone_color(tone, cx))
                    .child(SharedString::from(watch.status_label())),
            )
            .when_some(elapsed, |row, elapsed| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t4(cx))
                        .child(SharedString::from(elapsed)),
                )
            })
            .child(div().flex_1())
            .child({
                let url = model::github_actions_url(&watch);
                crate::ui::style::ghost_button_compact(("ship-actions-open", key), "Open on GitHub")
                    .text_color(crate::ui::design::t2(cx))
                    .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url))
            })
            .child(
                crate::ui::style::ghost_button_compact(
                    ("ship-actions-refresh", key),
                    if model::offers_resume(&watch) {
                        "Resume"
                    } else {
                        "Refresh"
                    },
                )
                .text_color(crate::ui::design::t2(cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_ship_actions(agent_id, watch_id, ShipActionsState::refresh, cx);
                })),
            )
            .when(model::offers_stop(&watch), |row| {
                row.child(
                    crate::ui::style::header_icon_button(
                        ("ship-actions-more", key),
                        IconName::Ellipsis,
                        cx,
                    )
                    .tooltip("More")
                    .dropdown_menu(move |menu, window, _| {
                        menu.item(PopupMenuItem::new("Stop tracking").on_click(
                            window.listener_for(&view, move |this: &mut CenterArea, _, _, cx| {
                                this.update_ship_actions(
                                    agent_id,
                                    watch_id,
                                    ShipActionsState::stop_tracking,
                                    cx,
                                );
                            }),
                        ))
                    }),
                )
            });

        let meta = h_flex()
            .w_full()
            .gap_4()
            .items_center()
            .px(crate::ui::design::chat_card_body_pad_x())
            .py(crate::ui::design::chat_card_body_pad_y())
            .child(meta_column(
                "Repository",
                watch.repository.clone(),
                false,
                cx,
            ))
            .child(meta_column(
                "Pull request",
                format!("#{}", watch.pr_number),
                false,
                cx,
            ))
            .when(!watch.base_branch.is_empty(), |row| {
                row.child(meta_column("Base", watch.base_branch.clone(), true, cx))
            })
            .when_some(watch.merge_sha.as_deref(), |row, sha| {
                row.child(meta_column(
                    "Merge commit",
                    model::short_sha(sha).to_string(),
                    true,
                    cx,
                ))
            });

        let total = watch.runs.len();
        let runs = model::visible_runs(&watch.runs, expanded)
            .iter()
            .map(|run| self.render_ship_actions_run(run, &watch, now, cx))
            .collect::<Vec<_>>();
        let run_list = v_flex()
            .w_full()
            .py_1()
            .border_t_1()
            .border_color(crate::ui::design::line(cx))
            .children(runs)
            .when(total > model::COLLAPSED_RUN_LIMIT, |list| {
                list.child(
                    h_flex()
                        .px(crate::ui::design::chat_card_body_pad_x())
                        .pt_1()
                        .child(
                            crate::ui::style::ghost_button_compact(
                                ("ship-actions-expand", key),
                                if expanded {
                                    SharedString::from("Show fewer workflows")
                                } else {
                                    SharedString::from(format!("Show all {total} workflows"))
                                },
                            )
                            .text_color(crate::ui::design::t2(cx))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.ship_actions_expanded.remove(&watch_id) {
                                        this.ship_actions_expanded.insert(watch_id);
                                    }
                                    this.remeasure_agent_chat_list(agent_id);
                                    cx.notify();
                                },
                            )),
                        ),
                )
            });

        let notice = |icon: lucide_icons::Icon, color: gpui::Hsla, text: SharedString| {
            h_flex()
                .w_full()
                .items_center()
                .gap_1p5()
                .px(crate::ui::design::chat_card_body_pad_x())
                .py_2()
                .border_t_1()
                .border_color(crate::ui::design::line(cx))
                .child(crate::ui::design::indicator::lucide_icon(
                    icon,
                    color,
                    crate::ui::design::icon_sm(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(crate::ui::design::text_ui())
                        .line_height(gpui::relative(1.4))
                        .text_color(crate::ui::design::t2(cx))
                        .child(text),
                )
        };

        Some(
            crate::ui::style::chat_card(cx)
                .child(head)
                .child(meta)
                .child(run_list)
                .when(failed, |card| {
                    card.child(
                        notice(
                            lucide_icons::Icon::CircleX,
                            crate::ui::design::rose(cx),
                            "A workflow failed on the merge commit.".into(),
                        )
                        .child(
                            crate::ui::style::accent_button_compact(
                                ("ship-actions-investigate", key),
                                "Ask agent to investigate",
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.draft_ship_actions_investigation(
                                        agent_id, watch_id, window, cx,
                                    );
                                },
                            )),
                        ),
                    )
                })
                .when(approval && !failed, |card| {
                    card.child(notice(
                        lucide_icons::Icon::Hand,
                        crate::ui::design::amber(cx),
                        "A workflow is waiting for approval on GitHub.".into(),
                    ))
                })
                .when_some(watch.refresh_error.clone(), |card, error| {
                    let checked = watch
                        .last_checked_at
                        .map(|at| format!(" Last checked {}.", branch_relative_time(at as i64)))
                        .unwrap_or_default();
                    card.child(notice(
                        lucide_icons::Icon::CloudOff,
                        crate::ui::design::t3(cx),
                        format!(
                            "Couldn't reach GitHub. Choro will retry.{checked} {}",
                            model::bounded_error(&error)
                        )
                        .into(),
                    ))
                })
                .when_some(global_error, |card, error| {
                    card.child(notice(
                        lucide_icons::Icon::AlertTriangle,
                        crate::ui::design::amber(cx),
                        format!(
                            "{}. Tracking continues while Choro is open.",
                            model::bounded_error(&error)
                        )
                        .into(),
                    ))
                })
                .into_any_element(),
        )
    }

    fn render_ship_actions_run(
        &self,
        run: &GithubActionRun,
        watch: &ShipActionsWatch,
        now: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tone = model::run_tone(run);
        let url = run.html_url.clone();
        let live = model::watch_live(watch);
        let elapsed = model::run_elapsed_shown(run, now, watch).map(compact_duration);
        h_flex()
            .id(("ship-actions-run", run.id))
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_2()
            .px(crate::ui::design::chat_card_body_pad_x())
            .py_0p5()
            .child(run_glyph(run, live, run.id, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t1(cx))
                    .child(SharedString::from(run.name.clone())),
            )
            .when(run.attempt > 1, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t4(cx))
                        .child(SharedString::from(format!("Attempt {}", run.attempt))),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(tone_color(tone, cx))
                    .child(SharedString::from(model::run_status_label(run))),
            )
            .when_some(elapsed, |row, elapsed| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(52.))
                        .text_right()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t4(cx))
                        .child(SharedString::from(elapsed)),
                )
            })
            // A real, focusable control so every run is reachable by keyboard.
            .child(
                crate::ui::style::row_link_icon_button(("ship-actions-run-open", run.id), cx)
                    .tooltip(SharedString::from(model::run_open_label(run)))
                    .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url)),
            )
            .into_any_element()
    }
}
