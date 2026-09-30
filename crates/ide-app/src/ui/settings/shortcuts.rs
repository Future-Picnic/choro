use super::*;

impl SettingsView {
    pub(super) fn render_shortcut_row(
        &self,
        shortcut: keymap::Shortcut,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let recording = self.recording_shortcut.as_deref() == Some(shortcut.id);
        let modified = shortcut.is_modified(&self.shortcut_overrides);
        let label = if recording {
            "Press shortcut…".to_string()
        } else {
            shortcut
                .keystroke(&self.shortcut_overrides)
                .map(keymap::display_keystroke)
                .unwrap_or_else(|| "Not set".to_string())
        };
        let id = shortcut.id.to_string();
        let reset_id = shortcut.id.to_string();

        h_flex()
            .w_full()
            .min_h(px(54.))
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(crate::ui::design::line(cx).opacity(0.55))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(shortcut.title),
                            )
                            .when(modified, |title| {
                                title.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(crate::ui::design::r_xs())
                                        .bg(crate::ui::design::accent_soft(cx))
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::accent(cx))
                                        .child("Modified"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(shortcut.description),
                    ),
            )
            .child(
                crate::ui::style::shortcut_key_button(
                    SharedString::from(format!("shortcut-recorder-{}", shortcut.id)),
                    label,
                    recording,
                    cx,
                )
                .min_w(px(116.))
                .tooltip(if recording {
                    "Press a shortcut, Esc to cancel, or Delete to unassign"
                } else {
                    "Click, then press the new shortcut"
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_shortcut_recording(id.clone(), window, cx);
                })),
            )
            .when(modified, |row| {
                row.child(
                    crate::ui::style::icon_button(
                        SharedString::from(format!("shortcut-reset-{}", shortcut.id)),
                        IconName::Undo2,
                        cx,
                    )
                    .tooltip("Restore default")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reset_shortcut(&reset_id, cx);
                    })),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_shortcuts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let query = self.shortcut_search.read(cx).value().trim().to_lowercase();
        let mut groups = Vec::new();
        for category in keymap::ShortcutCategory::ALL {
            let shortcuts = keymap::shortcuts()
                .into_iter()
                .filter(|shortcut| shortcut.category == category)
                .filter(|shortcut| {
                    !self.shortcut_modified_only || shortcut.is_modified(&self.shortcut_overrides)
                })
                .filter(|shortcut| {
                    query.is_empty()
                        || shortcut.title.to_lowercase().contains(&query)
                        || shortcut.description.to_lowercase().contains(&query)
                        || category.title().to_lowercase().contains(&query)
                })
                .collect::<Vec<_>>();
            if shortcuts.is_empty() {
                continue;
            }
            groups.push(
                v_flex()
                    .w_full()
                    .overflow_hidden()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        h_flex().h(px(38.)).px_3().items_center().child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t2(cx))
                                .child(category.title()),
                        ),
                    )
                    .children(
                        shortcuts
                            .into_iter()
                            .map(|shortcut| self.render_shortcut_row(shortcut, cx)),
                    )
                    .into_any_element(),
            );
        }

        if !self.shortcut_modified_only {
            let contextual = [
                (
                    "Find in current view",
                    "Standard search inside the focused view",
                    "⌘F",
                ),
                ("Send or continue", "Send from an agent composer", "↩"),
                ("New line", "Insert a line break in an agent composer", "⇧↩"),
                (
                    "Start or steer agent",
                    "Start a draft agent or steer the selected running agent",
                    "⌘↩",
                ),
            ]
            .into_iter()
            .filter(|(title, description, _)| {
                query.is_empty()
                    || title.to_lowercase().contains(&query)
                    || description.to_lowercase().contains(&query)
                    || "contextual essentials".contains(&query)
            })
            .collect::<Vec<_>>();
            if !contextual.is_empty() {
                groups.push(
                    v_flex()
                        .w_full()
                        .overflow_hidden()
                        .rounded(crate::ui::design::r_lg())
                        .border_1()
                        .border_color(crate::ui::design::line_2(cx))
                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                        .child(
                            h_flex().h(px(38.)).px_3().items_center().child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Contextual essentials"),
                            ),
                        )
                        .children(contextual.into_iter().map(|(title, description, keys)| {
                            h_flex()
                                .w_full()
                                .min_h(px(54.))
                                .px_3()
                                .gap_3()
                                .items_center()
                                .border_t_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.55))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(description),
                                        ),
                                )
                                .child(
                                    div()
                                        .min_w(px(116.))
                                        .px_2()
                                        .py_1()
                                        .rounded(crate::ui::design::r_xs())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::base(cx))
                                        .text_center()
                                        .font_family(crate::ui::design::FONT_MONO)
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(keys),
                                )
                        }))
                        .into_any_element(),
                );
            }
        }

        v_flex()
            .track_focus(&self.shortcut_focus)
            .on_key_down(cx.listener(Self::capture_shortcut))
            .w_full()
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
                            .child(Input::new(&self.shortcut_search).prefix(IconName::Search)),
                    )
                    .child(
                        if self.shortcut_modified_only {
                            crate::ui::style::ghost_button_compact("shortcuts-filter-all", "All")
                        } else {
                            crate::ui::style::secondary_button_compact(
                                "shortcuts-filter-all",
                                "All",
                            )
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shortcut_modified_only = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        if self.shortcut_modified_only {
                            crate::ui::style::secondary_button_compact(
                                "shortcuts-filter-modified",
                                "Modified",
                            )
                        } else {
                            crate::ui::style::ghost_button_compact(
                                "shortcuts-filter-modified",
                                "Modified",
                            )
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shortcut_modified_only = true;
                            cx.notify();
                        })),
                    )
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "shortcuts-reset-all",
                            "Reset all",
                            cx,
                        )
                        .disabled(self.shortcut_overrides.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.reset_all_shortcuts(cx);
                        })),
                    ),
            )
            .when_some(self.shortcut_error.clone(), |view, error| {
                view.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.45))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(groups.is_empty(), |view| {
                view.child(
                    div()
                        .w_full()
                        .p_6()
                        .text_center()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No shortcuts match this view."),
                )
            })
            .children(groups)
            .child(
                div()
                    .pb_2()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child("Click a keycap and press the new shortcut. Choro blocks duplicates and macOS-reserved combinations. Press Delete while recording to leave an action unassigned."),
            )
            .into_any_element()
    }
}
