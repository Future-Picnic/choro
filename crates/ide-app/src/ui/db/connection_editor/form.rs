//! Form and list rendering for the database connections dialog.

use gpui::{
    div, prelude::FluentBuilder, px, App, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex, input::Input, spinner::Spinner, v_flex, Disableable, Icon, IconName, Selectable,
    Sizable,
};
use ide_core::DbProvider;

use super::super::providers::{
    access_help, capability_facts, effective_read_only, endpoint_summary, env_references,
    has_inline_secret, provider_connection_help, provider_connection_label,
    provider_form_description, provider_help_steps, provider_help_title,
};
use super::{ConnectionRow, DbConnectionsEditor, TestState, DB_CONNECTION_EDITOR_CONTENT_WIDTH};
use gpui_component::scroll::ScrollableElement;

impl DbConnectionsEditor {
    fn render_connection_field(
        &self,
        index: usize,
        row: &ConnectionRow,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let provider = row.provider;
        let local_file = provider.capabilities().local_file;
        let uri = row.uri.read(cx).value().to_string();
        let references = env_references(&uri);
        let missing_reference = references.iter().any(|reference| !reference.defined);
        let inline_secret = has_inline_secret(provider, &uri);
        let connection_input = if local_file {
            Input::new(&row.uri).w_full().into_any_element()
        } else {
            Input::new(&row.uri)
                .w_full()
                .mask_toggle()
                .into_any_element()
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(db_form_field_label(provider_connection_label(provider), cx))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().min_w(px(320.)).child(connection_input))
                    .when(local_file, |field| {
                        field.child(
                            crate::ui::style::dialog_neutral_button(
                                "choose-sqlite-file",
                                "Choose file",
                                cx,
                            )
                            .icon(IconName::FolderOpen)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_sqlite_file(index, window, cx);
                            })),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::design::t3(cx))
                    .child(provider_connection_help(provider)),
            )
            .when(!references.is_empty(), |field| {
                field.child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_x_3()
                        .gap_y_1()
                        .children(references.into_iter().map(|reference| {
                            let (icon, color, state) = if reference.defined {
                                (lucide_icons::Icon::Check, crate::ui::design::sage(cx), "set")
                            } else {
                                (
                                    lucide_icons::Icon::CircleX,
                                    crate::ui::design::rose(cx),
                                    "not set",
                                )
                            };
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t2(cx))
                                .child(crate::ui::design::indicator::lucide_icon(
                                    icon,
                                    color,
                                    crate::ui::design::icon_sm(),
                                ))
                                .child(
                                    div()
                                        .font_family(crate::ui::design::FONT_MONO)
                                        .text_size(crate::ui::design::text_label())
                                        .child(format!("${{{}}}", reference.name)),
                                )
                                .child(state)
                        })),
                )
            })
            .when(missing_reference, |field| {
                field.child(form_notice(
                    lucide_icons::Icon::Info,
                    crate::ui::design::t3(cx),
                    "Choro reads variables from its own environment. Apps opened from the Dock do not see shell exports, so launch Choro from that shell or use launchctl setenv.",
                    cx,
                ))
            })
            .when(inline_secret, |field| {
                field.child(form_notice(
                    lucide_icons::Icon::AlertTriangle,
                    crate::ui::design::amber(cx),
                    "This URL contains a password or token, which is saved in the project configuration. Reference an environment variable instead.",
                    cx,
                ))
            })
            .into_any_element()
    }

    fn render_access_field(
        &self,
        index: usize,
        provider: DbProvider,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let can_edit = provider.capabilities().row_editing;
        v_flex()
            .w_full()
            .gap_1p5()
            .child(db_form_field_label("Access", cx))
            .when(can_edit, |field| {
                field.child(
                    h_flex()
                        .gap_2()
                        .child(
                            crate::ui::style::secondary_button_compact(
                                "db-access-read-only",
                                "Read only",
                            )
                            .icon(IconName::Eye)
                            .selected(read_only)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_read_only(index, true, cx);
                                },
                            )),
                        )
                        .child(
                            crate::ui::style::secondary_button_compact(
                                "db-access-writes",
                                "Allow edits",
                            )
                            .icon(IconName::Inspector)
                            .selected(!read_only)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_read_only(index, false, cx);
                                },
                            )),
                        ),
                )
            })
            .when(!can_edit, |field| {
                field.child(super::super::access_indicators(false, true, cx))
            })
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(access_help(provider, read_only)),
            )
            .into_any_element()
    }

    fn render_help(
        &self,
        index: usize,
        row: &ConnectionRow,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let provider = row.provider;
        let show_help = row.show_help;
        v_flex()
            .w_full()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.36))
            .bg(crate::ui::design::base(cx).opacity(0.5))
            .px_3()
            .py_2()
            .gap_1p5()
            .child(
                h_flex()
                    .id(("db-provider-help", index))
                    .w_full()
                    .gap_1p5()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_help(index, cx);
                    }))
                    .child(
                        Icon::new(if show_help {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(provider_help_title(provider)),
                    ),
            )
            .when(show_help, |help| {
                help.children(provider_help_steps(provider).iter().enumerate().map(
                    |(step_index, step)| {
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .line_height(gpui::relative(1.4))
                            .text_color(crate::ui::design::t3(cx))
                            .child(SharedString::from(format!("{}. {}", step_index + 1, step)))
                    },
                ))
            })
            .into_any_element()
    }

    fn render_test_section(
        &self,
        index: usize,
        row: &ConnectionRow,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let status = self.connection_tests.get(&row.id);
        let testing = matches!(status, Some(TestState::Testing { .. }));
        let uri = row.uri.read(cx).value().to_string();
        let endpoint = endpoint_summary(row.provider, &uri);
        let label = match status {
            Some(TestState::Testing { .. }) => "Testing",
            Some(TestState::Failed(_)) => "Test again",
            _ => "Test connection",
        };
        let result: gpui::AnyElement = match status {
            None => div()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child("Checks that Choro can reach and sign in. Nothing is saved.")
                .into_any_element(),
            Some(TestState::Testing { .. }) => h_flex()
                .gap_1p5()
                .items_center()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .child(Spinner::new().xsmall())
                .child(SharedString::from(match &endpoint {
                    Some(endpoint) => format!("Connecting to {endpoint}"),
                    None => "Connecting".to_string(),
                }))
                .into_any_element(),
            Some(TestState::Connected { elapsed }) => h_flex()
                .gap_1p5()
                .items_center()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::CheckCircle2,
                    crate::ui::design::sage(cx),
                    crate::ui::design::icon_sm(),
                ))
                .child(SharedString::from(format!(
                    "Connected in {} ms",
                    elapsed.as_millis().max(1)
                )))
                .into_any_element(),
            Some(TestState::Failed(_)) => h_flex()
                .gap_1p5()
                .items_center()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::CircleX,
                    crate::ui::design::rose(cx),
                    crate::ui::design::icon_sm(),
                ))
                .child("Connection failed")
                .into_any_element(),
        };
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        crate::ui::style::dialog_neutral_button("test-db-connection", label, cx)
                            .flex_none()
                            .icon(IconName::Redo2)
                            .loading(testing)
                            .disabled(testing)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.test_connection(index, cx);
                            })),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(result)),
            )
            .when_some(
                match status {
                    Some(TestState::Failed(error)) => Some(error.clone()),
                    _ => None,
                },
                |section, error| {
                    section.child(
                        div()
                            .w_full()
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::design::line(cx).opacity(0.36))
                            .bg(crate::ui::design::base(cx).opacity(0.5))
                            .px_3()
                            .py_2()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .line_height(gpui::relative(1.5))
                            .text_color(crate::ui::design::t2(cx))
                            .whitespace_normal()
                            .child(SharedString::from(error)),
                    )
                },
            )
            .into_any_element()
    }
}

impl Render for DbConnectionsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = v_flex()
            .gap_3()
            .w(px(DB_CONNECTION_EDITOR_CONTENT_WIDTH))
            .max_h(px(620.))
            .overflow_y_scrollbar();

        let Some(index) = self.active_row else {
            return editor
                .child(self.render_connection_list(cx))
                .into_any_element();
        };
        let Some(row) = self.rows.get(index) else {
            self.active_row = None;
            return editor
                .child(self.render_connection_list(cx))
                .into_any_element();
        };
        let provider = row.provider;
        let read_only = effective_read_only(provider, row.read_only);

        editor
            .child(
                v_flex()
                    .id(("database-connection-form", index))
                    .w_full()
                    .gap_4()
                    .child(
                        h_flex().child(
                            crate::ui::style::dialog_neutral_button(
                                "back-to-db-connections",
                                "All connections",
                                cx,
                            )
                            .flex_none()
                            .icon(IconName::ChevronLeft)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_row = None;
                                this.validation_error = None;
                                cx.notify();
                            })),
                        ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_start()
                            .child(super::super::provider_brand_mark(provider, 32.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_title())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(provider.display_name()),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(provider_form_description(provider)),
                                    )
                                    .child(
                                        h_flex().pt_0p5().gap_1p5().flex_wrap().children(
                                            capability_facts(provider)
                                                .into_iter()
                                                .map(|fact| crate::ui::style::tag(fact, cx)),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(db_form_field_label("Connection name", cx))
                            .child(Input::new(&row.name).w_full()),
                    )
                    .child(self.render_connection_field(index, row, cx))
                    .child(self.render_access_field(index, provider, read_only, cx))
                    .child(self.render_help(index, row, cx))
                    .when_some(self.validation_error.clone(), |form, error| {
                        form.child(form_notice(
                            lucide_icons::Icon::CircleX,
                            crate::ui::design::rose(cx),
                            error,
                            cx,
                        ))
                    })
                    .child(self.render_test_section(index, row, cx)),
            )
            .into_any_element()
    }
}

fn db_form_field_label(text: impl Into<SharedString>, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t2(cx))
        .child(text.into())
        .into_any_element()
}

/// A glyph-colored note under a form field; the text stays neutral.
fn form_notice(
    icon: lucide_icons::Icon,
    color: gpui::Hsla,
    text: impl Into<SharedString>,
    cx: &App,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .gap_1p5()
        .items_start()
        .child(
            div()
                .pt(px(2.))
                .child(crate::ui::design::indicator::lucide_icon(
                    icon,
                    color,
                    crate::ui::design::icon_sm(),
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(crate::ui::design::text_ui())
                .line_height(gpui::relative(1.4))
                .text_color(crate::ui::design::t2(cx))
                .whitespace_normal()
                .child(text.into()),
        )
        .into_any_element()
}

pub(super) fn test_state_indicator(
    status: Option<&TestState>,
    cx: &App,
) -> Option<gpui::AnyElement> {
    let (icon, color, label) = match status? {
        TestState::Testing { .. } => (
            lucide_icons::Icon::CircleDashed,
            crate::ui::design::t3(cx),
            "Testing",
        ),
        TestState::Connected { .. } => (
            lucide_icons::Icon::CheckCircle2,
            crate::ui::design::sage(cx),
            "Connected",
        ),
        TestState::Failed(_) => (
            lucide_icons::Icon::CircleX,
            crate::ui::design::rose(cx),
            "Failed",
        ),
    };
    Some(
        h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t2(cx))
            .child(crate::ui::design::indicator::lucide_icon(
                icon,
                color,
                crate::ui::design::icon_sm(),
            ))
            .child(label)
            .into_any_element(),
    )
}
