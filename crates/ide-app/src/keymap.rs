use std::collections::HashMap;

use gpui::{Action, App, KeyBinding, Keystroke, NoAction};

use crate::actions::*;

/// One row of the keymap: stable id, default keystroke, action, description.
/// Single source of truth for `bind_keys` and the Settings dialog.
pub struct Shortcut {
    pub id: &'static str,
    pub default_keystroke: Option<&'static str>,
    pub title: &'static str,
    pub description: &'static str,
    pub category: ShortcutCategory,
    pub in_commands: bool,
    context: Option<&'static str>,
    additional_contexts: &'static [&'static str],
    binding: fn(&str, Option<&str>) -> KeyBinding,
    action: fn() -> Box<dyn Action>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutCategory {
    FindCreate,
    Activities,
    NavigationLayout,
    SearchCode,
    Agents,
    App,
}

impl ShortcutCategory {
    pub const ALL: [Self; 6] = [
        Self::FindCreate,
        Self::Activities,
        Self::NavigationLayout,
        Self::SearchCode,
        Self::Agents,
        Self::App,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::FindCreate => "Find & create",
            Self::Activities => "Activities",
            Self::NavigationLayout => "Navigation & layout",
            Self::SearchCode => "Search & code",
            Self::Agents => "Agents",
            Self::App => "App",
        }
    }
}

impl Shortcut {
    /// The effective keystroke after user overrides.
    pub fn keystroke<'a>(&'a self, overrides: &'a HashMap<String, String>) -> Option<&'a str> {
        match overrides.get(self.id) {
            Some(value) if value.is_empty() => None,
            Some(value) if Keystroke::parse(value).is_ok() => Some(value.as_str()),
            _ => self.default_keystroke,
        }
    }

    pub fn is_modified(&self, overrides: &HashMap<String, String>) -> bool {
        overrides.contains_key(self.id)
    }

    pub fn action(&self) -> Box<dyn Action> {
        (self.action)()
    }
}

macro_rules! shortcut {
    ($id:literal, $keys:expr, $action:ty, $title:literal, $desc:literal, $category:expr, $commands:expr) => {
        Shortcut {
            id: $id,
            default_keystroke: $keys,
            title: $title,
            description: $desc,
            category: $category,
            in_commands: $commands,
            context: None,
            additional_contexts: &[],
            binding: |keys, context| KeyBinding::new(keys, <$action>::default(), context),
            action: || Box::new(<$action>::default()),
        }
    };
}

macro_rules! shortcut_in {
    ($id:literal, $keys:expr, $action:ty, $title:literal, $desc:literal, $category:expr, $commands:expr, $context:literal) => {
        Shortcut {
            id: $id,
            default_keystroke: $keys,
            title: $title,
            description: $desc,
            category: $category,
            in_commands: $commands,
            context: Some($context),
            additional_contexts: &[],
            binding: |keys, context| KeyBinding::new(keys, <$action>::default(), context),
            action: || Box::new(<$action>::default()),
        }
    };
}

macro_rules! shortcut_in_additional {
    ($id:literal, $keys:expr, $action:ty, $title:literal, $desc:literal, $category:expr, $commands:expr, $context:literal, [$($additional_context:literal),+ $(,)?]) => {
        Shortcut {
            id: $id,
            default_keystroke: $keys,
            title: $title,
            description: $desc,
            category: $category,
            in_commands: $commands,
            context: Some($context),
            additional_contexts: &[$($additional_context),+],
            binding: |keys, context| KeyBinding::new(keys, <$action>::default(), context),
            action: || Box::new(<$action>::default()),
        }
    };
}

pub fn shortcuts() -> Vec<Shortcut> {
    vec![
        shortcut!(
            "project_search",
            Some("cmd-p"),
            OpenProjectSearch,
            "Quick Open",
            "Find files, chats, tasks, docs, and projects",
            ShortcutCategory::FindCreate,
            false
        ),
        shortcut!(
            "commands",
            Some("cmd-k"),
            OpenCommands,
            "Commands",
            "Find and run an action",
            ShortcutCategory::FindCreate,
            false
        ),
        shortcut!(
            "new_agent_chat",
            Some("cmd-n"),
            NewAgentChat,
            "New agent",
            "Start a new agent conversation",
            ShortcutCategory::FindCreate,
            true
        ),
        shortcut!(
            "quick_add_task",
            Some("cmd-shift-n"),
            QuickAddTask,
            "Quick add task",
            "Create a personal task",
            ShortcutCategory::FindCreate,
            true
        ),
        shortcut!(
            "new_terminal",
            Some("cmd-t"),
            NewTerminal,
            "New terminal",
            "Open a terminal in the current project",
            ShortcutCategory::FindCreate,
            true
        ),
        shortcut!(
            "open_folder",
            Some("cmd-o"),
            OpenFolder,
            "Open project",
            "Open a project folder",
            ShortcutCategory::FindCreate,
            true
        ),
        shortcut!(
            "view_code",
            Some("cmd-1"),
            ViewCode,
            "Code",
            "Open the Code activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_agents",
            Some("cmd-2"),
            ViewAgents,
            "Agents",
            "Open the Agents activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_tasks",
            Some("cmd-3"),
            ViewTasks,
            "Tasks",
            "Open the Tasks activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_docs",
            Some("cmd-4"),
            ViewDocs,
            "Docs",
            "Open the Docs activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_designs",
            Some("cmd-5"),
            ViewDesigns,
            "Designs",
            "Open the Designs activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_design",
            None,
            ViewDesign,
            "Design",
            "Open the Design activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_db",
            Some("cmd-6"),
            ViewDb,
            "Databases",
            "Open the Databases activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "view_services",
            Some("cmd-7"),
            ViewServices,
            "Services",
            "Open the Services activity",
            ShortcutCategory::Activities,
            true
        ),
        shortcut!(
            "navigate_back",
            Some("cmd-["),
            NavigateBack,
            "Go back",
            "Go to the previous Choro view",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "navigate_forward",
            Some("cmd-]"),
            NavigateForward,
            "Go forward",
            "Go to the next Choro view",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_focus_mode",
            Some("cmd-i"),
            ToggleFocusMode,
            "Focus mode",
            "Hide both sidebars and focus on the current work",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_left",
            Some("cmd-b"),
            ToggleLeftPanel,
            "Toggle project sidebar",
            "Show or hide project navigation",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_right",
            Some("cmd-shift-b"),
            ToggleRightPanel,
            "Toggle tools panel",
            "Show or hide Git, files, and agent tools",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_preview",
            Some("ctrl-cmd-p"),
            TogglePreview,
            "Toggle preview",
            "Show or hide the project preview",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "next_open_item",
            Some("ctrl-tab"),
            NextOpenItem,
            "Next open item",
            "Move to the next file, diff, or terminal",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "previous_open_item",
            Some("ctrl-shift-tab"),
            PreviousOpenItem,
            "Previous open item",
            "Move to the previous file, diff, or terminal",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "content_search",
            Some("cmd-shift-f"),
            OpenContentSearch,
            "Find in project",
            "Search the contents of project files",
            ShortcutCategory::SearchCode,
            true
        ),
        shortcut_in_additional!(
            "agent_chat_search",
            Some("cmd-f"),
            OpenAgentChatSearch,
            "Find in conversation",
            "Search messages in the selected agent conversation",
            ShortcutCategory::Agents,
            true,
            "AgentChat",
            ["AgentChat > Input", "AgentChatWorkspace"]
        ),
        shortcut!(
            "save_file",
            Some("cmd-s"),
            SaveFile,
            "Save",
            "Save the current file",
            ShortcutCategory::SearchCode,
            true
        ),
        shortcut!(
            "close_tab",
            Some("cmd-w"),
            CloseTab,
            "Close current item",
            "Close the current file, diff, or terminal",
            ShortcutCategory::SearchCode,
            true
        ),
        shortcut!(
            "toggle_terminal_area",
            Some("cmd-j"),
            ToggleTerminalArea,
            "Toggle terminal area",
            "Show or hide the terminal in Code",
            ShortcutCategory::SearchCode,
            true
        ),
        shortcut!(
            "stop_current_agent",
            Some("cmd-."),
            StopCurrentAgent,
            "Stop current agent",
            "Stop the selected running agent",
            ShortcutCategory::Agents,
            true
        ),
        shortcut_in!(
            "quick_ask",
            Some("cmd-alt-a"),
            OpenQuickAsk,
            "Quick Ask",
            "Ask a quick question without starting an agent",
            ShortcutCategory::Agents,
            true,
            "Root"
        ),
        shortcut_in!(
            "toggle_voice_director",
            Some("cmd-shift-a"),
            ToggleVoiceDirector,
            "Assistant",
            "Start or end a read-only conversation about the active project",
            ShortcutCategory::Agents,
            true,
            "Root"
        ),
        shortcut_in!(
            "toggle_hands_free_dictation",
            Some("cmd-shift-l"),
            ToggleHandsFreeDictation,
            "Hands-off dictation",
            "Start or stop continuous dictation in the open agent chat",
            ShortcutCategory::Agents,
            true,
            "Root"
        ),
        shortcut_in!(
            "toggle_voice_dictation",
            Some("cmd-l"),
            ToggleVoiceDictation,
            "Dictate with voice",
            "Hold to dictate once; release to insert into the open agent chat",
            ShortcutCategory::Agents,
            false,
            "Root"
        ),
        shortcut!(
            "toggle_agent_plan_mode",
            Some("shift-tab"),
            ToggleAgentPlanMode,
            "Toggle plan mode",
            "Switch the selected agent between default and plan mode",
            ShortcutCategory::Agents,
            true
        ),
        shortcut!(
            "open_settings",
            Some("cmd-,"),
            OpenSettings,
            "Settings",
            "Open Choro settings",
            ShortcutCategory::App,
            true
        ),
        shortcut!(
            "quit",
            Some("cmd-q"),
            QuitApplication,
            "Quit Choro",
            "Quit after safely stopping active work",
            ShortcutCategory::App,
            false
        ),
    ]
}

/// Builds key bindings, applying user overrides (invalid ones fall back
/// to the default).
pub fn bindings(overrides: &HashMap<String, String>) -> Vec<KeyBinding> {
    shortcuts()
        .into_iter()
        .flat_map(|shortcut| {
            let Some(keys) = shortcut.keystroke(overrides).map(str::to_string) else {
                return Vec::new();
            };
            std::iter::once(shortcut.context)
                .chain(shortcut.additional_contexts.iter().copied().map(Some))
                .map(|context| (shortcut.binding)(&keys, context))
                .collect()
        })
        .collect()
}

/// GPUI keymaps are append-only. Mask the prior Choro bindings, then append the
/// new effective bindings so changes made in Settings take effect immediately.
pub fn apply_bindings(
    cx: &mut App,
    previous: &HashMap<String, String>,
    next: &HashMap<String, String>,
) {
    let masks: Vec<KeyBinding> = shortcuts()
        .into_iter()
        .flat_map(|shortcut| {
            let Some(keys) = shortcut.keystroke(previous) else {
                return Vec::new();
            };
            std::iter::once(shortcut.context)
                .chain(shortcut.additional_contexts.iter().copied().map(Some))
                .map(|context| KeyBinding::new(keys, NoAction {}, context))
                .collect()
        })
        .collect();
    cx.bind_keys(masks);
    cx.bind_keys(bindings(next));
}

pub fn display_keystroke(keys: &str) -> String {
    let Ok(stroke) = Keystroke::parse(keys) else {
        return keys.to_string();
    };
    let mut display = String::new();
    if stroke.modifiers.control {
        display.push('⌃');
    }
    if stroke.modifiers.alt {
        display.push('⌥');
    }
    if stroke.modifiers.shift {
        display.push('⇧');
    }
    if stroke.modifiers.platform {
        display.push('⌘');
    }
    if stroke.modifiers.function {
        display.push_str("fn");
    }
    display.push_str(match stroke.key.as_str() {
        "enter" | "return" => "↩",
        "tab" => "⇥",
        "escape" => "Esc",
        "backspace" => "⌫",
        "delete" => "⌦",
        "space" => "Space",
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        other => return format!("{}{}", display, other.to_uppercase()),
    });
    display
}

pub fn shortcut_display(id: &str, overrides: &HashMap<String, String>) -> Option<String> {
    shortcuts()
        .into_iter()
        .find(|shortcut| shortcut.id == id)
        .and_then(|shortcut| shortcut.keystroke(overrides).map(display_keystroke))
}

pub fn normalized_keystroke(keys: &str) -> Option<String> {
    Keystroke::parse(keys).ok().map(|stroke| stroke.unparse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn defaults_parse_and_do_not_conflict() {
        let mut used = HashSet::new();
        for shortcut in shortcuts() {
            let Some(keys) = shortcut.default_keystroke else {
                continue;
            };
            let normalized = normalized_keystroke(keys)
                .unwrap_or_else(|| panic!("invalid default shortcut: {}", shortcut.id));
            assert!(
                used.insert(normalized.clone()),
                "duplicate default shortcut: {normalized}"
            );
        }
    }

    #[test]
    fn empty_override_unassigns_a_shortcut() {
        let overrides = [("save_file".to_string(), String::new())]
            .into_iter()
            .collect();
        let save = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "save_file")
            .unwrap();
        assert_eq!(save.keystroke(&overrides), None);
    }

    #[test]
    fn agent_chat_find_resolves_after_palette_close_and_inside_chat_inputs() {
        let mut keymap = gpui::Keymap::new(vec![KeyBinding::new(
            "cmd-f",
            gpui_component::input::Search,
            Some("Input"),
        )]);
        keymap.add_bindings(bindings(&HashMap::new()));
        let keys = [Keystroke::parse("cmd-f").unwrap()];
        let workspace_context = crate::ui::center::agent_chat_search::WORKSPACE_CONTEXT;
        for path in [
            vec!["Root", workspace_context],
            vec!["Root", workspace_context, "AgentChat", "TextView"],
            vec!["Root", workspace_context, "AgentChat", "Input"],
            vec![
                "Root",
                workspace_context,
                "AgentChat",
                "AgentChatSearch",
                "Input",
            ],
        ] {
            let contexts = path
                .iter()
                .map(|context| gpui::KeyContext::parse(context).unwrap())
                .collect::<Vec<_>>();
            let (resolved, _) = keymap.bindings_for_input(&keys, &contexts);
            assert!(
                resolved
                    .first()
                    .is_some_and(|binding| binding.action().as_any().is::<OpenAgentChatSearch>()),
                "Find must resolve for focus path {path:?}: {resolved:?}"
            );
        }

        // The workspace fallback must not take over Find in other activities
        // or in a palette/sidebar input outside the conversation.
        for path in [
            vec!["Root"],
            vec!["Root", "Input"],
            vec!["Root", workspace_context, "Input"],
        ] {
            let contexts = path
                .iter()
                .map(|context| gpui::KeyContext::parse(context).unwrap())
                .collect::<Vec<_>>();
            let (resolved, _) = keymap.bindings_for_input(&keys, &contexts);
            assert!(!resolved
                .first()
                .is_some_and(|binding| binding.action().as_any().is::<OpenAgentChatSearch>()));
        }
    }

    #[test]
    fn layout_shortcuts_use_command_b_family_and_command_i() {
        let focus_mode = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_focus_mode")
            .unwrap();
        let left_sidebar = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_left")
            .unwrap();
        let right_sidebar = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_right")
            .unwrap();

        assert_eq!(focus_mode.default_keystroke, Some("cmd-i"));
        assert_eq!(left_sidebar.default_keystroke, Some("cmd-b"));
        assert_eq!(right_sidebar.default_keystroke, Some("cmd-shift-b"));
        assert_eq!(focus_mode.category, ShortcutCategory::NavigationLayout);
        assert!(focus_mode.in_commands);
    }

    #[test]
    fn push_to_talk_uses_command_l_everywhere_in_the_workspace() {
        let voice = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_voice_dictation")
            .unwrap();
        assert_eq!(voice.default_keystroke, Some("cmd-l"));
        assert_eq!(voice.context, Some("Root"));
        assert!(!voice.in_commands);
    }

    #[test]
    fn hands_off_uses_command_shift_l_everywhere_in_the_workspace() {
        let hands_off = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_hands_free_dictation")
            .unwrap();
        assert_eq!(hands_off.default_keystroke, Some("cmd-shift-l"));
        assert_eq!(hands_off.context, Some("Root"));
        assert!(hands_off.in_commands);
    }

    #[test]
    fn assistant_uses_command_shift_a_everywhere_in_the_workspace() {
        let assistant = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_voice_director")
            .unwrap();
        assert_eq!(assistant.default_keystroke, Some("cmd-shift-a"));
        assert_eq!(assistant.context, Some("Root"));
        assert!(assistant.in_commands);
    }

    #[test]
    fn quick_ask_uses_command_option_a_without_replacing_project_talk() {
        let quick_ask = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "quick_ask")
            .unwrap();
        let project_talk = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_voice_director")
            .unwrap();

        assert_eq!(quick_ask.default_keystroke, Some("cmd-alt-a"));
        assert_eq!(quick_ask.context, Some("Root"));
        assert!(quick_ask.in_commands);
        assert_eq!(project_talk.default_keystroke, Some("cmd-shift-a"));
    }

    #[test]
    fn every_voice_mode_is_independently_remappable() {
        let overrides = [
            ("toggle_voice_dictation".to_string(), "alt-d".to_string()),
            (
                "toggle_hands_free_dictation".to_string(),
                "alt-h".to_string(),
            ),
            ("toggle_voice_director".to_string(), "alt-a".to_string()),
        ]
        .into_iter()
        .collect();

        for (id, expected) in [
            ("toggle_voice_dictation", "alt-d"),
            ("toggle_hands_free_dictation", "alt-h"),
            ("toggle_voice_director", "alt-a"),
        ] {
            let shortcut = shortcuts()
                .into_iter()
                .find(|shortcut| shortcut.id == id)
                .unwrap();
            assert_eq!(shortcut.keystroke(&overrides), Some(expected));
        }
    }

    #[test]
    fn plan_mode_defaults_to_shift_tab() {
        let plan_mode = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_agent_plan_mode")
            .unwrap();

        assert_eq!(plan_mode.default_keystroke, Some("shift-tab"));
        assert_eq!(plan_mode.category, ShortcutCategory::Agents);
        assert!(plan_mode.in_commands);
    }
}
