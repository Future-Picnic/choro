use super::*;
use crate::ui::{design, style};

impl SettingsView {
    pub(super) fn sync_companion_playlist(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(inputs) = self.companion_playlists.get(index) else {
            return;
        };
        let urls = inputs
            .urls
            .iter()
            .map(|url| url.read(cx).value().to_string())
            .collect();
        self.workspace.update(cx, |workspace, cx| {
            let mut settings = workspace.companion_music.clone();
            let Some(playlist) = settings.playlists.get_mut(index) else {
                return;
            };
            playlist.label = ide_core::config::companion_music_mood_label(index).to_string();
            playlist.urls = urls;
            workspace.set_companion_music_settings(settings, cx);
        });
        cx.notify();
    }

    fn add_companion_playlist(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reuse an unfinished row instead of accumulating empty inputs.
        if let Some(url) = self.companion_playlists[index]
            .urls
            .iter()
            .find(|url| url.read(cx).value().trim().is_empty())
        {
            url.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let url =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste a Spotify playlist link…"));
        cx.subscribe(&url, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_companion_playlist(index, cx);
            }
        })
        .detach();
        self.companion_playlists[index].urls.push(url.clone());
        self.sync_companion_playlist(index, cx);
        url.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn render_companion_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let spotify_installed = std::path::Path::new("/Applications/Spotify.app").is_dir();
        let companion_enabled = self.workspace.read(cx).companion_enabled;
        let mood_details = [
            (lucide_icons::Icon::Focus, "Settle into uninterrupted work."),
            (
                lucide_icons::Icon::Headphones,
                "Find a steady, easy rhythm.",
            ),
            (lucide_icons::Icon::Leaf, "Slow down and clear your head."),
            (lucide_icons::Icon::Zap, "Turn up the pace."),
        ];
        let mood_sections = self
            .companion_playlists
            .iter()
            .enumerate()
            .map(|(index, inputs)| {
                let mood_label = ide_core::config::companion_music_mood_label(index);
                let (icon, description) = mood_details[index];
                let mut seen_uris = Vec::new();
                let rows = inputs
                    .urls
                    .iter()
                    .enumerate()
                    .map(|(link_index, url)| {
                        let value = url.read(cx).value();
                        let empty = value.trim().is_empty();
                        let uri = ide_core::config::spotify_playlist_uri(&value);
                        let duplicate = uri.as_ref().is_some_and(|uri| seen_uris.contains(uri));
                        if let Some(uri) = &uri {
                            seen_uris.push(uri.clone());
                        }
                        let invalid = !empty && uri.is_none();
                        let row_id = url.entity_id();
                        let message = if invalid {
                            Some("Use a Spotify playlist link or spotify:playlist URI.")
                        } else if duplicate {
                            Some("Already in this mood. This playlist will only be counted once.")
                        } else {
                            None
                        };
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w(px(18.))
                                            .flex_none()
                                            .text_size(design::text_label())
                                            .text_color(design::t3(cx))
                                            .child((link_index + 1).to_string()),
                                    )
                                    .child(
                                        div().flex_1().min_w(px(0.)).child(
                                            Input::new(url).suffix(
                                                Icon::new(if invalid {
                                                    IconName::TriangleAlert
                                                } else if empty {
                                                    IconName::Plus
                                                } else {
                                                    IconName::Check
                                                })
                                                .size(design::icon_sm())
                                                .text_color(if invalid {
                                                    design::rose(cx)
                                                } else if empty {
                                                    design::t3(cx)
                                                } else {
                                                    design::sage(cx)
                                                }),
                                            ),
                                        ),
                                    )
                                    .child(
                                        style::header_icon_button(
                                            SharedString::from(format!(
                                                "companion-remove-{row_id}"
                                            )),
                                            IconName::Close,
                                            cx,
                                        )
                                        .tooltip(format!(
                                            "Remove playlist {} from {mood_label}",
                                            link_index + 1
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                let urls =
                                                    &mut this.companion_playlists[index].urls;
                                                urls.retain(|url| url.entity_id() != row_id);
                                                if let Some(next) = urls.get(
                                                    link_index.min(urls.len().saturating_sub(1)),
                                                ) {
                                                    next.update(cx, |input, cx| {
                                                        input.focus(window, cx)
                                                    });
                                                }
                                                this.sync_companion_playlist(index, cx);
                                            }),
                                        ),
                                    ),
                            )
                            .when_some(message, |row, message| {
                                row.child(
                                    div()
                                        .pl(px(26.))
                                        .text_size(design::text_label())
                                        .text_color(if invalid {
                                            design::rose(cx)
                                        } else {
                                            design::t3(cx)
                                        })
                                        .child(message),
                                )
                            })
                    })
                    .collect::<Vec<_>>();
                v_flex()
                    .w_full()
                    .gap_3()
                    .py_4()
                    .when(index > 0, |section| {
                        section.border_t_1().border_color(design::line(cx))
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .child(div().flex_none().child(design::indicator::lucide_icon(
                                icon,
                                design::accent(cx),
                                design::icon(),
                            )))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(design::t1(cx))
                                            .child(mood_label),
                                    )
                                    .child(
                                        div()
                                            .text_size(design::text_ui())
                                            .text_color(design::t3(cx))
                                            .child(description),
                                    ),
                            )
                            .child(
                                style::settings_ghost_button(
                                    ("companion-add-playlist", index),
                                    "Add playlist",
                                )
                                .icon(IconName::Plus)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.add_companion_playlist(index, window, cx)
                                    },
                                )),
                            ),
                    )
                    .children(rows)
                    .when(inputs.urls.is_empty(), |section| {
                        section.child(
                            div()
                                .text_size(design::text_ui())
                                .text_color(design::t3(cx))
                                .child("No playlists yet. Add a link to make this mood available."),
                        )
                    })
            })
            .collect::<Vec<_>>();

        v_flex().w_full().gap_5()
            .child(
                h_flex().w_full().gap_4().items_center().p_4()
                    .rounded(design::r_md()).bg(design::surface(cx))
                    .child(div().flex_none().child(design::indicator::lucide_icon(lucide_icons::Icon::Bot, design::accent(cx), design::icon())))
                    .child(v_flex().flex_1().min_w(px(0.)).gap_1()
                        .child(div().text_size(design::text_body()).font_weight(FontWeight::SEMIBOLD).text_color(design::t1(cx)).child("Desktop companion"))
                        .child(div().text_size(design::text_ui()).text_color(design::t3(cx)).child(if companion_enabled {
                            "Shows agent activity and keeps macOS notification banners quiet."
                        } else {
                            "Hidden. Your macOS notification preferences apply."
                        })))
                    .child(style::segmented_container_quiet(cx).w(px(140.)).flex_none()
                        .child(style::segment("settings-companion-on", IconName::Check, "On", companion_enabled, cx).flex_1()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.workspace.update(cx, |workspace, cx| {
                                    workspace.set_companion_enabled(true, cx);
                                    workspace.save_now();
                                });
                            })))
                        .child(style::segment("settings-companion-off", IconName::Close, "Off", !companion_enabled, cx).flex_1()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.workspace.update(cx, |workspace, cx| {
                                    workspace.set_companion_enabled(false, cx);
                                    workspace.save_now();
                                });
                            })))),
            )
            .child(v_flex().w_full().gap_1()
                .child(h_flex().w_full().gap_3().items_center()
                    .child(div().flex_1().text_size(design::text_title()).font_weight(FontWeight::SEMIBOLD).text_color(design::t1(cx)).child("Music for every mood"))
                    .child(h_flex().gap_1p5().items_center()
                        .child(Icon::new(if spotify_installed { IconName::CircleCheck } else { IconName::TriangleAlert }).size(design::icon_sm())
                            .text_color(if spotify_installed { design::sage(cx) } else { design::amber(cx) }))
                        .child(div().text_size(design::text_ui()).text_color(design::t2(cx)).child(if spotify_installed { "Spotify ready" } else { "Spotify not found" }))))
                .child(div().text_size(design::text_ui()).text_color(design::t3(cx)).child("Add playlists for each mood. Choro picks one at random when you choose it."))
                .children(mood_sections)
            )
            .child(h_flex().w_full().gap_2().items_start().pt_3().border_t_1().border_color(design::line(cx))
                .child(Icon::new(IconName::Info).size(design::icon_sm()).text_color(design::t3(cx)))
                .child(div().flex_1().min_w(px(0.)).text_size(design::text_ui()).text_color(design::t3(cx)).child(if spotify_installed {
                    "Links save automatically. Uses your Spotify desktop app; no API key needed. macOS may ask for playback permission the first time."
                } else {
                    "Links save automatically. Install Spotify in Applications to play music from your companion. No API key needed."
                })))
            .into_any_element()
    }
}
