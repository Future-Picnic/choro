use super::*;

impl SettingsView {
    pub(super) fn render_design_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let penpot = self.penpot.read(cx);
        let status = penpot.status().clone();
        let active_provider = penpot.provider();
        let pending = matches!(
            status,
            PenpotConnectionStatus::Provisioning | PenpotConnectionStatus::Checking
        );
        let selected_provider_is_active = active_provider == self.design_provider;
        let status_text = match (selected_provider_is_active, &status, self.design_provider) {
            (false, _, DesignProvider::Choro) => {
                "Penpot Cloud stays active until you choose Use Choro Design.".to_string()
            }
            (false, _, DesignProvider::PenpotCloud) => {
                "Choro Design stays active until you save and connect the cloud account."
                    .to_string()
            }
            (true, PenpotConnectionStatus::NotChecked, _) => {
                "Connection has not been checked.".to_string()
            }
            (true, PenpotConnectionStatus::Provisioning, _) => {
                "Preparing your Choro Design workspace…".to_string()
            }
            (true, PenpotConnectionStatus::Checking, _) => {
                "Checking the Design connection…".to_string()
            }
            (true, PenpotConnectionStatus::Reachable, _) => "Design is connected.".to_string(),
            (true, PenpotConnectionStatus::Error(error), _) => error.clone(),
        };
        let status_color = if !selected_provider_is_active {
            crate::ui::design::t4(cx)
        } else {
            match status {
                PenpotConnectionStatus::Reachable => crate::ui::design::sage(cx),
                PenpotConnectionStatus::Error(_) => crate::ui::design::rose(cx),
                PenpotConnectionStatus::Provisioning | PenpotConnectionStatus::Checking => {
                    crate::ui::design::amber(cx)
                }
                PenpotConnectionStatus::NotChecked => crate::ui::design::t4(cx),
            }
        };
        let managed_selected = self.design_provider == DesignProvider::Choro;
        let cloud_selected = self.design_provider == DesignProvider::PenpotCloud;
        let instance_input = self.design_instance_input.clone();
        let mcp_input = self.design_mcp_input.clone();
        let access_token_input = self.design_access_token_input.clone();
        let mcp_key_input = self.design_mcp_key_input.clone();

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
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Design provider"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "Choro Design is automatic. Penpot Cloud uses your own account and credentials.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                if managed_selected {
                                    crate::ui::style::accent_button_compact(
                                        "settings-design-provider-choro",
                                        DesignProvider::Choro.label(),
                                        cx,
                                    )
                                } else {
                                    crate::ui::style::secondary_button_compact(
                                        "settings-design-provider-choro",
                                        DesignProvider::Choro.label(),
                                    )
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.design_provider = DesignProvider::Choro;
                                    this.design_error = None;
                                    cx.notify();
                                })),
                            )
                            .child(
                                if cloud_selected {
                                    crate::ui::style::accent_button_compact(
                                        "settings-design-provider-cloud",
                                        DesignProvider::PenpotCloud.label(),
                                        cx,
                                    )
                                } else {
                                    crate::ui::style::secondary_button_compact(
                                        "settings-design-provider-cloud",
                                        DesignProvider::PenpotCloud.label(),
                                    )
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.design_provider = DesignProvider::PenpotCloud;
                                    this.design_error = None;
                                    cx.notify();
                                })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .when(managed_selected, |card| {
                        card.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child("Managed by Choro"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(
                                            "No signup, URLs, or tokens are required. Choro creates and reconnects the workspace for this installation.",
                                        ),
                                ),
                        )
                        .child(
                            if pending {
                                crate::ui::style::busy_button_compact(
                                    "settings-design-use-managed",
                                    "Connecting",
                                    cx,
                                )
                            } else {
                                crate::ui::style::primary_button_compact(
                                    "settings-design-use-managed",
                                    "Use Choro Design",
                                    cx,
                                )
                                .on_click({
                                    let penpot = self.penpot.clone();
                                    move |_, _, cx| {
                                        penpot.update(cx, |penpot, cx| {
                                            penpot.switch_to_managed(cx)
                                        });
                                    }
                                })
                            },
                        )
                    })
                    .when(cloud_selected, |card| {
                        card.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child("Connect Penpot Cloud"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(
                                            "Use the service URL, copied MCP URL, personal access token, and MCP key from your Penpot account.",
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("Penpot URL", cx))
                                .child(Input::new(&self.design_instance_input).w_full()),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("MCP server URL", cx))
                                .child(Input::new(&self.design_mcp_input).w_full()),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("Personal access token", cx))
                                .child(
                                    Input::new(&self.design_access_token_input)
                                        .w_full()
                                        .mask_toggle(),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("MCP key", cx))
                                .child(
                                    Input::new(&self.design_mcp_key_input)
                                        .w_full()
                                        .mask_toggle(),
                                ),
                        )
                        .child(
                            crate::ui::style::primary_button_compact(
                                "settings-design-connect-cloud",
                                "Save & Connect",
                                cx,
                            )
                            .icon(IconName::Globe)
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let instance = instance_input.read(cx).value().to_string();
                                let mcp = mcp_input.read(cx).value().to_string();
                                let access_token =
                                    access_token_input.read(cx).value().to_string();
                                let key = mcp_key_input.read(cx).value().to_string();
                                let result = this.penpot.update(cx, |penpot, cx| {
                                    penpot.save_connection(
                                        &instance,
                                        &mcp,
                                        &key,
                                        &access_token,
                                        cx,
                                    )
                                });
                                match result {
                                    Ok(()) => {
                                        this.design_error = None;
                                        this.penpot.update(cx, |penpot, cx| {
                                            penpot.test_connection(cx)
                                        });
                                    }
                                    Err(error) => {
                                        this.design_error = Some(error.to_string());
                                    }
                                }
                                cx.notify();
                            })),
                        )
                    }),
            )
            .when_some(self.design_error.clone(), |view, error| {
                view.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.4))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::design::indicator::dot(status_color))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(status_text),
                    ),
            )
            .into_any_element()
    }
}
