use super::*;

impl SettingsView {
    pub(super) fn sync_companion_playlist(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(inputs) = self.companion_playlists.get(index) else {
            return;
        };
        let url = inputs.url.read(cx).value().to_string();
        self.workspace.update(cx, |workspace, cx| {
            let mut settings = workspace.companion_music.clone();
            let Some(playlist) = settings.playlists.get_mut(index) else {
                return;
            };
            playlist.label = ide_core::config::companion_music_mood_label(index).to_string();
            playlist.url = url;
            workspace.set_companion_music_settings(settings, cx);
        });
        cx.notify();
    }

    pub(super) fn render_companion_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let spotify_installed = std::path::Path::new("/Applications/Spotify.app").is_dir();
        let companion_enabled = self.workspace.read(cx).companion_enabled;
        let playlist_rows = self
            .companion_playlists
            .iter()
            .enumerate()
            .map(|(index, inputs)| {
                let mood_label = ide_core::config::companion_music_mood_label(index);
                let url = inputs.url.clone();
                let url_value = url.read(cx).value().trim().to_string();
                let valid = ide_core::config::spotify_playlist_uri(&url_value).is_some();
                let (status, status_color) = if url_value.is_empty() {
                    ("Not configured", crate::ui::design::t4(cx))
                } else if valid {
                    ("Ready", crate::ui::design::sage(cx))
                } else {
                    (
                        "Paste a Spotify playlist link or spotify:playlist URI",
                        crate::ui::design::rose(cx),
                    )
                };

                v_flex()
                    .w_full()
                    .gap_2()
                    .py_3()
                    .when(index > 0, |row| {
                        row.border_t_1()
                            .border_color(crate::ui::design::line(cx).opacity(0.45))
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(24.))
                                    .flex_none()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::accent(cx))
                                    .child((index + 1).to_string()),
                            )
                            .child(
                                div()
                                    .w(px(190.))
                                    .flex_none()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(mood_label),
                            )
                            .child(div().flex_1().min_w(px(0.)).child(Input::new(&url))),
                    )
                    .child(
                        div()
                            .pl(px(27.))
                            .text_size(crate::ui::design::text_label())
                            .text_color(status_color)
                            .child(status),
                    )
            })
            .collect::<Vec<_>>();

        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .min_h(px(72.))
                    .gap_5()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Desktop companion"),
                            )
                            .child(
                                div()
                                    .max_w(px(460.))
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(if companion_enabled {
                                        "The companion shows agent activity, so macOS notification banners stay quiet."
                                    } else {
                                        "The companion is hidden and macOS notification preferences apply instead."
                                    }),
                            ),
                    )
                    .child(
                        crate::ui::style::segmented_container_quiet(cx)
                            .w(px(210.))
                            .flex_none()
                            .child(
                                crate::ui::style::segment(
                                    "settings-companion-on",
                                    IconName::Check,
                                    "On",
                                    companion_enabled,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        workspace.set_companion_enabled(true, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-companion-off",
                                    IconName::Close,
                                    "Off",
                                    !companion_enabled,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        workspace.set_companion_enabled(false, cx);
                                        workspace.save_now();
                                    });
                                })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Spotify playlists"),
                    )
                    .child(
                        div()
                            .max_w(px(650.))
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Choose one Spotify playlist for each companion mood. Every mood has its own animation; Choro controls the local Spotify app, so no Spotify developer account or API key is required."),
                    )
                    .children(playlist_rows),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        Icon::new(if spotify_installed {
                            IconName::CircleCheck
                        } else {
                            IconName::TriangleAlert
                        })
                        .size(crate::ui::design::icon())
                        .text_color(if spotify_installed {
                            crate::ui::design::sage(cx)
                        } else {
                            crate::ui::design::amber(cx)
                        }),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if spotify_installed {
                                        "Spotify is ready"
                                    } else {
                                        "Spotify desktop app not found"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(if spotify_installed {
                                        "macOS may ask once for permission when Choro first controls playback."
                                    } else {
                                        "Install Spotify in Applications to use companion music."
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
