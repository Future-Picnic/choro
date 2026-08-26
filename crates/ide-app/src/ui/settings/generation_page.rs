use super::*;

impl SettingsView {
    pub(super) fn render_generation_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let verification_mode = self.workspace.read(cx).verification_mode;
        let review_checklist_mode = self.workspace.read(cx).review_checklist_mode;
        let generation_agent = self.workspace.read(cx).generation_agent.clone();
        let quick_ask_agent = self.workspace.read(cx).quick_ask_agent.clone();
        let quick_ask_history_count = self.quick_ask.read(cx).history().len();
        let agent_defaults = self.workspace.read(cx).new_agent_defaults();
        let code_review_prompt = self.code_review_prompt.clone();
        let code_review_prompt_dirty = self.code_review_prompt_dirty;
        let code_review_prompt_empty = code_review_prompt.read(cx).value().trim().is_empty();
        {
            let verification_buttons = [
                (
                    VerificationMode::Ask,
                    "Ask every time",
                    "Offer verification after eligible work finishes.",
                ),
                (
                    VerificationMode::Automatic,
                    "Automatic",
                    "Start verification immediately without asking.",
                ),
                (
                    VerificationMode::Off,
                    "Off",
                    "Do not offer or start verification automatically.",
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (mode, label, tooltip))| {
                let button = if verification_mode == mode {
                    crate::ui::style::primary_button_compact(
                        ("settings-verification-mode", index),
                        label,
                        cx,
                    )
                } else {
                    crate::ui::style::dialog_neutral_button(
                        ("settings-verification-mode", index),
                        label,
                        cx,
                    )
                };
                button
                    .tooltip(tooltip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_verification_mode(mode, cx);
                        });
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
            let checklist_buttons = [
                (ReviewChecklistMode::Automatic, "Automatic"),
                (ReviewChecklistMode::Off, "Off"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (mode, label))| {
                let button = if review_checklist_mode == mode {
                    crate::ui::style::primary_button_compact(
                        ("settings-review-checklist-mode", index),
                        label,
                        cx,
                    )
                } else {
                    crate::ui::style::dialog_neutral_button(
                        ("settings-review-checklist-mode", index),
                        label,
                        cx,
                    )
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_review_checklist_mode(mode, cx);
                    });
                    cx.notify();
                }))
            })
            .collect::<Vec<_>>();
            let provider_buttons = [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode]
                .into_iter()
                .enumerate()
                .map(|(index, provider)| {
                    let button = if generation_agent.provider == provider {
                        crate::ui::style::primary_button_compact(
                            ("settings-generation-provider", index),
                            provider.label(),
                            cx,
                        )
                    } else {
                        crate::ui::style::dialog_neutral_button(
                            ("settings-generation-provider", index),
                            provider.label(),
                            cx,
                        )
                    };
                    button
                        .icon(crate::ui::center::provider_brand_icon(provider))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_generation_provider(provider, cx);
                        }))
                })
                .collect::<Vec<_>>();
            let model_buttons = if generation_agent.provider == AgentKind::OpenCode {
                vec![(AgentModel::OpenCode, "Big Pickle".to_string())]
            } else {
                AgentModel::models_for(generation_agent.provider)
                    .iter()
                    .map(|model| (*model, model.menu_label().to_string()))
                    .collect::<Vec<_>>()
            }
            .into_iter()
            .enumerate()
            .map(|(index, (model, label))| {
                let selected = generation_agent.model == model;
                let button = if selected {
                    crate::ui::style::primary_button_compact(
                        ("settings-generation-model", index),
                        label,
                        cx,
                    )
                } else {
                    crate::ui::style::dialog_neutral_button(
                        ("settings-generation-model", index),
                        label,
                        cx,
                    )
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.select_generation_model(model, cx);
                }))
            })
            .collect::<Vec<_>>();
            let quick_ask_provider_buttons =
                [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode]
                    .into_iter()
                    .enumerate()
                    .map(|(index, provider)| {
                        let button = if quick_ask_agent.provider == provider {
                            crate::ui::style::primary_button_compact(
                                ("settings-quick-ask-provider", index),
                                provider.label(),
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-quick-ask-provider", index),
                                provider.label(),
                                cx,
                            )
                        };
                        button
                            .icon(crate::ui::center::provider_brand_icon(provider))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_quick_ask_provider(provider, cx);
                            }))
                    })
                    .collect::<Vec<_>>();
            let quick_ask_model_buttons = if quick_ask_agent.provider == AgentKind::OpenCode {
                vec![(AgentModel::OpenCode, "Big Pickle".to_string())]
            } else {
                AgentModel::models_for(quick_ask_agent.provider)
                    .iter()
                    .map(|model| (*model, model.menu_label().to_string()))
                    .collect::<Vec<_>>()
            }
            .into_iter()
            .enumerate()
            .map(|(index, (model, label))| {
                let button = if quick_ask_agent.model == model {
                    crate::ui::style::primary_button_compact(
                        ("settings-quick-ask-model", index),
                        label,
                        cx,
                    )
                } else {
                    crate::ui::style::dialog_neutral_button(
                        ("settings-quick-ask-model", index),
                        label,
                        cx,
                    )
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.select_quick_ask_model(model, cx);
                }))
            })
            .collect::<Vec<_>>();
            let default_provider_buttons =
                [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode]
                    .into_iter()
                    .enumerate()
                    .map(|(index, provider)| {
                        let button = if agent_defaults.provider == provider {
                            crate::ui::style::primary_button_compact(
                                ("settings-agent-default-provider", index),
                                provider.label(),
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-agent-default-provider", index),
                                provider.label(),
                                cx,
                            )
                        };
                        button
                            .icon(crate::ui::center::provider_brand_icon(provider))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_default_agent_provider(provider, cx);
                            }))
                    })
                    .collect::<Vec<_>>();
            let default_model_buttons = if agent_defaults.provider == AgentKind::OpenCode {
                vec![(AgentModel::OpenCode, "Big Pickle".to_string())]
            } else {
                AgentModel::models_for(agent_defaults.provider)
                    .iter()
                    .map(|model| (*model, model.menu_label().to_string()))
                    .collect::<Vec<_>>()
            }
            .into_iter()
            .enumerate()
            .map(|(index, (model, label))| {
                let button = if agent_defaults.model == model {
                    crate::ui::style::primary_button_compact(
                        ("settings-agent-default-model", index),
                        label,
                        cx,
                    )
                } else {
                    crate::ui::style::dialog_neutral_button(
                        ("settings-agent-default-model", index),
                        label,
                        cx,
                    )
                };
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.select_default_agent_model(model, cx);
                }))
            })
            .collect::<Vec<_>>();
            let default_effort_buttons = AgentEffort::ALL
                .into_iter()
                .enumerate()
                .map(|(index, effort)| {
                    let button = if agent_defaults.effort == effort {
                        crate::ui::style::primary_button_compact(
                            ("settings-agent-default-effort", index),
                            effort.label(),
                            cx,
                        )
                    } else {
                        crate::ui::style::dialog_neutral_button(
                            ("settings-agent-default-effort", index),
                            effort.label(),
                            cx,
                        )
                    };
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        this.select_default_agent_effort(effort, cx);
                    }))
                })
                .collect::<Vec<_>>();
            let label_size = crate::ui::design::text_label();
            let label_color = crate::ui::design::t4(cx);
            let row_label = move |label: &'static str| {
                div()
                    .text_size(label_size)
                    .text_color(label_color)
                    .child(label)
            };

            v_flex()
                                .w_full()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_3()
                                        .p_4()
                                        .rounded(crate::ui::design::r_lg())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("New agents"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("What a fresh agent starts with. Change any of it per agent in the composer."),
                                        )
                                        .child(row_label("Provider"))
                                        .child(h_flex().w_full().gap_2().flex_wrap().children(default_provider_buttons))
                                        .child(row_label("Model"))
                                        .child(h_flex().w_full().gap_2().flex_wrap().children(default_model_buttons))
                                        .child(row_label("Effort"))
                                        .child(h_flex().w_full().gap_2().flex_wrap().children(default_effort_buttons)),
                                )
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_3()
                                        .p_4()
                                        .rounded(crate::ui::design::r_lg())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Quick Ask"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("Default model for Quick Ask answers. A model override applies only to the open panel and resets when you close it."),
                                        )
                                        .child(row_label("Provider"))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_2()
                                                .flex_wrap()
                                                .children(quick_ask_provider_buttons),
                                        )
                                        .child(row_label("Model"))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_2()
                                                .flex_wrap()
                                                .children(quick_ask_model_buttons),
                                        )
                                        .child({
                                            let quick_ask = self.quick_ask.clone();
                                            h_flex()
                                                .w_full()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::t4(cx))
                                                        .child(format!(
                                                            "{} saved question{}",
                                                            quick_ask_history_count,
                                                            if quick_ask_history_count == 1 { "" } else { "s" }
                                                        )),
                                                )
                                                .child(div().flex_1())
                                                .child(
                                                    crate::ui::style::danger_button_compact(
                                                        "settings-clear-quick-ask-history",
                                                        "Clear history",
                                                    )
                                                    .disabled(quick_ask_history_count == 0)
                                                    .on_click(move |_, window, cx| {
                                                        let quick_ask = quick_ask.clone();
                                                        crate::ui::confirm::ConfirmDialog::new(
                                                            "Clear Quick Ask history?",
                                                            "This permanently clears Quick Ask history across every project.",
                                                        )
                                                        .confirm_label("Clear history")
                                                        .confirm_id("settings-confirm-clear-quick-ask-history")
                                                        .on_confirm(move |_, cx| {
                                                            if let Err(error) = quick_ask.update(cx, |state, cx| {
                                                                state.clear_history(cx)
                                                            }) {
                                                                eprintln!("could not clear Quick Ask history: {error:#}");
                                                            }
                                                        })
                                                        .open(window, cx);
                                                    }),
                                                )
                                        }),
                                )
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_3()
                                        .p_4()
                                        .rounded(crate::ui::design::r_lg())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Manual review checklist"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("After a turn changes files, ask the same agent for a short read-only checklist of what you should check manually."),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_2()
                                                .flex_wrap()
                                                .children(checklist_buttons),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_3()
                                        .p_4()
                                        .rounded(crate::ui::design::r_lg())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Quick generation"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(format!(
                                                    "Commit messages, PR titles and bodies, Riff instructions — fast one-shot calls, not chat agents. Currently {} · {}.",
                                                    generation_agent.provider.label(),
                                                    generation_agent.model_label()
                                                )),
                                        )
                                        .child(row_label("Provider"))
                                        .child(h_flex().w_full().gap_2().flex_wrap().children(provider_buttons))
                                        .child(row_label("Model"))
                                        .child(h_flex().w_full().gap_2().flex_wrap().children(model_buttons)),
                                )
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_3()
                                        .p_4()
                                        .rounded(crate::ui::design::r_lg())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Work verification"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("Verification is an extra agent pass and can use significant time and tokens."),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_2()
                                                .flex_wrap()
                                                .children(verification_buttons),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Code review prompt"),
                                        )
                                        .child(
                                            Input::new(&code_review_prompt)
                                                .w_full()
                                                .h(px(280.))
                                                .flex_none(),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .justify_between()
                                                .child(
                                                    crate::ui::style::dialog_neutral_button(
                                                        "restore-default-code-review-prompt",
                                                        "Restore default",
                                                        cx,
                                                    )
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.restore_default_code_review_prompt(window, cx);
                                                    })),
                                                )
                                                .child(
                                                    crate::ui::style::primary_button_compact(
                                                        "save-code-review-prompt",
                                                        "Save prompt",
                                                        cx,
                                                    )
                                                    .disabled(
                                                        !code_review_prompt_dirty
                                                            || code_review_prompt_empty,
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.save_code_review_prompt(cx);
                                                    })),
                                                ),
                                        ),
                                )
                                .into_any_element()
        }
    }
}
