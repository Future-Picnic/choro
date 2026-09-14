use super::editor::{field, multiline_field, multiline_input, text_input};
use super::*;

const CUSTOM_INSTRUCTIONS_H: f32 = 240.;
const NAME_MAX_CHARS: usize = 80;
const INSTRUCTIONS_MAX_BYTES: usize = 64_000;
const DESCRIPTION_MAX_BYTES: usize = 4_000;

impl SettingsView {
    pub(super) fn edit_custom_expert_skill(
        &mut self,
        skill: Option<ExpertCustomSkill>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let skill = skill.unwrap_or(ExpertCustomSkill {
            id: Uuid::new_v4(),
            name: String::new(),
            description: String::new(),
            instructions: String::new(),
        });
        let name = text_input(&skill.name, "Follow our design system", window, cx);
        let description = text_input(
            &skill.description,
            "When should this skill apply?",
            window,
            cx,
        );
        let instructions = multiline_input(
            &skill.instructions,
            "What should this bandmate do when the skill applies?",
            7,
            window,
            cx,
        );
        if let Some(editor) = &mut self.expert_editor {
            editor.custom_editor = Some(CustomSkillEditor {
                id: skill.id,
                name,
                description,
                instructions,
            });
            editor.skill_picker = None;
            editor.expanded_skill = None;
            editor.tab = ExpertEditorTab::Skills;
        }
        self.experts_status = None;
        cx.notify();
    }

    fn validate_custom_skill(&self, skill: &ExpertCustomSkill) -> Result<(), ExpertsNotice> {
        if skill.name.is_empty()
            || skill.name.chars().count() > NAME_MAX_CHARS
            || skill.instructions.trim().is_empty()
            || skill.instructions.len() > INSTRUCTIONS_MAX_BYTES
            || skill.description.len() > DESCRIPTION_MAX_BYTES
        {
            return Err(ExpertsNotice::error(
                "Give the skill a name and instructions. Keep its name under 81 characters and its instructions under 64 KB.",
            ));
        }
        let duplicate = self.expert_editor.as_ref().is_some_and(|editor| {
            editor.profile.additions.custom_skills.iter().any(|s| {
                s.id != skill.id
                    && ide_core::experts::normalized_expert_name(&s.name)
                        == ide_core::experts::normalized_expert_name(&skill.name)
            })
        });
        if duplicate {
            return Err(ExpertsNotice::error(
                "This Bandmate already has a custom skill with that name.",
            ));
        }
        Ok(())
    }

    pub(super) fn save_custom_expert_skill(&mut self, cx: &mut Context<Self>) {
        let Some(custom) = self
            .expert_editor
            .as_ref()
            .and_then(|editor| editor.custom_editor.as_ref())
        else {
            return;
        };
        let skill = ExpertCustomSkill {
            id: custom.id,
            name: custom.name.read(cx).value().trim().to_string(),
            description: custom.description.read(cx).value().to_string(),
            instructions: custom.instructions.read(cx).value().to_string(),
        };
        if let Err(notice) = self.validate_custom_skill(&skill) {
            self.experts_status = Some(notice);
            cx.notify();
            return;
        }
        if let Some(editor) = &mut self.expert_editor {
            let skills = &mut editor.profile.additions.custom_skills;
            match skills.iter_mut().find(|s| s.id == skill.id) {
                Some(existing) => *existing = skill,
                None => skills.push(skill),
            }
            editor.custom_editor = None;
        }
        self.experts_status = None;
        cx.notify();
    }

    pub(super) fn render_custom_skill_editor(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(custom) = self
            .expert_editor
            .as_ref()
            .and_then(|editor| editor.custom_editor.as_ref())
        else {
            return div();
        };
        let is_new = self.expert_editor.as_ref().is_some_and(|editor| {
            !editor
                .profile
                .additions
                .custom_skills
                .iter()
                .any(|s| s.id == custom.id)
        });
        let fields = v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_4()
            .pb_2()
            .child(field("Name", &custom.name, cx))
            .child(field("When to use", &custom.description, cx))
            .child(multiline_field(
                "Instructions",
                "Used only by this bandmate. Markdown is supported.",
                &custom.instructions,
                CUSTOM_INSTRUCTIONS_H,
                cx,
            ));
        v_flex()
            .w_full()
            .min_w(px(0.))
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .gap_3()
            .child(
                v_flex()
                    .w_full()
                    .flex_none()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .text_size(design::text_head())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(design::t1(cx))
                            .child(if is_new {
                                "Write a skill"
                            } else {
                                "Edit skill"
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child("Apply the skill here, then save your bandmate to keep it."),
                    ),
            )
            .child(
                style::expert_settings_scroll_body("band-custom-skill-scroll", window, cx)
                    .child(fields),
            )
            .when_some(self.experts_status.clone(), |panel, notice| {
                panel.child(div().w_full().flex_none().child(notice.render(cx)))
            })
            .child(
                style::expert_settings_editor_footer(cx)
                    .child(
                        style::dialog_neutral_button("cancel-expert-skill", "Cancel", cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                if let Some(e) = &mut this.expert_editor {
                                    e.custom_editor = None;
                                }
                                this.experts_status = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        style::primary_button_compact("save-expert-skill", "Apply skill", cx)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.save_custom_expert_skill(cx)),
                            ),
                    ),
            )
    }
}
