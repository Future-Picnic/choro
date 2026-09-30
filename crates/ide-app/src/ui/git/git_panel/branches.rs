use super::*;

impl GitPanel {
    /// Branch picker popup, anchored above the commit area: scrollable list
    /// with two-line rows, create-branch entry, and the filter input at the
    /// bottom.
    pub(super) fn render_branch_list(
        &self,
        git: Entity<GitState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.branch_query.read(cx).value().trim().to_string();
        let mut all_branches = git
            .read(cx)
            .snapshot
            .as_ref()
            .map(|s| s.branches.clone())
            .unwrap_or_default();
        // Solo branches belong to their agents, not the picker: a live lane's
        // branch can't be checked out here anyway (git refuses), and a
        // packed-up one must not be hijacked out from under its Solo.
        all_branches.retain(|branch| !branch.name.starts_with("solo/"));
        let create_name = branch_name_candidate(&query)
            .filter(|name| !all_branches.iter().any(|branch| branch.name == *name));
        let normalized_query = slug_branch_name(&query);
        let mut branches = all_branches;
        if !query.is_empty() {
            let needle = query.to_lowercase();
            branches.retain(|branch| {
                let name = branch.name.to_lowercase();
                name.contains(&needle)
                    || normalized_query
                        .as_ref()
                        .is_some_and(|normalized| name.contains(normalized))
            });
        }
        branches.sort_by(|a, b| {
            (!a.is_head, a.is_remote)
                .cmp(&(!b.is_head, b.is_remote))
                .then_with(|| b.tip_time.cmp(&a.tip_time))
                .then_with(|| a.name.cmp(&b.name))
        });

        v_flex()
            .w_full()
            .min_w(px(0.))
            .mb_1()
            .gap_0p5()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("branch-popup-scroll")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .when(branches.is_empty() && create_name.is_none(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No matching branches"),
                        )
                    })
                    .when_some(create_name, |list, name| {
                        let create_git = git.clone();
                        let label: SharedString = format!("＋ Create branch “{name}”").into();
                        list.child(
                            h_flex()
                                .id("create-branch-row")
                                .w_full()
                                .px_2()
                                .py_1()
                                .gap_2()
                                .items_center()
                                .rounded(crate::ui::design::r_xs())
                                .cursor_pointer()
                                .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    create_git
                                        .update(cx, |git, cx| git.create_branch(name.clone(), cx));
                                    this.branch_query
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    this.branches_expanded = false;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::accent(cx))
                                        .truncate()
                                        .child(label),
                                ),
                        )
                    })
                    .children(branches.into_iter().enumerate().map(|(ix, branch)| {
                        let name: SharedString = branch.name.clone().into();
                        let checkout_git = git.clone();
                        let checkout_name = branch.name.clone();
                        let ahead_behind: Option<SharedString> =
                            if branch.ahead > 0 || branch.behind > 0 {
                                Some(format!("↑{} ↓{}", branch.ahead, branch.behind).into())
                            } else {
                                None
                            };
                        // Second line like Zed's picker: author · when · commit summary.
                        let detail: SharedString = {
                            let mut parts: Vec<String> = Vec::new();
                            if !branch.tip_author.is_empty() {
                                parts.push(branch.tip_author.clone());
                            }
                            let when = relative_time(branch.tip_time);
                            if !when.is_empty() {
                                parts.push(when);
                            }
                            if !branch.tip_summary.is_empty() {
                                parts.push(branch.tip_summary.clone());
                            }
                            parts.join(" · ").into()
                        };
                        h_flex()
                            .id(("branch-row", ix))
                            .w_full()
                            .px_2()
                            .py_0p5()
                            .gap_2()
                            .items_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .when(branch.is_head, |row| {
                                row.bg(crate::ui::design::surface_2(cx))
                            })
                            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !branch.is_head {
                                    checkout_git.update(cx, |git, cx| {
                                        git.checkout(checkout_name.clone(), cx)
                                    });
                                }
                                this.branches_expanded = false;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if branch.is_head {
                                        gpui_component::Icon::new(IconName::Check)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::accent(cx))
                                    } else if branch.is_remote {
                                        gpui_component::Icon::new(IconName::Globe)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t3(cx))
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
                                            .child(name),
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
                            .when_some(ahead_behind, |row, label| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(label),
                                )
                            })
                    })),
            )
            // Filter input lives at the bottom of the popup, next to the
            // branch button that opened it.
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
                            .child(Input::new(&self.branch_query)),
                    )
                    .child(
                        gpui_component::Icon::new(IconName::SortDescending)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }
}
