mod actions;
mod app_assets;
mod companion_music;
mod demo;
mod keymap;
mod notifications;
mod onboarding;
mod open_with;
mod remote;
mod state;
mod theme;
mod ui;
mod voice;

use gpui::{
    px, size, App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem, OsAction,
    SystemMenuType, WindowBounds, WindowKind, WindowOptions,
};
use gpui_component::{
    input::{Copy, Cut, Enter, Paste, Redo, SelectAll, Undo},
    Root, Theme, ThemeMode, TitleBar,
};

use crate::actions::{QuitApplication, ToggleFocusMode};
use crate::app_assets::AppAssets;
use crate::ui::root_view::RootView;

fn main() {
    if let Some(exit_code) = ide_core::git::handle_git_credential() {
        std::process::exit(exit_code);
    }
    if let Err(error) = demo::prepare() {
        eprintln!("failed to prepare Choro Demo: {error:#}");
    }
    if let Err(error) = onboarding::prepare() {
        eprintln!("failed to prepare Choro onboarding: {error:#}");
    }
    let app = Application::new().with_assets(AppAssets);

    app.run(move |cx: &mut App| {
        // Debug tripwire: blocking git helpers warn if they ever run on this
        // thread (i.e. inside a render path). Marked here, after the startup
        // helpers above, so intentional pre-UI blocking work stays quiet.
        ide_core::mark_ui_thread();
        gpui_component::init(cx);
        velotype::init_embedded(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        theme::init(cx);
        cx.bind_keys(keymap::bindings(&ide_core::AppConfig::load().keymap));
        cx.bind_keys(ui::project_search::bindings());
        cx.bind_keys(ui::command_palette::bindings());
        cx.bind_keys(ui::content_search::bindings());
        cx.bind_keys(ui::quick_task::bindings());
        cx.bind_keys(vec![
            KeyBinding::new("shift-enter", Enter { secondary: false }, Some("Input")),
            KeyBinding::new("ctrl-enter", Enter { secondary: true }, Some("Input")),
            KeyBinding::new(
                "cmd-c",
                gpui_terminal::Copy,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
            KeyBinding::new(
                "ctrl-shift-c",
                gpui_terminal::Copy,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
            KeyBinding::new(
                "cmd-v",
                gpui_terminal::Paste,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
            KeyBinding::new(
                "ctrl-shift-v",
                gpui_terminal::Paste,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
            KeyBinding::new(
                "shift-insert",
                gpui_terminal::Paste,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
            KeyBinding::new(
                "shift-tab",
                gpui_terminal::Backtab,
                Some(gpui_terminal::KEY_CONTEXT),
            ),
        ]);

        // Install an explicit macOS application menu so the native Cmd+Q
        // command dispatches our graceful-shutdown action instead of asking
        // AppKit to terminate the process directly.
        cx.set_menus(vec![
            Menu {
                name: ide_core::APP_NAME.into(),
                items: vec![
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action(format!("Quit {}", ide_core::APP_NAME), QuitApplication),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Undo", Undo, OsAction::Undo),
                    MenuItem::os_action("Redo", Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cut", Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", Paste, OsAction::Paste),
                    MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![MenuItem::action("Focus Mode", ToggleFocusMode)],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        let mut titlebar = TitleBar::title_bar_options();
        titlebar.traffic_light_position = Some(gpui::point(px(9.0), px(16.0)));

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            kind: WindowKind::Normal,
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            let mut companion_context = None;
            let main_window = cx.open_window(options, |window, cx| {
                let view = RootView::view(window, cx);
                companion_context = Some(view.read(cx).companion_context());
                let root_view = view.downgrade();
                cx.on_action(move |_: &QuitApplication, cx| {
                    root_view
                        .update(cx, |view, cx| {
                            view.handle_close_request(cx);
                        })
                        .ok();
                });
                cx.new(|cx| Root::new(view, window, cx))
            })?;
            let (project_list, workspace, agents, agent_chats, voice, center) =
                companion_context.expect("root view should provide companion context");
            let attention_count = crate::notifications::companion_attention().len();
            let companion_options = cx.update(|cx| {
                let companion_enabled = workspace.read(cx).companion_enabled;
                ui::companion::window_options(cx, attention_count, companion_enabled)
            })?;
            cx.open_window(companion_options, move |window, cx| {
                ui::companion::view(
                    project_list,
                    workspace,
                    agents,
                    agent_chats,
                    voice,
                    center,
                    main_window,
                    attention_count,
                    window,
                    cx,
                )
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();

        cx.activate(true);
    });
}
