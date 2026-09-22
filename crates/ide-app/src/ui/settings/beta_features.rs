use super::*;
use crate::ui::{design, style};

impl SettingsView {
    pub(super) fn render_beta_features_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let enabled = self.workspace.read(cx).beta_features.delegation;
        let available = ide_core::delegation::enabled();
        let penpot_enabled = self.workspace.read(cx).beta_features.penpot;
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .p_4()
                    .items_center()
                    .rounded(design::r_md())
                    .border_1()
                    .border_color(design::line(cx))
                    .bg(design::surface(cx).opacity(0.55))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Delegation"),
                            )
                            .child(
                                div()
                                    .text_size(design::text_ui())
                                    .text_color(design::t3(cx))
                                    .child("Let your lead assign work to Bandmates through /delegate or a request in chat."),
                            )
                            .child(
                                div()
                                    .text_size(design::text_label())
                                    .text_color(design::t4(cx))
                                    .child(if available {
                                        "Off by default. Existing delegated work can finish when this is off."
                                    } else {
                                        "Delegation is unavailable in this app session."
                                    }),
                            ),
                    )
                    .child(
                        style::settings_option_button(
                            "beta-delegation-toggle",
                            if enabled { "On" } else { "Off" },
                            enabled,
                        )
                        .icon(if enabled { IconName::Check } else { IconName::Minus })
                        .disabled(!available)
                        .tooltip(if enabled { "Turn delegation off" } else { "Turn delegation on" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let result = this.workspace.update(cx, |workspace, cx| {
                                let mut features = workspace.beta_features;
                                features.delegation = !enabled;
                                workspace.set_beta_features(features, cx)
                            });
                            this.beta_features_error = result.err().map(|error| error.to_string());
                            cx.notify();
                        })),
                    ),
            )
            .child(
                h_flex().w_full().gap_4().p_4().items_center()
                    .rounded(design::r_md()).border_1().border_color(design::line(cx))
                    .bg(design::surface(cx).opacity(0.55))
                    .child(v_flex().flex_1().min_w(px(0.)).gap_1()
                        .child(div().text_size(design::text_body()).font_weight(FontWeight::SEMIBOLD).child("Penpot"))
                        .child(div().text_size(design::text_ui()).text_color(design::t3(cx))
                            .child("Use Penpot as an optional design tool. Studio remains the default for new designs, tasks, and documents."))
                        .child(div().text_size(design::text_label()).text_color(design::t4(cx))
                            .child("Off by default. Turning it off keeps existing Penpot designs and credentials.")))
                    .child(style::settings_option_button("beta-penpot-toggle", if penpot_enabled { "On" } else { "Off" }, penpot_enabled)
                        .icon(if penpot_enabled { IconName::Check } else { IconName::Minus })
                        .tooltip(if penpot_enabled { "Turn Penpot off" } else { "Turn Penpot on" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let result = this.workspace.update(cx, |workspace, cx| {
                                let mut features = workspace.beta_features;
                                features.penpot = !penpot_enabled;
                                workspace.set_beta_features(features, cx)
                            });
                            if result.is_ok() {
                                this.penpot.update(cx, |penpot, cx| penpot.set_enabled(!penpot_enabled, cx));
                            }
                            this.beta_features_error = result.err().map(|error| error.to_string());
                            cx.notify();
                        }))),
            )
            .when_some(self.beta_features_error.clone(), |page, error| {
                page.child(style::expert_settings_error_notice(error, cx))
            })
            .when(enabled && available, |page| page.child(self.render_delegation_limits(cx)))
            .into_any_element()
    }
}
