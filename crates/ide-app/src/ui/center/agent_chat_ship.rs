use super::*;

impl CenterArea {
    pub(super) fn render_ship_result_card(
        &self,
        agent: &AgentRecord,
        result: &crate::state::agent_chat::ShipResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let short_sha = if result.commit_sha.len() > 7 {
            &result.commit_sha[..7]
        } else {
            result.commit_sha.as_str()
        };
        let repo_path = self.ship_result_repository_path(agent, result, cx);
        let repository = result.repository.clone().or_else(|| {
            if agent.repository_path.is_some() {
                return None;
            }
            let repositories = self.git_states.read(cx).repositories(agent.project_id);
            if repositories.len() <= 1 {
                return None;
            }
            let repo_path = repo_path.as_ref()?;
            Some(
                repo_path
                    .strip_prefix(&agent.project_path)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        repo_path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| repo_path.display().to_string())
                    }),
            )
        });
        let is_onboarding_demo = result
            .pr_url
            .as_deref()
            .is_some_and(|url| url.starts_with("onboarding-demo://"));
        let pr_url = result.pr_url.clone().filter(|url| url.contains("/pull/"));
        let pr_title = result
            .pr_title
            .clone()
            .filter(|title| !title.trim().is_empty());
        let pr_body = result
            .pr_body
            .clone()
            .filter(|body| !body.trim().is_empty());
        let tracked_pr = self
            .agent_ship_prs
            .get(&agent.id)
            .filter(|pr| pr.branch == result.branch);
        let pr_base_branch = result
            .pr_base_branch
            .clone()
            .filter(|branch| !branch.trim().is_empty())
            .or_else(|| {
                tracked_pr
                    .map(|pr| pr.base_branch.clone())
                    .filter(|branch| !branch.trim().is_empty())
            });
        let can_merge =
            tracked_pr.is_none_or(|pr| pr.state.eq_ignore_ascii_case("OPEN") && !pr.is_draft);
        let merge_git = repo_path.as_ref().and_then(|path| {
            self.git_states
                .read(cx)
                .get_for_path(agent.project_id, path)
        });
        let merge_busy = merge_git.as_ref().is_some_and(|git| git.read(cx).is_busy);
        let card_title = if pr_url.is_some() {
            "Shipped"
        } else if result.action.to_ascii_lowercase().contains("push") {
            "Commit pushed"
        } else {
            "Committed"
        };
        let agent_id = agent.id;
        let task_done = agent.status == AgentStatus::Done;
        crate::ui::style::chat_card(cx)
            .relative()
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ShipResult,
                cx,
            ))
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(
                        gpui_component::Icon::new(IconName::GitHub)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t2(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::sage(cx))
                            .child(card_title),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t4(cx))
                            .child(SharedString::from(result.action.clone())),
                    )
                    .child(div().flex_1())
                    .when_some(pr_url.clone().filter(|_| !is_onboarding_demo), |row, url| {
                        row.child(
                            crate::ui::style::ghost_button_compact(
                                ("agent-chat-ship-open-pr", stable_text_key(&url)),
                                "Open PR",
                            )
                            .text_color(crate::ui::design::t2(cx))
                            .on_click(move |_, _, _| {
                                crate::ui::git::git_panel::open_url(&url);
                            }),
                        )
                    })
                    .when_some(
                        pr_url
                            .clone()
                            .filter(|_| !is_onboarding_demo && can_merge)
                            .zip(pr_base_branch.clone())
                            .zip(repo_path.clone())
                            .zip(merge_git.clone()),
                        |row, (((_, base_branch), repo_path), git)| {
                            let branch = result.branch.clone();
                            let pr_number = tracked_pr.map(|pr| pr.number);
                            let pr_title = pr_title.clone();
                            row.child(
                                crate::ui::style::accent_button_compact(
                                    (
                                        "agent-chat-ship-merge-pr",
                                        stable_text_key(&result.id),
                                    ),
                                    "Merge PR",
                                    cx,
                                )
                                .disabled(merge_busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_agent_ship_pr_merge(
                                        agent_id,
                                        repo_path.clone(),
                                        git.clone(),
                                        branch.clone(),
                                        base_branch.clone(),
                                        pr_number,
                                        pr_title.clone(),
                                        window,
                                        cx,
                                    );
                                })),
                            )
                        },
                    )
                    .child(
                        Button::new(("agent-chat-ship-mark-done", agent_id.as_u128() as u64))
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_xs())
                            .px(crate::ui::design::split_primary_pad_x())
                            .icon(
                                gpui_component::Icon::new(IconName::Check)
                                    .text_color(crate::ui::design::sage(cx)),
                            )
                            .label(if task_done { "Done" } else { "Mark done" })
                            .custom(crate::ui::style::chat_card_action_variant(cx))
                            .disabled(task_done)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agents.update(cx, |agents, cx| {
                                    agents.update_status(agent_id, AgentStatus::Done, cx);
                                });
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_center()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(crate::ui::design::chat_card_body_pad_y())
                    .when_some(repository, |row, repository| {
                        row.child(
                            v_flex()
                                .gap_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Repository"),
                                )
                                .child(
                                    h_flex()
                                        .min_w(px(0.))
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            crate::ui::design::indicator::lucide_icon(
                                                lucide_icons::Icon::FolderGit2,
                                                crate::ui::design::sky(cx).opacity(0.78),
                                                crate::ui::design::icon_sm(),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .min_w(px(0.))
                                                .truncate()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t1(cx))
                                                .child(repository),
                                        ),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(if pr_base_branch.is_some() {
                                        "Branches"
                                    } else {
                                        "Branch"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .truncate()
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(
                                        pr_base_branch
                                            .as_ref()
                                            .map(|base| format!("{} → {base}", result.branch))
                                            .unwrap_or_else(|| result.branch.clone()),
                                    )),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Commit"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(short_sha.to_string())),
                            ),
                    ),
            )
            .when(pr_title.is_some() || pr_body.is_some(), |card| {
                card.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .px_3()
                        .pb_3()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Pull request"),
                        )
                        .when_some(pr_title.clone(), |section, title| {
                            section.child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .line_height(gpui::relative(1.35))
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(title)),
                            )
                        })
                        .when_some(pr_body.clone(), |section, body| {
                            section.child(
                                div()
                                    .w_full()
                                    .rounded(px(crate::ui::style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::design::base(cx).opacity(0.28))
                                    .p_3()
                                    .child(render_plan_markdown(&body, window, cx)),
                            )
                        }),
                )
            })
            .when(is_onboarding_demo, |card| {
                card.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .pb_3()
                        .child(
                            Icon::new(IconName::Info)
                                .size(crate::ui::design::icon())
                                .text_color(crate::ui::design::accent(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(crate::ui::design::text_ui())
                                .line_height(gpui::relative(1.4))
                                .text_color(crate::ui::design::t3(cx))
                                .child("Playground simulation — this local onboarding project is not connected to GitHub, so no remote branch was pushed and no real PR was opened."),
                        ),
                )
            })
            .when_some(
                self.render_ship_task_actions(agent, result, cx),
                |card, section| card.child(section),
            )
            .into_any_element()
    }

    fn ship_result_repository_path(
        &self,
        agent: &AgentRecord,
        result: &crate::state::agent_chat::ShipResult,
        cx: &App,
    ) -> Option<PathBuf> {
        let repositories = self.git_states.read(cx).repositories(agent.project_id);
        if let Some(selected) = agent.repository_path.as_ref() {
            if repositories
                .iter()
                .any(|git| git.read(cx).repo_path == *selected)
            {
                return Some(selected.clone());
            }
        }

        if let Some(label) = result.repository.as_deref() {
            let mut matches = repositories.iter().filter_map(|git| {
                let path = git.read(cx).repo_path.clone();
                let relative = path
                    .strip_prefix(&agent.project_path)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.to_string_lossy());
                let file_name = path.file_name().map(|name| name.to_string_lossy());
                (relative.as_deref() == Some(label)
                    || file_name.as_deref() == Some(label)
                    || path.to_string_lossy() == label)
                    .then_some(path)
            });
            let matched = matches.next();
            if matched.is_some() && matches.next().is_none() {
                return matched;
            }
        }

        if let Some((tracked_repo, tracked_branch)) = self.agent_ship_pr_targets.get(&agent.id) {
            if tracked_branch == &result.branch
                && repositories
                    .iter()
                    .any(|git| git.read(cx).repo_path == *tracked_repo)
            {
                return Some(tracked_repo.clone());
            }
        }

        if repositories.len() == 1 {
            return repositories
                .first()
                .map(|git| git.read(cx).repo_path.clone());
        }

        let mut matches = repositories.into_iter().filter_map(|git| {
            let git = git.read(cx);
            git.history
                .iter()
                .any(|commit| {
                    commit.sha == result.commit_sha
                        || commit.sha.starts_with(&result.commit_sha)
                        || result.commit_sha.starts_with(&commit.sha_short)
                })
                .then(|| git.repo_path.clone())
        });
        let matched = matches.next();
        if matched.is_some() && matches.next().is_none() {
            matched
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn confirm_agent_ship_pr_merge(
        &mut self,
        agent_id: Uuid,
        repo_path: PathBuf,
        git: Entity<GitState>,
        branch: String,
        base_branch: String,
        pr_number: Option<u64>,
        pr_title: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if git.read(cx).is_busy {
            window.push_notification(
                Notification::info("Finish the current Git operation before merging"),
                cx,
            );
            return;
        }
        let title = pr_number
            .map(|number| format!("Merge pull request #{number}?"))
            .unwrap_or_else(|| "Merge pull request?".to_string());
        let message = pr_title
            .filter(|title| !title.trim().is_empty())
            .map(|title| {
                format!(
                    "“{title}” will be merged now if its required reviews, checks, and branch protections allow it."
                )
            })
            .unwrap_or_else(|| {
                "Choro will attempt the merge now. Required reviews, checks, and branch protections still apply."
                    .to_string()
            });
        let center = cx.entity();
        ConfirmDialog::new(title, message)
            .tone(ConfirmTone::Primary)
            .icon(IconName::GitHub)
            .branch_route(branch.clone(), base_branch.clone())
            .checkbox(
                "Delete GitHub branch after merge",
                "Delete the source branch on GitHub only. Your local branch, files, and history are kept.",
            )
            .confirm_label("Merge PR")
            .confirm_id("confirm-merge-agent-ship-pr")
            .on_confirm_with_checkbox(move |delete_branch, window, cx| {
                let window_handle = window.window_handle();
                let merge_git = git.clone();
                let repo_path = repo_path.clone();
                let branch = branch.clone();
                let base_branch = base_branch.clone();
                center.update(cx, |_center, cx| {
                    if merge_git.read(cx).is_busy {
                        window.push_notification(
                            Notification::info("Finish the current Git operation before merging"),
                            cx,
                        );
                        return;
                    }
                    merge_git.update(cx, |git, cx| {
                        git.is_busy = true;
                        git.last_error = None;
                        git.last_error_from_refresh = false;
                        git.last_message = Some("Merging pull request…".into());
                        cx.notify();
                    });

                    cx.spawn(async move |this, cx| {
                        let result = cx
                            .background_executor()
                            .spawn(async move {
                                crate::ui::git::git_panel::merge_pull_request_with_branch_cleanup(
                                    &repo_path,
                                    &branch,
                                    Some(&base_branch),
                                    None,
                                    delete_branch,
                                )
                            })
                            .await;
                        merge_git
                            .update(cx, |git, cx| {
                                git.is_busy = false;
                                match &result {
                                    Ok(outcome) => {
                                        git.last_message = Some(outcome.message());
                                        git.last_error = None;
                                    }
                                    Err(error) => {
                                        git.last_message = None;
                                        git.last_error = Some(format!("{error:#}"));
                                        git.last_error_from_refresh = false;
                                    }
                                }
                                git.refresh(cx);
                            })
                            .ok();
                        this.update(cx, |center, cx| {
                            center.agent_ship_pr_checked_at.remove(&agent_id);
                            if let (Some(cached), Ok(outcome)) =
                                (center.agent_ship_prs.get_mut(&agent_id), &result)
                            {
                                if cached.number == outcome.number {
                                    cached.state = "MERGED".into();
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                        let notification = match &result {
                            Ok(outcome) => outcome.notification(),
                            Err(error) => Notification::error(format!("{error:#}")),
                        };
                        window_handle
                            .update(cx, |_, window, cx| {
                                window.push_notification(notification, cx)
                            })
                            .ok();
                    })
                    .detach();
                });
            })
            .open(window, cx);
    }

    /// The opt-in "update the task" section shown on the ship card when the
    /// agent's chat is linked to a task. Renders the pending controls, or a
    /// static confirmation once the user has applied (or skipped) them.
    pub(super) fn render_ship_task_actions(
        &self,
        agent: &AgentRecord,
        result: &crate::state::agent_chat::ShipResult,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let task = result.task.clone()?;
        if task.provider == ide_core::IssueTrackerProvider::PocketComet {
            return None;
        }
        let pr_url = result.pr_url.clone()?;
        if pr_url.starts_with("onboarding-demo://") {
            return None;
        }
        let seed = super::agent_panel::ShipTaskSeed {
            id: result.id.clone(),
            agent_id: agent.id,
            project: agent.project_id,
            task: task.clone(),
            comment_body: super::agent_panel::ship_task_comment_body(&result.pr_title, &pr_url),
            suggested_status: result.suggested_status.clone(),
        };
        let key = stable_text_key(&result.id);

        if let Some(applied) = &result.applied {
            return Some(self.render_ship_task_resolved(&task, applied, cx));
        }

        let ui = self.ship_task_ui(&result.id);
        let comment_enabled = ui.map(|state| state.comment_enabled).unwrap_or(true);
        let selected_status = match ui {
            Some(state) => state.selected_status.clone(),
            None => result.suggested_status.clone(),
        };
        let statuses_loading = ui.map(|state| state.statuses_loading).unwrap_or(false);
        let applying = ui.map(|state| state.applying).unwrap_or(false);
        let error = ui.and_then(|state| state.error.clone());

        let is_asana = task.provider == ide_core::IssueTrackerProvider::Asana;
        let status_names: Vec<String> = if is_asana {
            Vec::new()
        } else if task.provider == ide_core::IssueTrackerProvider::Personal {
            ide_core::PersonalTaskStatus::ALL
                .iter()
                .map(|status| status.label().to_string())
                .collect()
        } else {
            let from_ui = ui
                .map(|state| {
                    state
                        .statuses
                        .iter()
                        .map(|option| option.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !from_ui.is_empty() {
                from_ui
            } else {
                self.tasks
                    .read(cx)
                    .board(agent.project_id)
                    .filter(|board| board.provider == task.provider)
                    .map(|board| {
                        board
                            .columns
                            .iter()
                            .map(|column| column.name.clone())
                            .collect::<Vec<_>>()
                    })
                    .filter(|names| !names.is_empty())
                    .unwrap_or_else(|| result.suggested_status.clone().into_iter().collect())
            }
        };
        let show_status_row = !is_asana && (!status_names.is_empty() || statuses_loading);
        let can_apply = !applying && (comment_enabled || selected_status.is_some());

        let muted = crate::ui::design::t3(cx);
        let accent = crate::ui::design::accent(cx);

        let comment_seed = seed.clone();
        let comment_row = h_flex()
            .w_full()
            .gap_2p5()
            .items_start()
            .child(
                crate::ui::style::checkbox(("ship-task-comment", key), comment_enabled, cx)
                    .mt(px(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_ship_task_comment(&comment_seed, cx);
                    })),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::style::focus_text(cx))
                            .child("Add a comment to the task"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(muted)
                            .line_height(gpui::relative(1.4))
                            .child(SharedString::from(seed.comment_body.clone())),
                    ),
            );

        let status_row = show_status_row.then(|| {
            let mut pills: Vec<gpui::AnyElement> = Vec::new();
            pills.push(self.render_ship_status_pill(
                &seed,
                key,
                0,
                None,
                selected_status.is_none(),
                false,
                cx,
            ));
            for (index, name) in status_names.iter().enumerate() {
                let selected = selected_status.as_deref() == Some(name.as_str());
                let suggested = result.suggested_status.as_deref() == Some(name.as_str());
                pills.push(self.render_ship_status_pill(
                    &seed,
                    key,
                    index + 1,
                    Some(name.clone()),
                    selected,
                    suggested,
                    cx,
                ));
            }
            v_flex()
                .w_full()
                .gap_1p5()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::style::focus_text(cx))
                        .child(if statuses_loading {
                            SharedString::from("Move status to… (loading options)")
                        } else {
                            SharedString::from("Move status to")
                        }),
                )
                .child(h_flex().w_full().flex_wrap().gap_1p5().children(pills))
        });

        let apply_seed = seed.clone();
        let skip_seed = seed.clone();
        let buttons = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(div().flex_1())
            .child(
                crate::ui::style::ghost_button_compact(("ship-task-skip", key), "Skip").on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.skip_ship_task_actions(&skip_seed, cx);
                    }),
                ),
            )
            .child(
                Button::new(("ship-task-apply", key))
                    .small()
                    .compact()
                    .h(crate::ui::design::control_h())
                    .px_3()
                    .icon(IconName::Check)
                    .label(if applying {
                        "Applying…"
                    } else {
                        "Update task"
                    })
                    .custom(crate::ui::style::success_variant(cx))
                    .disabled(!can_apply)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_ship_task_actions(&apply_seed, cx);
                    })),
            );

        Some(
            v_flex()
                .w_full()
                .gap_3()
                .px_3()
                .py_3()
                .border_t_1()
                .border_color(crate::ui::design::line(cx))
                .child(
                    h_flex()
                        .w_full()
                        .gap_1p5()
                        .items_center()
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .size(crate::ui::design::icon())
                                .text_color(accent),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::style::focus_text(cx))
                                .child(SharedString::from(format!("Update {}", task.issue_key))),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(muted)
                                .child(SharedString::from(task.provider.label().to_string())),
                        ),
                )
                .child(comment_row)
                .when_some(status_row, |col, row| col.child(row))
                .when_some(error, |col, message| {
                    col.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(SharedString::from(message)),
                    )
                })
                .child(buttons)
                .into_any_element(),
        )
    }

    pub(super) fn render_ship_status_pill(
        &self,
        seed: &super::agent_panel::ShipTaskSeed,
        key: u64,
        index: usize,
        label: Option<String>,
        selected: bool,
        suggested: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let text = label.clone().unwrap_or_else(|| "Don't change".to_string());
        let accent = crate::ui::design::accent(cx);
        let seed = seed.clone();
        let value = label;
        h_flex()
            .id((
                "ship-status-pill",
                key.wrapping_add(index as u64 * 1_000_003),
            ))
            .h(crate::ui::design::control_h_sm())
            .items_center()
            .gap_1()
            .px_2p5()
            .rounded_full()
            .border_1()
            .border_color(if selected {
                accent
            } else {
                crate::ui::design::line(cx)
            })
            .bg(if selected {
                accent.opacity(0.14)
            } else {
                crate::ui::design::surface(cx)
            })
            .cursor_pointer()
            .hover(move |pill| pill.border_color(accent.opacity(0.6)))
            .when(selected, |pill| {
                pill.child(
                    Icon::new(IconName::Check)
                        .size(crate::ui::design::icon_sm())
                        .text_color(accent),
                )
            })
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(if selected {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::MEDIUM
                    })
                    .text_color(if selected {
                        crate::ui::style::focus_text(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(SharedString::from(text)),
            )
            .when(suggested && !selected, |pill| {
                pill.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(accent)
                        .child(SharedString::from("· suggested")),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_ship_task_status(&seed, value.clone(), cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_ship_task_resolved(
        &self,
        task: &TaskRef,
        applied: &crate::state::agent_chat::ShipTaskApplied,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let muted = crate::ui::design::t3(cx);
        let mut parts: Vec<String> = Vec::new();
        if applied.commented {
            parts.push(format!("Commented on {}", task.issue_key));
        }
        if let Some(status) = &applied.status_name {
            parts.push(format!("Moved to {status}"));
        }
        let (icon, color, text) = if parts.is_empty() {
            (IconName::Info, muted, "Task update skipped".to_string())
        } else {
            (
                IconName::CircleCheck,
                crate::ui::design::sage(cx),
                parts.join(" · "),
            )
        };
        v_flex()
            .w_full()
            .px_3()
            .py_2p5()
            .border_t_1()
            .border_color(crate::ui::design::line(cx))
            .child(
                h_flex()
                    .w_full()
                    .gap_1p5()
                    .items_center()
                    .child(
                        Icon::new(icon)
                            .size(crate::ui::design::icon())
                            .text_color(color),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(muted)
                            .child(SharedString::from(text)),
                    ),
            )
            .into_any_element()
    }
}
