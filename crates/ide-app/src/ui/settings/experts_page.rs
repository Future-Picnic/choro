use super::*;
use crate::ui::design;
use crate::ui::style;
use gpui::{Div, Stateful};
use ide_core::experts::{catalog, ExpertCustomSkill, ExpertProfile, ExpertSkill};

mod custom_skill;
mod editor;
#[cfg(all(test, feature = "ui-layout-tests"))]
mod layout_tests;
mod limits;
mod skill_picker;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SkillPicker {
    Bundled,
    Installed,
    Riffs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpertEditorTab {
    Overview,
    Skills,
}

struct CustomSkillEditor {
    id: Uuid,
    name: Entity<InputState>,
    description: Entity<InputState>,
    instructions: Entity<InputState>,
}

pub(super) struct ExpertEditor {
    profile: ExpertProfile,
    name: Entity<InputState>,
    description: Entity<InputState>,
    instructions: Entity<InputState>,
    outcome: Entity<InputState>,
    skill_search: Entity<InputState>,
    skill_picker: Option<SkillPicker>,
    custom_editor: Option<CustomSkillEditor>,
    expanded_skill: Option<String>,
    tab: ExpertEditorTab,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoticeTone {
    Info,
    Error,
}

/// A page-level message. Errors keep their own treatment so a failed save or
/// an invalid skill never reads like a confirmation.
#[derive(Clone)]
pub(super) struct ExpertsNotice {
    text: String,
    tone: NoticeTone,
}

impl ExpertsNotice {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: NoticeTone::Info,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: NoticeTone::Error,
        }
    }

    fn render(&self, cx: &App) -> gpui::AnyElement {
        match self.tone {
            NoticeTone::Info => style::expert_settings_info_notice(self.text.clone(), cx),
            NoticeTone::Error => style::expert_settings_error_notice(self.text.clone(), cx),
        }
        .into_any_element()
    }
}

/// Text shown when there is no active Expert or no search match.
fn expert_list_empty_text(query: &str) -> String {
    if query.is_empty() {
        "No bandmates yet. Create one, then choose it with / in a new chat or ask your lead to delegate to it by name.".to_string()
    } else {
        format!("No matches for {query}")
    }
}

fn expert_initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_string())
}

fn expert_meta(profile: &ExpertProfile) -> String {
    format!(
        "{} · {} · {}",
        profile.provider.label(),
        profile.model.label(),
        profile.effort.label()
    )
}

impl SettingsView {
    pub(super) fn render_experts_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.expert_editor.is_some() {
            return self.render_expert_editor(window, cx);
        }
        let query = self.experts_search.read(cx).value().trim().to_lowercase();
        let active = self
            .experts
            .iter()
            .filter(|e| !e.archived)
            .cloned()
            .collect::<Vec<_>>();
        let enabled = active.iter().filter(|e| e.enabled).count();
        let shown = active
            .iter()
            .filter(|e| expert_matches(e, &query))
            .cloned()
            .collect::<Vec<_>>();
        let summary = if query.is_empty() {
            format!("{} Bandmates · {enabled} enabled", active.len())
        } else {
            format!("{} of {} Bandmates shown", shown.len(), active.len())
        };
        let mut list = style::expert_settings_list_frame(cx);
        if shown.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .whitespace_normal()
                    .text_size(design::text_body())
                    .text_color(design::t3(cx))
                    .child(expert_list_empty_text(&query)),
            );
        }
        for (index, profile) in shown.into_iter().enumerate() {
            list = list.child(self.render_expert_row(index, profile, cx));
        }
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .child(self.render_delegation_settings(cx))
            .child(self.render_experts_toolbar(summary, cx))
            .when_some(self.experts_status.clone(), |page, notice| {
                page.child(notice.render(cx))
            })
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .child(style::expert_settings_scroll_body("band-list-scroll", window, cx).child(list))
            .into_any_element()
    }

    fn render_experts_toolbar(&self, summary: String, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .items_center()
            .child(
                div().flex_1().min_w(px(0.)).max_w(px(360.)).child(
                    style::expert_settings_input(&self.experts_search)
                        .w_full()
                        .prefix(IconName::Search),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(design::text_ui())
                    .text_color(design::t4(cx))
                    .child(summary),
            )
            .child(
                style::primary_button_compact("new-expert", "New bandmate", cx)
                    .icon(IconName::Plus)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.edit_expert(None, window, cx)),
                    ),
            )
    }

    fn render_expert_row(
        &self,
        index: usize,
        profile: ExpertProfile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let edit = profile.clone();
        let toggle = profile.clone();
        let archive = profile.clone();
        let view = cx.entity();
        expert_row_frame(index, &profile, cx).child(
            h_flex()
                .flex_none()
                .gap_1()
                .child(
                    style::dialog_neutral_button(
                        ("toggle-expert", index),
                        if profile.enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        },
                        cx,
                    )
                    .tooltip(if profile.enabled {
                        "Disable this bandmate for new chats and tasks"
                    } else {
                        "Enable this bandmate for new chats and tasks"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let profile = ExpertProfile {
                            enabled: !toggle.enabled,
                            ..toggle.clone()
                        };
                        this.update_expert_profile(profile, cx);
                    })),
                )
                .child(
                    style::dialog_neutral_button(("edit-expert", index), "Edit", cx).on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.edit_expert(Some(edit.clone()), window, cx)
                        }),
                    ),
                )
                .child(
                    style::settings_inline_icon_button(("expert-more", index), IconName::Ellipsis)
                        .tooltip("More actions")
                        .dropdown_menu(move |menu, window, _| {
                            let archive = archive.clone();
                            menu.item(PopupMenuItem::new("Archive Bandmate").on_click(
                                window.listener_for(&view, move |this: &mut Self, _, _, cx| {
                                    let profile = ExpertProfile {
                                        archived: true,
                                        enabled: false,
                                        ..archive.clone()
                                    };
                                    this.update_expert_profile(profile, cx);
                                }),
                            ))
                        }),
                ),
        )
    }

    fn update_expert_profile(&mut self, profile: ExpertProfile, cx: &mut Context<Self>) {
        match LocalStore::open_default().and_then(|s| {
            s.save_expert(profile.clone(), Some(profile.revision))?;
            s.load_experts()
        }) {
            Ok(profiles) => self.experts = profiles,
            Err(e) => self.experts_status = Some(ExpertsNotice::error(e.to_string())),
        }
        cx.notify();
    }
}

/// Keep descriptive text flexible while the row's action group retains its width.
/// Used by the live list and the native layout regression fixture.
fn expert_row_frame(index: usize, profile: &ExpertProfile, cx: &App) -> Stateful<Div> {
    let title_color = if profile.enabled {
        design::t1(cx)
    } else {
        design::t3(cx)
    };
    style::expert_settings_list_row(("settings-expert-row", index), cx)
        .flex_none()
        .min_h(px(64.))
        .child(
            style::expert_settings_row_badge(px(32.), profile.enabled, cx)
                .text_size(design::text_body())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(style::expert_settings_badge_ink(profile.enabled, cx))
                .child(expert_initial(&profile.name)),
        )
        .child(
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w(px(0.))
                .overflow_hidden()
                .debug_selector(|| format!("band-row-content-{index}"))
                .gap_0p5()
                .child(
                    h_flex()
                        .w_full()
                        .min_w(px(0.))
                        .gap_2()
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(design::text_body())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(title_color)
                                .child(profile.name.clone()),
                        )
                        .when(profile.additions.builtin_id.is_some(), |row| {
                            row.child(style::expert_settings_source_chip("Choro", cx))
                        }),
                )
                .when(!profile.description.trim().is_empty(), |column| {
                    column.child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child(profile.description.clone()),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(design::text_label())
                        .text_color(design::t3(cx))
                        .debug_selector(|| format!("band-row-model-{index}"))
                        .child(expert_meta(profile)),
                ),
        )
}

fn expert_matches(profile: &ExpertProfile, query: &str) -> bool {
    query.is_empty()
        || profile.name.to_lowercase().contains(query)
        || profile.description.to_lowercase().contains(query)
        || profile.model.label().to_lowercase().contains(query)
        || profile.provider.label().to_lowercase().contains(query)
}
