use super::*;

impl GitPanel {
    /// Commits tab: recent commits; click one to see its diff.
    pub(super) fn render_history(
        &self,
        git: Entity<GitState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let commit_count = git.read(cx).history.len();
        let project = self.workspace.read(cx).active;
        let center = self.center.clone();

        if commit_count == 0 {
            return v_flex()
                .id("git-history-empty")
                .flex_1()
                .min_h(px(0.))
                .child(
                    div()
                        .p_4()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No commits yet"),
                )
                .into_any_element();
        }

        let list_git = git.clone();
        v_flex()
            .id("git-history-wrap")
            .flex_1()
            .size_full()
            .overflow_hidden()
            .child(
                uniform_list(
                    "git-history-scroll",
                    commit_count,
                    move |visible_range, _, cx| {
                        let visible_commits = {
                            let state = list_git.read(cx);
                            visible_range
                                .clone()
                                .filter_map(|ix| state.history.get(ix).cloned().map(|c| (ix, c)))
                                .collect::<Vec<_>>()
                        };

                        let mut rows = Vec::with_capacity(visible_commits.len());
                        for (ix, commit) in visible_commits {
                            let center = center.clone();
                            let sha = commit.sha.clone();
                            let short = commit.sha_short.clone();
                            let detail: SharedString = format!(
                                "{} · {} · {}",
                                commit.sha_short,
                                commit.author,
                                relative_time(commit.time)
                            )
                            .into();

                            rows.push(
                                h_flex()
                                    .id(("commit-row", ix))
                                    .h(px(44.))
                                    .px_2()
                                    .mx_1()
                                    .gap_2()
                                    .items_center()
                                    .rounded(crate::ui::design::r_sm())
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                    .on_click(move |_, _, cx| {
                                        let Some(project) = project else { return };
                                        let kind = crate::ui::git::diff_pane::DiffKind::Commit {
                                            sha: sha.clone(),
                                        };
                                        let title: SharedString = short.clone().into();
                                        center
                                            .update(cx, |center, cx| {
                                                center.open_diff(project, kind, title, cx);
                                            })
                                            .ok();
                                    })
                                    .child(commit_icon(crate::ui::design::t3(cx)))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .gap_0p5()
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_body())
                                                    .truncate()
                                                    .child(SharedString::from(
                                                        commit.summary.clone(),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .truncate()
                                                    .child(detail),
                                            ),
                                    ),
                            );
                        }
                        rows
                    },
                )
                .size_full()
                .py_1(),
            )
            .into_any_element()
    }
}
