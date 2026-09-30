use super::*;

const INSTRUCTIONS_H: f32 = 168.;
const OUTCOME_H: f32 = 88.;

impl SettingsView {
    pub(super) fn render_expert_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(editor) = self.expert_editor.as_ref() else {
            return div().into_any_element();
        };
        let profile = editor.profile.clone();
        let name = editor.name.clone();
        let description = editor.description.clone();
        let instructions = editor.instructions.clone();
        let outcome = editor.outcome.clone();
        let tab = editor.tab;
        let custom_open = editor.custom_editor.is_some();
        let skill_count = profile.skills.len()
            + profile.additions.bundled_skills.len()
            + profile.additions.custom_skills.len()
            + profile.additions.riff_ids.len();
        let body = match tab {
            ExpertEditorTab::Overview => {
                style::expert_settings_scroll_body("band-overview-scroll", window, cx)
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.))
                            .gap_5()
                            .pb_2()
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .gap_3()
                                    .child(field("Name", &name, cx))
                                    .child(field("When to use", &description, cx)),
                            )
                            .child(self.render_model_controls(&profile, cx))
                            .child(multiline_field(
                                "Job instructions",
                                "How this bandmate should approach its work.",
                                &instructions,
                                INSTRUCTIONS_H,
                                cx,
                            ))
                            .child(multiline_field(
                                "Expected result",
                                "Optional. What should a completed task include?",
                                &outcome,
                                OUTCOME_H,
                                cx,
                            )),
                    )
                    .into_any_element()
            }
            ExpertEditorTab::Skills => self.render_expert_skills(window, cx),
        };
        v_flex()
            .w_full()
            .min_w(px(0.))
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .gap_4()
            .child(self.render_editor_header(&profile, cx))
            .child(
                style::segmented_container_quiet(cx)
                    .flex_none()
                    .child(
                        style::segment_text_button(
                            "band-overview-tab",
                            "Overview",
                            tab == ExpertEditorTab::Overview,
                            cx,
                        )
                        .disabled(custom_open)
                        .tooltip(if custom_open {
                            "Finish editing this skill first"
                        } else {
                            "Name, model and instructions"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = &mut this.expert_editor {
                                if editor.custom_editor.is_none() {
                                    editor.tab = ExpertEditorTab::Overview;
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        style::segment_text_button(
                            "band-skills-tab",
                            format!("Skills ({skill_count})"),
                            tab == ExpertEditorTab::Skills,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = &mut this.expert_editor {
                                editor.tab = ExpertEditorTab::Skills;
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(body)
            .when(!custom_open, |page| {
                page.when_some(self.experts_status.clone(), |page, notice| {
                    page.child(div().w_full().flex_none().child(notice.render(cx)))
                })
                .child(
                    style::expert_settings_editor_footer(cx)
                        .child(
                            style::expert_settings_field_hint(
                                "Applies to new chats and tasks.",
                                cx,
                            )
                            .flex_1(),
                        )
                        .child(
                            style::dialog_neutral_button("cancel-expert", "Cancel", cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.expert_editor = None;
                                    this.experts_status = None;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            style::primary_button_compact("save-expert", "Save bandmate", cx)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_expert_editor(cx)),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_editor_header(&self, profile: &ExpertProfile, cx: &App) -> Div {
        h_flex()
            .w_full()
            .min_w(px(0.))
            .flex_none()
            .gap_3()
            .child(
                style::expert_settings_row_badge(px(32.), true, cx)
                    .text_size(design::text_body())
                    .text_color(design::amber(cx))
                    .child(expert_initial(&profile.name)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(design::text_title())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(design::t1(cx))
                            .child(if profile.revision == 0 {
                                "New bandmate".to_string()
                            } else {
                                profile.name.clone()
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child("Set up how this bandmate works."),
                    ),
            )
    }

    fn render_model_controls(&self, profile: &ExpertProfile, cx: &mut Context<Self>) -> Div {
        let view = cx.entity();
        let provider = profile.provider;
        let model = profile.model;
        let effort = profile.effort;
        v_flex()
            .w_full()
            .gap_1()
            .child(style::expert_settings_field_label("Model", cx))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        style::dialog_neutral_button("expert-provider", provider.label(), cx)
                            .icon(IconName::ChevronDown)
                            .tooltip("Provider")
                            .dropdown_menu({
                                let view = view.clone();
                                move |mut menu, window, _| {
                                    for candidate in [AgentKind::Claude, AgentKind::Codex] {
                                        menu = menu.item(
                                            PopupMenuItem::new(candidate.label())
                                                .checked(provider == candidate)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this: &mut Self, _, _, cx| {
                                                        this.set_expert_provider(candidate, cx)
                                                    },
                                                )),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(
                        style::dialog_neutral_button("expert-model", model.label(), cx)
                            .icon(IconName::ChevronDown)
                            .tooltip("Model")
                            .dropdown_menu({
                                let view = view.clone();
                                move |mut menu, window, _| {
                                    for candidate in
                                        AgentModel::models_for(provider).iter().copied()
                                    {
                                        menu = menu.item(
                                            PopupMenuItem::new(candidate.menu_label())
                                                .checked(model == candidate)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this: &mut Self, _, _, cx| {
                                                        if let Some(e) = &mut this.expert_editor {
                                                            e.profile.model = candidate;
                                                            e.profile.effort = candidate
                                                                .normalize_effort(e.profile.effort);
                                                        }
                                                        cx.notify();
                                                    },
                                                )),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(
                        style::dialog_neutral_button(
                            "expert-effort",
                            format!("{} effort", effort.label()),
                            cx,
                        )
                        .icon(IconName::ChevronDown)
                        .tooltip("Reasoning effort")
                        .dropdown_menu(move |mut menu, window, _| {
                            for candidate in model.efforts().iter().copied() {
                                menu = menu.item(
                                    PopupMenuItem::new(candidate.label())
                                        .checked(effort == candidate)
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this: &mut Self, _, _, cx| {
                                                if let Some(e) = &mut this.expert_editor {
                                                    e.profile.effort = candidate;
                                                }
                                                cx.notify();
                                            },
                                        )),
                                );
                            }
                            menu
                        }),
                    ),
            )
    }

    fn set_expert_provider(&mut self, candidate: AgentKind, cx: &mut Context<Self>) {
        if let Some(e) = &mut self.expert_editor {
            if e.profile.provider != candidate {
                e.profile.provider = candidate;
                e.profile.model = AgentModel::default_for(candidate);
                e.profile.effort = e.profile.model.default_effort();
                if e.profile.skills.iter().any(|s| s.provider != candidate) {
                    self.experts_status = Some(ExpertsNotice::error(
                        "Some installed skills belong to the previous provider. Replace or detach them before saving. Choro skills, custom skills, and Riffs are preserved.",
                    ));
                }
            }
        }
        cx.notify();
    }

    pub(super) fn edit_expert(
        &mut self,
        profile: Option<ExpertProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = AgentModel::default_for(AgentKind::Claude);
        let profile = profile.unwrap_or(ExpertProfile {
            id: Uuid::new_v4(),
            revision: 0,
            name: String::new(),
            description: String::new(),
            provider: AgentKind::Claude,
            model,
            effort: model.default_effort(),
            instructions: String::new(),
            skills: vec![],
            expected_outcome: String::new(),
            enabled: true,
            archived: false,
            additions: Default::default(),
        });
        let name = text_input(&profile.name, "UI Designer", window, cx);
        let description = text_input(
            &profile.description,
            "When should this bandmate help?",
            window,
            cx,
        );
        let instructions = multiline_input(
            &profile.instructions,
            "Job, working approach, and constraints",
            8,
            window,
            cx,
        );
        let outcome = multiline_input(
            &profile.expected_outcome,
            "What should good completion include?",
            3,
            window,
            cx,
        );
        let skill_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search skills or Riffs"));
        cx.subscribe(&skill_search, |_: &mut Self, _, _: &InputEvent, cx| {
            cx.notify()
        })
        .detach();
        self.expert_editor = Some(ExpertEditor {
            profile,
            name,
            description,
            instructions,
            outcome,
            skill_search,
            skill_picker: None,
            custom_editor: None,
            expanded_skill: None,
            tab: ExpertEditorTab::Overview,
        });
        self.experts_status = None;
        cx.notify();
    }

    fn save_expert_editor(&mut self, cx: &mut Context<Self>) {
        let Some(e) = &self.expert_editor else {
            return;
        };
        if e.custom_editor.is_some() {
            return;
        }
        let profile = ExpertProfile {
            name: e.name.read(cx).value().to_string(),
            description: e.description.read(cx).value().to_string(),
            instructions: e.instructions.read(cx).value().to_string(),
            expected_outcome: e.outcome.read(cx).value().to_string(),
            ..e.profile.clone()
        };
        let result = LocalStore::open_default().and_then(|s| {
            s.save_expert(
                profile.clone(),
                (profile.revision > 0).then_some(profile.revision),
            )?;
            s.load_experts()
        });
        match result {
            Ok(profiles) => {
                self.experts = profiles;
                self.expert_editor = None;
                self.experts_status = Some(ExpertsNotice::info(
                    "Bandmate saved. Existing chats keep their original setup.",
                ));
            }
            Err(e) => self.experts_status = Some(ExpertsNotice::error(e.to_string())),
        };
        cx.notify();
    }
}

pub(super) fn field(label: &'static str, state: &Entity<InputState>, cx: &App) -> Div {
    v_flex()
        .w_full()
        .min_w(px(0.))
        .flex_none()
        .gap_1()
        .child(style::expert_settings_field_label(label, cx))
        .child(style::expert_settings_input(state).w_full())
}

/// Explicit field and input bounds keep long instructions inside the form.
pub(super) fn multiline_field(
    label: &'static str,
    hint: &'static str,
    state: &Entity<InputState>,
    height: f32,
    cx: &App,
) -> Div {
    v_flex()
        .w_full()
        .min_w(px(0.))
        .flex_none()
        .gap_1p5()
        .child(style::expert_settings_field_label(label, cx))
        .child(style::expert_settings_multiline_input(state).h(px(height)))
        .child(
            div()
                .w_full()
                .text_size(design::text_ui())
                .text_color(design::t3(cx))
                .child(hint),
        )
}

pub(super) fn text_input(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    let value = value.to_string();
    input.update(cx, |input, cx| input.set_value(value, window, cx));
    input
}

pub(super) fn multiline_input(
    value: &str,
    placeholder: &'static str,
    rows: usize,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<InputState> {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .multi_line(true)
            .soft_wrap(true)
            .rows(rows)
            .placeholder(placeholder)
    });
    let value = value.to_string();
    input.update(cx, |input, cx| input.set_value(value, window, cx));
    input
}
