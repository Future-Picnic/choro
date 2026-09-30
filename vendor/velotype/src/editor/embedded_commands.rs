// Modified by Choro contributors; see vendor/velotype/CHORO_MODIFICATIONS.md.
//! Small public command surface for embedding Velotype inside another GPUI app.

#![allow(
    dead_code,
    reason = "the embedding API intentionally exposes optional commands"
)]

use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use gpui::*;

use super::{Editor, ViewMode};
use crate::components::{
    BlockEvent, BlockKind, BlockRecord, DismissTransientUi, FocusNext, FocusPrev, IndentBlock,
    InlineFormat, InlineTextTree, Newline, PastedImageSource, TableData, UndoCaptureKind,
};
use crate::config::ImagePasteBehavior;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedBlockStyle {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    BulletedList,
    NumberedList,
    Task,
    Quote,
    CodeBlock,
}

impl EmbeddedBlockStyle {
    fn block_kind(self) -> BlockKind {
        match self {
            Self::Paragraph => BlockKind::Paragraph,
            Self::Heading1 => BlockKind::Heading { level: 1 },
            Self::Heading2 => BlockKind::Heading { level: 2 },
            Self::Heading3 => BlockKind::Heading { level: 3 },
            Self::BulletedList => BlockKind::BulletedListItem,
            Self::NumberedList => BlockKind::NumberedListItem,
            Self::Task => BlockKind::TaskListItem { checked: false },
            Self::Quote => BlockKind::Quote,
            Self::CodeBlock => BlockKind::CodeBlock { language: None },
        }
    }
}

/// Which inline mention trigger is active: `@` (a file) or `@@` (a doc/design).
/// The editor only detects the trigger and query; the host app decides what the
/// candidates are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MentionTrigger {
    File,
    DocOrDesign,
}

/// An in-progress `@…` / `@@…` mention at the caret.
#[derive(Clone, Debug)]
pub struct MentionQuery {
    pub trigger: MentionTrigger,
    /// The text typed after the trigger (may be empty).
    pub query: String,
    /// Visible-text range (within the active block) covering the trigger and
    /// query — what gets replaced on insert.
    pub range: Range<usize>,
}

/// One pickable mention result supplied by the host. The host owns the search
/// (files/docs/designs); the editor owns the caret-anchored overlay, keyboard
/// navigation, and inserting `markdown` when a row is confirmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionCandidate {
    /// Primary line (e.g. a file name or doc/design title).
    pub label: String,
    /// Secondary muted line (e.g. a relative path or source URL).
    pub sublabel: String,
    /// Short tag rendered in the leading badge box (e.g. "File", "Doc").
    pub badge: String,
    /// Markdown inserted on confirm — an inline link or a fenced block.
    pub markdown: String,
    /// When true, `markdown` is inserted as a structural block (a reference
    /// card); otherwise as an inline link.
    pub is_block: bool,
}

/// Find a live mention immediately before the caret: a run of one or two `@`
/// at a word boundary, followed by a whitespace-free query up to the caret.
fn mention_in_text(text: &str, cursor: usize) -> Option<MentionQuery> {
    let cursor = cursor.min(text.len());
    let prefix = &text[..cursor];
    let last_at = prefix.rfind('@')?;
    let mut run_start = last_at;
    while run_start > 0 && prefix.as_bytes()[run_start - 1] == b'@' {
        run_start -= 1;
    }
    let query = &prefix[last_at + 1..];
    if query.contains('@') || query.chars().any(char::is_whitespace) {
        return None;
    }
    if run_start > 0 {
        let before = prefix[..run_start].chars().next_back();
        if before.is_some_and(|ch| !ch.is_whitespace()) {
            return None;
        }
    }
    let trigger = if last_at - run_start + 1 >= 2 {
        MentionTrigger::DocOrDesign
    } else {
        MentionTrigger::File
    };
    Some(MentionQuery {
        trigger,
        query: query.to_string(),
        range: run_start..cursor,
    })
}

/// What a slash-menu entry does when chosen.
#[derive(Clone, Copy)]
pub(crate) enum SlashAction {
    /// Convert the active block to the given style.
    SetStyle(EmbeddedBlockStyle),
    /// Insert a table after the active block.
    InsertTable,
    /// Turn the active block into a horizontal divider.
    InsertDivider,
    /// Pick an image file and insert it as an image block.
    InsertImage,
}

/// A single entry in the embedded slash-command menu.
#[derive(Clone, Copy)]
pub(crate) struct SlashCommand {
    pub label: &'static str,
    pub description: &'static str,
    pub aliases: &'static str,
    pub action: SlashAction,
    /// Visual grouping. A divider is drawn in the menu wherever adjacent visible
    /// items belong to different groups (text/headings vs. lists & blocks).
    pub group: u8,
}

impl SlashCommand {
    /// Two-character badge shown at the left of each menu row.
    pub(crate) fn badge(&self) -> String {
        match self.action {
            SlashAction::InsertTable => "Tb".to_string(),
            SlashAction::InsertDivider => "—".to_string(),
            SlashAction::InsertImage => "Im".to_string(),
            SlashAction::SetStyle(_) => self
                .label
                .split_whitespace()
                .next()
                .unwrap_or(self.label)
                .chars()
                .take(2)
                .collect::<String>(),
        }
    }
}

/// The full slash-command palette, shared between rendering and keyboard
/// navigation so both stay in sync.
pub(crate) const SLASH_COMMANDS: [SlashCommand; 11] = [
    SlashCommand {
        label: "Text",
        description: "Plain paragraph",
        aliases: "text paragraph",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Paragraph),
        group: 0,
    },
    SlashCommand {
        label: "Heading 1",
        description: "Large section heading",
        aliases: "h1 heading title",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Heading1),
        group: 0,
    },
    SlashCommand {
        label: "Heading 2",
        description: "Medium section heading",
        aliases: "h2 heading subtitle",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Heading2),
        group: 0,
    },
    SlashCommand {
        label: "Heading 3",
        description: "Small section heading",
        aliases: "h3 heading",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Heading3),
        group: 0,
    },
    SlashCommand {
        label: "Bullet List",
        description: "Unordered list item",
        aliases: "bullet list unordered",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::BulletedList),
        group: 1,
    },
    SlashCommand {
        label: "Numbered List",
        description: "Ordered list item",
        aliases: "number ordered list",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::NumberedList),
        group: 1,
    },
    SlashCommand {
        label: "Task",
        description: "Checklist item",
        aliases: "task todo checkbox checklist",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Task),
        group: 1,
    },
    SlashCommand {
        label: "Quote",
        description: "Quoted block",
        aliases: "quote blockquote",
        action: SlashAction::SetStyle(EmbeddedBlockStyle::Quote),
        group: 1,
    },
    SlashCommand {
        label: "Divider",
        description: "Horizontal rule",
        aliases: "divider separator rule horizontal line hr",
        action: SlashAction::InsertDivider,
        group: 1,
    },
    SlashCommand {
        label: "Image",
        description: "Insert an image file",
        aliases: "image picture photo file png jpg",
        action: SlashAction::InsertImage,
        group: 1,
    },
    SlashCommand {
        label: "Table",
        description: "3 x 3 table",
        aliases: "table grid",
        action: SlashAction::InsertTable,
        group: 1,
    },
];

/// Maximum number of matches shown in the slash menu at once.
pub(crate) const SLASH_MENU_MAX_VISIBLE: usize = 6;

impl Editor {
    pub fn embedded_set_chrome_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.embedded_show_chrome == visible {
            return;
        }
        self.embedded_show_chrome = visible;
        cx.notify();
    }

    /// Embedded editors must not steal the window focus on their first
    /// render; the host decides what starts focused (e.g. a dialog's title
    /// field). Clears the initial pending-focus set at construction.
    pub fn embedded_skip_initial_focus(&mut self) {
        self.pending_focus = None;
    }

    /// Show or hide the Rendered/Source view-mode pill. Dialog-sized embedded
    /// editors hide it; the full docs surface keeps it.
    pub fn embedded_set_view_mode_toggle_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.embedded_view_mode_toggle == visible {
            return;
        }
        self.embedded_view_mode_toggle = visible;
        cx.notify();
    }

    /// Controls whether the editor owns native window state such as the title,
    /// edited marker, and close guard. Disable this when embedding the editor in
    /// a larger host application.
    pub fn embedded_set_host_window_integration(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.embedded_host_window_integration == enabled {
            return;
        }
        self.embedded_host_window_integration = enabled;
        if !enabled {
            self.pending_window_edited = false;
            self.pending_window_title_refresh = false;
        }
        cx.notify();
    }

    /// Preserve the editor's current scroll position when it is embedded inside
    /// another scrolling surface. Caret visibility still adjusts the viewport
    /// when the caret actually leaves it.
    pub fn embedded_set_nested_scroll(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.embedded_nested_scroll == enabled {
            return;
        }
        self.embedded_nested_scroll = enabled;
        cx.notify();
    }

    pub fn embedded_markdown(&self, cx: &App) -> String {
        self.serialized_document_text(cx)
    }

    pub fn embedded_is_dirty(&self) -> bool {
        self.document_dirty
    }

    /// Marks host-managed content clean without changing its synthetic file
    /// path. This is useful when an embedded host persists to a database.
    pub fn embedded_mark_clean(&mut self, cx: &mut Context<Self>) {
        if !self.document_dirty {
            return;
        }
        self.document_dirty = false;
        self.pending_window_edited = false;
        self.pending_window_title_refresh = false;
        cx.notify();
    }

    pub fn embedded_mark_saved(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.apply_successful_save(path, cx);
    }

    pub fn embedded_replace_markdown(
        &mut self,
        markdown: String,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.replace_document_from_markdown(markdown, Some(path), cx);
        self.mark_dirty(cx);
    }

    pub fn embedded_reload_markdown(
        &mut self,
        markdown: String,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.replace_document_from_markdown(markdown, Some(path), cx);
    }

    pub fn embedded_slash_query(&self, cx: &App) -> Option<String> {
        let block = self.current_edit_target_from_state(cx)?;
        let text = block.read(cx).display_text().to_string();
        text.strip_prefix('/')
            .filter(|query| !query.contains('\n'))
            .map(|query| query.trim().to_ascii_lowercase())
    }

    /// An active inline mention being typed at the caret, if any: `@query`
    /// (file) or `@@query` (doc/design). The host app reads this to render its
    /// own picker and supply candidates, then calls
    /// [`Editor::embedded_insert_mention_link`] with the chosen link.
    pub fn embedded_mention_query(&self, cx: &App) -> Option<MentionQuery> {
        let block = self.current_edit_target_from_state(cx)?;
        let block = block.read(cx);
        let text = block.display_text();
        let cursor = block.cursor_offset().min(text.len());
        mention_in_text(text, cursor)
    }

    /// Replace the active `@…`/`@@…` mention with parsed Markdown (e.g. a link).
    /// Runs through the editor's paste pipeline so the rest of the block's
    /// inline formatting is preserved — never a lossy display-text rebuild.
    pub fn embedded_insert_mention_link(
        &mut self,
        range: Range<usize>,
        link_markdown: String,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        let (leading, trailing) = block.read(cx).mention_split(range);
        // Dispatch straight into the editor's block-event handler (the same path
        // a real paste takes), rather than relying on a deferred subscription —
        // the insert must land even when the picker click moved focus.
        self.on_block_event(
            block,
            &BlockEvent::RequestPasteMultiline {
                leading,
                lines: vec![link_markdown],
                trailing,
                split_physical_lines: true,
            },
            cx,
        );
        self.mark_dirty(cx);
        cx.notify();
    }

    /// Replace the active `@…`/`@@…` mention with a block-level embed (e.g. a
    /// reference card). The fenced Markdown is handed to the importer as
    /// structural content (`split_physical_lines: false`) so it parses into a
    /// native block instead of being folded into the current paragraph.
    pub fn embedded_insert_reference_block(
        &mut self,
        range: Range<usize>,
        fence_markdown: String,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        let (leading, trailing) = block.read(cx).mention_split(range);
        let lines = fence_markdown
            .split('\n')
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        self.on_block_event(
            block,
            &BlockEvent::RequestPasteMultiline {
                leading,
                lines,
                trailing,
                split_physical_lines: false,
            },
            cx,
        );
        self.mark_dirty(cx);
        cx.notify();
    }

    /// Replace the host-supplied mention candidates for the current `@`/`@@`
    /// query. The host calls this every render with the results of its own
    /// search; the editor renders the caret-anchored overlay and navigates it.
    /// Equality-guarded so a stable list doesn't churn renders.
    pub fn embedded_set_mention_candidates(
        &mut self,
        candidates: Vec<MentionCandidate>,
        cx: &mut Context<Self>,
    ) {
        if self.mention_candidates != candidates {
            self.mention_candidates = candidates;
            if self.mention_selection >= self.mention_candidates.len() {
                self.mention_selection = 0;
            }
            cx.notify();
        }
    }

    /// The active mention query when its overlay should be visible: a live
    /// `@`/`@@` query that has candidates and wasn't dismissed with Escape.
    pub(crate) fn active_mention_query(&self, cx: &App) -> Option<MentionQuery> {
        if self.mention_candidates.is_empty() {
            return None;
        }
        let query = self.embedded_mention_query(cx)?;
        if self.mention_dismissed_query.as_deref() == Some(query.query.as_str()) {
            return None;
        }
        Some(query)
    }

    pub(crate) fn move_mention_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.mention_candidates.len();
        if count == 0 {
            return;
        }
        let current = self.mention_selection.min(count - 1) as isize;
        self.mention_selection = current.wrapping_add(delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    /// Insert the highlighted mention candidate, replacing the `@…` query.
    /// Returns true when a candidate was inserted (so the key is consumed).
    pub(crate) fn confirm_mention_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(query) = self.active_mention_query(cx) else {
            return false;
        };
        cx.stop_propagation();
        let index = self
            .mention_selection
            .min(self.mention_candidates.len() - 1);
        let candidate = self.mention_candidates[index].clone();
        if candidate.is_block {
            self.embedded_insert_reference_block(query.range, candidate.markdown, cx);
        } else {
            self.embedded_insert_mention_link(query.range, candidate.markdown, cx);
        }
        self.mention_candidates.clear();
        self.mention_selection = 0;
        self.mention_last_query = None;
        self.mention_dismissed_query = None;
        true
    }

    pub(crate) fn confirm_mention_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.mention_candidates.len() {
            self.mention_selection = index;
            self.confirm_mention_selection(cx);
        }
    }

    /// Hide the overlay for the current query (Escape). It reappears once the
    /// query text changes.
    pub(crate) fn dismiss_mention_overlay(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(query) = self.active_mention_query(cx) else {
            return false;
        };
        self.mention_dismissed_query = Some(query.query);
        cx.notify();
        true
    }

    fn clear_active_slash_query(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        block.update(cx, |block, cx| {
            if block.display_text().starts_with('/') {
                block.record.set_title(InlineTextTree::plain(String::new()));
                block.sync_render_cache();
                block.move_to(0, cx);
                cx.notify();
            }
        });
    }

    pub fn embedded_apply_slash_block_style(
        &mut self,
        style: EmbeddedBlockStyle,
        cx: &mut Context<Self>,
    ) {
        self.clear_active_slash_query(cx);
        self.embedded_set_active_block_style(style, cx);
    }

    pub fn embedded_apply_slash_table(&mut self, cx: &mut Context<Self>) {
        self.clear_active_slash_query(cx);
        self.embedded_insert_table_after_active(3, 3, cx);
    }

    pub fn embedded_apply_slash_divider(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        // Mirror the `---` shortcut: convert the active "/" block into a divider
        // (this also clears the typed query) and request a fresh paragraph below
        // so the caret lands in editable text instead of the divider itself.
        block.update(cx, |block, cx| {
            block.convert_to_separator(cx);
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: true,
            });
        });
        self.mark_dirty(cx);
        cx.notify();
    }

    /// When enabled, images inserted or pasted in this editor are copied into
    /// an `assets` folder next to the document and referenced with a relative
    /// path, so the document stays self-contained and portable.
    pub fn set_copy_images_into_assets(&mut self, enabled: bool) {
        self.image_paste_behavior = enabled.then_some(ImagePasteBehavior::CopyToAssetsFolder);
    }

    pub fn embedded_apply_slash_image(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        // Keep only a weak handle across the (potentially long-lived) file
        // picker: if the target block is removed while the dialog is open, the
        // insert below becomes a clean no-op instead of resurrecting a stale
        // block or panicking.
        let block = block.downgrade();
        // Drop the "/" query so the menu closes; the picked image lands in this
        // now-empty block if it still exists when the picker returns.
        self.clear_active_slash_query(cx);

        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an image".into()),
        });
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // Reuse the clipboard-paste pipeline: emitting RequestPasteImage runs
            // the editor's image insertion, which honors the configured paste
            // behavior (copying into `assets` with a relative path) and renders
            // the result inline.
            let _ = block.update(cx, |_block, block_cx| {
                block_cx.emit(BlockEvent::RequestPasteImage {
                    leading: InlineTextTree::plain(String::new()),
                    source: PastedImageSource::LocalPath(path),
                    trailing: InlineTextTree::plain(String::new()),
                });
            });
        })
        .detach();
    }

    /// Filter the slash palette by the active query. Returns an empty list when
    /// the menu is not open, which the keyboard handlers use to decide whether
    /// to intercept navigation keys.
    pub(crate) fn embedded_slash_matches(&self, cx: &App) -> Vec<SlashCommand> {
        let Some(query) = self.embedded_slash_query(cx) else {
            return Vec::new();
        };
        SLASH_COMMANDS
            .into_iter()
            .filter(|command| {
                query.is_empty()
                    || command.label.to_ascii_lowercase().contains(&query)
                    || command.description.to_ascii_lowercase().contains(&query)
                    || command.aliases.contains(query.as_str())
            })
            .take(SLASH_MENU_MAX_VISIBLE)
            .collect()
    }

    pub(crate) fn embedded_apply_slash_command(
        &mut self,
        command: SlashCommand,
        cx: &mut Context<Self>,
    ) {
        self.embedded_slash_selection = 0;
        self.embedded_slash_last_query = None;
        match command.action {
            SlashAction::SetStyle(style) => self.embedded_apply_slash_block_style(style, cx),
            SlashAction::InsertTable => self.embedded_apply_slash_table(cx),
            SlashAction::InsertDivider => self.embedded_apply_slash_divider(cx),
            SlashAction::InsertImage => self.embedded_apply_slash_image(cx),
        }
    }

    /// Move the slash-menu highlight up (Arrow Up / `FocusPrev`). When the menu
    /// is closed the event propagates so normal block-focus navigation runs.
    pub(crate) fn on_slash_menu_prev(
        &mut self,
        _: &FocusPrev,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.embedded_slash_matches(cx).len();
        if count == 0 {
            return;
        }
        cx.stop_propagation();
        let current = self.embedded_slash_selection.min(count - 1);
        self.embedded_slash_selection = (current + count - 1) % count;
        cx.notify();
    }

    /// Move the slash-menu highlight down (Arrow Down / `FocusNext`).
    pub(crate) fn on_slash_menu_next(
        &mut self,
        _: &FocusNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.embedded_slash_matches(cx).len();
        if count == 0 {
            return;
        }
        cx.stop_propagation();
        let current = self.embedded_slash_selection.min(count - 1);
        self.embedded_slash_selection = (current + 1) % count;
        cx.notify();
    }

    /// Apply the highlighted command on Enter.
    pub(crate) fn on_slash_menu_confirm_newline(
        &mut self,
        _: &Newline,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_slash_menu_selection(cx);
    }

    /// Apply the highlighted command on Tab.
    pub(crate) fn on_slash_menu_confirm_tab(
        &mut self,
        _: &IndentBlock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_slash_menu_selection(cx);
    }

    fn confirm_slash_menu_selection(&mut self, cx: &mut Context<Self>) {
        let matches = self.embedded_slash_matches(cx);
        if matches.is_empty() {
            return;
        }
        cx.stop_propagation();
        let index = self.embedded_slash_selection.min(matches.len() - 1);
        self.embedded_apply_slash_command(matches[index], cx);
    }

    /// Close the menu on Escape by clearing the active `/` query.
    pub(crate) fn on_slash_menu_dismiss(
        &mut self,
        _: &DismissTransientUi,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.embedded_slash_matches(cx).is_empty() {
            return;
        }
        cx.stop_propagation();
        self.clear_active_slash_query(cx);
        self.embedded_slash_selection = 0;
        self.embedded_slash_last_query = None;
        cx.notify();
    }

    fn embedded_toggle_inline_format(&mut self, format: InlineFormat, cx: &mut Context<Self>) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        block.update(cx, |block, cx| block.toggle_inline_format(format, cx));
        self.mark_dirty(cx);
        cx.notify();
    }

    pub fn embedded_toggle_bold(&mut self, cx: &mut Context<Self>) {
        self.embedded_toggle_inline_format(InlineFormat::Bold, cx);
    }

    pub fn embedded_toggle_italic(&mut self, cx: &mut Context<Self>) {
        self.embedded_toggle_inline_format(InlineFormat::Italic, cx);
    }

    pub fn embedded_toggle_underline(&mut self, cx: &mut Context<Self>) {
        self.embedded_toggle_inline_format(InlineFormat::Underline, cx);
    }

    pub fn embedded_toggle_code(&mut self, cx: &mut Context<Self>) {
        self.embedded_toggle_inline_format(InlineFormat::Code, cx);
    }

    pub fn embedded_toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        self.toggle_view_mode(cx);
    }

    pub fn embedded_set_active_block_style(
        &mut self,
        style: EmbeddedBlockStyle,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        let next_kind = style.block_kind();

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        block.update(cx, |block, cx| {
            if block.record.kind == next_kind {
                return;
            }
            block.record.kind = next_kind;
            block.record.raw_fallback = None;
            block.sync_edit_mode_from_kind();
            block.sync_render_cache();
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    pub fn embedded_insert_table_after_active(
        &mut self,
        body_rows: usize,
        columns: usize,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            self.view_mode = ViewMode::Rendered;
        }

        let target = self
            .current_edit_target_entity_id_from_state(cx)
            .map(|entity_id| self.root_ancestor_entity_id(entity_id));
        let table = TableData::new_empty(body_rows, columns);
        let new_block = Self::new_table_block(cx, table);

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        if let Some(entity_id) = target {
            if let Some(location) = self.document.find_block_location(entity_id) {
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    vec![new_block.clone()],
                    cx,
                );
            } else {
                self.document.insert_blocks_at(
                    None,
                    self.document.root_count(),
                    vec![new_block.clone()],
                    cx,
                );
            }
        } else {
            self.document.insert_blocks_at(
                None,
                self.document.root_count(),
                vec![new_block.clone()],
                cx,
            );
        }

        if let Some(location) = self.document.find_block_location(new_block.entity_id()) {
            let sibling_count = match location.parent.as_ref() {
                Some(parent) => parent.read(cx).children.len(),
                None => self.document.root_count(),
            };
            if location.index + 1 >= sibling_count {
                let trailing = Self::new_block(cx, BlockRecord::paragraph(String::new()));
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    vec![trailing],
                    cx,
                );
            }
        }

        self.rebuild_table_runtimes(cx);
        if let Some(first_cell) = new_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.first())
        {
            self.focus_block(first_cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }
}
