use super::*;

impl SettingsView {
    pub(super) fn render_voice_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let settings = self.workspace.read(cx).voice.clone();
        let voice = self.voice.read(cx);
        let model_status = voice.model_status();
        let history_count = voice.history().len();
        let phase_label = match voice.phase() {
            crate::voice::VoicePhase::Idle => match model_status {
                crate::voice::VoiceModelStatus::Ready => "Ready",
                crate::voice::VoiceModelStatus::Installing => "Installing",
                crate::voice::VoiceModelStatus::NotInstalled => "Not installed",
            },
            crate::voice::VoicePhase::Downloading { .. } => "Installing",
            crate::voice::VoicePhase::RequestingPermission => "Waiting for microphone access",
            crate::voice::VoicePhase::Loading => "Loading",
            crate::voice::VoicePhase::Listening => "Listening",
            crate::voice::VoicePhase::Transcribing => "Transcribing",
            crate::voice::VoicePhase::Thinking => "Thinking",
            crate::voice::VoicePhase::Speaking => "Speaking",
            crate::voice::VoicePhase::Resuming => "Resuming",
            crate::voice::VoicePhase::Error(_) => "Needs attention",
        };
        let announcement_buttons = [
            (VoiceAnnouncements::Minimal, "Minimal"),
            (VoiceAnnouncements::Balanced, "Balanced"),
            (VoiceAnnouncements::All, "All"),
            (VoiceAnnouncements::Off, "Off"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (mode, label))| {
            let button = if settings.announcements == mode {
                crate::ui::style::primary_button_compact(
                    ("voice-announcement-mode", index),
                    label,
                    cx,
                )
            } else {
                crate::ui::style::dialog_neutral_button(
                    ("voice-announcement-mode", index),
                    label,
                    cx,
                )
            };
            button.on_click(cx.listener(move |this, _, _, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_voice_announcements(mode, cx)
                });
            }))
        })
        .collect::<Vec<_>>();
        let voice_for_install = self.voice.clone();
        let voice_for_removal = self.voice.clone();
        let voice_for_history = self.voice.clone();
        let patient = settings.patient_turn_taking;

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
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(crate::ui::design::indicator::dot(
                                if model_status == crate::voice::VoiceModelStatus::Ready {
                                    crate::ui::design::sage(cx)
                                } else {
                                    crate::ui::design::amber(cx)
                                },
                            ))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child("Moonshine Small Streaming + Smart Turn v3.2"),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(format!(
                                                "{phase_label} · about 165 MiB · English · stored locally"
                                            )),
                                    ),
                            )
                            .when(
                                model_status == crate::voice::VoiceModelStatus::NotInstalled,
                                |row| {
                                    row.child(
                                        crate::ui::style::primary_button_compact(
                                            "settings-install-voice-models",
                                            "Install",
                                            cx,
                                        )
                                        .on_click(move |_, _, cx| {
                                            voice_for_install.update(cx, |voice, cx| {
                                                voice.prepare_models(cx)
                                            });
                                        }),
                                    )
                                },
                            )
                            .when(
                                model_status == crate::voice::VoiceModelStatus::Ready,
                                |row| {
                                    row.child(
                                        crate::ui::style::danger_button_compact(
                                            "settings-remove-voice-models",
                                            "Remove models",
                                        )
                                        .on_click(move |_, window, cx| {
                                            let voice = voice_for_removal.clone();
                                            crate::ui::confirm::ConfirmDialog::new(
                                                "Remove local voice models?",
                                                "This frees about 165 MiB. Voice can download the verified models again the next time you use it.",
                                            )
                                            .confirm_label("Remove models")
                                            .confirm_id("settings-confirm-remove-voice-models")
                                            .on_confirm(move |_, cx| {
                                                if let Err(error) = voice
                                                    .update(cx, |voice, cx| voice.remove_models(cx))
                                                {
                                                    eprintln!(
                                                        "could not remove voice models: {error:#}"
                                                    );
                                                }
                                            })
                                            .open(window, cx);
                                        }),
                                    )
                                },
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Audio is processed on this Mac and never retained. Models download only when Voice is first activated or installed here."),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Spoken feedback"),
                    )
                    .child(h_flex().gap_2().children(announcement_buttons))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Balanced speaks completed actions and blockers while routine detail stays in the visible transcript."),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Project Talk shortcut"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!(
                                "{} starts or ends a read-only conversation about the active project. Change it under Keyboard shortcuts.",
                                crate::keymap::shortcut_display(
                                    "toggle_voice_director",
                                    &self.workspace.read(cx).keymap,
                                )
                                .unwrap_or_else(|| "No shortcut".to_string())
                            )),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Patient turn-taking"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Smart Turn waits through brief pauses instead of cutting the thought short."),
                            ),
                    )
                    .child(
                        if patient {
                            crate::ui::style::primary_button_compact(
                                "settings-patient-turn-taking",
                                "On",
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                "settings-patient-turn-taking",
                                "Off",
                                cx,
                            )
                        }
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.set_voice_patient_turn_taking(!patient, cx)
                            });
                        })),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!(
                                "{history_count} recent text turns on this device · retained for {} days",
                                settings.transcript_retention_days
                            )),
                    )
                    .child(
                        crate::ui::style::danger_button_compact(
                            "settings-clear-voice-history",
                            "Clear transcript",
                        )
                        .disabled(history_count == 0)
                        .on_click(move |_, window, cx| {
                            let voice = voice_for_history.clone();
                            crate::ui::confirm::ConfirmDialog::new(
                                "Clear local voice transcript?",
                                "This permanently removes the text-only Project Talk and dictation history from this Mac. Audio was never stored.",
                            )
                            .confirm_label("Clear transcript")
                            .confirm_id("settings-confirm-clear-voice-history")
                            .on_confirm(move |_, cx| {
                                if let Err(error) =
                                    voice.update(cx, |voice, cx| voice.clear_history(cx))
                                {
                                    eprintln!("could not clear voice transcript: {error:#}");
                                }
                            })
                            .open(window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }
}
