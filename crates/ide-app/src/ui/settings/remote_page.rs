use super::*;

impl SettingsView {
    pub(super) fn render_remote_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        {
            let pairing = self.remote_auth.snapshot();
            let active_code = pairing.active_code.clone();
            let device_count = pairing.devices.len();
            let mac_id = self.remote_room_id.clone();
            let mac_id_for_copy = mac_id.clone();
            let relay_state = self.remote_relay_control.state();
            let secure_pairing = active_code.as_ref().map(|_| self.remote_relay_control.pairing_payload(&mac_id, &self.remote_auth));
            let relay_description = match relay_state {
                RelayState::Disconnected => {
                    "Off. Connect to pair a phone or let paired iPhones reach this Mac."
                }
                RelayState::Connecting => "Connecting securely to the relay…",
                RelayState::Connected => "On. Paired iPhones can reach this Mac.",
                RelayState::Reconnecting => {
                    "Connection interrupted. Choro is reconnecting securely…"
                }
            };
            let relay_button = match relay_state {
                RelayState::Disconnected => crate::ui::style::primary_button_compact(
                    "settings-remote-connect",
                    "Connect",
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.remote_relay_control.connect();
                    cx.notify();
                })),
                RelayState::Connecting => crate::ui::style::secondary_button_compact(
                    "settings-remote-disconnect",
                    "Cancel",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.remote_relay_control.disconnect();
                    cx.notify();
                })),
                RelayState::Connected | RelayState::Reconnecting => {
                    crate::ui::style::secondary_button_compact(
                        "settings-remote-disconnect",
                        "Disconnect",
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.remote_relay_control.disconnect();
                        cx.notify();
                    }))
                }
            };
            v_flex()
                                .w_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
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
                                                        .child("Remote connection"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(relay_description),
                                                ),
                                        )
                                        .child(relay_button),
                                )
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
                                                .w_full()
                                                .items_center()
                                                .gap_3()
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .text_size(crate::ui::design::text_body())
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child("Pair a phone"),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(crate::ui::design::text_ui())
                                                                .text_color(crate::ui::design::t3(cx))
                                                                .child("Pairing stays on this Mac. No Choro account, email, or cloud identity is used."),
                                                        ),
                                                )
                                                .child(
                                                    crate::ui::style::primary_button_compact(
                                                        "settings-start-remote-pairing",
                                                        if active_code.is_some() {
                                                            "New code"
                                                        } else {
                                                            "Connect & pair"
                                                        },
                                                        cx,
                                                    )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.remote_relay_control.connect();
                                                            this.remote_auth.start_pairing();
                                                            this.remote_status = None;
                                                            cx.notify();
                                                        })),
                                                ),
                                        )
                                        .when_some(secure_pairing, |card, payload| match payload {
                                            Ok(payload) => {
                                                let copy = payload.clone();
                                                card.child(v_flex().gap_2()
                                                    .child(div().text_size(crate::ui::design::text_ui()).child("Scan with Choro Remote, or copy the pairing information to your iPhone."))
                                                    .child(pairing_qr(&payload))
                                                    .child(crate::ui::style::secondary_button_compact("settings-copy-secure-pairing", "Copy pairing information")
                                                        .on_click(move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()))))
                                                    .child(div().text_size(crate::ui::design::text_label()).text_color(crate::ui::design::t3(cx)).child("Valid for five minutes. Share only with the phone you want to pair.")))
                                            }
                                            Err(error) => card.child(div().text_size(crate::ui::design::text_ui()).text_color(crate::ui::design::rose(cx)).child(error)),
                                        })
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_label())
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(crate::ui::design::t4(cx))
                                                        .child("MAC ID · OLDER APPS"),
                                                )
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w(px(0.))
                                                                .font_family(crate::ui::design::FONT_MONO)
                                                                .text_size(crate::ui::design::text_body())
                                                                .text_color(crate::ui::design::t2(cx))
                                                                .child(mac_id),
                                                        )
                                                        .child(
                                                            crate::ui::style::settings_inline_icon_button(
                                                                "settings-copy-remote-mac-id",
                                                                IconName::Copy,
                                                            )
                                                                .label("Copy")
                                                                .tooltip("Copy Mac ID")
                                                                .on_click(move |_, _, cx| {
                                                                    cx.write_to_clipboard(
                                                                        gpui::ClipboardItem::new_string(
                                                                            mac_id_for_copy.clone(),
                                                                        ),
                                                                    );
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .when_some(active_code, |card, code| {
                                            let code_for_copy = code.clone();
                                            card.child(
                                                v_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .p_3()
                                                    .rounded(crate::ui::design::r_md())
                                                    .border_1()
                                                    .border_color(crate::ui::design::accent_line(cx))
                                                    .bg(crate::ui::design::accent_soft(cx))
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_label())
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child("ONE-TIME PAIRING CODE · VALID FOR 5 MINUTES"),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .w_full()
                                                            .items_center()
                                                            .gap_2()
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .min_w(px(0.))
                                                                    .font_family(crate::ui::design::FONT_MONO)
                                                                    .text_size(px(24.))
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .text_color(crate::ui::design::accent(cx))
                                                                    .child(code),
                                                            )
                                                            .child(
                                                                crate::ui::style::settings_inline_icon_button(
                                                                    "settings-copy-remote-pairing-code",
                                                                    IconName::Copy,
                                                                )
                                                                    .label("Copy")
                                                                    .tooltip("Copy pairing code")
                                                                    .on_click(move |_, _, cx| {
                                                                        cx.write_to_clipboard(
                                                                            gpui::ClipboardItem::new_string(
                                                                                code_for_copy.clone(),
                                                                            ),
                                                                        );
                                                                    }),
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .w_full()
                                                            .items_center()
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .text_size(crate::ui::design::text_ui())
                                                                    .text_color(crate::ui::design::t3(cx))
                                                                    .child("For an older app, enter this Mac ID and code. Updated Choro Remote uses the QR code or copied pairing information above."),
                                                            )
                                                            .child(
                                                                crate::ui::style::settings_ghost_button(
                                                                    "settings-cancel-remote-pairing",
                                                                    "Cancel",
                                                                )
                                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                                        this.remote_auth.cancel_pairing();
                                                                        cx.notify();
                                                                    })),
                                                            ),
                                                    ),
                                            )
                                        }),
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
                                            h_flex()
                                                .w_full()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .text_size(crate::ui::design::text_body())
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child("Paired devices"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(format!("{device_count}")),
                                                ),
                                        )
                                        .when(pairing.devices.is_empty(), |card| {
                                            card.child(
                                                div()
                                                    .py_2()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child("No phones are paired. Remote data stays locked until a device is paired."),
                                            )
                                        })
                                        .children(pairing.devices.into_iter().enumerate().map(|(index, device)| {
                                            let device_id = device.id.clone();
                                            let permission = device.permission;
                                            let permission_label = match permission {
                                                DevicePermission::ViewOnly => "View only",
                                                DevicePermission::Control => "Control",
                                                DevicePermission::FullAccess => "Full access",
                                            };
                                            h_flex()
                                                .id(("settings-paired-device", index))
                                                .w_full()
                                                .min_h(px(42.))
                                                .gap_2()
                                                .items_center()
                                                .border_t_1()
                                                .border_color(crate::ui::design::line(cx).opacity(0.35))
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .gap_0p5()
                                                        .child(
                                                            div()
                                                                .text_size(crate::ui::design::text_body())
                                                                .text_color(crate::ui::design::t1(cx))
                                                                .child(device.name),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(crate::ui::design::text_label())
                                                                .text_color(crate::ui::design::t4(cx))
                                                                .child(format!(
                                                                    "Paired {} · expires {}",
                                                                    format_remote_time(device.paired_at),
                                                                    format_remote_expiry(device.expires_at)
                                                                )),
                                                        ),
                                                )
                                                .child(
                                                    crate::ui::style::settings_ghost_button(
                                                        ("settings-device-permission", index),
                                                        permission_label,
                                                    )
                                                        .tooltip("Change what this phone may do")
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            let next = match permission {
                                                                DevicePermission::ViewOnly => DevicePermission::Control,
                                                                DevicePermission::Control => DevicePermission::FullAccess,
                                                                DevicePermission::FullAccess => DevicePermission::ViewOnly,
                                                            };
                                                            this.remote_status = this
                                                                .remote_auth
                                                                .set_device_permission(&device_id, next)
                                                                .err()
                                                                .map(|error| format!("Could not update device access: {error:?}"));
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    crate::ui::style::settings_ghost_button(
                                                        ("settings-revoke-device", index),
                                                        "Revoke",
                                                    )
                                                        .on_click(cx.listener({
                                                            let device_id = device.id.clone();
                                                            move |this, _, _, cx| {
                                                            this.remote_status = match this.remote_auth.revoke(&device_id) {
                                                                Ok(_) => {
                                                                    this.workspace.update(cx, |workspace, cx| {
                                                                        workspace.remove_pocketcomet_task_sources_for_device(&device_id, cx);
                                                                    });
                                                                    None
                                                                }
                                                                Err(error) => Some(format!("Could not revoke device: {error:?}")),
                                                            };
                                                            cx.notify();
                                                        }})),
                                                )
                                        }))
                                        .when_some(self.remote_status.clone().or_else(|| self.remote_relay_control.secure_connection_error()), |card, status| {
                                            card.child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::rose(cx))
                                                    .child(status),
                                            )
                                        }),
                                )
                                .into_any_element()
        }
    }
}

fn pairing_qr(payload: &str) -> gpui::AnyElement {
    let Ok(code) = qrcode::QrCode::new(payload.as_bytes()) else {
        return div().child("Use Copy pairing information to pair this phone.").into_any_element();
    };
    let width = code.width();
    let cells = code.to_colors();
    // Four white modules on every side are the QR quiet zone. Fixed square
    // modules keep the code readable on both light and dark desktop themes.
    let cell = 3.0_f32;
    gpui::canvas(|_, _, _| (), move |bounds, _, window, _| {
        window.paint_quad(gpui::fill(bounds, gpui::rgb(0xffffff)));
        for y in 0..width {
            let mut x = 0;
            while x < width {
                if cells[y * width + x] != qrcode::Color::Dark { x += 1; continue; }
                let start = x;
                while x < width && cells[y * width + x] == qrcode::Color::Dark { x += 1; }
                let rect = gpui::Bounds::new(
                    bounds.origin + gpui::point(px((start + 4) as f32 * cell), px((y + 4) as f32 * cell)),
                    gpui::size(px((x - start) as f32 * cell), px(cell)),
                );
                window.paint_quad(gpui::fill(rect, gpui::rgb(0x000000)));
            }
        }
    }).size(px((width + 8) as f32 * cell)).flex_shrink_0().into_any_element()
}
