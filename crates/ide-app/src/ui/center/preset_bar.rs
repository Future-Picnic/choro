use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Corner, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    menu::PopupMenuItem,
    tooltip::Tooltip,
    Icon, IconName, Sizable,
};

use crate::state::{SessionId, TerminalManager, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::preset_editor::PresetEditor;
use crate::ui::split_button::{SplitButton, SplitPalette};
use crate::ui::style;

/// Run state of one script preset, derived from its latest terminal session.
enum ScriptState {
    Idle,
    Running(crate::state::SessionId),
    Failed,
    Succeeded,
}

/// Top strip of the center panel: one-click run buttons for the active
/// project's script presets (tinted by run state) plus script editing.
pub struct PresetBar {
    workspace: Entity<Workspace>,
    terminals: Entity<TerminalManager>,
    center: WeakEntity<CenterArea>,
    compact: bool,
    show_settings: bool,
}

impl PresetBar {
    pub fn view(
        workspace: Entity<Workspace>,
        terminals: Entity<TerminalManager>,
        center: WeakEntity<CenterArea>,
        compact: bool,
        show_settings: bool,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&terminals, |_, _, cx| cx.notify()).detach();
            if let Some(center_entity) = center.upgrade() {
                cx.observe(&center_entity, |_, _, cx| cx.notify()).detach();
            }
            Self {
                workspace,
                terminals,
                center,
                compact,
                show_settings,
            }
        })
    }
}

impl Render for PresetBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(project) = self.workspace.read(cx).active_project() else {
            return div().into_any_element();
        };
        let project_id = project.id;
        let presets: Vec<(SharedString, String)> = project
            .presets
            .iter()
            .map(|p| (SharedString::from(p.name.clone()), p.command.clone()))
            .collect();

        let manager = self.terminals.read(cx);
        let states: Vec<ScriptState> = presets
            .iter()
            .map(
                |(name, _)| match manager.preset_state(project_id, name.as_ref()) {
                    Some((id, true, _)) => ScriptState::Running(id),
                    Some((_, false, Some(false))) => ScriptState::Failed,
                    Some((_, false, _)) => ScriptState::Succeeded,
                    None => ScriptState::Idle,
                },
            )
            .collect();

        // Compact (header) mode renders a single "Run" dropdown instead of a row
        // of chips: the presets live in the menu (click to run or, if already
        // running, jump to its terminal), alongside a New terminal action and an
        // "Edit scripts" entry that opens the preset editor. The live run state
        // still shows on the project in the sidebar.
        if self.compact {
            let success = crate::ui::design::sage(cx);
            let items: Vec<(SharedString, String, Option<SessionId>, gpui::Hsla)> = presets
                .iter()
                .zip(&states)
                .map(|((name, command), state)| {
                    // A clean coloured circle carries the run state in the
                    // menu: green when running or last-clean, red on failure,
                    // muted terminal glyph when it has never run.
                    let (running, color) = match state {
                        ScriptState::Running(id) => (Some(*id), success),
                        ScriptState::Failed => (None, crate::ui::design::rose(cx)),
                        ScriptState::Succeeded => (None, success),
                        ScriptState::Idle => (None, crate::ui::design::sage(cx)),
                    };
                    (name.clone(), command.clone(), running, color)
                })
                .collect();
            let center = self.center.clone();
            let edit_ws = self.workspace.clone();
            let shell_center = self.center.clone();
            // The trigger follows the mock's neutral split-control language;
            // the sage play glyph alone carries run semantics.
            let palette = SplitPalette::accent(cx);
            // With a single script the button *is* that script (no "Run"
            // wrapper); with several it's a "Run" launcher; with none it invites
            // you to add one.
            let label = match items.len() {
                0 => SharedString::from("Scripts"),
                1 => items[0].0.clone(),
                _ => SharedString::from("Run"),
            };
            let single_script = items.len() == 1;
            let onboarding_primary = (crate::ui::onboarding::is_project(project_id, cx)
                && single_script)
                .then(|| items.first().cloned())
                .flatten();
            let primary_center = self.center.clone();
            let run_menu_center = self.center.clone();
            let mut button = SplitButton::new(
                "run-menu",
                label,
                palette,
                move |mut menu, _window, menu_cx| {
                    run_menu_center
                        .update(menu_cx, |center, cx| {
                            center.set_project_preview_overlay_suspended(true, cx)
                        })
                        .ok();
                    let center_after_menu = run_menu_center.clone();
                    menu_cx
                        .on_release(move |_, cx| {
                            center_after_menu
                                .update(cx, |center, cx| {
                                    center.set_project_preview_overlay_suspended(false, cx)
                                })
                                .ok();
                        })
                        .detach();
                    menu = menu.label("Run script");
                    if items.is_empty() {
                        menu = menu.item(PopupMenuItem::new("No run scripts yet").disabled(true));
                    } else {
                        for (name, command, running_id, color) in items.clone() {
                            let center = center.clone();
                            menu = menu.item(
                                PopupMenuItem::new(name.clone())
                                    .icon(Icon::empty().path("icons/play.svg").text_color(color))
                                    .checked(single_script || running_id.is_some())
                                    .on_click(move |_, window, cx| {
                                        center
                                            .update(cx, |center, cx| {
                                                if let Some(id) = running_id {
                                                    center
                                                        .focus_terminal(project_id, id, window, cx);
                                                } else {
                                                    center.run_preset(&name, &command, window, cx);
                                                }
                                            })
                                            .ok();
                                    }),
                            );
                        }
                    }
                    let shell = shell_center.clone();
                    let ws = edit_ws.clone();
                    menu.separator()
                        .item(
                            PopupMenuItem::new("New terminal")
                                .icon(Icon::new(IconName::Plus))
                                .on_click(move |_, window, cx| {
                                    shell
                                        .update(cx, |center, cx| center.spawn_shell(window, cx))
                                        .ok();
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Edit scripts…")
                                .icon(Icon::new(IconName::Settings2))
                                .on_click(move |_, window, cx| {
                                    PresetEditor::open(ws.clone(), window, cx);
                                }),
                        )
                },
            )
            .icon(
                Icon::empty()
                    .path("icons/play.svg")
                    .text_color(crate::ui::design::sage(cx)),
            )
            .menu_anchor(Corner::TopRight)
            .tooltip("Run scripts");
            if let Some((name, command, running_id, _)) = onboarding_primary {
                button = button.on_primary(move |window, cx| {
                    primary_center
                        .update(cx, |center, cx| {
                            if let Some(id) = running_id {
                                center.focus_terminal(project_id, id, window, cx);
                            } else {
                                center.run_preset(&name, &command, window, cx);
                            }
                        })
                        .ok();
                });
            }
            let (supports_preview, preview_open) = self
                .center
                .upgrade()
                .map(|center| {
                    let center = center.read(cx);
                    (
                        center.supports_project_preview(),
                        center.is_project_preview_open(project_id),
                    )
                })
                .unwrap_or((false, false));
            let preview_center = self.center.clone();
            let hide_preview_toggle =
                crate::ui::onboarding::hides_project_preview_toggle(project_id, cx);
            return div()
                .relative()
                .flex_none()
                .child(h_flex().items_center().gap_1().child(button).when(
                    supports_preview && !hide_preview_toggle,
                    |row| {
                        row.child(
                            style::header_workspace_toggle_button(
                                "toggle-project-preview",
                                IconName::Eye,
                                "Preview",
                                preview_open,
                                cx,
                            )
                            .tooltip(if preview_open {
                                "Close project Preview"
                            } else {
                                "Open project Preview"
                            })
                            .on_click(move |_, _, cx| {
                                preview_center
                                    .update(cx, |center, cx| center.toggle_project_preview(cx))
                                    .ok();
                            }),
                        )
                    },
                ))
                .child(crate::ui::onboarding::target_marker(
                    crate::ui::onboarding::SpotlightTarget::RunScript,
                    cx,
                ))
                .into_any_element();
        }

        let edit_workspace = self.workspace.clone();
        let shell_center = self.center.clone();

        let divider = || {
            div()
                .h_full()
                .w(px(1.))
                .bg(crate::ui::design::line(cx).opacity(0.45))
        };
        let scripts_empty = presets.is_empty();
        let control_height = px(if self.compact { 24. } else { 32. });
        let empty_min_width = px(if self.compact { 0. } else { 128. });
        let terminal_min_width = px(if self.compact { 0. } else { 98. });
        let icon_button_size = px(if self.compact { 24. } else { 32. });
        let settings_icon_size = px(if self.compact { 15. } else { 17. });
        let script_buttons: Vec<_> = presets
            .into_iter()
            .zip(states)
            .enumerate()
            .map(|(ix, ((name, command), state))| {
                let center = self.center.clone();
                let run_name = name.clone();

                // The chip itself stays one neutral style for every script —
                // only the status dot carries the run state: green when running
                // (or last finished cleanly), red when the last run failed,
                // muted gray when it has never run.
                let (dot, tooltip) = match &state {
                    ScriptState::Running(_) => {
                        (crate::ui::design::sage(cx), Some("Running — click to view"))
                    }
                    ScriptState::Failed => (
                        crate::ui::design::rose(cx),
                        Some("Last run failed — click to run again"),
                    ),
                    ScriptState::Succeeded => (
                        crate::ui::design::sage(cx),
                        Some("Last run finished — click to run again"),
                    ),
                    ScriptState::Idle => (crate::ui::design::t3(cx).opacity(0.55), None),
                };
                let hover = crate::ui::design::hover(cx);

                let running_id = match state {
                    ScriptState::Running(id) => Some(id),
                    _ => None,
                };
                style::script_chip(name, dot, cx)
                    .id(("run-preset", ix))
                    .cursor_pointer()
                    .hover(move |button| button.bg(hover))
                    .when_some(tooltip, |button, tooltip| {
                        button.tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
                    })
                    .on_click(move |_, window, cx| {
                        center
                            .update(cx, |center, cx| {
                                if let Some(id) = running_id {
                                    center.focus_terminal(project_id, id, window, cx);
                                } else {
                                    center.run_preset(&run_name, &command, window, cx);
                                }
                            })
                            .ok();
                    })
            })
            .collect();

        h_flex()
            .when(!self.compact, |bar| {
                bar.w_full()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx))
            })
            .when(self.compact, |bar| bar.w_full().justify_end())
            .gap_1p5()
            .items_center()
            .when(scripts_empty, |bar| {
                let workspace = self.workspace.clone();
                bar.child(
                    Button::new("add-first-script")
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(crate::ui::design::base(cx))
                                .foreground(crate::ui::design::t1(cx))
                                .border(crate::ui::design::line(cx).opacity(0.5))
                                .hover(crate::ui::design::surface_2(cx).opacity(0.85))
                                .active(crate::ui::design::surface_2(cx)),
                        )
                        .xsmall()
                        .compact()
                        .h(control_height)
                        .min_w(empty_min_width)
                        .rounded(crate::ui::design::r_xs())
                        .icon(IconName::Plus)
                        .label("Add Run Script")
                        .on_click(move |_, window, cx| {
                            PresetEditor::open(workspace.clone(), window, cx);
                        }),
                )
            })
            .when(!scripts_empty, |bar| {
                bar.child(h_flex().gap_1().children(script_buttons))
            })
            // "+ Terminal" stays inside the terminal panel in compact/header mode.
            .when(!self.compact, |bar| {
                bar.child(
                    Button::new("bar-new-terminal")
                        .ghost()
                        .xsmall()
                        .compact()
                        .h(control_height)
                        .min_w(terminal_min_width)
                        .rounded(crate::ui::design::r_xs())
                        .icon(IconName::Plus)
                        .label("Terminal")
                        .text_color(crate::ui::design::t3(cx))
                        .tooltip("New terminal")
                        .on_click(move |_, window, cx| {
                            shell_center
                                .update(cx, |center, cx| center.spawn_shell(window, cx))
                                .ok();
                        }),
                )
            })
            .when(!self.compact, |bar| bar.child(div().flex_1()))
            .when(
                self.compact && !scripts_empty && self.show_settings,
                |bar| bar.child(divider()),
            )
            .when(!scripts_empty && self.show_settings, |bar| {
                bar.child(
                    h_flex()
                        .id("edit-presets")
                        .h(icon_button_size)
                        .w(icon_button_size)
                        .min_w(icon_button_size)
                        .items_center()
                        .justify_center()
                        .rounded(crate::ui::design::r_xs())
                        .border_1()
                        .border_color(crate::ui::design::line(cx).opacity(0.4))
                        .bg(crate::ui::design::base(cx))
                        .cursor_pointer()
                        .hover(|button| button.bg(crate::ui::design::surface_2(cx).opacity(0.84)))
                        .tooltip(|window, cx| Tooltip::new("Script settings").build(window, cx))
                        .child(
                            Icon::new(IconName::Settings2)
                                .size(settings_icon_size)
                                .text_color(crate::ui::design::t3(cx)),
                        )
                        .on_click(move |_, window, cx| {
                            PresetEditor::open(edit_workspace.clone(), window, cx);
                        }),
                )
            })
            .into_any_element()
    }
}
