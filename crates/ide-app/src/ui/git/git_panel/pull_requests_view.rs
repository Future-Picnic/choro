use super::*;

impl GitPanel {
    pub(super) fn render_pull_requests(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let prs = self.repo_prs.clone();
        if self.repo_prs_fetching && prs.is_empty() {
            return v_flex()
                .id("git-prs-loading")
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(Spinner::new().small())
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Loading pull requests"),
                )
                .into_any_element();
        }

        if let Some(error) = self.repo_prs_error.clone() {
            return v_flex()
                .id("git-prs-error")
                .flex_1()
                .p_3()
                .child(
                    v_flex()
                        .gap_2()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.3))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .p_3()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::rose(cx))
                        .child("Could not load pull requests")
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child(SharedString::from(error)),
                        ),
                )
                .into_any_element();
        }

        if prs.is_empty() {
            return v_flex()
                .id("git-prs-empty")
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(pr_icon(crate::ui::design::t3(cx)))
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("No pull requests"),
                )
                .into_any_element();
        }

        v_flex()
            .id("git-prs-list")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .py_1()
            .children(prs.into_iter().map(|pr| {
                let (status_label, accent) = pull_request_status_style(&pr, cx);
                let url = pr.url.clone();
                let title: SharedString = pr.title.clone().into();
                let meta: SharedString =
                    format!("{} · #{} · {}", status_label, pr.number, pr.branch).into();

                h_flex()
                    .id(("git-pr-row", pr.number as usize))
                    .w_full()
                    .px_2()
                    .py_1()
                    .child(
                        h_flex()
                            .id(("git-pr-row-action", pr.number as usize))
                            .w_full()
                            .min_w(px(0.))
                            .gap_2()
                            .items_center()
                            .rounded(crate::ui::design::r_md())
                            .border_1()
                            .border_color(crate::ui::design::line(cx))
                            .px_2()
                            .py_2()
                            .cursor_pointer()
                            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                            .on_click(move |_, _, _| open_url(&url))
                            .child(pr_icon(accent))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t1(cx))
                                            .truncate()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .truncate()
                                            .child(meta),
                                    ),
                            )
                            .child(
                                gpui_component::Icon::new(IconName::ExternalLink)
                                    .size(crate::ui::design::icon_md())
                                    .text_color(crate::ui::design::t3(cx)),
                            ),
                    )
            }))
            .into_any_element()
    }
}
