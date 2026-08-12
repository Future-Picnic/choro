//! Solo lane orchestration: prepare an agent's lane in the background, gate
//! its first turn on the worktree step, and let env/deps finish async.
//!
//! The lane engine itself lives in `ide_core::lanes`; this module is the glue
//! between it and the center view — setup state for the "Preparing lane"
//! strip, the deferred (window-free) agent start, and lane recreation when a
//! Solo resumes after a teardown.

use super::*;
use crate::state::agent_chat::{RejoinConflictCard, RejoinedCard};
use ide_core::lanes::{self, LaneStep, LaneStepResult, RejoinOutcome};
use ide_core::LaneProfile;

struct LaneExitStop {
    backend: Option<crate::state::agent_chat::ChatBackendStopSignal>,
}

#[derive(Clone)]
enum RejoinDialogPhase {
    Choosing,
    Rejoining,
    Succeeded,
    Paused(String),
    CleanupNeeded(String),
    Failed(String),
}

fn choose_rejoin_target(
    names: &[String],
    preferred: Option<&str>,
    solo_base: Option<&str>,
    current: Option<&str>,
) -> String {
    [preferred, solo_base, current]
        .into_iter()
        .flatten()
        .find_map(|candidate| {
            names
                .iter()
                .find(|name| name.as_str() == candidate)
                .cloned()
        })
        .or_else(|| names.first().cloned())
        .unwrap_or_default()
}

/// The Rejoin confirmation: which branch the Solo merges into, with the
/// switch-notice when the pick isn't what the main tree has checked out.
pub(super) struct RejoinDialog {
    agent_id: Uuid,
    branch: String,
    /// Candidate targets with their `author · time · summary` detail line,
    /// read once from the main tree's snapshot when the dialog opens.
    options: Vec<(String, SharedString)>,
    target: String,
    current: Option<String>,
    expanded: bool,
    query: Entity<InputState>,
    center: gpui::WeakEntity<CenterArea>,
    phase: RejoinDialogPhase,
}

impl RejoinDialog {
    fn start_rejoin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.phase, RejoinDialogPhase::Rejoining) {
            return;
        }
        self.phase = RejoinDialogPhase::Rejoining;
        self.expanded = false;
        cx.notify();

        let target = self.target.clone();
        let agent_id = self.agent_id;
        let dialog = cx.entity().downgrade();
        let Some(center) = self.center.upgrade() else {
            self.phase = RejoinDialogPhase::Failed(
                "The workspace is no longer available. Close this dialog and try again.".into(),
            );
            cx.notify();
            return;
        };
        center.update(cx, |center, cx| {
            center.rejoin_solo(agent_id, target, dialog, window, cx);
        });
    }

    fn render_status(
        &self,
        title: &'static str,
        description: impl Into<SharedString>,
        icon: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .min_h(px(150.))
            .items_center()
            .justify_center()
            .gap_3()
            .child(icon)
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .max_w(px(340.))
                            .text_center()
                            .whitespace_normal()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.45))
                            .text_color(crate::ui::design::t3(cx))
                            .child(description.into()),
                    ),
            )
            .into_any_element()
    }

    fn render_error_details(&self, error: String, cx: &mut Context<Self>) -> gpui::AnyElement {
        let readable_error = error.replace('/', "/\u{200b}");
        v_flex()
            .w_full()
            .gap_2()
            .rounded(crate::ui::design::r_md())
            .bg(crate::ui::design::rose(cx).opacity(0.08))
            .p_3()
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::rose(cx))
                    .child("Technical details"),
            )
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .whitespace_normal()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::design::t2(cx))
                    .child(readable_error),
            )
            .into_any_element()
    }

    /// Target picker cloned from the Ship dialog's base-branch selector: a
    /// bordered trigger with the branch icon and left-aligned name that
    /// expands an inline, searchable branch list. Select-only.
    fn render_target_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.expanded;
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .id("rejoin-target")
                    .w_full()
                    .items_center()
                    .gap_1p5()
                    .h(crate::ui::design::subhead_h())
                    .px_2p5()
                    .rounded(px(crate::ui::style::RADIUS))
                    .border_1()
                    .border_color(if expanded {
                        crate::ui::design::accent(cx).opacity(0.6)
                    } else {
                        crate::ui::design::line(cx)
                    })
                    .bg(crate::ui::style::surface(cx))
                    .cursor_pointer()
                    .hover(|row| row.border_color(crate::ui::design::accent(cx).opacity(0.45)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.expanded = !this.expanded;
                        if this.expanded {
                            this.query
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        cx.notify();
                    }))
                    .child(branch_icon(crate::ui::design::t3(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .truncate()
                            .text_color(crate::ui::design::t1(cx))
                            .child(SharedString::from(self.target.clone())),
                    )
                    .child(
                        gpui_component::Icon::new(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx)),
                    ),
            )
            .when(expanded, |col| col.child(self.render_target_popup(cx)))
    }

    /// The expanded list: candidate branches with `author · time · summary`
    /// detail, filtered by the search field at the bottom — the Ship dialog's
    /// popup, verbatim.
    fn render_target_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let needle = self.query.read(cx).value().trim().to_lowercase();
        let selected = self.target.clone();
        let rows: Vec<(String, SharedString, bool)> = self
            .options
            .iter()
            .filter(|(name, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|(name, detail)| (name.clone(), detail.clone(), *name == selected))
            .collect();

        v_flex()
            .w_full()
            .min_w(px(0.))
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("rejoin-target-scroll")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No matching branches"),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(
                        |(ix, (name, detail, is_selected))| {
                            let value = name.clone();
                            let label: SharedString = name.into();
                            h_flex()
                                .id(("rejoin-target-row", ix))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .gap_2()
                                .items_center()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .when(is_selected, |row| row.bg(crate::ui::design::surface_2(cx)))
                                .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.target = value.clone();
                                    this.expanded = false;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .w(px(18.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(if is_selected {
                                            gpui_component::Icon::new(IconName::Check)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::accent(cx))
                                        } else {
                                            gpui_component::Icon::new(IconName::Replace)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t3(cx))
                                        }),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .truncate()
                                                .child(label),
                                        )
                                        .when(!detail.is_empty(), |col| {
                                            col.child(
                                                div()
                                                    .text_size(crate::ui::design::text_label())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .truncate()
                                                    .child(detail),
                                            )
                                        }),
                                )
                        },
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.28))
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.query)))
                    .child(
                        gpui_component::Icon::new(IconName::Search)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }
}

impl Render for RejoinDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let phase = self.phase.clone();
        let target = self.target.clone();
        match phase {
            RejoinDialogPhase::Choosing => {
                let switching = self.current.as_deref() != Some(self.target.as_str());
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .line_height(gpui::relative(1.5))
                            .child(
                                "One merge commit lands on the branch you pick. Rejoin completes \
                                 only after the lane folder is removed. The branch and chat are kept; nothing is pushed.",
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Merge into"),
                            )
                            .child(self.render_target_selector(cx)),
                    )
                    .when(switching, |dialog| {
                        dialog.child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::amber(cx))
                                .child(format!("Your project will switch to {}.", self.target)),
                        )
                    })
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-cancel",
                                    "Cancel",
                                    cx,
                                )
                                .on_click(cx.listener(|_, _, window, cx| {
                                    window.close_dialog(cx);
                                })),
                            )
                            .child(
                                crate::ui::style::primary_button_compact(
                                    "rejoin-confirm",
                                    "Rejoin",
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_rejoin(window, cx);
                                })),
                            ),
                    )
                    .into_any_element()
            }
            RejoinDialogPhase::Rejoining => v_flex()
                .gap_3()
                .child(self.render_status(
                    "Rejoining…",
                    format!(
                        "Stopping the Solo, merging into {target}, and removing its lane. You can hide this and keep working."
                    ),
                    logo_spinner(
                        34.,
                        "rejoin-progress",
                        self.agent_id.as_u128() as usize,
                        crate::ui::design::sky(cx),
                    ),
                    cx,
                ))
                .child(
                    h_flex().justify_end().child(
                        crate::ui::style::dialog_neutral_button(
                            "rejoin-progress-hide",
                            "Hide",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element(),
            RejoinDialogPhase::Succeeded => self.render_status(
                "Rejoin complete",
                format!(
                    "The Solo lane was removed. This agent now continues on {target} with the rest of the project."
                ),
                crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::Check,
                    crate::ui::design::sage(cx),
                    px(32.),
                )
                .into_any_element(),
                cx,
            ),
            RejoinDialogPhase::Paused(error) => {
                let copy_error = error.clone();
                v_flex()
                    .gap_3()
                    .child(self.render_status(
                        "Rejoin paused",
                        "The target branch is untouched. Resolve the conflicts from the card in this chat, then try again.",
                        crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::AlertTriangle,
                            crate::ui::design::amber(cx),
                            px(28.),
                        )
                        .into_any_element(),
                        cx,
                    ))
                    .child(self.render_error_details(error, cx))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-paused-copy",
                                    "Copy details",
                                    cx,
                                )
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_error.clone(),
                                    ));
                                }),
                            )
                            .child(
                                crate::ui::style::primary_button_compact(
                                    "rejoin-paused-review",
                                    "Review in chat",
                                    cx,
                                )
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    )
                    .into_any_element()
            }
            RejoinDialogPhase::CleanupNeeded(error) => {
                let copy_error = error.clone();
                v_flex()
                    .gap_3()
                    .child(self.render_status(
                        "Merge finished — cleanup failed",
                        format!(
                            "The work reached {target}, but Rejoin is not complete because the lane folder remains. Future messages now run on the project branch; use Retry cleanup to finish."
                        ),
                        crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::AlertTriangle,
                            crate::ui::design::amber(cx),
                            px(28.),
                        )
                        .into_any_element(),
                        cx,
                    ))
                    .child(self.render_error_details(error, cx))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-cleanup-copy",
                                    "Copy error",
                                    cx,
                                )
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_error.clone(),
                                    ));
                                }),
                            )
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-cleanup-done",
                                    "Close",
                                    cx,
                                )
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    )
                    .into_any_element()
            }
            RejoinDialogPhase::Failed(error) => {
                let copy_error = error.clone();
                v_flex()
                    .gap_3()
                    .child(self.render_status(
                        "Couldn’t rejoin",
                        "The lane and branch are still available. Copy the details if needed, then retry when the Git issue is resolved.",
                        crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::AlertTriangle,
                            crate::ui::design::rose(cx),
                            px(28.),
                        )
                        .into_any_element(),
                        cx,
                    ))
                    .child(self.render_error_details(error, cx))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-error-copy",
                                    "Copy error",
                                    cx,
                                )
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_error.clone(),
                                    ));
                                }),
                            )
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "rejoin-error-close",
                                    "Close",
                                    cx,
                                )
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                crate::ui::style::primary_button_compact(
                                    "rejoin-error-retry",
                                    "Retry",
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_rejoin(window, cx);
                                })),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

/// A free localhost port for a lane's dev server (the OS picks it).
fn free_lane_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|addr| addr.port())
}

/// Live setup state for one agent's lane — drives the "Preparing lane" strip.
#[derive(Clone)]
pub(super) struct LaneSetup {
    pub steps: Vec<(LaneStep, WorkLogStatus, String)>,
}

impl LaneSetup {
    fn begin() -> Self {
        Self {
            steps: [
                LaneStep::Worktree,
                LaneStep::ChoroDocs,
                LaneStep::EnvFiles,
                LaneStep::Dependencies,
            ]
            .into_iter()
            .map(|step| (step, WorkLogStatus::Pending, String::new()))
            .collect(),
        }
    }

    fn apply(&mut self, result: &LaneStepResult) {
        if let Some(entry) = self
            .steps
            .iter_mut()
            .find(|(step, _, _)| *step == result.step)
        {
            entry.1 = if result.ok {
                WorkLogStatus::Completed
            } else {
                WorkLogStatus::Failed
            };
            entry.2 = result.detail.clone();
        }
    }

    pub fn failed(&self) -> bool {
        self.steps
            .iter()
            .any(|(_, status, _)| *status == WorkLogStatus::Failed)
    }

    fn finished(&self) -> bool {
        self.steps
            .iter()
            .all(|(_, status, _)| *status == WorkLogStatus::Completed)
    }
}

impl CenterArea {
    fn lane_path_is_expected(
        &mut self,
        agent: &ide_core::AgentRecord,
        lane_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> bool {
        let expected = lanes::lane_path_for(agent.project_id, agent.id);
        if lane_path == expected {
            return true;
        }
        self.agent_start_errors.insert(
            agent.id,
            format!(
                "Refused to remove an unexpected lane path (expected {}, found {}).",
                expected.display(),
                lane_path.display()
            ),
        );
        cx.notify();
        false
    }

    /// Freeze every app-owned process rooted in a lane before Git starts an exit
    /// transition. This closes chat backends, CLI terminals, preview servers,
    /// and any plain terminal whose cwd is the worktree.
    fn begin_lane_exit(
        &mut self,
        agent: &ide_core::AgentRecord,
        cx: &mut Context<Self>,
    ) -> Option<LaneExitStop> {
        if !self.lane_exit_pending.insert(agent.id) {
            return None;
        }
        self.cancel_agent_chat_hydration(agent.id);
        self.sync_chat_session_ids(cx);
        let backend = self.agent_chats.update(cx, |chats, cx| {
            chats.stop_backend_for_lane_exit(agent.id, cx)
        });
        self.agent_chat_terminal_open.remove(&agent.id);
        self.lane_preview_pending.remove(&agent.id);

        if let Some(lane_path) = agent.lane_path.as_ref() {
            let (session_ids, lane_preview_urls) = {
                let terminals = self.terminals.read(cx);
                let session_ids = terminals
                    .sessions
                    .iter()
                    .filter(|session| {
                        session.project == agent.project_id
                            && (session.agent_record_id == Some(agent.id)
                                || session.cwd == *lane_path)
                    })
                    .map(|session| session.id)
                    .collect::<Vec<_>>();
                let lane_titles = terminals
                    .sessions
                    .iter()
                    .filter(|session| {
                        session.project == agent.project_id && session.cwd == *lane_path
                    })
                    .map(|session| session.title.to_string())
                    .collect::<HashSet<_>>();
                let lane_preview_urls = terminals
                    .project_preview_services(agent.project_id)
                    .into_iter()
                    .filter(|service| lane_titles.contains(&service.title))
                    .map(|service| service.url)
                    .collect::<HashSet<_>>();
                (session_ids, lane_preview_urls)
            };
            self.terminals.update(cx, |terminals, cx| {
                for id in session_ids {
                    terminals.close(id, cx);
                }
            });
            if self
                .project_preview_selected_urls
                .get(&agent.project_id)
                .is_some_and(|url| lane_preview_urls.contains(url))
            {
                self.project_preview_selected_urls.remove(&agent.project_id);
                if let Some(ui) = self.project_preview_ui.get_mut(&agent.project_id) {
                    ui.status = Some("The Solo preview stopped".to_string());
                }
            }
            self.project_preview_solo_urls.remove(&agent.id);
            self.reconcile_project_preview_for_selected_agent(agent.project_id, cx);
        }
        Some(LaneExitStop { backend })
    }

    /// Ensure a Solo agent's lane and its requested extras are ready, then start
    /// the agent. Existing lanes rerun the idempotent extras checks so a restart
    /// after a partial copy cannot skip preparation.
    pub(super) fn ensure_solo_lane_then_start(
        &mut self,
        agent_id: Uuid,
        target_mode: CenterMode,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return false;
        };
        if !agent.is_active_solo() {
            return self.start_agent_without_focus(agent_id, target_mode, cx);
        }
        let Some(branch) = agent.solo_branch.clone() else {
            return self.start_agent_without_focus(agent_id, target_mode, cx);
        };
        let lane_path = lanes::lane_path_for(agent.project_id, agent.id);
        if self
            .lane_setups
            .get(&agent_id)
            .is_some_and(|setup| !setup.failed())
        {
            // Already preparing — the completion callback will start the agent.
            return true;
        }

        let project_root = agent.repository_root().to_path_buf();
        let canonical_project_root = agent.project_path.clone();
        let profile = agent.lane_profile.unwrap_or(LaneProfile::Full);
        let base_hint = agent.solo_base_branch.clone();
        self.lane_setups.insert(agent_id, LaneSetup::begin());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let worktree_result = cx
                .background_executor()
                .spawn({
                    let project_root = project_root.clone();
                    let lane_path = lane_path.clone();
                    let branch = branch.clone();
                    async move {
                        // A surviving branch means this is a recreation; a
                        // fresh Solo branches off the recorded base. Always
                        // pass through the idempotent materializer so a crash
                        // leftover is verified as this exact branch before it
                        // is adopted.
                        let base = if lanes::branch_exists(&project_root, &branch) {
                            None
                        } else {
                            Some(
                                base_hint
                                    .or_else(|| {
                                        ide_core::git::read_head(&project_root)
                                            .ok()
                                            .and_then(|head| head.branch)
                                    })
                                    .unwrap_or_else(|| "HEAD".to_string()),
                            )
                        };
                        lanes::materialize_worktree(
                            &project_root,
                            &lane_path,
                            &branch,
                            base.as_deref(),
                        )
                    }
                })
                .await;

            let worktree_ok = worktree_result.ok;
            this.update(cx, |this, cx| {
                this.apply_lane_step(agent_id, &worktree_result, cx);
                if worktree_ok {
                    this.agents.update(cx, |agents, cx| {
                        agents.set_lane_path(agent_id, Some(lane_path.clone()), cx)
                    });
                }
            })?;
            if !worktree_ok {
                return anyhow::Ok(());
            }

            let extras = cx
                .background_executor()
                .spawn({
                    let project_root = project_root.clone();
                    let canonical_project_root = canonical_project_root.clone();
                    let lane_path = lane_path.clone();
                    async move {
                        let mut results = vec![lanes::refresh_choro_docs_snapshot(
                            &canonical_project_root,
                            &lane_path,
                        )];
                        results.extend(lanes::prepare_extras(&project_root, &lane_path, profile));
                        results
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                for result in &extras {
                    this.apply_lane_step(agent_id, result, cx);
                }
                if extras.iter().all(|result| result.ok) {
                    this.start_agent_without_focus(agent_id, target_mode, cx);
                }
            })?;
            anyhow::Ok(())
        })
        .detach();
        true
    }

    /// Re-run failed preparation against the existing worktree. Copying env and
    /// dependencies is idempotent, so successful steps stay intact.
    pub(super) fn retry_lane_setup(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.lane_setups.remove(&agent_id);
        self.ensure_solo_lane_then_start(agent_id, CenterMode::Agents, cx);
    }

    /// Debounce canonical document saves into background refreshes of every
    /// active Solo snapshot. A refresh never writes back into the canonical
    /// project and only replaces files recorded in Choro's snapshot manifest.
    pub(super) fn schedule_solo_docs_refresh(&mut self, cx: &mut Context<Self>) {
        self.solo_docs_refresh_generation = self.solo_docs_refresh_generation.wrapping_add(1);
        let generation = self.solo_docs_refresh_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;
            let jobs = this
                .update(cx, |this, cx| {
                    if this.solo_docs_refresh_generation != generation {
                        return Vec::new();
                    }
                    this.agents
                        .read(cx)
                        .all_records()
                        .into_iter()
                        .filter(|agent| {
                            agent.is_active_solo()
                                && agent.solo_rejoined_branch.is_none()
                                && agent.lane_path.is_some()
                        })
                        .filter_map(|agent| Some((agent.id, agent.project_path, agent.lane_path?)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if jobs.is_empty() {
                return;
            }
            let results = cx
                .background_executor()
                .spawn(async move {
                    jobs.into_iter()
                        .map(|(agent_id, project_path, lane_path)| {
                            (
                                agent_id,
                                lanes::refresh_choro_docs_snapshot(&project_path, &lane_path),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            for (agent_id, result) in results {
                if !result.ok {
                    eprintln!(
                        "failed to refresh Choro Docs for Solo {agent_id}: {}",
                        result.detail
                    );
                }
            }
        })
        .detach();
    }

    /// Open the Rejoin dialog: pick the branch the Solo merges into. Defaults
    /// to the branch it forked from — not wherever you happen to be standing.
    pub(super) fn open_rejoin_dialog(
        &mut self,
        agent_id: Uuid,
        preferred_target: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        if !agent.is_active_solo() {
            return;
        }
        let Some(branch) = agent
            .solo_branch
            .clone()
            .filter(|_| agent.lane_path.is_some())
        else {
            return;
        };
        // One-shot read of the main tree: its local branches (Solo branches
        // are never targets) and what's currently checked out.
        let snapshot = ide_core::git::read_snapshot(agent.repository_root()).ok();
        let current = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.head.branch.clone());
        let mut names: Vec<String> = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .branches
                    .iter()
                    .filter(|candidate| !candidate.is_remote)
                    .filter(|candidate| !candidate.name.starts_with("solo/"))
                    .map(|candidate| candidate.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.dedup();
        if names.is_empty() {
            if let Some(current) = current.clone() {
                names.push(current);
            }
        }
        // The `author · time · summary` subline the Ship dialog's branch list
        // shows — same snapshot fields, same fallback to the remote twin.
        let detail_for = |name: &str| -> SharedString {
            let Some(branches) = snapshot.as_ref().map(|snapshot| &snapshot.branches) else {
                return SharedString::default();
            };
            let info = branches
                .iter()
                .find(|candidate| candidate.name == name)
                .or_else(|| {
                    let remote = format!("origin/{name}");
                    branches.iter().find(|candidate| candidate.name == remote)
                });
            match info {
                Some(info) => {
                    let mut parts: Vec<String> = Vec::new();
                    if !info.tip_author.is_empty() {
                        parts.push(info.tip_author.clone());
                    }
                    let when = crate::ui::git::git_panel::relative_time(info.tip_time);
                    if !when.is_empty() {
                        parts.push(when);
                    }
                    if !info.tip_summary.is_empty() {
                        parts.push(info.tip_summary.clone());
                    }
                    parts.join(" · ").into()
                }
                None => SharedString::default(),
            }
        };
        let options: Vec<(String, SharedString)> = names
            .iter()
            .map(|name| (name.clone(), detail_for(name)))
            .collect();
        let target = choose_rejoin_target(
            &names,
            preferred_target.as_deref(),
            agent.solo_base_branch.as_deref(),
            current.as_deref(),
        );

        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search branch…"));
        let center = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            // Re-render as the user types so the filtered list updates live.
            cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            RejoinDialog {
                agent_id,
                branch: branch.clone(),
                options,
                target,
                current,
                expanded: false,
                query: query.clone(),
                center,
                phase: RejoinDialogPhase::Choosing,
            }
        });
        window.open_dialog(cx, move |dialog_view, _, _| {
            dialog_view
                .title(SharedString::from(format!("Rejoin {branch}")))
                .w(px(420.))
                .overlay_closable(false)
                .child(dialog.clone())
        });
    }

    /// Rejoin: merge the Solo's branch into the chosen target branch, then
    /// pack the lane up. Uncommitted lane work is committed first; conflicts
    /// abort cleanly and leave both sides intact.
    pub(super) fn rejoin_solo(
        &mut self,
        agent_id: Uuid,
        target: String,
        dialog: gpui::WeakEntity<RejoinDialog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            dialog
                .update(cx, |dialog, cx| {
                    dialog.phase = RejoinDialogPhase::Failed(
                        "This Solo is no longer available in the workspace.".into(),
                    );
                    cx.notify();
                })
                .ok();
            return;
        };
        let (Some(branch), Some(lane_path)) = (agent.solo_branch.clone(), agent.lane_path.clone())
        else {
            dialog
                .update(cx, |dialog, cx| {
                    dialog.phase = RejoinDialogPhase::Failed(
                        "This Solo no longer has a lane to rejoin.".into(),
                    );
                    cx.notify();
                })
                .ok();
            return;
        };
        if !self.lane_path_is_expected(&agent, &lane_path, cx) {
            let message = self
                .agent_start_errors
                .remove(&agent_id)
                .unwrap_or_else(|| "The Solo lane path could not be verified.".into());
            dialog
                .update(cx, |dialog, cx| {
                    dialog.phase = RejoinDialogPhase::Failed(message);
                    cx.notify();
                })
                .ok();
            return;
        }
        let project_root = agent.repository_root().to_path_buf();
        let title = agent.title.clone();
        self.agent_start_errors.remove(&agent_id);
        let Some(lane_exit) = self.begin_lane_exit(&agent, cx) else {
            dialog
                .update(cx, |dialog, cx| {
                    dialog.phase = RejoinDialogPhase::Failed(
                        "Another lane operation is already running. Wait for it to finish, then retry."
                            .into(),
                    );
                    cx.notify();
                })
                .ok();
            return;
        };
        let window_handle = window.window_handle();

        cx.spawn(async move |this, cx| {
            let backend_stopped = if let Some(stop) = lane_exit.backend.as_ref() {
                for _ in 0..100 {
                    if stop.is_stopped() {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                }
                stop.is_stopped()
            } else {
                true
            };
            let outcome = if backend_stopped {
                cx.background_executor()
                    .spawn({
                        let project_root = project_root.clone();
                        let lane_path = lane_path.clone();
                        let branch = branch.clone();
                        let target = target.clone();
                        async move {
                            lanes::rejoin(
                                &project_root,
                                &lane_path,
                                &branch,
                                &title,
                                Some(&target),
                            )
                        }
                    })
                    .await
            } else {
                Err(anyhow::anyhow!(
                    "The Solo agent process did not stop within 5 seconds. The merge was not started; retry Rejoin after the process exits."
                ))
            };

            let close_after_success = this.update(cx, |this, cx| {
                // Keep the lane operation locked if the backend ignored the
                // initial shutdown deadline. A follow-up task below continues
                // watching the same process; a retry must never mistake the
                // removed controller for proof that the process exited.
                if backend_stopped {
                    this.lane_exit_pending.remove(&agent_id);
                }
                let (phase, close_after_success, persistent_error) = match outcome {
                    Ok(RejoinOutcome::Merged) => {
                        // Deliberately NOT status → Done here: that would pull
                        // the agent out of the sidebar the user is looking at.
                        // The settled band and Rejoined card suggest it instead.
                        this.agents.update(cx, |agents, cx| {
                            agents.set_lane_path(agent_id, None, cx);
                            // Where the work went home — the settled band's
                            // "rejoined into X" reads from this.
                            agents.set_solo_rejoined_branch(agent_id, target.clone(), cx);
                        });
                        this.append_rejoined_card(agent_id, &branch, &target, cx);
                        (RejoinDialogPhase::Succeeded, true, None)
                    }
                    Ok(RejoinOutcome::MergedLaneRetained(message)) => {
                        this.agents.update(cx, |agents, cx| {
                            agents.set_solo_rejoined_branch(agent_id, target.clone(), cx);
                        });
                        this.retire_rejoin_conflict_cards(agent_id, cx);
                        (RejoinDialogPhase::CleanupNeeded(message), false, None)
                    }
                    Ok(RejoinOutcome::Conflict(message)) => {
                        let files = lanes::conflicted_paths(&message);
                        if files.is_empty() {
                            // Not a per-file conflict (dirty tree, failed
                            // switch, …) — no agent hand-off applies, so the
                            // plain error stays the honest surface.
                            let detail = format!(
                                "{message} The lane files and branch remain available, and any uncommitted work was preserved in a lane commit."
                            );
                            (
                                RejoinDialogPhase::Failed(detail.clone()),
                                false,
                                Some(format!("Rejoin failed — {detail}")),
                            )
                        } else {
                            this.append_rejoin_conflict_card(
                                agent_id, &branch, &target, files, &message, cx,
                            );
                            (RejoinDialogPhase::Paused(message), false, None)
                        }
                    }
                    Err(error) => {
                        let detail = format!("{error:#}");
                        (
                            RejoinDialogPhase::Failed(detail.clone()),
                            false,
                            Some(format!("Rejoin failed — {detail}")),
                        )
                    }
                };
                if let Some(error) = persistent_error {
                    this.agent_start_errors.insert(agent_id, error);
                }
                let dialog_is_open = dialog
                    .update(cx, |dialog, cx| {
                        dialog.phase = phase;
                        cx.notify();
                    })
                    .is_ok();
                cx.notify();
                close_after_success && dialog_is_open
            });
            if close_after_success.unwrap_or(false) {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(550))
                    .await;
                window_handle
                    .update(cx, |_, window, cx| window.close_dialog(cx))
                    .ok();
            }
            if !backend_stopped {
                if let Some(stop) = lane_exit.backend {
                    while !stop.is_stopped() {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                    }
                }
                this.update(cx, |this, cx| {
                    this.lane_exit_pending.remove(&agent_id);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Finish the folder-removal half of a Rejoin whose merge already landed.
    /// The agent is intentionally not stopped here: once `solo_rejoined_branch`
    /// is set, every runtime path points at the project repository, never this
    /// retained cleanup folder.
    fn finish_rejoin_cleanup(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let (Some(lane_path), Some(branch), Some(target)) = (
            agent.lane_path.clone(),
            agent.solo_branch.clone(),
            agent.solo_rejoined_branch.clone(),
        ) else {
            return;
        };
        if !self.lane_path_is_expected(&agent, &lane_path, cx)
            || !self.lane_exit_pending.insert(agent_id)
        {
            return;
        }
        self.agent_start_errors.remove(&agent_id);
        let project_root = agent.repository_root().to_path_buf();
        cx.spawn(async move |this, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { lanes::teardown_rejoined_lane(&project_root, &lane_path) })
                .await;
            this.update(cx, |this, cx| {
                this.lane_exit_pending.remove(&agent_id);
                match removed {
                    Ok(output) if output.success => {
                        this.agents
                            .update(cx, |agents, cx| agents.set_lane_path(agent_id, None, cx));
                        this.append_rejoined_card(agent_id, &branch, &target, cx);
                    }
                    Ok(output) => {
                        this.agent_start_errors.insert(
                            agent_id,
                            format!(
                                "Rejoin cleanup is still incomplete — {} The agent is working on {target}; the retained lane was not changed.",
                                output.message()
                            ),
                        );
                    }
                    Err(error) => {
                        this.agent_start_errors.insert(
                            agent_id,
                            format!(
                                "Rejoin cleanup is still incomplete — {error:#} The agent is working on {target}; the retained lane was not changed."
                            ),
                        );
                    }
                }
                cx.notify();
            })?;
            anyhow::Ok(())
        })
        .detach();
    }

    /// After a Solo's PR lands: tear the lane down (branch survives — pushes
    /// to it update the same PR; the lane recreates on demand for follow-ups).
    /// A lane with leftover uncommitted work refuses teardown and simply stays.
    pub(super) fn finish_solo_ship(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let Some(lane_path) = agent.lane_path.clone().filter(|_| agent.is_solo()) else {
            return;
        };
        if !self.lane_path_is_expected(&agent, &lane_path, cx) {
            return;
        }
        self.agent_start_errors.remove(&agent_id);
        let Some(_lane_exit) = self.begin_lane_exit(&agent, cx) else {
            return;
        };
        let project_root = agent.repository_root().to_path_buf();
        cx.spawn(async move |this, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { lanes::teardown_lane(&project_root, &lane_path, false) })
                .await;
            this.update(cx, |this, cx| {
                this.lane_exit_pending.remove(&agent_id);
                match removed {
                    Ok(output) if output.success => {
                        this.agents
                            .update(cx, |agents, cx| agents.set_lane_path(agent_id, None, cx));
                    }
                    Ok(output) => {
                        this.agent_start_errors.insert(
                            agent_id,
                            format!(
                                "Couldn't remove the shipped lane — {} Lane processes were stopped; use Clean up to retry.",
                                output.message()
                            ),
                        );
                    }
                    Err(error) => {
                        this.agent_start_errors.insert(
                            agent_id,
                            format!(
                                "Couldn't remove the shipped lane — {error:#} Lane processes were stopped; use Clean up to retry."
                            ),
                        );
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Offer to pack a Solo's lane up — shown when the agent is rejected.
    /// A dirty lane gets the louder wording and an explicit force; the branch
    /// and the chat always survive.
    pub(in crate::ui) fn confirm_discard_solo_lane(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let Some(lane) = agent.lane_path.clone().filter(|_| agent.is_solo()) else {
            return;
        };
        let dirty = lanes::lane_is_dirty(&lane).unwrap_or(false);
        let message = if dirty {
            "This lane has uncommitted changes — they will be lost. The branch and the chat are kept."
        } else {
            "Removes the Solo's folder. The branch and the chat are kept."
        };
        let view = cx.entity();
        crate::ui::confirm::ConfirmDialog::new("Discard the lane too?", message)
            .confirm_label(if dirty {
                "Discard anyway"
            } else {
                "Discard lane"
            })
            .cancel_label("Keep lane")
            .confirm_id("confirm-discard-solo-lane")
            .on_confirm(move |_, cx| {
                view.update(cx, |this, cx| {
                    this.discard_solo_lane(agent_id, dirty, cx);
                });
            })
            .open(window, cx);
    }

    /// Confirm removal initiated from the Solo worktree's overflow menu. This
    /// is separate from the Rejected-status prompt so the copy names the direct
    /// action the user chose. The branch and chat remain recoverable.
    pub(in crate::ui) fn confirm_delete_solo_worktree(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let Some(lane) = agent.lane_path.clone().filter(|_| agent.is_solo()) else {
            return;
        };
        let dirty = lanes::lane_is_dirty(&lane).unwrap_or(false);
        let message = if dirty {
            "This worktree has uncommitted changes. Deleting it will permanently remove those changes. The Solo branch and chat are kept."
        } else {
            "This removes the Solo worktree folder. The Solo branch and chat are kept, and the worktree can be recreated later."
        };
        let detail = agent.solo_branch.clone().unwrap_or_else(|| {
            lane.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Solo worktree".to_string())
        });
        let view = cx.entity();
        crate::ui::confirm::ConfirmDialog::new("Delete Solo worktree?", message)
            .icon(IconName::Delete)
            .detail(detail)
            .confirm_label(if dirty {
                "Delete anyway"
            } else {
                "Delete worktree"
            })
            .cancel_label("Cancel")
            .confirm_id("confirm-delete-solo-worktree")
            .on_confirm(move |_, cx| {
                view.update(cx, |this, cx| {
                    this.discard_solo_lane(agent_id, dirty, cx);
                });
            })
            .open(window, cx);
    }

    /// Discard a Solo's lane outright. `force` only after the user confirmed
    /// losing uncommitted changes; without it a dirty lane refuses and the
    /// caller asks. Branch and chat always survive.
    pub(in crate::ui) fn discard_solo_lane(
        &mut self,
        agent_id: Uuid,
        force: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return true;
        };
        let Some(lane_path) = agent.lane_path.clone().filter(|_| agent.is_solo()) else {
            return true;
        };
        if !self.lane_path_is_expected(&agent, &lane_path, cx) {
            return true;
        }
        let dirty = lanes::lane_is_dirty(&lane_path).unwrap_or(false);
        if dirty && !force {
            return false;
        }
        self.agent_start_errors.remove(&agent_id);
        let Some(_lane_exit) = self.begin_lane_exit(&agent, cx) else {
            return true;
        };
        let project_root = agent.repository_root().to_path_buf();
        cx.spawn(async move |this, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { lanes::teardown_lane(&project_root, &lane_path, force) })
                .await;
            this.update(cx, |this, cx| {
                this.lane_exit_pending.remove(&agent_id);
                match removed {
                    Ok(output) if output.success => {
                        this.agents
                            .update(cx, |agents, cx| agents.set_lane_path(agent_id, None, cx));
                    }
                    Ok(output) => {
                        this.agent_start_errors.insert(
                            agent_id,
                            format!(
                                "Couldn't remove the lane — {} Lane processes were stopped; retry when the Git error is resolved.",
                                output.message()
                            ),
                        );
                    }
                    Err(error) => {
                        this.agent_start_errors
                            .insert(
                                agent_id,
                                format!(
                                    "Couldn't remove the lane — {error:#} Lane processes were stopped; retry when the Git error is resolved."
                                ),
                            );
                    }
                }
                cx.notify();
            })
        })
        .detach();
        true
    }

    fn append_rejoined_card(
        &mut self,
        agent_id: Uuid,
        branch: &str,
        base: &str,
        cx: &mut Context<Self>,
    ) {
        let card = RejoinedCard {
            id: format!("{branch}:{base}:{}", unix_now_secs()),
            branch: branch.to_string(),
            base: base.to_string(),
            created_at: unix_now_secs(),
        };
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            if session.timeline.iter().any(|item| {
                matches!(
                    item,
                    AgentChatTimelineItem::Rejoined(existing)
                        if existing.branch == branch && existing.base == base
                )
            }) {
                return;
            }
            // The Rejoined card is the durable record now — retire any
            // conflict card still offering actions for this merge.
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::RejoinConflict(conflict) = item {
                    if conflict.dismissed_at.is_none() {
                        conflict.dismissed_at = Some(unix_now_secs());
                    }
                }
            }
            session.timeline.push(AgentChatTimelineItem::Rejoined(card));
            timeline_to_persist = Some(session.timeline.clone());
            cx.notify();
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist rejoin card: {error:#}");
            }
            self.agent_summary_requests_pending.remove(&agent_id);
            self.request_agent_summary_maintenance(agent_id, cx);
        }
    }

    fn retire_rejoin_conflict_cards(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            let mut changed = false;
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::RejoinConflict(conflict) = item {
                    if conflict.dismissed_at.is_none() {
                        conflict.dismissed_at = Some(unix_now_secs());
                        changed = true;
                    }
                }
            }
            if changed {
                timeline_to_persist = Some(session.timeline.clone());
                cx.notify();
            }
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to retire Rejoin conflict cards: {error:#}");
            }
        }
    }

    /// The "Rejoined" chat card: quiet confirmation of where the Solo went
    /// home, with the same close-out suggestion the Ship card carries.
    pub(super) fn render_rejoined_card(
        &self,
        agent: &ide_core::AgentRecord,
        card: &RejoinedCard,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let task_done = agent.status == AgentStatus::Done;
        let cleanup_pending = agent.lane_path.is_some()
            && agent.solo_rejoined_branch.as_deref() == Some(card.base.as_str());
        let status_color = if cleanup_pending {
            crate::ui::design::amber(cx)
        } else {
            crate::ui::design::sage(cx)
        };
        crate::ui::style::chat_card(cx)
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(crate::ui::design::indicator::lucide_icon(
                        if cleanup_pending {
                            lucide_icons::Icon::AlertTriangle
                        } else {
                            lucide_icons::Icon::GitMerge
                        },
                        status_color,
                        crate::ui::design::icon_sm(),
                    ))
                    .child(if cleanup_pending {
                        "Merge finished"
                    } else {
                        "Rejoined"
                    })
                    .child(div().flex_1())
                    .when(!cleanup_pending, |head| {
                        head.child(
                            crate::ui::style::chat_card_done_button(
                                ("rejoined-mark-done", agent_id.as_u128() as u64),
                                if task_done { "Done" } else { "Mark done" },
                                cx,
                            )
                            .disabled(task_done)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.agents.update(cx, |agents, cx| {
                                        agents.update_status(agent_id, AgentStatus::Done, cx);
                                    });
                                },
                            )),
                        )
                    }),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .items_center()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t2(cx))
                    .child(format!("{} merged into {}", card.branch, card.base))
                    .child(
                        div()
                            .text_color(if cleanup_pending {
                                crate::ui::design::amber(cx)
                            } else {
                                crate::ui::design::t4(cx)
                            })
                            .child(if cleanup_pending {
                                "· lane cleanup incomplete"
                            } else {
                                "· lane removed"
                            }),
                    ),
            )
            .into_any_element()
    }

    fn append_rejoin_conflict_card(
        &mut self,
        agent_id: Uuid,
        branch: &str,
        target: &str,
        files: Vec<String>,
        detail: &str,
        cx: &mut Context<Self>,
    ) {
        let card = RejoinConflictCard {
            id: format!("{branch}:{target}:{}", unix_now_secs()),
            branch: branch.to_string(),
            target: target.to_string(),
            files,
            detail: detail.to_string(),
            created_at: unix_now_secs(),
            requested_at: None,
            dismissed_at: None,
            resolved_at: None,
        };
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            session
                .timeline
                .push(AgentChatTimelineItem::RejoinConflict(card));
            timeline_to_persist = Some(session.timeline.clone());
            cx.notify();
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist rejoin-conflict card: {error:#}");
            }
        }
    }

    /// Hand the conflict to the Solo itself: mark the card, then send the
    /// merge-and-resolve directive as a turn. The work happens inside the
    /// lane on the Solo's own branch — the target branch is never touched.
    pub(super) fn resolve_rejoin_conflict(
        &mut self,
        agent_id: Uuid,
        card_id: String,
        cx: &mut Context<Self>,
    ) {
        let mut found: Option<(String, Vec<String>)> = None;
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::RejoinConflict(card) = item {
                    if card.id == card_id && card.requested_at.is_none() {
                        card.requested_at = Some(unix_now_secs());
                        found = Some((card.target.clone(), card.files.clone()));
                    }
                }
            }
            if found.is_some() {
                timeline_to_persist = Some(session.timeline.clone());
                cx.notify();
            }
        });
        let Some((target, files)) = found else {
            return;
        };
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist rejoin-conflict card: {error:#}");
            }
        }
        let list = files
            .iter()
            .map(|file| format!("- {file}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "<choro-rejoin-conflict-context>\nThis Solo's branch couldn't rejoin into `{target}`: the merge hit conflicts and was aborted, so `{target}` is untouched. Resolve it from your side, entirely inside your own working directory: run `git merge {target}`, open each conflicted file, combine both sides' work so nothing either side did is lost, remove every conflict marker, then stage everything and commit the merge. Do not push, and do not switch branches.\nConflicted files:\n{list}\n</choro-rejoin-conflict-context>\n\nResolve the rejoin conflicts with {target}."
        );
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        self.dispatch_agent_chat_submission(agent_id, prompt, mode, cx);
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    /// "Not now" on the conflict card. Timeline persistence only upserts, so
    /// dismissal flips a flag and the card simply stops rendering.
    pub(super) fn dismiss_rejoin_conflict(
        &mut self,
        agent_id: Uuid,
        card_id: String,
        cx: &mut Context<Self>,
    ) {
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            let mut dismissed = false;
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::RejoinConflict(card) = item {
                    if card.id == card_id && card.dismissed_at.is_none() {
                        card.dismissed_at = Some(unix_now_secs());
                        dismissed = true;
                    }
                }
            }
            if dismissed {
                timeline_to_persist = Some(session.timeline.clone());
                cx.notify();
            }
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist rejoin-conflict dismissal: {error:#}");
            }
        }
    }

    /// After a conflict hand-off: when the Solo's turn ends, check in the
    /// background whether the target branch now sits inside the lane's HEAD
    /// and flip the card to "Ready to rejoin". The `last_activity_at`
    /// watermark bounds this to one check per finished turn, not per poll.
    pub(super) fn maybe_check_rejoin_ready(&mut self, cx: &mut Context<Self>) {
        struct Candidate {
            agent_id: Uuid,
            card_id: String,
            target: String,
            lane: std::path::PathBuf,
            watermark: u64,
        }
        let candidates: Vec<Candidate> = {
            let chats = self.agent_chats.read(cx);
            let agents = self.agents.read(cx);
            chats
                .sessions
                .iter()
                .filter_map(|(agent_id, session)| {
                    if session.status != AgentChatStatus::Idle {
                        return None;
                    }
                    if self.rejoin_ready_inflight.contains(agent_id) {
                        return None;
                    }
                    if self.rejoin_ready_checks.get(agent_id) == Some(&session.last_activity_at) {
                        return None;
                    }
                    // Only lane-based agents can carry a rejoin-conflict card.
                    // Checking the lane first keeps this 650ms poll from
                    // walking the full timeline of every ordinary chat.
                    let lane = agents.agent(*agent_id)?.lane_path.clone()?;
                    let card = session.timeline.iter().rev().find_map(|item| match item {
                        AgentChatTimelineItem::RejoinConflict(card)
                            if card.requested_at.is_some()
                                && card.resolved_at.is_none()
                                && card.dismissed_at.is_none() =>
                        {
                            Some(card)
                        }
                        _ => None,
                    })?;
                    Some(Candidate {
                        agent_id: *agent_id,
                        card_id: card.id.clone(),
                        target: card.target.clone(),
                        lane,
                        watermark: session.last_activity_at,
                    })
                })
                .collect()
        };
        for candidate in candidates {
            self.rejoin_ready_inflight.insert(candidate.agent_id);
            cx.spawn(async move |this, cx| {
                let Candidate {
                    agent_id,
                    card_id,
                    target,
                    lane,
                    watermark,
                } = candidate;
                let ready = cx
                    .background_executor()
                    .spawn(async move { lanes::rejoin_ready(&lane, &target).unwrap_or(false) })
                    .await;
                this.update(cx, |this, cx| {
                    this.rejoin_ready_inflight.remove(&agent_id);
                    this.rejoin_ready_checks.insert(agent_id, watermark);
                    if ready {
                        this.mark_rejoin_conflict_resolved(agent_id, card_id, cx);
                    }
                })
            })
            .detach();
        }
    }

    fn mark_rejoin_conflict_resolved(
        &mut self,
        agent_id: Uuid,
        card_id: String,
        cx: &mut Context<Self>,
    ) {
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            let mut resolved_index = None;
            for (index, item) in session.timeline.iter_mut().enumerate() {
                if let AgentChatTimelineItem::RejoinConflict(card) = item {
                    if card.id == card_id && card.resolved_at.is_none() {
                        card.resolved_at = Some(unix_now_secs());
                        // Re-dated and moved below the agent's resolution
                        // turns — flipping in place would leave "Ready to
                        // rejoin" stranded above them, where it gets missed.
                        card.created_at = unix_now_secs();
                        resolved_index = Some(index);
                    }
                }
            }
            if let Some(index) = resolved_index {
                let card = session.timeline.remove(index);
                session.timeline.push(card);
                timeline_to_persist = Some(session.timeline.clone());
                cx.notify();
            }
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist rejoin-ready card: {error:#}");
            }
        }
    }

    /// The "Rejoin paused" card: which files overlap, and the one-tap hand-off
    /// that lets the Solo resolve the conflict inside its own lane.
    pub(super) fn render_rejoin_conflict_card(
        &self,
        agent_id: Uuid,
        card: &RejoinConflictCard,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if card.dismissed_at.is_some() {
            return div().into_any_element();
        }
        // Resolved: the compact "ready" form — one line, one button, no file
        // list. Sized for narrow lanes: the hint truncates first, the Rejoin
        // button never moves.
        if card.resolved_at.is_some() {
            let target = card.target.clone();
            let preferred_target = target.clone();
            return crate::ui::style::chat_card(cx)
                .child(
                    crate::ui::style::chat_card_head(cx)
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::Check,
                            crate::ui::design::sage(cx),
                            crate::ui::design::icon_sm(),
                        ))
                        .child("Ready to rejoin")
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t4(cx))
                                .child("· conflicts resolved"),
                        ),
                )
                .child(
                    h_flex()
                        .px_3()
                        .py_2()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t2(cx))
                                .child(format!("One merge commit lands on {target}.")),
                        )
                        .child(
                            crate::ui::style::primary_button_compact(
                                SharedString::from(format!("rejoin-ready-{}", card.id)),
                                "Rejoin",
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_rejoin_dialog(
                                        agent_id,
                                        Some(preferred_target.clone()),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        ),
                )
                .into_any_element();
        }
        let requested = card.requested_at.is_some();
        let resolve_id = card.id.clone();
        let dismiss_id = card.id.clone();
        crate::ui::style::chat_card(cx)
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::AlertTriangle,
                        crate::ui::design::amber(cx),
                        crate::ui::design::icon_sm(),
                    ))
                    .child("Rejoin paused")
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(format!("· overlaps with {}", card.target)),
                    ),
            )
            .child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child(format!(
                                "{} wasn't touched. Both sides changed these files:",
                                card.target
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_0p5()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .children(card.files.iter().map(|file| {
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_color(crate::ui::design::amber(cx))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("!"),
                                    )
                                    .child(
                                        div()
                                            .text_color(crate::ui::design::t2(cx))
                                            .child(file.clone()),
                                    )
                            })),
                    ),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        // Truncates before the buttons ever move: the actions
                        // stay pinned to the right at any card width.
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(if requested {
                                SharedString::from("Sent to the agent — Rejoin when it's done.")
                            } else {
                                SharedString::from("The agent fixes these in its lane.")
                            }),
                    )
                    .when(!requested, |footer| {
                        footer.child(
                            h_flex()
                                .flex_none()
                                .gap_2()
                                .child(
                                    crate::ui::style::ghost_button_compact(
                                        SharedString::from(format!(
                                            "rejoin-conflict-dismiss-{dismiss_id}"
                                        )),
                                        "Not now",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.dismiss_rejoin_conflict(
                                                agent_id,
                                                dismiss_id.clone(),
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    crate::ui::style::primary_button_compact(
                                        SharedString::from(format!(
                                            "rejoin-conflict-resolve-{resolve_id}"
                                        )),
                                        "Have the agent sort it",
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.resolve_rejoin_conflict(
                                                agent_id,
                                                resolve_id.clone(),
                                                cx,
                                            );
                                        },
                                    )),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn apply_lane_step(&mut self, agent_id: Uuid, result: &LaneStepResult, cx: &mut Context<Self>) {
        if let Some(setup) = self.lane_setups.get_mut(&agent_id) {
            setup.apply(result);
            // A fully successful setup needs no strip; failures stay visible.
            if setup.finished() {
                self.lane_setups.remove(&agent_id);
            }
        }
        cx.notify();
    }

    /// The project's dev preset for lane runs — first preset wins.
    fn first_lane_preset(&self, project: ProjectId, cx: &App) -> Option<(String, String)> {
        self.workspace
            .read(cx)
            .projects
            .iter()
            .find(|candidate| candidate.id == project)
            .and_then(|candidate| candidate.presets.first())
            .map(|preset| (preset.name.clone(), preset.command.clone()))
    }

    /// The suffixed terminal title a Solo's lane server runs under — distinct
    /// from the main preset so the preview picker keeps both.
    fn lane_preview_title(agent: &ide_core::AgentRecord, preset_name: &str) -> String {
        let slug = agent
            .solo_branch
            .as_deref()
            .map(|branch| branch.trim_start_matches("solo/"))
            .unwrap_or("lane");
        format!(
            "{preset_name}{}{slug}",
            crate::state::terminals::TerminalManager::SOLO_SCRIPT_MARKER
        )
    }

    /// One tap: see the Solo's version of the app. Starts the dev preset
    /// inside the lane (own PORT, suffixed title, terminal stays in the
    /// background) and opens the Preview on its URL the moment the server
    /// prints one. A server that's already up just opens.
    pub(super) fn open_lane_preview(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        if !agent.is_active_solo() {
            return;
        }
        let Some(lane) = agent.lane_path.clone() else {
            return;
        };
        let Some((name, command)) = self.first_lane_preset(agent.project_id, cx) else {
            return;
        };
        let project = agent.project_id;
        let title = Self::lane_preview_title(&agent, &name);

        // Already running? Point the Preview at it and we're done.
        let running = self
            .terminals
            .read(cx)
            .project_preview_services(project)
            .into_iter()
            .find(|service| service.title == title);
        if let Some(service) = running {
            self.project_preview_selected_urls
                .insert(project, service.url.clone());
            let ui = self.project_preview_ui.entry(project).or_default();
            ui.open = true;
            cx.notify();
            return;
        }
        if !self.lane_preview_pending.insert(agent_id) {
            return;
        }

        // The main server usually owns the script's default port; hand the
        // lane a free one via PORT. Servers that honor it (Next, Express, CRA)
        // bind there; Vite-style servers auto-bump on their own; either way
        // we open whatever URL actually gets printed.
        let envs: Vec<(String, String)> = free_lane_port()
            .map(|port| vec![("PORT".to_string(), port.to_string())])
            .unwrap_or_default();
        let spawned = self.terminals.update(cx, |manager, cx| {
            manager.spawn_preset_with_env(project, lane, &title, &command, &envs, cx)
        });
        if let Err(error) = spawned {
            self.lane_preview_pending.remove(&agent_id);
            self.agent_start_errors.insert(
                agent_id,
                format!("Couldn't start the lane server: {error:#}"),
            );
            cx.notify();
            return;
        }
        cx.notify();

        // Watch for the server's URL, then open the Preview on it.
        cx.spawn(async move |this, cx| {
            for _ in 0..120 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let done = this.update(cx, |this, cx| {
                    let Some(service) = this
                        .terminals
                        .read(cx)
                        .project_preview_services(project)
                        .into_iter()
                        .find(|service| service.title == title)
                    else {
                        return false;
                    };
                    this.lane_preview_pending.remove(&agent_id);
                    this.project_preview_selected_urls
                        .insert(project, service.url.clone());
                    let ui = this.project_preview_ui.entry(project).or_default();
                    ui.open = true;
                    ui.status = Some(format!("Opened {}", service.title));
                    cx.notify();
                    true
                })?;
                if done {
                    return anyhow::Ok(());
                }
            }
            this.update(cx, |this, cx| {
                if this.lane_preview_pending.remove(&agent_id) {
                    this.agent_start_errors.insert(
                        agent_id,
                        "The lane server didn't print a URL — check its terminal.".to_string(),
                    );
                    cx.notify();
                }
            })?;
            anyhow::Ok(())
        })
        .detach();
    }

    /// The lane band sitting on the composer: fused inside the frame's top
    /// edge as a recessed one-liner. Identity in the ink (sky fork + branch),
    /// quiet Preview / Rejoin text actions — no card, no border, no headline.
    pub(super) fn render_solo_lane_band(
        &self,
        agent: &ide_core::AgentRecord,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let branch = agent.solo_branch.clone()?;
        // A packed-up lane leaves a permanent trace. A successfully merged lane
        // whose cleanup failed is also settled, but keeps one safe cleanup action
        // instead of offering Rejoin a second time.
        let cleanup_needed = agent.lane_path.is_some() && agent.solo_rejoined_branch.is_some();
        if agent.lane_path.is_none() || cleanup_needed {
            let detail: String = match agent.solo_rejoined_branch.as_ref() {
                Some(base) if cleanup_needed => {
                    format!("· merged into {base} · cleanup incomplete")
                }
                Some(base) => format!("· rejoined into {base}"),
                None => "· packed up".to_string(),
            };
            let agent_id = agent.id;
            let cleaning = self.lane_exit_pending.contains(&agent_id);
            return Some(
                h_flex()
                    .w_full()
                    .px_3p5()
                    .py_1p5()
                    .gap_2()
                    .items_center()
                    .rounded_t(crate::ui::design::r_lg())
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx))
                    .child(crate::ui::design::indicator::solo_icon(
                        crate::ui::design::t4(cx),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .truncate()
                            .child(branch),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(detail),
                    )
                    .child(div().flex_1())
                    .when(cleanup_needed, |band| {
                        band.child(
                            crate::ui::style::solo_lane_action_button(
                                ("solo-band-cleanup", agent_id.as_u128() as u64),
                                if cleaning {
                                    "Cleaning up…"
                                } else {
                                    "Retry cleanup"
                                },
                                false,
                                cx,
                            )
                            .loading(cleaning)
                            .disabled(cleaning)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.finish_rejoin_cleanup(agent_id, cx);
                                },
                            )),
                        )
                    })
                    // A suggestion, never automatic: rejoining doesn't yank
                    // the agent out of the sidebar — the user closes it out.
                    .when(
                        !cleanup_needed && agent.status != AgentStatus::Done,
                        |band| {
                            band.child(
                                crate::ui::style::solo_lane_action_button(
                                    ("solo-band-mark-done", agent_id.as_u128() as u64),
                                    "Mark done",
                                    false,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.agents.update(cx, |agents, cx| {
                                            agents.update_status(agent_id, AgentStatus::Done, cx);
                                        });
                                    },
                                )),
                            )
                        },
                    )
                    .when(cleanup_needed, |band| {
                        band.child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::AlertTriangle,
                            crate::ui::design::amber(cx),
                            crate::ui::design::icon_sm(),
                        ))
                    })
                    .when(!cleanup_needed, |band| {
                        band.child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::Check,
                            crate::ui::design::sage(cx),
                            crate::ui::design::icon_sm(),
                        ))
                    })
                    .into_any_element(),
            );
        }
        let agent_id = agent.id;
        let sky = crate::ui::design::sky(cx);
        let starting = self.lane_preview_pending.contains(&agent_id);
        let rejoining = self.lane_exit_pending.contains(&agent_id);
        let show_preview = agent.lane_profile == Some(LaneProfile::Full)
            && self.first_lane_preset(agent.project_id, cx).is_some();

        Some(
            h_flex()
                .w_full()
                .px_3p5()
                .py_1p5()
                .gap_2()
                .items_center()
                // A strip perched on the composer: its own top corners, side
                // borders continuing the frame's, and the composer's top
                // border underneath acting as its bottom rule.
                .rounded_t(crate::ui::design::r_lg())
                .border_t_1()
                .border_l_1()
                .border_r_1()
                .border_color(crate::ui::design::line_2(cx))
                .bg(crate::ui::design::surface(cx))
                .child(crate::ui::design::indicator::solo_icon(
                    sky,
                    crate::ui::design::icon_sm(),
                ))
                .child(
                    div()
                        .min_w(px(0.))
                        .text_size(crate::ui::design::text_label())
                        .text_color(sky)
                        .truncate()
                        .child(branch),
                )
                .child(div().flex_1())
                .when(show_preview, |band| {
                    // The point is *seeing* the Solo's version — the script
                    // running behind it is an implementation detail.
                    band.child(
                        crate::ui::style::solo_lane_action_button(
                            ("solo-band-preview", agent_id.as_u128() as u64),
                            if starting { "Starting…" } else { "Preview" },
                            false,
                            cx,
                        )
                        .icon(IconName::Eye)
                        .disabled(starting || self.lane_exit_pending.contains(&agent_id))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_lane_preview(agent_id, cx);
                        })),
                    )
                })
                // Rejoin is the band's important action: a soft sky fill lifts
                // it above the quiet Preview without shouting over send.
                .child(
                    crate::ui::style::solo_lane_action_button(
                        ("solo-band-rejoin", agent_id.as_u128() as u64),
                        if rejoining { "Rejoining…" } else { "Rejoin" },
                        true,
                        cx,
                    )
                    .loading(rejoining)
                    .disabled(rejoining)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_rejoin_dialog(agent_id, None, window, cx);
                    })),
                )
                .into_any_element(),
        )
    }

    /// Compact strip under the agent header while a lane prepares (or after a
    /// step failed). Modeled on the agent-start-error strip.
    pub(super) fn render_lane_setup_strip(
        &self,
        agent_id: Uuid,
        setup: &LaneSetup,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let failed = setup.failed();
        let accent = if failed {
            crate::ui::design::rose(cx)
        } else {
            crate::ui::design::t3(cx)
        };
        let failed_detail = setup
            .steps
            .iter()
            .find(|(_, status, _)| *status == WorkLogStatus::Failed)
            .map(|(_, _, detail)| detail.clone())
            .filter(|detail| !detail.is_empty());

        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(accent.opacity(0.2))
            .bg(accent.opacity(0.06))
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(if failed {
                        crate::ui::design::rose(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .child(if failed {
                        "Lane setup failed"
                    } else {
                        "Preparing lane"
                    }),
            )
            .children(setup.steps.iter().map(|(step, status, _)| {
                let (icon, color) = match status {
                    WorkLogStatus::Completed => (IconName::Check, crate::ui::design::sage(cx)),
                    WorkLogStatus::Failed => (IconName::TriangleAlert, crate::ui::design::rose(cx)),
                    _ => (IconName::LoaderCircle, crate::ui::design::t3(cx)),
                };
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        gpui_component::Icon::new(icon)
                            .size(crate::ui::design::icon_sm())
                            .text_color(color),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(step.label()),
                    )
            }))
            .when_some(failed_detail, |strip, detail| {
                strip.child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .truncate()
                        .child(detail),
                )
            })
            .when(failed, |strip| {
                strip.child(
                    crate::ui::style::ghost_button_compact(
                        ("retry-lane-setup", agent_id.as_u128() as u64),
                        "Retry",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.retry_lane_setup(agent_id, cx);
                    })),
                )
            })
            .into_any_element()
    }

    /// Start an agent without needing a `Window` — the deferred path used when
    /// a lane finishes preparing in the background. Identical to
    /// `start_agent_in_mode` except the terminal-focus nicety.
    pub(super) fn start_agent_without_focus(
        &mut self,
        agent_id: Uuid,
        target_mode: CenterMode,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return false;
        };
        if agent.runtime == AgentRuntimeKind::Chat {
            return self.start_chat_agent_in_mode(agent, target_mode, cx);
        }

        let project = agent.project_id;
        let existing = {
            let manager = self.terminals.read(cx);
            manager
                .agent_record_terminal(project, agent_id)
                .or_else(|| {
                    agent
                        .cli_session_id
                        .as_deref()
                        .and_then(|session_id| manager.agent_session_terminal(project, session_id))
                })
        };
        if existing.is_some() {
            self.agent_detail_tabs
                .insert(agent_id, AgentDetailTab::Terminal);
            self.set_view_mode(target_mode, cx);
            return true;
        }

        let runtime_cwd = agent.runtime_path().to_path_buf();
        let resume_command = agent.resume_command();
        let connected_context = self.agent_connected_context_extras(&agent, cx);
        let command = resume_command
            .clone()
            .unwrap_or_else(|| agent.start_command_with_connected_context(&connected_context));
        let cli_session_id = if resume_command.is_some() {
            agent.cli_session_id.clone()
        } else {
            None
        };
        let spawned = self.terminals.update(cx, |manager, cx| {
            manager.spawn_agent_record(
                project,
                runtime_cwd,
                agent.id,
                agent.provider,
                agent.title.clone(),
                command,
                cli_session_id.clone(),
                cx,
            )
        });
        match spawned {
            Ok(_) => {
                self.agent_start_errors.remove(&agent_id);
                self.agents.update(cx, |agents, cx| {
                    agents.mark_started(agent_id, cli_session_id, cx)
                });
                self.agent_detail_tabs
                    .insert(agent_id, AgentDetailTab::Terminal);
                self.set_view_mode(target_mode, cx);
                true
            }
            Err(error) => {
                self.agent_start_errors
                    .insert(agent_id, format!("failed to spawn terminal: {error:#}"));
                eprintln!("failed to spawn terminal: {error:#}");
                cx.notify();
                false
            }
        }
    }
}

#[cfg(test)]
mod rejoin_dialog_tests {
    use super::choose_rejoin_target;

    fn branches() -> Vec<String> {
        ["develop", "main", "release"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn resolved_conflict_target_stays_selected() {
        assert_eq!(
            choose_rejoin_target(&branches(), Some("release"), Some("main"), Some("develop"),),
            "release"
        );
    }

    #[test]
    fn missing_preferred_target_falls_back_to_solo_base() {
        assert_eq!(
            choose_rejoin_target(&branches(), Some("deleted"), Some("main"), Some("develop"),),
            "main"
        );
    }
}
