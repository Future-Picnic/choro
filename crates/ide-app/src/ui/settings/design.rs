use super::*;
use crate::ui::{design as tokens, style};
use gpui::Div;
use ide_core::studio::{StudioSettings, StudioSkill, STUDIO_BUILTIN_SKILLS};

#[cfg(all(test, feature = "ui-layout-tests"))]
#[path = "studio_layout_tests.rs"]
mod layout_tests;

pub(super) struct StudioSettingsEditor {
    pub preferences: Entity<InputState>,
    pub search: Entity<InputState>,
    picker: Option<SkillSource>,
    detail: Option<(String, String)>,
    custom: Option<StudioSkillEditor>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum SkillSource {
    Installed,
    Riffs,
}

struct StudioSkillEditor {
    skill: StudioSkill,
    name: Entity<InputState>,
    description: Entity<InputState>,
    instructions: Entity<InputState>,
}

impl StudioSettingsEditor {
    pub fn new(preferences: &str, window: &mut Window, cx: &mut App) -> Self {
        Self {
            preferences: cx.new(|cx| InputState::new(window, cx).multi_line(true).rows(4)
                .default_value(preferences.to_string()).placeholder("For example: Keep layouts compact, use subtle motion, and write short button labels.")),
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search skills…")),
            picker: None, detail: None, custom: None, error: None,
        }
    }
}

fn heading(title: impl Into<SharedString>, description: impl Into<SharedString>, cx: &App) -> Div {
    v_flex()
        .flex_none()
        .min_w(px(0.))
        .gap_1()
        .child(
            div()
                .text_size(tokens::text_body())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::t1(cx))
                .child(title.into()),
        )
        .child(hint(description, cx))
}

fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .w_full()
        .min_w(px(0.))
        .whitespace_normal()
        .text_size(tokens::text_ui())
        .text_color(tokens::t3(cx))
        .child(text.into())
}

fn skill_copy(name: String, description: String, source: &str, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .min_w(px(0.))
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .min_w(px(0.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(tokens::text_ui())
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                )
                .child(style::expert_settings_source_chip(source.to_string(), cx)),
        )
        .child(hint(description, cx))
}

impl SettingsView {
    fn persist_studio_settings(
        &mut self,
        settings: StudioSettings,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = self.workspace.update(cx, |workspace, cx| {
            workspace.set_studio_settings(settings, cx)
        });
        self.studio_settings.error = result
            .as_ref()
            .err()
            .map(|e| format!("Could not save Studio settings: {e:#}"));
        cx.notify();
        result.is_ok()
    }

    fn add_studio_skill(&mut self, skill: StudioSkill, cx: &mut Context<Self>) {
        let mut settings = self.workspace.read(cx).studio.clone();
        settings.skills.push(skill);
        self.persist_studio_settings(settings, cx);
    }

    fn edit_studio_skill(
        &mut self,
        skill: Option<StudioSkill>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = Uuid::new_v4();
        let skill = skill.unwrap_or(StudioSkill {
            id,
            name: String::new(),
            description: String::new(),
            instructions: String::new(),
            source: "Custom".into(),
            source_key: format!("custom:{id}"),
            source_directory: None,
        });
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(&skill.name)
                .placeholder("For example: Accessible forms")
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(&skill.description)
                .placeholder("When should Studio use this skill?")
        });
        let instructions = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(10)
                .default_value(&skill.instructions)
                .placeholder("Describe the design guidance Studio should follow…")
        });
        for input in [&name, &description, &instructions] {
            cx.subscribe(input, |_: &mut Self, _, _: &InputEvent, cx| cx.notify())
                .detach();
        }
        name.update(cx, |input, cx| input.focus(window, cx));
        self.studio_settings.custom = Some(StudioSkillEditor {
            skill,
            name,
            description,
            instructions,
        });
        self.studio_settings.picker = None;
        self.studio_settings.error = None;
        cx.notify();
    }

    pub(super) fn render_design_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let content = if self.studio_settings.custom.is_some() {
            self.render_studio_skill_editor(window, cx)
        } else if let Some((title, content)) = self.studio_settings.detail.clone() {
            v_flex()
                .size_full()
                .min_h(px(0.))
                .gap_3()
                .child(self.studio_settings_back(title, cx))
                .child(
                    div().flex_1().min_h(px(0.)).min_w(px(0.)).child(
                        TextView::markdown(
                            "studio-skill-detail",
                            skill_document_body(&content).to_string(),
                            window,
                            cx,
                        )
                        .selectable(true)
                        .scrollable(true)
                        .size_full()
                        .text_size(tokens::text_ui())
                        .style(skill_markdown_style()),
                    ),
                )
        } else if let Some(source) = self.studio_settings.picker {
            self.render_studio_skill_picker(source, window, cx)
        } else {
            self.render_studio_settings_overview(window, cx)
        };
        v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .when_some(self.studio_settings.error.clone(), |body, error| {
                body.child(style::expert_settings_error_notice(error, cx))
            })
            .child(content)
            .into_any_element()
    }

    fn studio_settings_back(&self, title: String, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .flex_none()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(tokens::text_body())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                style::dialog_neutral_button("studio-settings-back", "Back to Studio", cx)
                    .icon(IconName::ArrowLeft)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.studio_settings.picker = None;
                        this.studio_settings.detail = None;
                        this.studio_settings.error = None;
                        cx.notify();
                    })),
            )
    }

    fn render_studio_settings_overview(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let settings = self.workspace.read(cx).studio.clone();
        let preferences = self.studio_settings.preferences.clone();
        let dirty = preferences.read(cx).value().as_ref() != settings.preferences;
        let too_long = preferences.read(cx).value().len() > ide_core::studio::PREFERENCES_MAX_BYTES;
        let mut builtins = style::expert_settings_list_frame(cx);
        for (index, skill) in STUDIO_BUILTIN_SKILLS.iter().enumerate() {
            builtins = builtins.child(
                style::expert_settings_list_row(("studio-builtin", index), cx)
                    .child(skill_copy(
                        skill.name.into(),
                        skill.description.into(),
                        "Built-in",
                        cx,
                    ))
                    .child(
                        style::ghost_button_compact(("studio-builtin-view", index), "View")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.studio_settings.detail =
                                    Some((skill.name.into(), skill.instructions.into()));
                                cx.notify();
                            })),
                    ),
            );
        }
        let mut added = style::expert_settings_list_frame(cx);
        if settings.skills.is_empty() {
            added = added.child(div().p_3().child(hint("No additional skills. Add an installed skill, copy a Riff, or write guidance for Studio.", cx)));
        }
        for (index, skill) in settings.skills.iter().enumerate() {
            let edit = skill.clone();
            let id = skill.id;
            added = added.child(
                style::expert_settings_list_row(("studio-added", index), cx)
                    .child(skill_copy(
                        skill.name.clone(),
                        if skill.description.is_empty() {
                            "Used when relevant to your design request.".into()
                        } else {
                            skill.description.clone()
                        },
                        &skill.source,
                        cx,
                    ))
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(
                                style::ghost_button_compact(("studio-skill-edit", index), "Edit")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.edit_studio_skill(Some(edit.clone()), window, cx)
                                    })),
                            )
                            .child(
                                style::ghost_button_compact(
                                    ("studio-skill-detach", index),
                                    "Detach",
                                )
                                .tooltip("Stop using this copy in Studio; keep the original skill")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let mut settings = this.workspace.read(cx).studio.clone();
                                        settings.skills.retain(|skill| skill.id != id);
                                        this.persist_studio_settings(settings, cx);
                                    },
                                )),
                            ),
                    ),
            );
        }
        let view = cx.entity();
        let add = style::dialog_neutral_button("studio-add-skill", "Add skill", cx)
            .icon(IconName::Plus)
            .dropdown_menu(move |menu, window, _| {
                let menu = menu.item(PopupMenuItem::new("Write a skill for Studio").on_click(
                    window.listener_for(&view, |this: &mut Self, _, window, cx| {
                        this.edit_studio_skill(None, window, cx)
                    }),
                ));
                let mut menu = menu;
                for (label, source) in [
                    ("Copy an installed skill", SkillSource::Installed),
                    ("Copy a Riff", SkillSource::Riffs),
                ] {
                    menu = menu.item(PopupMenuItem::new(label).on_click(window.listener_for(
                        &view,
                        move |this: &mut Self, _, window, cx| {
                            this.studio_settings.picker = Some(source);
                            this.studio_settings.error = None;
                            this.riffs = ChoroRiffStore::load().riffs;
                            this.studio_settings.search.update(cx, |input, cx| {
                                input.set_value("", window, cx);
                                input.focus(window, cx);
                            });
                            cx.notify();
                        },
                    )));
                }
                menu
            });
        v_flex().flex_1().min_h(px(0.)).w_full().child(style::expert_settings_scroll_body("studio-settings-overview", window, cx).gap_5()
            .child(hint("Applies to Studio screen designs in every project, starting with your next request. Project instructions and the design system take priority.", cx))
            .child(v_flex().flex_none().gap_3()
                .child(heading("Built-in guidance", "Always active. Studio uses these instructions to design and review your screens.", cx))
                .child(builtins))
            .child(v_flex().flex_none().gap_3()
                .child(h_flex().gap_3().items_start().child(heading("Additional skills", "Add reusable design guidance. Copies are editable here; originals stay unchanged.", cx).flex_1()).child(add))
                .child(added))
            .child(v_flex().flex_none().gap_3()
                .child(heading("Design preferences", "Tell Studio how you like to work. Leave this blank to follow its built-in guidance and your project.", cx))
                .child(style::expert_settings_multiline_input(&preferences).h(px(120.)))
                .when(too_long, |body| body.child(style::expert_settings_error_notice("Keep design preferences under 8 KB.", cx)))
                .child(h_flex().gap_2().justify_end()
                    .when(dirty, |row| row.child(style::expert_settings_field_hint("Unsaved changes", cx)))
                    .child(style::dialog_neutral_button("studio-preferences-revert", "Revert", cx).disabled(!dirty)
                        .on_click(cx.listener(|this, _, window, cx| {
                            let saved = this.workspace.read(cx).studio.preferences.clone();
                            this.studio_settings.preferences.update(cx, |input, cx| input.set_value(saved, window, cx));
                            this.studio_settings.error = None;
                            cx.notify();
                        })))
                    .child(style::primary_button_compact("studio-preferences-save", "Save preferences", cx).disabled(!dirty || too_long)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let mut settings = this.workspace.read(cx).studio.clone();
                            settings.preferences = this.studio_settings.preferences.read(cx).value().to_string();
                            this.persist_studio_settings(settings, cx);
                        })))))
            )
    }

    fn render_studio_skill_picker(
        &self,
        source: SkillSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self
            .studio_settings
            .search
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let settings = &self.workspace.read(cx).studio;
        let mut choices: Vec<(String, String, String, String, Option<StudioSkill>)> = Vec::new();
        match source {
            SkillSource::Installed => {
                for skill in &self.skills {
                    if !skill.enabled
                        || skill.source
                            != crate::state::agent_capabilities::AgentCapabilitySource::Skill
                        || !skill.matches(&query)
                    {
                        continue;
                    }
                    let path = crate::state::agent_capabilities::expert_skill_path(
                        skill,
                        &self.skills_cwd,
                    );
                    let key = format!(
                        "installed:{}:{}",
                        skill.provider.label(),
                        path.as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| skill.name.clone())
                    );
                    // File content is read only on Add, never during render.
                    let copy = path.map(|path| StudioSkill {
                        id: Uuid::new_v4(),
                        name: skill.title.clone(),
                        description: skill.description.clone().unwrap_or_default(),
                        instructions: String::new(),
                        source: skill.provider.label().into(),
                        source_key: key.clone(),
                        source_directory: Some(path),
                    });
                    choices.push((
                        skill.title.clone(),
                        skill.description.clone().unwrap_or_default(),
                        skill.provider.label().into(),
                        key,
                        copy,
                    ));
                }
            }
            SkillSource::Riffs => {
                for riff in &self.riffs {
                    if !riff.enabled || !riff.matches(&query) {
                        continue;
                    }
                    let key = format!("riff:{}", riff.id);
                    choices.push((
                        riff.name.clone(),
                        riff.description.clone().unwrap_or_default(),
                        "Riff".into(),
                        key.clone(),
                        Some(StudioSkill {
                            id: Uuid::new_v4(),
                            name: riff.name.clone(),
                            description: riff.description.clone().unwrap_or_default(),
                            instructions: riff.instructions.clone(),
                            source: "Riff".into(),
                            source_key: key,
                            source_directory: None,
                        }),
                    ));
                }
            }
        }
        let mut rows = style::expert_settings_list_frame(cx);
        if choices.is_empty() {
            let empty = if !query.is_empty() {
                "No matching skills. Try another search."
            } else if matches!(source, SkillSource::Installed) && self.skills_loading {
                "Loading installed skills…"
            } else {
                "No skills available here. Manage skills to create a Riff or see your installed skills."
            };
            rows = rows.child(div().p_3().child(hint(empty, cx)));
        }
        for (index, (name, description, origin, key, copy)) in choices.into_iter().enumerate() {
            let added = settings.skills.iter().any(|s| s.source_key == key);
            let available = copy.is_some();
            rows = rows.child(
                style::expert_settings_list_row(("studio-choice", index), cx)
                    .child(skill_copy(
                        name,
                        if available {
                            description
                        } else {
                            "Source file unavailable. Refresh skills and try again.".into()
                        },
                        &origin,
                        cx,
                    ))
                    .child(
                        style::dialog_neutral_button(
                            ("studio-choice-add", index),
                            if added { "Added" } else { "Add" },
                            cx,
                        )
                        .disabled(added || !available)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let Some(mut skill) = copy.clone() else {
                                return;
                            };
                            if let Some(path) = skill.source_directory.take() {
                                let result = (|| -> anyhow::Result<String> {
                                    anyhow::ensure!(
                                        std::fs::metadata(&path)?.len()
                                            <= ide_core::studio::SKILL_MAX_BYTES as u64,
                                        "This skill exceeds 64 KB. Copy a shorter version instead."
                                    );
                                    Ok(std::fs::read_to_string(&path)?)
                                })();
                                match result {
                                    Ok(content) => {
                                        skill.instructions = content;
                                        skill.source_directory = path.parent().map(PathBuf::from);
                                    }
                                    Err(error) => {
                                        this.studio_settings.error =
                                            Some(format!("Could not read skill: {error:#}"));
                                        cx.notify();
                                        return;
                                    }
                                }
                            }
                            this.add_studio_skill(skill, cx);
                        })),
                    ),
            );
        }
        let title = match source {
            SkillSource::Installed => "Installed skills",
            SkillSource::Riffs => "Riffs",
        };
        v_flex().flex_1().min_h(px(0.)).w_full().min_w(px(0.)).gap_3()
            .child(self.studio_settings_back(title.into(), cx))
            .child(hint("Copy instructions into Studio for either provider. This does not install tools or update the copy when its original changes.", cx))
            .child(h_flex().gap_2()
                .child(style::expert_settings_input(&self.studio_settings.search).w_full().prefix(IconName::Search))
                .child(style::refresh_icon_button("studio-skills-refresh", cx).disabled(self.skills_loading).tooltip("Refresh skills")
                    .on_click(cx.listener(|this, _, _, cx| { this.refresh_agent_skills(cx); this.riffs = ChoroRiffStore::load().riffs; }))))
            .when_some(self.skills_error.clone().filter(|_| matches!(source, SkillSource::Installed)), |body, error| body.child(style::expert_settings_error_notice(error, cx)))
            .child(style::expert_settings_scroll_body("studio-skill-picker", window, cx).child(rows))
            .child(style::expert_settings_editor_footer(cx).child(style::dialog_neutral_button("studio-manage-skills", "Manage skills", cx)
                .on_click(cx.listener(|this, _, _, cx| { this.section = SettingsSection::AgentSkills; cx.notify(); }))))
    }

    fn render_studio_skill_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let editor = self.studio_settings.custom.as_ref().unwrap();
        let is_new = !self
            .workspace
            .read(cx)
            .studio
            .skills
            .iter()
            .any(|s| s.id == editor.skill.id);
        let invalid = editor.name.read(cx).value().trim().is_empty()
            || editor.instructions.read(cx).value().trim().is_empty();
        let fields = v_flex()
            .gap_4()
            .w_full()
            .min_w(px(0.))
            .child(
                v_flex()
                    .gap_1()
                    .child(style::expert_settings_field_label("Name", cx))
                    .child(style::expert_settings_input(&editor.name).w_full()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(style::expert_settings_field_label("When to use", cx))
                    .child(style::expert_settings_input(&editor.description).w_full()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(style::expert_settings_field_label("Instructions", cx))
                    .child(
                        style::expert_settings_multiline_input(&editor.instructions).h(px(260.)),
                    ),
            );
        v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .child(heading(
                if is_new {
                    "Write a skill"
                } else {
                    "Edit Studio skill"
                },
                "Used only by Studio. Markdown is supported. Studio’s core rules remain active.",
                cx,
            ))
            .child(
                style::expert_settings_scroll_body("studio-skill-editor", window, cx).child(fields),
            )
            .child(
                style::expert_settings_editor_footer(cx)
                    .child(
                        style::dialog_neutral_button("studio-skill-cancel", "Cancel", cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.studio_settings.custom = None;
                                this.studio_settings.error = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        style::primary_button_compact(
                            "studio-skill-save",
                            if is_new { "Add skill" } else { "Save skill" },
                            cx,
                        )
                        .disabled(invalid)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let Some(editor) = &this.studio_settings.custom else {
                                return;
                            };
                            let mut skill = editor.skill.clone();
                            skill.name = editor.name.read(cx).value().trim().to_string();
                            skill.description =
                                editor.description.read(cx).value().trim().to_string();
                            skill.instructions =
                                editor.instructions.read(cx).value().trim().to_string();
                            let mut settings = this.workspace.read(cx).studio.clone();
                            match settings.skills.iter_mut().find(|s| s.id == skill.id) {
                                Some(existing) => *existing = skill,
                                None => settings.skills.push(skill),
                            }
                            if this.persist_studio_settings(settings, cx) {
                                this.studio_settings.custom = None;
                            }
                        })),
                    ),
            )
    }
}
