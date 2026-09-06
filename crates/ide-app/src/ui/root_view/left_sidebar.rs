//! Left sidebar chrome around the project list: a header row with Quick Open
//! and the two everyday actions, and a footer bar with the occasional actions
//! plus the Active/All view switch. Needs attention, Pinned, and the project
//! sections themselves live in `ProjectList` and are untouched here.

use super::*;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use crate::ui::onboarding::{self, OnboardingEvent, SpotlightTarget};

/// Height of the footer bar; matches the right sidebar's Help/Settings footer.
const FOOTER_H: f32 = 44.;

impl RootView {
    /// Top of the left sidebar. The search pill only launches Quick Open, so
    /// there is one search to learn. New Agent takes the right edge as the
    /// row's primary; Add Project sits beside it as a plain icon.
    pub(super) fn left_sidebar_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let right_panel = self.right_panel.clone();
        let workspace = self.workspace.clone();
        h_flex()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .items_center()
            .child(
                style::sidebar_search_pill("left-sidebar-search", cx)
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Quick Open (⌘P)").build(window, cx)
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(OpenProjectSearch), cx);
                    }),
            )
            .child(
                style::sidebar_bar_icon_button("left-sidebar-add-project", IconName::FolderOpen, cx)
                    .relative()
                    .tooltip("Add project (⌘O)")
                    // The tour's final pointer targets this button; the marker
                    // reports its bounds so it can be framed.
                    .child(onboarding::target_marker(SpotlightTarget::AddProject, cx))
                    .on_click(move |_, _, cx| {
                        // Clicking Add Project is how the tour ends: one click
                        // both dismisses the pointer and opens the real flow.
                        if onboarding::finishing_at_add_project(cx) {
                            onboarding::emit(OnboardingEvent::Exit, cx);
                        }
                        workspace.update(cx, |workspace, cx| workspace.open_folder_dialog(cx));
                    }),
            )
            .child(
                style::sidebar_bar_primary_button("left-sidebar-new-agent", IconName::Plus, cx)
                    .relative()
                    .tooltip("New agent (⌘N)")
                    .child(onboarding::target_marker(SpotlightTarget::NewAgent, cx))
                    .on_click(move |_, window, cx| {
                        right_panel.update(cx, |panel, cx| panel.open_new_agent(window, cx));
                    }),
            )
    }

    /// Bottom of the left sidebar: the actions used now and then, and the view
    /// switch for the project list. Same bar height as the right sidebar's
    /// footer. No rule above it and no box around the icons: the list's bottom
    /// padding separates the bar, and zero gap between the three squares
    /// groups them, like the icons in the sidebar's top zone.
    pub(super) fn left_sidebar_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let my_tasks_center = self.center.clone();
        let ask_history_center = self.center.clone();
        let section_workspace = self.workspace.clone();
        h_flex()
            .h(px(FOOTER_H))
            .w_full()
            .flex_none()
            .px_2()
            .items_center()
            .child(
                style::sidebar_footer_icon_button("left-sidebar-my-tasks", IconName::CircleCheck, cx)
                    .tooltip("My tasks across all projects")
                    .on_click(move |_, _, cx| {
                        my_tasks_center.update(cx, |center, cx| center.show_my_tasks(cx));
                    }),
            )
            .child(
                style::sidebar_footer_icon_button("left-sidebar-ask-history", IconName::BookOpen, cx)
                    .tooltip("Quick questions across all projects")
                    .on_click(move |_, _, cx| {
                        ask_history_center.update(cx, |center, cx| center.show_quick_ask_history(cx));
                    }),
            )
            .child(
                Button::new("left-sidebar-add-section")
                    .ghost()
                    .xsmall()
                    .compact()
                    .h(px(style::SIDEBAR_FOOTER_CONTROL_H))
                    .w(px(style::SIDEBAR_FOOTER_CONTROL_H))
                    // Same Lucide set as its two neighbours, so the stroke
                    // weight matches; the custom add-row asset is drawn heavier.
                    .child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::ListPlus,
                        crate::ui::design::t3(cx),
                        crate::ui::design::icon(),
                    ))
                    .tooltip("Add section")
                    .on_click(move |_, window, cx| {
                        crate::ui::project_list::ProjectList::open_section_name_dialog(
                            section_workspace.clone(),
                            None,
                            "".into(),
                            window,
                            cx,
                        );
                    }),
            )
            .child(div().flex_1())
            .child(self.sidebar_view_menu(cx))
    }

    /// "Active ⌄" / "All ⌄": which agents the project list shows. A labelled
    /// dropdown rather than a segmented control because it is a preference
    /// people set once, yet the current view must stay readable since Active
    /// hides agents. Needs attention and Pinned are not affected by it.
    fn sidebar_view_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_work = self.workspace.read(cx).sidebar_active_work;
        let workspace = self.workspace.clone();
        Button::new("left-sidebar-view")
            .ghost()
            .xsmall()
            .compact()
            .h(px(style::SIDEBAR_FOOTER_CONTROL_H))
            .tooltip("Which agents the project list shows")
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::ListFilter,
                        crate::ui::design::t3(cx),
                        crate::ui::design::icon(),
                    ))
                    .child(
                        // Sized to the 16px filter glyph beside it, not to the
                        // footer buttons: this is a value, not an action.
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::ui::design::t2(cx))
                            .child(if active_work { "Active" } else { "All" }),
                    )
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
            .dropdown_menu_with_anchor(gpui::Corner::BottomRight, move |menu, _, _| {
                let workspace_all = workspace.clone();
                let workspace_active = workspace.clone();
                menu.min_w(px(200.))
                    .item(
                        PopupMenuItem::new("All")
                            .checked(!active_work)
                            .on_click(move |_, _, cx| {
                                workspace_all.update(cx, |workspace, cx| {
                                    workspace.set_sidebar_active_work(false, cx)
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Active")
                            .checked(active_work)
                            .on_click(move |_, _, cx| {
                                workspace_active.update(cx, |workspace, cx| {
                                    workspace.set_sidebar_active_work(true, cx)
                                });
                            }),
                    )
            })
    }
}
