use super::*;

impl SettingsView {
    pub(super) fn render_skills_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        {
            let search = self.skills_search.read(cx).value().trim().to_string();
            let choro_total = self.riffs.len();
            let codex_total = self
                .skills
                .iter()
                .filter(|skill| skill.provider == AgentKind::Codex)
                .count();
            let claude_total = self
                .skills
                .iter()
                .filter(|skill| skill.provider == AgentKind::Claude)
                .count();
            let filtered_skills = self
                .skills
                .iter()
                .filter(|skill| {
                    self.skills_provider
                        .provider()
                        .is_some_and(|provider| skill.provider == provider)
                })
                .filter(|skill| skill.matches(&search))
                .cloned()
                .collect::<Vec<_>>();
            let filtered_riffs = self
                .riffs
                .iter()
                .filter(|_| self.skills_provider == SkillProviderFilter::Choro)
                .filter(|riff| riff.matches(&search))
                .cloned()
                .collect::<Vec<_>>();
            let editor = self.riff_editor.as_ref().map(|editor| {
                let name_value = editor.name.read(cx).value().trim().to_string();
                let description_value = editor.description.read(cx).value().trim().to_string();
                let instructions_empty = editor.instructions.read(cx).value().trim().is_empty();
                (
                    editor.id,
                    editor.name.clone(),
                    editor.description.clone(),
                    editor.instructions.clone(),
                    editor.generating,
                    instructions_empty,
                    !name_value.is_empty() && !description_value.is_empty(),
                    editor.error.clone(),
                )
            });
            let editing_riff = editor.is_some();
            let show_riff_empty_state = self.skills_provider == SkillProviderFilter::Choro
                && self.riffs.is_empty()
                && search.is_empty()
                && editor.is_none();
            let source_description = match self.skills_provider {
                SkillProviderFilter::Choro => {
                    "Reusable instructions for Codex and Claude, available in every project."
                }
                SkillProviderFilter::Codex => {
                    "Skills discovered from Codex for the active workspace. Managed by Codex."
                }
                SkillProviderFilter::Claude => {
                    "Skills and commands discovered from Claude Code for the active workspace."
                }
            };
            v_flex()
                                .w_full()
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_hidden()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .child(
                                            crate::ui::style::segmented_container_quiet(cx)
                                                .w_full()
                                                .min_w(px(0.))
                                                .child(
                                                    crate::ui::style::segment_with_leading(
                                                        "settings-skills-choro",
                                                        crate::ui::style::choro_riff_icon(
                                                            crate::ui::design::icon_md(),
                                                            if self.skills_provider
                                                                == SkillProviderFilter::Choro
                                                            {
                                                                crate::ui::design::accent(cx)
                                                            } else {
                                                                crate::ui::design::t3(cx)
                                                            },
                                                        ),
                                                        format!("Choro Riffs ({choro_total})"),
                                                        self.skills_provider == SkillProviderFilter::Choro,
                                                        cx,
                                                    )
                                                    .flex_1()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.skills_provider = SkillProviderFilter::Choro;
                                                        cx.notify();
                                                    })),
                                                )
                                                .child(
                                                    crate::ui::style::segment(
                                                        "settings-skills-codex",
                                                        IconName::SquareTerminal,
                                                        format!("Codex ({codex_total})"),
                                                        self.skills_provider == SkillProviderFilter::Codex,
                                                        cx,
                                                    )
                                                    .flex_1()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.skills_provider = SkillProviderFilter::Codex;
                                                        cx.notify();
                                                    })),
                                                )
                                                .child(
                                                    crate::ui::style::segment(
                                                        "settings-skills-claude",
                                                        IconName::Bot,
                                                        format!("Claude ({claude_total})"),
                                                        self.skills_provider == SkillProviderFilter::Claude,
                                                        cx,
                                                    )
                                                    .flex_1()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.skills_provider = SkillProviderFilter::Claude;
                                                        cx.notify();
                                                    })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .min_w(px(0.))
                                        .text_size(crate::ui::design::text_ui())
                                        .line_height(gpui::relative(1.4))
                                        .whitespace_normal()
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(source_description),
                                )
                                .when_some(editor, |section, (id, name, description, instructions, generating, instructions_empty, has_generation_source, error)| {
                                    section.child(
                                        v_flex()
                                            .w_full()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .overflow_y_scrollbar()
                                            .gap_3()
                                            .rounded(crate::ui::design::r_lg())
                                            .border_1()
                                            .border_color(crate::ui::design::accent_line(cx))
                                            .bg(crate::ui::design::surface(cx).opacity(0.64))
                                            .p_4()
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .size(px(34.))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .rounded_full()
                                                            .bg(crate::ui::design::accent_soft(cx))
                                                            .child(crate::ui::style::choro_riff_icon(
                                                                crate::ui::design::icon_lg(),
                                                                crate::ui::design::accent(cx),
                                                            )),
                                                    )
                                                    .child(
                                                        v_flex()
                                                            .gap_0p5()
                                                            .child(
                                                                div()
                                                                    .text_size(crate::ui::design::text_body())
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .text_color(crate::ui::design::t1(cx))
                                                                    .child(if id.is_some() { "Edit Riff" } else { "New Riff" }),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(crate::ui::design::text_ui())
                                                                    .text_color(crate::ui::design::t3(cx))
                                                                    .child("Give agents a reusable way of working."),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(crate::ui::design::t2(cx))
                                                            .child("Name"),
                                                    )
                                                    .child(Input::new(&name)),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(crate::ui::design::t2(cx))
                                                            .child("Description"),
                                                    )
                                                    .child(Input::new(&description)),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        h_flex()
                                                            .w_full()
                                                            .child(
                                                                div()
                                                                    .text_size(crate::ui::design::text_ui())
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(crate::ui::design::t2(cx))
                                                                    .child("Instructions"),
                                                            )
                                                            .child(div().flex_1())
                                                            .child(
                                                                div()
                                                                    .text_size(crate::ui::design::text_label())
                                                                    .text_color(crate::ui::design::t4(cx))
                                                                    .child("Hidden from the composer"),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .h(px(280.))
                                                            .flex_none()
                                                            .overflow_hidden()
                                                            .rounded(crate::ui::design::r_md())
                                                            .border_1()
                                                            .border_color(crate::ui::design::line(cx))
                                                            .bg(crate::ui::design::base(cx).opacity(0.42))
                                                            .child(
                                                                v_flex()
                                                                    .size_full()
                                                                    .child(
                                                                        div()
                                                                            .w_full()
                                                                            .flex_1()
                                                                            .min_h(px(0.))
                                                                            .overflow_hidden()
                                                                            .px_3()
                                                                            .py_2()
                                                                            .child(
                                                                                Input::new(&instructions)
                                                                                    .appearance(false)
                                                                                    .bordered(false)
                                                                                    .focus_bordered(false)
                                                                                    .w_full()
                                                                                    .min_w(px(0.))
                                                                                    .h_full(),
                                                                            ),
                                                                    )
                                                                    .when(instructions_empty, |editor| {
                                                                        editor.child(
                                                                            h_flex()
                                                                                .w_full()
                                                                                .h(px(42.))
                                                                                .flex_none()
                                                                                .items_center()
                                                                                .gap_2()
                                                                                .px_2()
                                                                                .border_t_1()
                                                                                .border_color(crate::ui::design::line(cx).opacity(0.55))
                                                                                .child(
                                                                                    crate::ui::style::ghost_button_compact(
                                                                                        "generate-riff-instructions",
                                                                                        if generating { "Generating…" } else { "Generate with AI" },
                                                                                    )
                                                                                    .icon(IconName::Bot)
                                                                                    .text_color(crate::ui::design::accent(cx))
                                                                                    .disabled(generating || !has_generation_source)
                                                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                                                        this.generate_riff_instructions(window, cx);
                                                                                    })),
                                                                                )
                                                                                .when(generating, |row| {
                                                                                    row.child(Spinner::new().xsmall())
                                                                                })
                                                                                .child(div().flex_1())
                                                                                .child(
                                                                                    div()
                                                                                        .text_size(crate::ui::design::text_label())
                                                                                        .text_color(crate::ui::design::t4(cx))
                                                                                        .child("Uses the name and description"),
                                                                                ),
                                                                        )
                                                                    }),
                                                            ),
                                                    ),
                                            )
                                            .when_some(error, |card, error| {
                                                card.child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::rose(cx))
                                                        .child(error),
                                                )
                                            })
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .justify_end()
                                                    .when_some(id, |row, id| {
                                                        row.child(
                                                            crate::ui::style::danger_button_compact(
                                                                ("delete-choro-riff", id.as_u128() as u64),
                                                                "Delete",
                                                            )
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.delete_riff(id, cx);
                                                            })),
                                                        )
                                                    })
                                                    .child(
                                                        crate::ui::style::dialog_neutral_button(
                                                            "cancel-choro-riff",
                                                            "Cancel",
                                                            cx,
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.riff_editor = None;
                                                            cx.notify();
                                                        })),
                                                    )
                                                    .child(
                                                        crate::ui::style::primary_button_compact(
                                                            "save-choro-riff",
                                                            "Save Riff",
                                                            cx,
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.save_riff_editor(cx);
                                                        })),
                                                    ),
                                            ),
                                        )
                                    })
                                .when(show_riff_empty_state, |section| {
                                    section.child(
                                        v_flex()
                                            .w_full()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .items_center()
                                            .justify_center()
                                            .gap_4()
                                            .rounded(crate::ui::design::r_lg())
                                            .border_1()
                                            .border_color(crate::ui::design::line(cx).opacity(0.38))
                                            .bg(crate::ui::design::surface(cx).opacity(0.32))
                                            .child(
                                                div()
                                                    .size(px(64.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded_full()
                                                    .border_1()
                                                    .border_color(crate::ui::design::accent_line(cx))
                                                    .bg(crate::ui::design::accent_soft(cx))
                                                    .child(crate::ui::style::choro_riff_icon(
                                                        px(30.),
                                                        crate::ui::design::accent(cx),
                                                    )),
                                            )
                                            .child(
                                                v_flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_title())
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(crate::ui::design::t1(cx))
                                                            .child("Create your first Riff"),
                                                    )
                                                    .child(
                                                        div()
                                                            .max_w(px(430.))
                                                            .text_center()
                                                            .whitespace_normal()
                                                            .line_height(gpui::relative(1.45))
                                                            .text_size(crate::ui::design::text_body())
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child("Save the way you like agents to work, then call it from any project with /."),
                                                    ),
                                            )
                                            .child(
                                                crate::ui::style::primary_button_compact(
                                                    "empty-add-choro-riff",
                                                    "Create a Riff",
                                                    cx,
                                                )
                                                .icon(IconName::Plus)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_riff_editor(None, window, cx);
                                                })),
                                            ),
                                    )
                                })
                                .when(!show_riff_empty_state && !editing_riff, |section| {
                                    section.child(
                                        h_flex()
                                            .w_full()
                                            .gap_3()
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .max_w(px(360.))
                                                    .child(Input::new(&self.skills_search)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t4(cx))
                                                    .child(if self.skills_provider == SkillProviderFilter::Choro {
                                                        format!("{} Riffs · available everywhere", filtered_riffs.len())
                                                    } else {
                                                        format!(
                                                            "{} shown · {}{}",
                                                            filtered_skills.len(),
                                                            self.skills_cwd,
                                                            self.skills_last_refreshed
                                                                .as_ref()
                                                                .filter(|value| !value.is_empty())
                                                                .map(|value| format!(" · refreshed {value}"))
                                                                .unwrap_or_default()
                                                        )
                                                    }),
                                            )
                                            .when(
                                                self.skills_provider == SkillProviderFilter::Choro,
                                                |row| {
                                                    row.child(
                                                        crate::ui::style::primary_button_compact(
                                                            "add-choro-riff",
                                                            "New Riff",
                                                            cx,
                                                        )
                                                        .icon(IconName::Plus)
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.open_riff_editor(None, window, cx);
                                                        })),
                                                    )
                                                },
                                            )
                                            .when(
                                                self.skills_provider != SkillProviderFilter::Choro,
                                                |row| {
                                                    row.when(self.skills_loading, |row| {
                                                        row.child(Spinner::new().xsmall())
                                                    })
                                                    .child(
                                                        crate::ui::style::refresh_button(
                                                            "refresh-agent-skills",
                                                            "Refresh",
                                                            cx,
                                                        )
                                                        .disabled(self.skills_loading)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.refresh_agent_skills(cx);
                                                        })),
                                                    )
                                                },
                                            ),
                                    )
                                })
                                .when_some(
                                    (!editing_riff).then(|| self.riffs_status.clone()).flatten(),
                                    |section, status| {
                                    section.child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(status),
                                    )
                                })
                                .when_some(
                                    (!editing_riff).then(|| self.skills_error.clone()).flatten(),
                                    |section, error| {
                                    section.child(
                                        div()
                                            .rounded(crate::ui::design::r_sm())
                                            .border_1()
                                            .border_color(crate::ui::design::rose(cx).opacity(0.35))
                                            .bg(crate::ui::design::rose(cx).opacity(0.08))
                                            .px_2()
                                            .py_1()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::rose(cx))
                                            .child(SharedString::from(error)),
                                    )
                                })
                                .when(!show_riff_empty_state && !editing_riff, |section| {
                                    section.child(v_flex()
                                        .w_full()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .overflow_y_scrollbar()
                                        .rounded(crate::ui::design::r_md())
                                        .border_1()
                                        .border_color(crate::ui::design::line(cx))
                                        .bg(crate::ui::design::surface(cx).opacity(0.32))
                                        .when(
                                            self.skills_provider == SkillProviderFilter::Choro
                                                && filtered_riffs.is_empty(),
                                            |list| {
                                            list.child(
                                                div()
                                                    .px_3()
                                                    .py_2()
                                                    .text_size(crate::ui::design::text_body())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(if search.is_empty() {
                                                        "No Choro Riffs yet. Create one to make it available in every project.".to_string()
                                                    } else {
                                                        format!("No matches for {search}")
                                                    }),
                                            )
                                        })
                                        .when(
                                            self.skills_provider != SkillProviderFilter::Choro
                                                && filtered_skills.is_empty(),
                                            |list| list.child(
                                                div()
                                                    .px_3()
                                                    .py_2()
                                                    .text_size(crate::ui::design::text_body())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(if self.skills_loading {
                                                        "Loading skills...".to_string()
                                                    } else if search.is_empty() {
                                                        format!("No {} skills loaded", self.skills_provider.label())
                                                    } else {
                                                        format!("No matches for {search}")
                                                    }),
                                            ),
                                        )
                                        .children(filtered_riffs.iter().enumerate().map(|(index, riff)| {
                                            let riff_for_edit = riff.clone();
                                            let id = riff.id;
                                            h_flex()
                                                .id(("settings-choro-riff-row", index))
                                                .w_full()
                                                .min_w(px(0.))
                                                .min_h(px(64.))
                                                .gap_3()
                                                .items_center()
                                                .px_3()
                                                .py_2()
                                                .border_b_1()
                                                .border_color(crate::ui::design::line(cx).opacity(0.18))
                                                .child(
                                                    div()
                                                        .size(px(32.))
                                                        .flex_none()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded(crate::ui::design::r_sm())
                                                        .bg(crate::ui::design::accent_soft(cx))
                                                        .child(crate::ui::style::choro_riff_icon(
                                                            crate::ui::design::icon_md(),
                                                            crate::ui::design::accent(cx),
                                                        )),
                                                )
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(crate::ui::design::text_body())
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .text_color(crate::ui::design::t1(cx))
                                                                .child(riff.name.clone()),
                                                        )
                                                        .when_some(riff.description.clone(), |col, description| {
                                                            col.child(
                                                                div()
                                                                    .truncate()
                                                                    .text_size(crate::ui::design::text_ui())
                                                                    .text_color(crate::ui::design::t3(cx))
                                                                    .child(description),
                                                            )
                                                        }),
                                                )
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .child(
                                                            crate::ui::style::dialog_neutral_button(
                                                                ("toggle-choro-riff", id.as_u128() as u64),
                                                                if riff.enabled { "Enabled" } else { "Disabled" },
                                                                cx,
                                                            )
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.toggle_riff(id, cx);
                                                            })),
                                                        )
                                                        .child(
                                                            crate::ui::style::dialog_neutral_button(
                                                                ("edit-choro-riff", id.as_u128() as u64),
                                                                "Edit",
                                                                cx,
                                                            )
                                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                                this.open_riff_editor(Some(riff_for_edit.clone()), window, cx);
                                                            })),
                                                        ),
                                                )
                                        }))
                                        .children(filtered_skills.iter().enumerate().map(|(index, skill)| {
                                            let subtitle = skill
                                                .description
                                                .clone()
                                                .unwrap_or_else(|| skill.name.clone());
                                            h_flex()
                                                .id(("settings-agent-skill-row", index))
                                                .w_full()
                                                .min_w(px(0.))
                                                .min_h(px(64.))
                                                .gap_3()
                                                .items_center()
                                                .px_3()
                                                .py_2()
                                                .border_b_1()
                                                .border_color(crate::ui::design::line(cx).opacity(0.18))
                                                .child(
                                                    div()
                                                        .w(px(120.))
                                                        .flex_none()
                                                        .truncate()
                                                        .font_family(crate::ui::design::FONT_MONO)
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::accent(cx))
                                                        .child(skill.invocation.trim().to_string()),
                                                )
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .gap_0p5()
                                                        .child(
                                                            h_flex()
                                                                .w_full()
                                                                .min_w(px(0.))
                                                                .gap_2()
                                                                .items_center()
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w(px(0.))
                                                                        .truncate()
                                                                        .text_size(crate::ui::design::text_body())
                                                                        .font_weight(FontWeight::SEMIBOLD)
                                                                        .child(skill.title.clone()),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_none()
                                                                        .rounded(crate::ui::design::r_sm())
                                                                        .border_1()
                                                                        .border_color(
                                                                            crate::ui::design::line(cx)
                                                                                .opacity(0.24),
                                                                        )
                                                                        .bg(
                                                                            crate::ui::design::base(cx)
                                                                                .opacity(0.55),
                                                                        )
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .text_size(crate::ui::design::text_ui())
                                                                        .text_color(
                                                                            crate::ui::design::t3(cx),
                                                                        )
                                                                        .child(skill.source.label()),
                                                                ),
                                                        )
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(crate::ui::design::text_ui())
                                                                .text_color(crate::ui::design::t3(cx))
                                                                .child(subtitle),
                                                        ),
                                                )
                                        }))
                                    )
                                })
                                .into_any_element()
        }
    }
}
