use super::*;

const SKILL_ROW_H: f32 = 46.;

/// The trailing group of controls on a skill row.
type RowActions = Div;

impl SettingsView {
    pub(super) fn render_expert_skills(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(editor) = self.expert_editor.as_ref() else {
            return div().into_any_element();
        };
        let profile = editor.profile.clone();
        let custom_open = editor.custom_editor.is_some();
        let expanded = editor.expanded_skill.clone();
        let picker = editor.skill_picker;
        let skill_search = editor.skill_search.clone();
        // These are mutually exclusive surfaces. A document or editor must never
        // expand between list rows, where it would compete with the page scroll.
        if custom_open {
            return self
                .render_custom_skill_editor(window, cx)
                .into_any_element();
        }
        if let Some(picker) = picker {
            return self
                .render_skill_picker(picker, &profile, &skill_search, window, cx)
                .into_any_element();
        }
        if let Some(skill) = expanded.as_deref().and_then(catalog::skill) {
            return render_bundled_skill_details(&skill, window, cx).into_any_element();
        }
        let mut list = style::expert_settings_list_frame(cx);
        let mut count = 0usize;
        for (index, id) in profile.additions.bundled_skills.iter().enumerate() {
            count += 1;
            match catalog::skill(id) {
                Some(skill) => {
                    list = list.child(self.render_bundled_skill_row(index, &skill, cx));
                }
                None => list = list.child(self.render_missing_skill_row(index, id.clone(), cx)),
            }
        }
        for (index, skill) in profile.additions.custom_skills.iter().enumerate() {
            count += 1;
            list = list.child(self.render_custom_skill_row(index, skill.clone(), custom_open, cx));
        }
        for (index, skill) in profile.skills.iter().enumerate() {
            count += 1;
            list = list.child(self.render_installed_skill_row(index, skill, profile.provider, cx));
        }
        for (index, id) in profile.additions.riff_ids.iter().copied().enumerate() {
            count += 1;
            list = list.child(self.render_riff_row(index, id, cx));
        }
        if count == 0 {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .whitespace_normal()
                    .text_size(design::text_ui())
                    .text_color(design::t3(cx))
                    .child("No skills yet. Add a Choro skill, link an installed skill or Riff, or write one for this bandmate."),
            );
        }
        v_flex()
            .w_full()
            .min_w(px(0.))
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .gap_3()
            .child(self.render_skills_header(custom_open, cx))
            .child(
                style::expert_settings_scroll_body("band-attached-skills-scroll", window, cx)
                    .child(list),
            )
            .into_any_element()
    }

    fn render_skills_header(&self, custom_open: bool, cx: &mut Context<Self>) -> Div {
        let view = cx.entity();
        h_flex()
            .w_full()
            .gap_3()
            .items_start()
            .flex_none()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(design::text_head())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(design::t1(cx))
                            .w_full()
                            .child("Skills for this bandmate"),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .w_full()
                            .child("Add reusable guidance, or write your own skill."),
                    ),
            )
            .child(
                style::dialog_neutral_button("expert-add-skill", "Add skill", cx)
                    .icon(IconName::Plus)
                    .disabled(custom_open)
                    .dropdown_menu(move |mut menu, window, _| {
                        menu = menu.item(
                            PopupMenuItem::new("Write a skill for this bandmate").on_click(
                                window.listener_for(&view, |this: &mut Self, _, window, cx| {
                                    this.edit_custom_expert_skill(None, window, cx)
                                }),
                            ),
                        );
                        for (label, choice) in [
                            ("Add a Choro skill", SkillPicker::Bundled),
                            ("Link an installed skill", SkillPicker::Installed),
                            ("Link a Riff", SkillPicker::Riffs),
                        ] {
                            menu =
                                menu.item(PopupMenuItem::new(label).on_click(window.listener_for(
                                    &view,
                                    move |this: &mut Self, _, window, cx| {
                                        if let Some(e) = &mut this.expert_editor {
                                            e.skill_picker = Some(choice);
                                            e.expanded_skill = None;
                                            e.skill_search.update(cx, |input, cx| {
                                                input.set_value("", window, cx)
                                            });
                                        }
                                        cx.notify();
                                    },
                                )));
                        }
                        menu
                    }),
            )
    }

    fn render_bundled_skill_row(
        &self,
        index: usize,
        skill: &catalog::CatalogSkill,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = skill.id.clone();
        let detail_id = skill.id.clone();
        let actions = h_flex()
            .flex_none()
            .gap_1()
            .child(
                style::ghost_button_compact(("expert-skill-details", index), "View")
                    .tooltip("Read the skill and its source details")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(e) = &mut this.expert_editor {
                            e.expanded_skill = Some(detail_id.clone());
                        }
                        cx.notify();
                    })),
            )
            .child(
                style::ghost_button_compact(("expert-skill-detach", index), "Detach").on_click(
                    cx.listener(move |this, _, _, cx| {
                        if let Some(e) = &mut this.expert_editor {
                            e.profile.additions.bundled_skills.retain(|s| s != &id);
                        }
                        cx.notify();
                    }),
                ),
            );
        skill_row(
            ("expert-bundled-row", index),
            badge(IconName::BookOpen, true, cx),
            skill.title.clone(),
            "Choro",
            subline_text(skill.description.clone(), cx),
            None,
            actions,
            cx,
        )
    }

    fn render_missing_skill_row(
        &self,
        index: usize,
        id: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let missing = id.clone();
        let actions = h_flex().flex_none().child(
            style::ghost_button_compact(("expert-missing-detach", index), "Detach").on_click(
                cx.listener(move |this, _, _, cx| {
                    if let Some(e) = &mut this.expert_editor {
                        e.profile.additions.bundled_skills.retain(|s| s != &missing);
                    }
                    cx.notify();
                }),
            ),
        );
        skill_row(
            ("expert-missing-row", index),
            badge(IconName::TriangleAlert, false, cx),
            id,
            "Choro",
            design::indicator::status(
                "Not in this version of Choro. Detach it or update Choro.",
                design::amber(cx),
                cx,
            )
            .min_w(px(0.))
            .overflow_hidden(),
            None,
            actions,
            cx,
        )
    }

    fn render_custom_skill_row(
        &self,
        index: usize,
        skill: ExpertCustomSkill,
        custom_open: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let edit = skill.clone();
        let id = skill.id;
        let actions = h_flex()
            .flex_none()
            .gap_1()
            .child(
                style::ghost_button_compact(("expert-custom-edit", index), "Edit")
                    .disabled(custom_open)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_custom_expert_skill(Some(edit.clone()), window, cx)
                    })),
            )
            .child(
                style::ghost_button_compact(("expert-custom-detach", index), "Detach")
                    .disabled(custom_open)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(e) = &mut this.expert_editor {
                            e.profile.additions.custom_skills.retain(|s| s.id != id);
                        }
                        cx.notify();
                    })),
            );
        let subline = if skill.description.trim().is_empty() {
            "Written for this bandmate only".to_string()
        } else {
            skill.description
        };
        skill_row(
            ("expert-custom-row", index),
            badge(IconName::FileText, true, cx),
            skill.name,
            "Custom",
            subline_text(subline, cx),
            None,
            actions,
            cx,
        )
    }

    fn render_installed_skill_row(
        &self,
        index: usize,
        skill: &ExpertSkill,
        provider: AgentKind,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let path = skill.source.clone();
        let available = path.is_file() && skill.provider == provider;
        let actions = h_flex().flex_none().child(
            style::ghost_button_compact(("expert-linked-detach", index), "Detach").on_click(
                cx.listener(move |this, _, _, cx| {
                    if let Some(e) = &mut this.expert_editor {
                        e.profile.skills.retain(|s| s.source != path);
                    }
                    cx.notify();
                }),
            ),
        );
        let icon = match skill.provider {
            AgentKind::Codex => IconName::SquareTerminal,
            _ => IconName::Bot,
        };
        skill_row(
            ("expert-installed-row", index),
            badge(icon, available, cx),
            skill.name.clone(),
            skill.provider.label(),
            link_status(available, skill.provider != provider, cx),
            None,
            actions,
            cx,
        )
    }

    fn render_riff_row(&self, index: usize, id: Uuid, cx: &mut Context<Self>) -> Stateful<Div> {
        let riff = self.riffs.iter().find(|r| r.id == id);
        let available = riff.is_some_and(|r| r.enabled);
        let actions = h_flex().flex_none().child(
            style::ghost_button_compact(("expert-riff-detach", index), "Detach").on_click(
                cx.listener(move |this, _, _, cx| {
                    if let Some(e) = &mut this.expert_editor {
                        e.profile.additions.riff_ids.retain(|r| *r != id);
                    }
                    cx.notify();
                }),
            ),
        );
        let subline = match riff {
            Some(r) if r.enabled => design::indicator::status("Linked", design::t4(cx), cx),
            Some(_) => design::indicator::status(
                "Disabled in Settings → Skills. Enable it or detach.",
                design::amber(cx),
                cx,
            ),
            None => design::indicator::status(
                "This Riff no longer exists. Detach it.",
                design::amber(cx),
                cx,
            ),
        };
        skill_row(
            ("expert-riff-row", index),
            style::expert_settings_row_badge(px(28.), available, cx).child(style::choro_riff_icon(
                design::icon_md(),
                style::expert_settings_badge_ink(available, cx),
            )),
            riff.map(|r| r.name.clone())
                .unwrap_or_else(|| "Missing Riff".into()),
            "Riff",
            subline.min_w(px(0.)).overflow_hidden(),
            None,
            actions,
            cx,
        )
    }

    fn render_skill_picker(
        &self,
        picker: SkillPicker,
        profile: &ExpertProfile,
        skill_search: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = skill_search.read(cx).value().trim().to_lowercase();
        let choices = self.picker_choices(picker, profile, &query, cx);
        let (title, empty) = match picker {
            SkillPicker::Bundled => ("Choro skills", "No matching Choro skills to add."),
            SkillPicker::Installed => (
                "Installed skills",
                "No matching skills for this provider. Refresh the catalog in Settings → Skills.",
            ),
            SkillPicker::Riffs => ("Riffs", "No matching enabled Riffs to add."),
        };
        let mut rows = v_flex().w_full().min_w(px(0.));
        let total = choices.len();
        for (index, choice) in choices.into_iter().enumerate() {
            rows = rows.child(choice_row(index, choice, cx));
        }
        if total == 0 {
            rows = rows.child(
                div()
                    .py_1()
                    .whitespace_normal()
                    .text_size(design::text_ui())
                    .text_color(design::t3(cx))
                    .child(empty),
            );
        }
        v_flex()
            .w_full()
            .min_w(px(0.))
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(design::t1(cx))
                            .child(title),
                    )
                    .child(
                        style::dialog_neutral_button("expert-picker-done", "Back to skills", cx)
                            .icon(IconName::ArrowLeft)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(e) = &mut this.expert_editor {
                                    e.skill_picker = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                style::expert_settings_input(skill_search)
                    .w_full()
                    .prefix(IconName::Search),
            )
            .child(
                style::expert_settings_scroll_body("expert-skill-choices", window, cx)
                    .child(style::expert_settings_list_frame(cx).child(rows)),
            )
    }

    fn picker_choices(
        &self,
        picker: SkillPicker,
        profile: &ExpertProfile,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Vec<PickerChoice> {
        match picker {
            SkillPicker::Bundled => catalog::catalog()
                .skills
                .iter()
                .filter(|skill| !profile.additions.bundled_skills.contains(&skill.id))
                .filter(|skill| {
                    format!("{} {} {}", skill.title, skill.description, skill.repository)
                        .to_lowercase()
                        .contains(query)
                })
                .map(|skill| {
                    let id = skill.id.clone();
                    PickerChoice {
                        title: skill.title.clone(),
                        description: skill.description.clone(),
                        add: Box::new(cx.listener(move |this, _, _, cx| {
                            if let Some(e) = &mut this.expert_editor {
                                if !e.profile.additions.bundled_skills.contains(&id) {
                                    e.profile.additions.bundled_skills.push(id.clone());
                                }
                            }
                            cx.notify();
                        })),
                    }
                })
                .collect(),
            SkillPicker::Installed => self
                .skills
                .iter()
                .filter(|s| {
                    s.source == crate::state::AgentCapabilitySource::Skill
                        && s.provider == profile.provider
                        && s.enabled
                        && s.matches(query)
                })
                .filter_map(|skill| {
                    let path = crate::state::agent_capabilities::expert_skill_path(
                        skill,
                        &self.skills_cwd,
                    )?;
                    if profile.skills.iter().any(|s| s.source == path) {
                        return None;
                    }
                    let skill = skill.clone();
                    Some(PickerChoice {
                        title: skill.title.clone(),
                        description: skill.description.clone().unwrap_or_default(),
                        add: Box::new(cx.listener(move |this, _, _, cx| {
                            if let Some(e) = &mut this.expert_editor {
                                if !e.profile.skills.iter().any(|s| s.source == path) {
                                    e.profile.skills.push(ExpertSkill {
                                        provider: skill.provider,
                                        name: skill.name.clone(),
                                        source: path.clone(),
                                    });
                                }
                            }
                            cx.notify();
                        })),
                    })
                })
                .collect(),
            SkillPicker::Riffs => self
                .riffs
                .iter()
                .filter(|r| {
                    r.enabled && r.matches(query) && !profile.additions.riff_ids.contains(&r.id)
                })
                .map(|riff| {
                    let id = riff.id;
                    PickerChoice {
                        title: riff.name.clone(),
                        description: riff.description.clone().unwrap_or_default(),
                        add: Box::new(cx.listener(move |this, _, _, cx| {
                            if let Some(e) = &mut this.expert_editor {
                                if !e.profile.additions.riff_ids.contains(&id) {
                                    e.profile.additions.riff_ids.push(id);
                                }
                            }
                            cx.notify();
                        })),
                    }
                })
                .collect(),
        }
    }
}

type AddHandler = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static>;

struct PickerChoice {
    title: String,
    description: String,
    add: AddHandler,
}

fn choice_row(index: usize, choice: PickerChoice, cx: &App) -> Div {
    h_flex()
        .w_full()
        .min_w(px(0.))
        .gap_2()
        .items_center()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(design::line(cx).opacity(0.18))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(design::text_ui())
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(design::t1(cx))
                        .child(choice.title),
                )
                .when(!choice.description.trim().is_empty(), |col| {
                    col.child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child(choice.description),
                    )
                }),
        )
        .child(
            style::ghost_button_compact(("expert-skill-choice-add", index), "Add")
                .icon(IconName::Plus)
                .on_click(choice.add),
        )
}

fn badge(icon: IconName, accent: bool, cx: &App) -> Div {
    style::expert_settings_row_badge(px(28.), accent, cx).child(
        Icon::new(icon)
            .size(design::icon_md())
            .text_color(style::expert_settings_badge_ink(accent, cx)),
    )
}

fn subline_text(text: String, cx: &App) -> Div {
    div()
        .min_w(px(0.))
        .truncate()
        .text_size(design::text_ui())
        .text_color(design::t3(cx))
        .child(text)
}

fn link_status(available: bool, wrong_provider: bool, cx: &App) -> Div {
    let (label, color) = if available {
        ("Linked", design::t4(cx))
    } else if wrong_provider {
        (
            "Belongs to another provider. Replace or detach before saving.",
            design::amber(cx),
        )
    } else {
        (
            "Skill file not found. Refresh Settings → Skills or detach.",
            design::amber(cx),
        )
    };
    design::indicator::status(label, color, cx)
        .min_w(px(0.))
        .overflow_hidden()
}

#[allow(clippy::too_many_arguments)]
fn skill_row(
    id: impl Into<gpui::ElementId>,
    badge: Div,
    title: String,
    kind: &'static str,
    subline: Div,
    trailing_meta: Option<String>,
    actions: RowActions,
    cx: &App,
) -> Stateful<Div> {
    style::expert_settings_list_row(id, cx)
        .min_h(px(SKILL_ROW_H))
        .py_1p5()
        .child(badge)
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
                                .text_size(design::text_body())
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(design::t1(cx))
                                .child(title),
                        )
                        .child(style::expert_settings_source_chip(kind, cx)),
                )
                .child(subline.w_full()),
        )
        .when_some(trailing_meta, |row, meta| {
            row.child(
                div()
                    .flex_none()
                    .max_w(px(200.))
                    .truncate()
                    .text_size(design::text_label())
                    .text_color(design::t4(cx))
                    .child(meta),
            )
        })
        .child(actions)
}

fn render_bundled_skill_details(
    skill: &catalog::CatalogSkill,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Div {
    let content = catalog::files(&skill.id)
        .ok()
        .and_then(|files| files.get("SKILL.md").map(|f| skill_document_body(&f.content).to_string()))
        .unwrap_or_else(|| "The bundled instructions could not be read. Reinstall this Choro build to restore them.".to_string());
    let source = skill.upstream.clone();
    let note = if skill.changes.trim().is_empty() {
        "Original instructions, included unchanged.".to_string()
    } else {
        skill.changes.clone()
    };
    v_flex()
        .w_full()
        .min_w(px(0.))
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .gap_3()
        .child(
            h_flex()
                .w_full()
                .min_w(px(0.))
                .flex_none()
                .gap_2()
                .child(
                    style::dialog_neutral_button("band-skill-back", "Back to skills", cx)
                        .icon(IconName::ArrowLeft)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = &mut this.expert_editor {
                                editor.expanded_skill = None;
                            }
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    style::ghost_button_compact("band-skill-source", "View source")
                        .icon(IconName::ExternalLink)
                        .on_click(move |_, _, cx| cx.open_url(&source)),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .min_w(px(0.))
                .flex_none()
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(design::text_head())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(design::t1(cx))
                        .child(skill.title.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .text_size(design::text_ui())
                        .text_color(design::t3(cx))
                        .child(format!("{} • {} license", skill.repository, skill.license)),
                )
                .child(
                    div()
                        .w_full()
                        .text_size(design::text_ui())
                        .text_color(design::t3(cx))
                        .child(note),
                ),
        )
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .rounded(design::r_md())
                .border_1()
                .border_color(design::line(cx))
                .bg(design::surface(cx).opacity(0.32))
                .p_3()
                .child(
                    TextView::markdown(
                        gpui::ElementId::Name(format!("band-skill-document-{}", skill.id).into()),
                        content,
                        window,
                        cx,
                    )
                    .selectable(true)
                    .scrollable(true)
                    .size_full()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_size(design::text_ui())
                    .style(skill_markdown_style()),
                ),
        )
}

/// The catalog already presents title, source and license. YAML is metadata,
/// not part of the readable instructions. Leave malformed/non-leading fences alone.
fn skill_document_body(content: &str) -> &str {
    let source = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = source.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return content;
    }
    let mut offset = source.len() - lines.clone().map(str::len).sum::<usize>();
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return source[offset..].trim_start_matches(['\r', '\n']);
        }
    }
    content
}

fn skill_markdown_style() -> TextViewStyle {
    TextViewStyle::default()
        .paragraph_gap(gpui::rems(0.6))
        .heading_font_size(|level, _| match level {
            1 => design::text_head(),
            _ => design::text_body(),
        })
}

#[cfg(test)]
mod tests {
    use super::skill_document_body;

    #[test]
    fn reader_hides_only_complete_leading_metadata() {
        assert_eq!(
            skill_document_body(
                "---\nname: demo\ndescription: A skill\n---\n\n# Instructions\nText"
            ),
            "# Instructions\nText"
        );
        assert_eq!(
            skill_document_body("\u{feff}---\r\nname: demo\r\n...\r\n# Instructions"),
            "# Instructions"
        );
        for text in [
            "# Instructions\n---\nText",
            "---\nname: unfinished",
            "Ordinary instructions",
        ] {
            assert_eq!(skill_document_body(text), text);
        }
    }

    #[test]
    fn bundled_readers_keep_instruction_bodies_and_relative_references() {
        for skill in &ide_core::experts::catalog::catalog().skills {
            let files = ide_core::experts::catalog::files(&skill.id).unwrap();
            let content = &files.get("SKILL.md").unwrap().content;
            let body = skill_document_body(content);
            assert!(!body.trim().is_empty(), "{} has an empty reader", skill.id);
            assert!(content.ends_with(body));
            assert!(!body.starts_with("---\nname:"));
        }
    }
}
