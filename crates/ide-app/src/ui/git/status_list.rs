use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use gpui::{
    div, prelude::FluentBuilder, px, AnyElement, App, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled,
};
use gpui_component::{h_flex, tooltip::Tooltip, Icon, IconName};
use ide_core::config::GitStatusViewMode;
use ide_core::git::{ChangeKind, LineStats, StatusEntry};

use crate::state::GitState;
use crate::ui::center::CenterArea;
use crate::ui::confirm::ConfirmDialog;
use crate::ui::git::diff_pane::DiffKind;
use crate::ui::git::git_panel::GitPanel;
use crate::ui::style;
use ide_core::ProjectId;

fn kind_color(kind: ChangeKind, cx: &App) -> gpui::Hsla {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => crate::ui::design::sage(cx),
        ChangeKind::Deleted | ChangeKind::Conflicted => crate::ui::design::rose(cx),
        _ => crate::ui::design::amber(cx),
    }
}

fn kind_soft_color(kind: ChangeKind, cx: &App) -> gpui::Hsla {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => crate::ui::design::sage_soft(cx),
        ChangeKind::Deleted | ChangeKind::Conflicted => crate::ui::design::rose_soft(cx),
        _ => crate::ui::design::amber_soft(cx),
    }
}

#[derive(Clone)]
pub struct GitStatusListEntry {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub staged: bool,
    pub stats: LineStats,
}

pub fn status_list_entries(
    entries: Vec<(PathBuf, ChangeKind, LineStats)>,
    staged: bool,
) -> Vec<GitStatusListEntry> {
    entries
        .into_iter()
        .map(|(path, kind, stats)| GitStatusListEntry {
            path,
            kind,
            staged,
            stats,
        })
        .collect()
}

#[derive(Default)]
struct StatusTreeNode {
    folders: BTreeMap<String, StatusTreeNode>,
    files: Vec<GitStatusListEntry>,
}

#[derive(Clone)]
enum StatusTreeRow {
    Folder {
        name: SharedString,
        full_path: SharedString,
        key: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        entry: GitStatusListEntry,
        depth: usize,
    },
}

/// A single lazily rendered row in the Git changes list. Keeping section
/// headers, folders, and files in one flat collection lets GPUI mount only the
/// visible slice instead of rebuilding every changed file during unrelated
/// workspace redraws (for example, while scrolling chat).
#[derive(Clone)]
pub enum GitStatusListRow {
    Section {
        title: &'static str,
        count: usize,
        show_bulk_action: bool,
    },
    Folder {
        name: SharedString,
        full_path: SharedString,
        key: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        scope: &'static str,
        entry: GitStatusListEntry,
        depth: usize,
    },
}

fn status_tree(entries: Vec<GitStatusListEntry>) -> StatusTreeNode {
    let mut root = StatusTreeNode::default();
    for entry in entries {
        let components = entry
            .path
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();
        let Some((file_name, folders)) = components.split_last() else {
            continue;
        };
        let mut node = &mut root;
        for folder in folders {
            node = node.folders.entry(folder.clone()).or_default();
        }
        let mut entry = entry;
        if entry.path.file_name().is_none() {
            entry.path.push(file_name);
        }
        node.files.push(entry);
    }
    root
}

fn flatten_status_tree(
    scope: &'static str,
    node: &StatusTreeNode,
    parent_path: &str,
    depth: usize,
    collapsed_folders: &HashSet<String>,
    rows: &mut Vec<StatusTreeRow>,
) {
    for (folder_name, folder_node) in &node.folders {
        let mut display_name = folder_name.clone();
        let mut full_path = if parent_path.is_empty() {
            folder_name.clone()
        } else {
            format!("{parent_path}/{folder_name}")
        };
        let mut visible_node = folder_node;

        // Compress single-child folder chains like `crates/ide-app/src/ui`.
        while visible_node.files.is_empty() && visible_node.folders.len() == 1 {
            let (next_name, next_node) = visible_node.folders.iter().next().unwrap();
            display_name.push('/');
            display_name.push_str(next_name);
            full_path.push('/');
            full_path.push_str(next_name);
            visible_node = next_node;
        }

        let key = format!("{scope}:{full_path}");
        let collapsed = collapsed_folders.contains(&key);
        rows.push(StatusTreeRow::Folder {
            name: display_name.into(),
            full_path: full_path.clone().into(),
            key,
            depth,
            collapsed,
        });
        if !collapsed {
            flatten_status_tree(
                scope,
                visible_node,
                &full_path,
                depth + 1,
                collapsed_folders,
                rows,
            );
        }
    }

    let mut files = node.files.clone();
    files.sort_by_key(|entry| {
        entry
            .path
            .file_name()
            .map(|name| name.to_ascii_lowercase())
            .unwrap_or_default()
    });
    rows.extend(
        files
            .into_iter()
            .map(|entry| StatusTreeRow::File { entry, depth }),
    );
}

/// Flatten one section (Staged / Changes / Untracked) into cheap row data.
/// Element construction is deferred to the virtual list's visible-range
/// callback.
pub fn status_section_rows(
    title: Option<&'static str>,
    entries: Vec<GitStatusListEntry>,
    show_bulk_action: bool,
    view_mode: GitStatusViewMode,
    collapsed_folders: &HashSet<String>,
) -> Vec<GitStatusListRow> {
    let count = entries.len();
    let scope = title.unwrap_or("ALL");
    let mut rows = Vec::with_capacity(count + usize::from(title.is_some()));

    if let Some(title) = title {
        rows.push(GitStatusListRow::Section {
            title,
            count,
            show_bulk_action,
        });
    }

    match view_mode {
        GitStatusViewMode::List => {
            rows.extend(entries.into_iter().map(|entry| GitStatusListRow::File {
                scope,
                entry,
                depth: 0,
            }))
        }
        GitStatusViewMode::Tree => {
            let tree = status_tree(entries);
            let mut tree_rows = Vec::new();
            flatten_status_tree(scope, &tree, "", 0, collapsed_folders, &mut tree_rows);
            rows.extend(tree_rows.into_iter().map(|row| match row {
                StatusTreeRow::Folder {
                    name,
                    full_path,
                    key,
                    depth,
                    collapsed,
                } => GitStatusListRow::Folder {
                    name,
                    full_path,
                    key,
                    depth,
                    collapsed,
                },
                StatusTreeRow::File { entry, depth } => GitStatusListRow::File {
                    scope,
                    entry,
                    depth,
                },
            }));
        }
    }

    rows
}

#[allow(clippy::too_many_arguments)]
pub fn render_status_row(
    ix: usize,
    row: GitStatusListRow,
    git: Entity<GitState>,
    project: Option<ProjectId>,
    center: gpui::WeakEntity<CenterArea>,
    panel: gpui::WeakEntity<GitPanel>,
    cx: &mut App,
) -> AnyElement {
    match row {
        GitStatusListRow::Section {
            title,
            count,
            show_bulk_action,
        } => render_section_header(title, count, show_bulk_action, git, cx),
        GitStatusListRow::Folder {
            name,
            full_path,
            key,
            depth,
            collapsed,
        } => render_folder_row(name, full_path, key, depth, collapsed, panel, cx),
        GitStatusListRow::File {
            scope,
            entry,
            depth,
        } => render_file_row(scope, ix, entry, depth, git, project, center, cx),
    }
}

fn render_section_header(
    title: &'static str,
    count: usize,
    show_bulk_action: bool,
    git: Entity<GitState>,
    cx: &mut App,
) -> AnyElement {
    h_flex()
        .h(crate::ui::design::git_status_row_h())
        .w_full()
        // List container carries an 8px inset; +6px here lands the label on
        // the panel header's 14px identity line.
        .px(px(6.))
        .items_center()
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_label())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t4(cx))
                .child(format!("{} ({count})", title.to_uppercase())),
        )
        .when(show_bulk_action, |row| {
            row.child(
                style::ghost_button_compact("bulk-unstage", "Unstage all").on_click(
                    move |_, _, cx| {
                        git.update(cx, |git, cx| git.unstage_all(cx));
                    },
                ),
            )
        })
        .into_any_element()
}

fn render_folder_row(
    name: SharedString,
    full_path: SharedString,
    key: String,
    depth: usize,
    collapsed: bool,
    panel: gpui::WeakEntity<GitPanel>,
    cx: &mut App,
) -> AnyElement {
    let tooltip_path = full_path.clone();
    let row_id = SharedString::from(format!("git-status-folder:{key}"));
    h_flex()
        .id(row_id)
        .h(crate::ui::design::git_status_row_h())
        .w_full()
        // The list container carries the 8px pill inset (margins on w_full
        // rows are not honored inside the list); +6px lead lands the content
        // on the panel header's 14px identity line.
        .pl(px(6. + depth as f32 * 14.))
        .pr_2()
        .gap_1()
        .items_center()
        .rounded(crate::ui::design::r_sm())
        .cursor_pointer()
        .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
        .tooltip(move |window, cx| Tooltip::new(tooltip_path.clone()).build(window, cx))
        .on_click(move |_, _, cx| {
            panel
                .update(cx, |panel, cx| {
                    if !panel.collapsed_status_folders.remove(&key) {
                        panel.collapsed_status_folders.insert(key.clone());
                    }
                    cx.notify();
                })
                .ok();
        })
        .child(
            Icon::new(if collapsed {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            })
            .size(crate::ui::design::icon_sm())
            .text_color(crate::ui::design::t3(cx)),
        )
        .child(
            Icon::new(if collapsed {
                IconName::Folder
            } else {
                IconName::FolderOpen
            })
            .size(crate::ui::design::icon_md())
            .text_color(crate::ui::design::t3(cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(crate::ui::design::text_file())
                .font_family(crate::ui::design::FONT_MONO)
                .text_color(crate::ui::design::t2(cx))
                .child(name),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_file_row(
    scope: &'static str,
    ix: usize,
    entry: GitStatusListEntry,
    depth: usize,
    git: Entity<GitState>,
    project: Option<ProjectId>,
    center: gpui::WeakEntity<CenterArea>,
    cx: &mut App,
) -> AnyElement {
    let full_path: SharedString = entry.path.display().to_string().into();
    let file_name: SharedString = entry
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.display().to_string())
        .into();
    let tooltip_path = full_path.clone();
    let diff_title = file_name.clone();
    let color = kind_color(entry.kind, cx);
    let soft_color = kind_soft_color(entry.kind, cx);
    let action_path = entry.path.clone();
    let row_center = center;
    let staged = entry.staged;
    let can_discard = !staged;
    let insertions = entry.stats.insertions;
    let deletions = entry.stats.deletions;
    let row_id: SharedString = format!("git-status-{scope}-{ix}").into();
    let checkbox_id: SharedString = format!("stage-check-{scope}-{ix}").into();
    let diff_git = git.clone();

    h_flex()
        .id(row_id)
        .group("git-status-file-row")
        .h(crate::ui::design::git_status_row_h())
        .w_full()
        // The list container carries the 8px pill inset (margins on w_full
        // rows are not honored inside the list); +6px lead lands the content
        // on the panel header's 14px identity line.
        .pl(px(6. + depth as f32 * 14.))
        .pr_2()
        .gap_2()
        .items_center()
        .rounded(crate::ui::design::r_sm())
        .cursor_pointer()
        .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
        .tooltip(move |window, cx| Tooltip::new(tooltip_path.clone()).build(window, cx))
        .on_click(move |_, _, cx| {
            let Some(project) = project else { return };
            let kind = DiffKind::File {
                path: action_path.clone(),
                staged,
            };
            row_center
                .update(cx, |center, cx| {
                    center.open_diff_from_git(
                        project,
                        diff_git.clone(),
                        kind,
                        diff_title.clone(),
                        cx,
                    );
                })
                .ok();
        })
        .child({
            let toggle_git = git.clone();
            let toggle_path = entry.path.clone();
            crate::ui::design::indicator::status_checkbox(
                checkbox_id,
                staged,
                color,
                soft_color,
                crate::ui::design::t4(cx),
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let path = toggle_path.clone();
                toggle_git.update(cx, |git, cx| {
                    if staged {
                        git.unstage(path, cx);
                    } else {
                        git.stage(path, cx);
                    }
                });
            })
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(crate::ui::design::text_file())
                .font_family(crate::ui::design::FONT_MONO)
                .text_color(crate::ui::design::t2(cx))
                .truncate()
                .child(file_name),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_1()
                .whitespace_nowrap()
                .text_size(crate::ui::design::text_ui())
                .invisible()
                .group_hover("git-status-file-row", |stats| stats.visible())
                .child(
                    div()
                        .text_color(crate::ui::design::sage(cx))
                        .child(format!("+{insertions}")),
                )
                .child(
                    div()
                        .text_color(crate::ui::design::rose(cx))
                        .child(format!("−{deletions}")),
                ),
        )
        .when(can_discard, |row| {
            let discard_git = git;
            let discard_path = entry.path;
            row.child(
                style::icon_button(("discard", ix), IconName::Undo2, cx)
                    .invisible()
                    .group_hover("git-status-file-row", |button| button.visible())
                    .tooltip("Discard changes")
                    .on_click(move |_, window, cx| {
                        confirm_discard(
                            discard_git.clone(),
                            discard_path.clone(),
                            entry.kind,
                            window,
                            cx,
                        );
                    }),
            )
        })
        .into_any_element()
}

/// Asks before throwing away local changes — discard is destructive.
fn confirm_discard(
    git: Entity<GitState>,
    path: PathBuf,
    kind: ChangeKind,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let name: SharedString = path.display().to_string().into();
    let is_untracked = kind == ChangeKind::Untracked;
    let (title, message, confirm_label) = if is_untracked {
        (
            "Delete untracked file",
            "This deletes the file from disk. It cannot be recovered from Git.",
            "Delete",
        )
    } else {
        (
            "Discard changes",
            "This restores the file from Git. The local edits cannot be recovered.",
            "Discard",
        )
    };
    ConfirmDialog::new(title, message)
        .detail(name)
        .confirm_label(confirm_label)
        .confirm_id("confirm-discard")
        .on_confirm(move |_, cx| {
            git.update(cx, |git, cx| git.discard(path.clone(), cx));
        })
        .open(window, cx);
}

type SectionEntries = Vec<(PathBuf, ChangeKind, LineStats)>;

/// Splits a snapshot's entries into the three sections.
pub fn split_entries(entries: &[StatusEntry]) -> (SectionEntries, SectionEntries, SectionEntries) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    for entry in entries {
        if let Some(kind) = entry.staged {
            staged.push((entry.path.clone(), kind, entry.staged_stats));
        }
        match entry.unstaged {
            Some(ChangeKind::Untracked) => untracked.push((
                entry.path.clone(),
                ChangeKind::Untracked,
                entry.unstaged_stats,
            )),
            Some(kind) => unstaged.push((entry.path.clone(), kind, entry.unstaged_stats)),
            None => {}
        }
    }
    (staged, unstaged, untracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_compresses_single_child_folder_chains() {
        let entries = status_list_entries(
            vec![
                (
                    PathBuf::from("crates/ide-app/src/ui/center/message.rs"),
                    ChangeKind::Modified,
                    LineStats::default(),
                ),
                (
                    PathBuf::from("crates/ide-app/src/ui/git/status_list.rs"),
                    ChangeKind::Modified,
                    LineStats::default(),
                ),
            ],
            false,
        );
        let tree = status_tree(entries);
        let mut rows = Vec::new();
        flatten_status_tree("CHANGES", &tree, "", 0, &HashSet::new(), &mut rows);

        let StatusTreeRow::Folder { name, depth, .. } = &rows[0] else {
            panic!("expected compressed root folder");
        };
        assert_eq!(name.as_ref(), "crates/ide-app/src/ui");
        assert_eq!(*depth, 0);
    }

    #[test]
    fn split_entries_keeps_staged_and_untracked_sides() {
        let entries = vec![
            StatusEntry {
                path: PathBuf::from("src/lib.rs"),
                staged: Some(ChangeKind::Modified),
                unstaged: None,
                staged_stats: LineStats {
                    insertions: 3,
                    deletions: 1,
                },
                unstaged_stats: LineStats::default(),
            },
            StatusEntry {
                path: PathBuf::from("notes.md"),
                staged: None,
                unstaged: Some(ChangeKind::Untracked),
                staged_stats: LineStats::default(),
                unstaged_stats: LineStats {
                    insertions: 8,
                    deletions: 0,
                },
            },
        ];
        let (staged, unstaged, untracked) = split_entries(&entries);
        assert_eq!(staged.len(), 1);
        assert!(unstaged.is_empty());
        assert_eq!(untracked.len(), 1);
        assert_eq!(staged[0].2.insertions, 3);
        assert_eq!(untracked[0].2.insertions, 8);
    }

    #[test]
    fn large_sections_flatten_to_data_rows_for_lazy_rendering() {
        let entries = status_list_entries(
            (0..5_000)
                .map(|ix| {
                    (
                        PathBuf::from(format!("src/generated/file-{ix}.rs")),
                        ChangeKind::Modified,
                        LineStats::default(),
                    )
                })
                .collect(),
            false,
        );

        let rows = status_section_rows(
            Some("CHANGES"),
            entries,
            false,
            GitStatusViewMode::List,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 5_001);
        assert!(matches!(
            rows.first(),
            Some(GitStatusListRow::Section {
                title: "CHANGES",
                count: 5_000,
                ..
            })
        ));
        assert!(rows[1..]
            .iter()
            .all(|row| matches!(row, GitStatusListRow::File { .. })));
    }
}
