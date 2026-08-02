//! One canonical split / dropdown button — a labelled primary zone, a
//! full-height divider, and a caret zone that opens a popup menu.
//!
//! Every split button in the app (Run scripts, git Fetch/Pull/Push, Commit)
//! is built from this so they share one shape: same height, radius, font size,
//! padding, and an edge-to-edge divider right before the arrow. Only the
//! [`SplitPalette`] (background / text / divider) changes between them — Run
//! carries the accent, the git buttons stay quiet on the panel/editor surface.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, App, Corner, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu},
    Disableable, Icon, IconName, Sizable,
};

/// The only thing that differs between split buttons: the surface it sits on,
/// its text, its hover wash, its border, and the divider tone. Everything
/// structural (size, padding, divider geometry) is fixed by the component.
#[derive(Clone, Copy)]
pub struct SplitPalette {
    pub bg: Hsla,
    pub fg: Hsla,
    pub hover: Hsla,
    pub border: Hsla,
    pub divider: Hsla,
}

impl SplitPalette {
    /// Run scripts — a fill-first control on a panel: it lifts off the canvas
    /// (no resting stroke). State color stays on the play glyph.
    pub fn accent(cx: &App) -> Self {
        Self {
            bg: crate::ui::design::control_raised(cx),
            fg: crate::ui::design::t1(cx),
            hover: crate::ui::design::control_raised_hover(cx),
            border: crate::ui::design::control_line(cx),
            divider: crate::ui::design::t3(cx).opacity(0.22),
        }
    }

    /// Git Fetch/Pull/Push — sits on the dark panel, so it lifts *up* off the
    /// background (raised) and carries the main foreground text.
    pub fn chatbox(cx: &App) -> Self {
        Self {
            bg: crate::ui::design::control_raised(cx),
            fg: crate::ui::design::t1(cx),
            hover: crate::ui::design::control_raised_hover(cx),
            border: crate::ui::design::control_line(cx),
            divider: crate::ui::design::t3(cx).opacity(0.22),
        }
    }

    /// Commit — sits *inside* the commit box, which is on the `focus` plane, so
    /// it steps off that plane to separate without a stroke.
    pub fn editor(cx: &App) -> Self {
        let plane = crate::ui::design::focus(cx);
        Self {
            bg: crate::ui::design::control_on(plane, cx),
            fg: crate::ui::design::t1(cx),
            hover: crate::ui::design::control_on_hover(plane, cx),
            border: crate::ui::design::control_line(cx),
            divider: crate::ui::design::t3(cx).opacity(0.22),
        }
    }
}

type MenuFn = Rc<dyn Fn(PopupMenu, &mut Window, &mut gpui::Context<PopupMenu>) -> PopupMenu>;
type ClickFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// A split / dropdown button. Build with [`SplitButton::new`], optionally give
/// the primary zone its own action with [`SplitButton::on_primary`] (without one
/// the whole button just opens the menu, e.g. Run scripts).
#[derive(IntoElement)]
pub struct SplitButton {
    id: SharedString,
    label: SharedString,
    icon: Option<Icon>,
    tooltip: Option<SharedString>,
    disabled: bool,
    palette: SplitPalette,
    primary: Option<ClickFn>,
    menu_anchor: Corner,
    menu: MenuFn,
}

impl SplitButton {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        palette: SplitPalette,
        menu: impl Fn(PopupMenu, &mut Window, &mut gpui::Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            tooltip: None,
            disabled: false,
            palette,
            primary: None,
            menu_anchor: Corner::TopLeft,
            menu: Rc::new(menu),
        }
    }

    /// Give the primary (label) zone its own click action. Without this the
    /// whole button opens the dropdown menu.
    pub fn on_primary(mut self, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.primary = Some(Rc::new(on_click));
        self
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Align the popup to a trigger corner. Header controls use `TopRight` so
    /// their menus stay flush with the app's right edge, matching the mock.
    pub fn menu_anchor(mut self, anchor: Corner) -> Self {
        self.menu_anchor = anchor;
        self
    }
}

impl RenderOnce for SplitButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.palette;
        let menu_anchor = self.menu_anchor;
        // Both zones are transparent — the container carries the fill/border so
        // the divider can be a single hairline between two ghost zones.
        let zone_variant = || {
            ButtonCustomVariant::new(cx)
                .color(gpui::transparent_black())
                .foreground(p.fg)
                .border(gpui::transparent_black())
                .hover(p.hover)
                .active(p.hover)
        };

        let mut primary_base = Button::new(SharedString::from(format!("{}-primary", self.id)))
            .xsmall()
            .compact()
            .h_full()
            .rounded(gpui::Pixels::ZERO)
            .px(crate::ui::design::split_primary_pad_x())
            .text_size(crate::ui::design::text_ui())
            .font_weight(gpui::FontWeight::MEDIUM)
            .custom(zone_variant())
            .disabled(self.disabled)
            .label(self.label);
        if let Some(icon) = self.icon {
            primary_base = primary_base.icon(icon);
        }
        if let Some(tooltip) = self.tooltip {
            primary_base = primary_base.tooltip(tooltip);
        }
        // A primary action makes the label its own button; without one the label
        // zone opens the same menu as the caret (e.g. Run scripts). The two arms
        // are different element types, so unify to `AnyElement`.
        let primary: gpui::AnyElement = match self.primary.clone() {
            Some(on_click) => primary_base
                .on_click(move |_, window, cx| on_click(window, cx))
                .into_any_element(),
            None => {
                let menu = self.menu.clone();
                primary_base
                    .dropdown_menu_with_anchor(menu_anchor, move |m, window, cx| {
                        menu(m, window, cx)
                    })
                    .into_any_element()
            }
        };

        let caret = {
            let menu = self.menu.clone();
            Button::new(SharedString::from(format!("{}-caret", self.id)))
                .xsmall()
                .compact()
                .h_full()
                .w(crate::ui::design::split_caret_w())
                .px_0()
                .rounded(gpui::Pixels::ZERO)
                .custom(zone_variant())
                .disabled(self.disabled)
                .icon(Icon::new(IconName::ChevronDown).size(crate::ui::design::icon_sm()))
                .dropdown_menu_with_anchor(menu_anchor, move |m, window, cx| menu(m, window, cx))
        };

        h_flex()
            .h(crate::ui::design::control_h_sm())
            .rounded(crate::ui::design::r_sm())
            .bg(p.bg)
            .border_1()
            .border_color(p.border)
            .text_color(p.fg)
            .overflow_hidden()
            .when(self.disabled, |row| row.opacity(0.55))
            .child(primary)
            .child(
                div()
                    .flex_none()
                    .w(crate::ui::design::split_divider_w())
                    .h_full()
                    .bg(p.divider),
            )
            .child(caret)
    }
}
