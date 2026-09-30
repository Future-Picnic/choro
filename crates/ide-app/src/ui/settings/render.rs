use super::*;

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        let contained_page = matches!(
            section,
            SettingsSection::AgentSkills | SettingsSection::Experts | SettingsSection::Design
        );
        let editing_bandmate = section == SettingsSection::Experts && self.expert_editor.is_some();
        let settings_query = self.settings_search.read(cx).value().trim().to_lowercase();
        let brain_matches = settings_query.is_empty()
            || "brain".contains(&settings_query)
            || SettingsSection::Brain.matches(&settings_query)
            || SettingsSection::Memory.matches(&settings_query);
        let brain_children_visible = self.brain_expanded || !settings_query.is_empty();
        h_flex()
            .h_full()
            .w_full()
            .child(
                v_flex()
                    .w(px(236.))
                    .h_full()
                    .flex_none()
                    .gap_1()
                    .p_3()
                    .border_r_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div()
                            .w_full()
                            .mb_2()
                            .child(Input::new(&self.settings_search).prefix(IconName::Search)),
                    )
                    .when(
                        SettingsSection::Appearance.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-appearance-section",
                                SettingsSection::Appearance,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(SettingsSection::Design.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-design-section",
                            SettingsSection::Design,
                            section,
                            cx,
                        ))
                    })
                    .when(
                        SettingsSection::Generation.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-generation-section",
                                SettingsSection::Generation,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(SettingsSection::Voice.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-voice-section",
                            SettingsSection::Voice,
                            section,
                            cx,
                        ))
                    })
                    .when(SettingsSection::Companion.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-companion-section",
                            SettingsSection::Companion,
                            section,
                            cx,
                        ))
                    })
                    .when(
                        SettingsSection::Notifications.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-notifications-section",
                                SettingsSection::Notifications,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(SettingsSection::Shortcuts.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-shortcuts-section",
                            SettingsSection::Shortcuts,
                            section,
                            cx,
                        ))
                    })
                    .when(
                        SettingsSection::AgentSkills.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-agent-skills-section",
                                SettingsSection::AgentSkills,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(
                        ide_core::delegation::enabled()
                            && SettingsSection::Experts.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-experts",
                                SettingsSection::Experts,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(SettingsSection::Orbit.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-orbit-section",
                            SettingsSection::Orbit,
                            section,
                            cx,
                        ))
                    })
                    .when(brain_matches, |nav| {
                        nav.child(
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child(Self::brain_group_button(self.brain_expanded, section, cx))
                                .when(brain_children_visible, |group| {
                                    group
                                        .child(Self::nested_section_button(
                                            "settings-brain-knowledge-section",
                                            SettingsSection::Brain,
                                            section,
                                            cx,
                                        ))
                                        .child(Self::nested_section_button(
                                            "settings-memory-section",
                                            SettingsSection::Memory,
                                            section,
                                            cx,
                                        ))
                                }),
                        )
                    })
                    .when(SettingsSection::Remote.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-remote-section",
                            SettingsSection::Remote,
                            section,
                            cx,
                        ))
                    })
                    .when(SettingsSection::Data.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-data-section",
                            SettingsSection::Data,
                            section,
                            cx,
                        ))
                    })
                    .when(
                        SettingsSection::BetaFeatures.matches(&settings_query),
                        |nav| {
                            nav.child(Self::section_button(
                                "settings-beta-features",
                                SettingsSection::BetaFeatures,
                                section,
                                cx,
                            ))
                        },
                    )
                    .when(SettingsSection::Process.matches(&settings_query), |nav| {
                        nav.child(Self::section_button(
                            "settings-process-section",
                            SettingsSection::Process,
                            section,
                            cx,
                        ))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .border_t_1()
                            .border_color(crate::ui::design::line(cx))
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child("CHORO SETTINGS"),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .id("settings-content-scroll")
                    .when(contained_page, |content| content.overflow_hidden())
                    .when(!contained_page, |content| content.overflow_y_scroll())
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.))
                            .mx_auto()
                            .when(contained_page, |page| {
                                page.h_full()
                                    .min_h(px(0.))
                                    .max_w(px(960.))
                                    .gap_3()
                                    .px_6()
                                    .py_5()
                            })
                            .when(section == SettingsSection::Process, |page| {
                                page.gap_5().px_6().py_7()
                            })
                            .when(
                                !contained_page && section != SettingsSection::Process,
                                |page| page.max_w(px(1200.)).gap_5().px_8().py_7(),
                            )
                            .when(section == SettingsSection::Companion, |page| {
                                page.max_w(px(960.))
                            })
                            .when(!editing_bandmate, |page| {
                                page.child(
                                    div()
                                        .w_full()
                                        .flex_none()
                                        .child(Self::page_header(section, cx)),
                                )
                            })
                            .child(match section {
                                SettingsSection::Design => self.render_design_section(window, cx),
                                SettingsSection::Brain => self.render_brain_section(window, cx),
                                SettingsSection::Memory => self.render_memory_section(cx),
                                SettingsSection::Voice => self.render_voice_section(cx),
                                SettingsSection::Companion => self.render_companion_section(cx),
                                SettingsSection::Notifications => {
                                    self.render_notifications_section(cx)
                                }
                                SettingsSection::Generation => self.render_generation_page(cx),
                                SettingsSection::Appearance => self.render_appearance_page(cx),
                                SettingsSection::Remote => self.render_remote_page(cx),
                                SettingsSection::Data => self.render_data_page(cx),
                                SettingsSection::AgentSkills => self.render_skills_page(cx),
                                SettingsSection::Experts => self.render_experts_page(window, cx),
                                SettingsSection::BetaFeatures => self.render_beta_features_page(cx),
                                SettingsSection::Orbit => self.render_orbit_page(window, cx),
                                SettingsSection::Process => self.render_process_page(cx),
                                SettingsSection::Shortcuts => self.render_shortcuts(cx),
                            }),
                    ),
            )
    }
}
