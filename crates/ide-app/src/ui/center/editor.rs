use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window};
use gpui_component::{h_flex, input::InputEvent, input::InputState, input::RopeExt as _};
use ide_core::ProjectId;

/// Limit for opening files in the editor (2 MB) — protects the UI thread.
pub const MAX_EDITOR_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// One open file in the center area.
pub struct EditorItem {
    pub project: ProjectId,
    pub path: PathBuf,
    pub title: SharedString,
    pub input: Entity<InputState>,
    pub cursor_status: Entity<EditorCursorStatus>,
    pub dirty: bool,
    pub saving: bool,
    pub modified_at: Option<SystemTime>,
}

/// Cursor metadata is isolated from the center workspace so rapid navigation
/// repaints only this compact fragment instead of the entire editor shell.
pub struct EditorCursorStatus {
    input: Entity<InputState>,
    language: &'static str,
}

impl EditorCursorStatus {
    pub fn new(input: Entity<InputState>, language: &'static str, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change | InputEvent::SelectionChange) {
                cx.notify();
            }
        })
        .detach();
        Self { input, language }
    }
}

impl Render for EditorCursorStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.input.read(cx);
        let position = state.cursor_position();
        let line_count = state.text().lines_len();
        h_flex()
            .flex_none()
            .items_center()
            .gap_3()
            .font_family(crate::ui::design::FONT_MONO)
            .text_size(crate::ui::design::text_label())
            .text_color(crate::ui::design::t4(cx))
            .child(format!("{line_count} lines"))
            .child(format!(
                "Ln {}, Col {}",
                position.line + 1,
                position.character + 1
            ))
            .child(language_label(self.language))
    }
}

/// Maps a file extension to a tree-sitter language name supported by
/// gpui-component's highlighter. Unknown extensions fall back to "text".
pub fn language_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "py" => "python",
        "go" => "go",
        "css" | "scss" => "css",
        "html" | "htm" | "vue" | "svelte" => "html",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "zsh" | "bash" => "bash",
        "sql" => "sql",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "swift" => "swift",
        "rb" => "ruby",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "cs" => "csharp",
        "diff" | "patch" => "diff",
        "cmake" => "cmake",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        _ => "text",
    }
}

pub fn language_label(language: &str) -> &'static str {
    match language {
        "bash" => "Shell",
        "cpp" => "C++",
        "csharp" => "C#",
        "css" => "CSS",
        "diff" => "Diff",
        "elixir" => "Elixir",
        "go" => "Go",
        "graphql" => "GraphQL",
        "html" => "HTML",
        "java" => "Java",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "markdown" => "Markdown",
        "python" => "Python",
        "ruby" => "Ruby",
        "rust" => "Rust",
        "sql" => "SQL",
        "swift" => "Swift",
        "toml" => "TOML",
        "typescript" => "TypeScript",
        "yaml" => "YAML",
        "zig" => "Zig",
        _ => "Plain Text",
    }
}

pub fn relative_editor_path<'a>(project_root: Option<&'a Path>, path: &'a Path) -> &'a Path {
    project_root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_code_languages_to_human_labels() {
        assert_eq!(language_label("typescript"), "TypeScript");
        assert_eq!(language_label("csharp"), "C#");
        assert_eq!(language_label("text"), "Plain Text");
    }

    #[test]
    fn prefers_project_relative_editor_paths() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/src/main.rs");
        assert_eq!(
            relative_editor_path(Some(root), path),
            Path::new("src/main.rs")
        );
        assert_eq!(relative_editor_path(None, path), path);
    }
}
