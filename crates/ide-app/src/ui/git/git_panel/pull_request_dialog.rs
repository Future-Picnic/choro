use super::*;

fn can_create_pull_request(has_url: bool, base_branch: &str, generating: bool) -> bool {
    has_url && !base_branch.trim().is_empty() && !generating
}

pub(super) struct GitConfirmation {
    pub(super) title: &'static str,
    pub(super) description: &'static str,
    pub(super) detail: SharedString,
    pub(super) confirm_id: &'static str,
    pub(super) confirm_label: &'static str,
    pub(super) action: fn(&mut GitState, &mut Context<GitState>),
}

pub(super) fn confirm_git_action(
    git: Entity<GitState>,
    window: &mut Window,
    cx: &mut App,
    confirmation: GitConfirmation,
) {
    let GitConfirmation {
        title,
        description,
        detail,
        confirm_id,
        confirm_label,
        action,
    } = confirmation;
    ConfirmDialog::new(title, description)
        .detail(detail)
        .confirm_label(confirm_label)
        .confirm_id(confirm_id)
        .width(520.0)
        .on_confirm(move |_, cx| {
            git.update(cx, action);
        })
        .open(window, cx);
}

impl PullRequestDialog {
    fn create_pull_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let base_branch = self.base_branch.trim().to_string();
        if base_branch.is_empty() {
            self.error = Some("Choose a base branch for the pull request".into());
            cx.notify();
            return;
        }
        let repo = self.git.read(cx).repo_path.clone();
        let generation_agent = self.generation_agent.clone();
        let branch = self.notice.branch.clone();
        let Some(url) = github_pull_request_url(&repo, &branch, &base_branch) else {
            self.error = Some("GitHub origin remote not found".into());
            cx.notify();
            return;
        };
        self.error = None;
        if !self.ai_enabled {
            window.close_dialog(cx);
            open_url(&url);
            return;
        }
        if self.generating {
            return;
        }
        self.generating = true;
        cx.notify();

        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    generate_pull_request(&generation_agent, &repo, &branch, &base_branch)
                })
                .await;

            this.update(cx, |dialog, cx| {
                dialog.generating = false;
                match result {
                    Ok(pull_request) => {
                        notifications::play_generated_sound();
                        let url = pull_request_url_with_text(&url, &pull_request);
                        window_handle
                            .update(cx, |_, window, cx| {
                                window.close_dialog(cx);
                                open_url(&url);
                            })
                            .ok();
                    }
                    Err(error) => {
                        dialog.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl PullRequestDialog {
    /// Base-branch picker cloned from the Ship dialog's selector: a bordered
    /// trigger with the branch icon and left-aligned name that expands an
    /// inline, searchable branch list. Select-only.
    fn render_base_branch_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.base_branch_expanded;
        let has_selection = !self.base_branch.trim().is_empty();
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .id("create-pr-base-branch")
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
                        this.base_branch_expanded = !this.base_branch_expanded;
                        if this.base_branch_expanded {
                            this.base_branch_query
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
                            .text_color(if has_selection {
                                crate::ui::design::t1(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .child(if has_selection {
                                SharedString::from(self.base_branch.clone())
                            } else {
                                SharedString::from("Choose a target branch")
                            }),
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
            .when(expanded, |col| col.child(self.render_base_branch_popup(cx)))
    }

    /// The expanded list: candidate branches with `author · time · summary`
    /// detail from the git snapshot, filtered by the search field at the
    /// bottom — the Ship dialog's popup, verbatim.
    fn render_base_branch_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let needle = self
            .base_branch_query
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let snapshot_branches = self
            .git
            .read(cx)
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.branches.clone())
            .unwrap_or_default();
        let detail_for = |name: &str| -> SharedString {
            let info = snapshot_branches
                .iter()
                .find(|branch| branch.name == name)
                .or_else(|| {
                    let remote = format!("origin/{name}");
                    snapshot_branches
                        .iter()
                        .find(|branch| branch.name == remote)
                });
            match info {
                Some(info) => {
                    let mut parts: Vec<String> = Vec::new();
                    if !info.tip_author.is_empty() {
                        parts.push(info.tip_author.clone());
                    }
                    let when = relative_time(info.tip_time);
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

        let selected = self.base_branch.clone();
        let mut rows: Vec<(String, SharedString, bool)> = self
            .base_branch_options
            .iter()
            .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|name| (name.clone(), detail_for(name), *name == selected))
            .collect();
        rows.sort_by_key(|(name, _, _)| crate::ui::branch_order::branch_priority(name, false));

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
                    .id("create-pr-base-branch-scroll")
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
                                .child(if needle.is_empty() {
                                    "No target branches available"
                                } else {
                                    "No matching branches"
                                }),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(
                        |(ix, (name, detail, is_selected))| {
                            let value = name.clone();
                            let label: SharedString = name.into();
                            h_flex()
                                .id(("create-pr-base-branch-row", ix))
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
                                    this.base_branch = value.clone();
                                    this.base_branch_expanded = false;
                                    this.error = None;
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
                                                .font_weight(FontWeight::SEMIBOLD)
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
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.base_branch_query)),
                    )
                    .child(
                        gpui_component::Icon::new(IconName::Search)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }
}

impl Render for PullRequestDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = can_create_pull_request(
            self.notice.url.is_some(),
            &self.base_branch,
            self.generating,
        );
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .child(pr_icon(crate::ui::design::accent(cx))),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .truncate()
                                    .child(self.notice.message.clone()),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Create a pull request from the published branch."),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Target branch"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Choose the branch this pull request should merge into."),
                    )
                    .child(self.render_base_branch_selector(cx)),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.45))
                    .bg(crate::ui::design::surface(cx))
                    .px_3()
                    .py_2()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Generate PR text with AI"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(format!(
                                        "Uses {} to fill the GitHub title and body.",
                                        self.generation_agent.provider.label()
                                    )),
                            ),
                    )
                    .child(
                        Toggle::new("generate-pr-text-with-ai")
                            .outline()
                            .small()
                            .checked(self.ai_enabled)
                            .label(if self.ai_enabled { "On" } else { "Off" })
                            .disabled(self.generating)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.ai_enabled = *checked;
                                this.error = None;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.generating, |dialog| {
                dialog.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(Spinner::new().xsmall())
                        .child("Generating pull request text…"),
                )
            })
            .when_some(self.error.clone(), |dialog, error| {
                dialog.child(
                    div()
                        .w_full()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.45))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(SharedString::from(error)),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        crate::ui::style::dialog_neutral_button("cancel-create-pr", "Cancel", cx)
                            .disabled(self.generating)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::ui::style::primary_button_compact(
                            "confirm-create-pr",
                            if self.generating {
                                "Generating…"
                            } else {
                                "Create pull request"
                            },
                            cx,
                        )
                        .disabled(!can_create)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_pull_request(window, cx);
                        })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::can_create_pull_request;

    #[test]
    fn creating_a_pull_request_requires_an_explicit_target_branch() {
        assert!(!can_create_pull_request(true, "", false));
        assert!(!can_create_pull_request(true, "   ", false));
        assert!(can_create_pull_request(true, "release", false));
        assert!(!can_create_pull_request(false, "release", false));
        assert!(!can_create_pull_request(true, "release", true));
    }
}
