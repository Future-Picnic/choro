use super::*;

const LIMIT_MIN: usize = 1;

struct LimitRow {
    label: &'static str,
    help: &'static str,
    value: usize,
    maximum: usize,
}

impl SettingsView {
    /// The top of the Band page: the delegation switch, then — while it is on —
    /// how much delegated work may run at once. On unless the user turned it off.
    pub(super) fn render_delegation_settings(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.workspace.read(cx).beta_features.delegation;
        let available = ide_core::delegation::enabled();
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .p_4()
                    .items_center()
                    .rounded(design::r_md())
                    .border_1()
                    .border_color(design::line(cx))
                    .bg(design::surface(cx).opacity(0.55))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Delegation"),
                            )
                            .child(
                                div()
                                    .whitespace_normal()
                                    .text_size(design::text_ui())
                                    .text_color(design::t3(cx))
                                    .child("Let your lead assign work to Bandmates through /delegate or a request in chat."),
                            )
                            .child(
                                div()
                                    .whitespace_normal()
                                    .text_size(design::text_label())
                                    .text_color(design::t4(cx))
                                    .child(if available {
                                        "On by default. Existing delegated work can finish when this is off."
                                    } else {
                                        "Delegation is unavailable in this app session."
                                    }),
                            ),
                    )
                    .child(
                        style::settings_option_button(
                            "band-delegation-toggle",
                            if enabled { "On" } else { "Off" },
                            enabled,
                        )
                        .icon(if enabled { IconName::Check } else { IconName::Minus })
                        .disabled(!available)
                        .tooltip(if enabled { "Turn delegation off" } else { "Turn delegation on" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let result = this.workspace.update(cx, |workspace, cx| {
                                let mut features = workspace.beta_features;
                                features.delegation = !enabled;
                                workspace.set_beta_features(features, cx)
                            });
                            this.delegation_error = result.err().map(|error| error.to_string());
                            cx.notify();
                        })),
                    ),
            )
            .when_some(self.delegation_error.clone(), |section, error| {
                section.child(style::expert_settings_error_notice(error, cx))
            })
            .when(enabled && available, |section| {
                section.child(self.render_delegation_limits(cx))
            })
    }

    fn render_delegation_limits(&self, cx: &mut Context<Self>) -> Div {
        let limits = LocalStore::open_default()
            .and_then(|s| s.delegation_limits())
            .unwrap_or_default();
        let rows = [
            LimitRow {
                label: "Bandmates per task",
                help: "Bandmates one lead can run at the same time",
                value: limits.concurrent_per_run,
                maximum: 6,
            },
            LimitRow {
                label: "Bandmates across Choro",
                help: "Bandmates running at once across every lead",
                value: limits.concurrent_global,
                maximum: 12,
            },
            LimitRow {
                label: "Work revisions",
                help: "Rounds of changes a delegated task may take",
                value: limits.work_revisions,
                maximum: 10,
            },
        ];
        let mut frame = style::expert_settings_list_frame(cx);
        for (index, row) in rows.into_iter().enumerate() {
            frame = frame.child(self.render_limit_row(index, row, cx));
        }
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_2()
            .pt_2()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(design::text_head())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(design::t1(cx))
                            .child("Delegation limits"),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child("How much delegated work can run at once."),
                    ),
            )
            .child(frame)
    }

    fn render_limit_row(
        &self,
        index: usize,
        row: LimitRow,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let at_min = row.value <= LIMIT_MIN;
        let at_max = row.value >= row.maximum;
        let value = row.value;
        style::expert_settings_list_row(("expert-limit-row", index), cx)
            .min_h(px(44.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        div()
                            .truncate()
                            .text_size(design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(design::t1(cx))
                            .child(row.label),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child(row.help),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .child(
                        style::expert_settings_stepper_button(
                            ("expert-limit-dec", index),
                            IconName::Minus,
                            cx,
                        )
                        .disabled(at_min)
                        .tooltip(if at_min {
                            "Already at the minimum"
                        } else {
                            "Lower this limit"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_delegation_limit(index, value.saturating_sub(1), cx)
                        })),
                    )
                    .child(
                        div()
                            .w(px(28.))
                            .text_center()
                            .text_size(design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(design::t1(cx))
                            .child(value.to_string()),
                    )
                    .child(
                        style::expert_settings_stepper_button(
                            ("expert-limit-inc", index),
                            IconName::Plus,
                            cx,
                        )
                        .disabled(at_max)
                        .tooltip(if at_max {
                            format!("Maximum is {}", row.maximum)
                        } else {
                            "Raise this limit".to_string()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_delegation_limit(index, value.saturating_add(1), cx)
                        })),
                    ),
            )
    }

    fn set_delegation_limit(&mut self, index: usize, value: usize, cx: &mut Context<Self>) {
        let result = LocalStore::open_default().and_then(|s| {
            let current = s.delegation_limits()?;
            let limits = match index {
                0 => ide_core::delegation::DelegationLimits {
                    concurrent_per_run: value,
                    ..current
                },
                1 => ide_core::delegation::DelegationLimits {
                    concurrent_global: value,
                    ..current
                },
                _ => ide_core::delegation::DelegationLimits {
                    work_revisions: value,
                    ..current
                },
            };
            s.save_delegation_limits(&limits)
        });
        self.experts_status = result.err().map(|e| ExpertsNotice::error(e.to_string()));
        cx.notify();
    }
}
