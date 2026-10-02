//! Shared confirmation dialog.
//!
//! One polished alert/confirmation modal used across the whole app so every
//! destructive or important prompt looks identical and respects the active
//! theme. Build one with [`ConfirmDialog::new`], configure it with the builder
//! methods, and present it with [`ConfirmDialog::open`].
//!
//! Layout: a tinted icon badge + title, a muted supporting line, an optional
//! "detail" chip for the affected target (file path, branch, …) and a footer
//! with a ghost *Cancel* and a tone-colored confirm button.

use std::{cell::Cell, rc::Rc};

use gpui::{
    div, px, relative, App, FontWeight, Hsla, IntoElement, ParentElement, SharedString, Styled,
    Window,
};
use gpui_component::{
    checkbox::Checkbox, h_flex, v_flex, ActiveTheme, Icon, IconName, Sizable, WindowExt,
};

/// Accent tone for a confirmation dialog. Picks the icon-badge tint and the
/// confirm button variant. Every color is pulled from the theme, so the dialog
/// reads correctly in every light/dark palette.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmTone {
    /// Destructive, irreversible actions (discard, delete, overwrite).
    Danger,
    /// Cautionary actions that are notable but not strictly destructive.
    Warning,
    /// Neutral / affirmative confirmations.
    #[allow(dead_code, reason = "reserved for affirmative confirmation flows")]
    Primary,
}

type ConfirmHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// A reusable confirmation modal.
pub struct ConfirmDialog {
    tone: ConfirmTone,
    icon: IconName,
    title: SharedString,
    message: SharedString,
    detail: Option<SharedString>,
    route: Option<(SharedString, SharedString)>,
    checkbox: Option<(SharedString, SharedString)>,
    confirm_label: SharedString,
    cancel_label: SharedString,
    confirm_id: SharedString,
    width: f32,
    on_confirm: Option<ConfirmHandler>,
}

impl ConfirmDialog {
    /// Starts a danger-tone confirmation with a warning triangle. Override the
    /// tone/icon/labels with the builder methods as needed.
    pub fn new(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self {
            tone: ConfirmTone::Danger,
            icon: IconName::TriangleAlert,
            title: title.into(),
            message: message.into(),
            detail: None,
            route: None,
            checkbox: None,
            confirm_label: "Confirm".into(),
            cancel_label: "Cancel".into(),
            confirm_id: "confirm-dialog-ok".into(),
            width: 440.0,
            on_confirm: None,
        }
    }

    pub fn tone(mut self, tone: ConfirmTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }

    /// A secondary line shown in a subtle chip (e.g. the file path or branch).
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// A branch route shown as a stacked two-row chip: the source branch on top,
    /// the branch it lands in underneath. A single `source → base` line hides the
    /// destination behind an ellipsis whenever the source name is long, and the
    /// destination is exactly the part people re-read before merging, so it gets
    /// its own row and never truncates away.
    pub fn branch_route(
        mut self,
        from: impl Into<SharedString>,
        into: impl Into<SharedString>,
    ) -> Self {
        self.route = Some((from.into(), into.into()));
        self
    }

    pub fn confirm_label(mut self, label: impl Into<SharedString>) -> Self {
        self.confirm_label = label.into();
        self
    }

    /// An opt-in option that starts unchecked each time the dialog opens.
    pub fn checkbox(
        mut self,
        label: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        self.checkbox = Some((label.into(), description.into()));
        self
    }

    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Stable element id for the confirm button. Keep it unique per dialog so
    /// GPUI's input routing stays predictable.
    pub fn confirm_id(mut self, id: impl Into<SharedString>) -> Self {
        self.confirm_id = id.into();
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Runs when the user accepts. The dialog is already closed by the time this
    /// fires.
    pub fn on_confirm(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(move |_, window, cx| handler(window, cx)));
        self
    }

    pub fn on_confirm_with_checkbox(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    /// Presents the dialog in the given window.
    pub fn open(self, window: &mut Window, cx: &mut App) {
        let ConfirmDialog {
            tone,
            icon,
            title,
            message,
            detail,
            route,
            checkbox,
            confirm_label,
            cancel_label,
            confirm_id,
            width,
            on_confirm,
        } = self;
        let checked = Rc::new(Cell::new(false));

        window.open_dialog(cx, move |dialog, _, cx| {
            let accent = tone_color(tone, cx);
            let icon = icon.clone();
            let on_confirm = on_confirm.clone();
            let confirm_label = confirm_label.clone();
            let confirm_id = confirm_id.clone();
            let cancel_label = cancel_label.clone();
            let detail = detail.clone();
            let route = route.clone();
            let checked_for_footer = checked.clone();
            let checkbox = checkbox.clone().map(|(label, description)| {
                let checked_for_click = checked.clone();
                v_flex()
                    .gap_1()
                    .child(
                        Checkbox::new("confirm-dialog-checkbox")
                            .small()
                            .label(label)
                            .checked(checked.get())
                            .on_click(move |value, _, cx| {
                                checked_for_click.set(*value);
                                cx.refresh_windows();
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(description),
                    )
            });

            dialog
                .w(px(width))
                .overlay_closable(true)
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(icon_badge(icon, accent, cx))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.clone())),
                )
                .child(
                    v_flex()
                        .pt_1()
                        .gap_3()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .line_height(relative(1.45))
                                .text_color(crate::ui::design::t3(cx))
                                .child(message.clone()),
                        )
                        .children(detail.map(|detail| detail_chip(detail, cx)))
                        .children(route.map(|(from, into)| route_chip(from, into, cx)))
                        .children(checkbox),
                )
                .footer(move |_, _, _, cx| {
                    let on_confirm = on_confirm.clone();
                    let checked = checked_for_footer.clone();
                    let confirm = match tone {
                        ConfirmTone::Danger => crate::ui::style::danger_button_compact(
                            confirm_id.clone(),
                            confirm_label.clone(),
                        ),
                        ConfirmTone::Warning => crate::ui::style::warning_button_compact(
                            confirm_id.clone(),
                            confirm_label.clone(),
                        ),
                        ConfirmTone::Primary => crate::ui::style::primary_button_compact(
                            confirm_id.clone(),
                            confirm_label.clone(),
                            cx,
                        ),
                    }
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        if let Some(handler) = on_confirm.clone() {
                            handler(checked.get(), window, cx);
                        }
                    });
                    vec![
                        // A dialog is a lifted box, so its neutral action is
                        // *recessed* (a step darker than the popover surface) —
                        // a raised fill would sit almost on the dialog colour and
                        // read as bare text.
                        crate::ui::style::dialog_neutral_button(
                            "confirm-dialog-cancel",
                            cancel_label.clone(),
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                        confirm,
                    ]
                })
        });
    }
}

/// Resolves the accent color for a tone from the active theme.
fn tone_color(tone: ConfirmTone, cx: &App) -> Hsla {
    match tone {
        ConfirmTone::Danger => crate::ui::design::rose(cx),
        ConfirmTone::Warning => crate::ui::design::amber(cx),
        ConfirmTone::Primary => crate::ui::design::accent(cx),
    }
}

/// The tinted, rounded icon badge shown next to the title.
/// The tinted identity badge that leads every dialog title. Shared so any modal
/// reads with the same identity as the confirmation dialogs rather than pairing
/// its title with a bare, untinted glyph.
pub fn icon_badge(icon: IconName, accent: Hsla, cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(28.))
        .rounded(cx.theme().radius)
        .bg(accent.opacity(0.12))
        .border_1()
        .border_color(accent.opacity(0.22))
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(icon)
                .size(crate::ui::design::icon())
                .text_color(accent),
        )
}

/// The stacked chip that spells out a branch route: where the work is now, and
/// the branch it lands in. Both rows are labelled so neither has to be inferred
/// from an arrow that may have been truncated away.
fn route_chip(from: SharedString, into: SharedString, cx: &App) -> impl IntoElement {
    v_flex()
        .w_full()
        .min_w(px(0.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(crate::ui::design::line(cx).opacity(0.6))
        .bg(crate::ui::design::surface(cx).opacity(0.4))
        .child(route_row("From", from, crate::ui::design::t2(cx), cx))
        .child(
            div()
                .h(px(1.))
                .w_full()
                .bg(crate::ui::design::line(cx).opacity(0.45)),
        )
        .child(route_row("Into", into, crate::ui::design::accent(cx), cx))
}

/// One labelled branch row inside [`route_chip`].
fn route_row(
    label: &'static str,
    value: SharedString,
    value_color: Hsla,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_w(px(0.))
        .items_center()
        .gap_2()
        .px_3()
        .py(px(7.))
        .child(
            div()
                .flex_none()
                .w(px(32.))
                .text_size(crate::ui::design::text_label())
                .text_color(crate::ui::design::t4(cx))
                .child(label),
        )
        .child(crate::ui::branch_icon::branch_icon(
            value_color.opacity(0.7),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(crate::ui::design::text_ui())
                .text_color(value_color)
                .child(value),
        )
}

/// The subtle chip that names the affected target (a path, branch, …).
fn detail_chip(detail: SharedString, cx: &App) -> impl IntoElement {
    div()
        .w_full()
        .min_w(px(0.))
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(crate::ui::design::line(cx).opacity(0.6))
        .bg(crate::ui::design::surface(cx).opacity(0.4))
        .text_size(crate::ui::design::text_ui())
        .text_color(crate::ui::design::t3(cx))
        .truncate()
        .child(detail)
}
