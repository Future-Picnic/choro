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
    binding: fn(&str) -> KeyBinding,
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
            binding: |keys| KeyBinding::new(keys, <$action>::default(), None),
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
            Some("cmd-f"),
            ToggleFocusMode,
            "Focus mode",
            "Hide both sidebars and focus on the current work",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_left",
            Some("ctrl-cmd-s"),
            ToggleLeftPanel,
            "Toggle project sidebar",
            "Show or hide project navigation",
            ShortcutCategory::NavigationLayout,
            true
        ),
        shortcut!(
            "toggle_right",
            Some("ctrl-cmd-t"),
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
        .filter_map(|s| {
            let keys = s.keystroke(overrides)?.to_string();
            Some((s.binding)(&keys))
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
        .filter_map(|shortcut| {
            shortcut
                .keystroke(previous)
                .map(|keys| KeyBinding::new(keys, NoAction {}, None))
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
    fn focus_mode_owns_command_f() {
        let focus_mode = shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == "toggle_focus_mode")
            .unwrap();

        assert_eq!(focus_mode.default_keystroke, Some("cmd-f"));
        assert_eq!(focus_mode.category, ShortcutCategory::NavigationLayout);
        assert!(focus_mode.in_commands);
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
