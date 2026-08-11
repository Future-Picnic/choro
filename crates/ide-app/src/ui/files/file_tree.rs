use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, ClipboardItem, Context, Entity,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState, SelectAll},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    notification::Notification,
    v_flex, Icon, IconName, Sizable, WindowExt,
};
use ide_core::git::{ChangeKind, GitSnapshot};

use crate::state::{AgentRecords, GitState, GitStates, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::confirm::ConfirmDialog;

const HIDDEN_ENTRIES: &[&str] = &[".git", ".DS_Store"];

#[derive(Clone)]
struct Row {
    path: PathBuf,
    name: SharedString,
    depth: usize,
    is_dir: bool,
    expanded: bool,
    ignored: bool,
}

#[derive(Clone, Copy)]
enum NewEntryKind {
    File,
    Folder,
}

struct InlineRename {
    path: PathBuf,
    root: PathBuf,
    input: Entity<InputState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GitDecoration {
    Created,
    Modified,
    Deleted,
    Conflicted,
}

/// Right panel "Files" tab: a lazy directory tree for the active project.
pub struct FileTree {
    workspace: Entity<Workspace>,
    git_states: Entity<GitStates>,
    agents: Entity<AgentRecords>,
    center: Entity<CenterArea>,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    rows_root: Option<PathBuf>,
    selected: Option<PathBuf>,
    renaming: Option<InlineRename>,
    loading: bool,
    error: Option<SharedString>,
    load_seq: u64,
    /// The Solo agent the tree is currently scoped around, if any.
    scope_agent: Option<uuid::Uuid>,
    /// User flipped the tree back to the project branch while a Solo is open.
    /// Resets to the Solo default whenever the focused Solo changes.
    scope_project: bool,
    /// Git state for the focused Solo's worktree.
    lane_git: Option<(uuid::Uuid, Entity<GitState>)>,
}

impl FileTree {
    pub fn view(
        workspace: Entity<Workspace>,
        git_states: Entity<GitStates>,
        agents: Entity<AgentRecords>,
        center: Entity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&git_states, |_, _, cx| cx.notify()).detach();
            cx.observe(&agents, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                git_states,
                agents,
                center,
                expanded: HashSet::new(),
                rows: Vec::new(),
                rows_root: None,
                selected: None,
                renaming: None,
                loading: false,
                error: None,
                load_seq: 0,
                scope_agent: None,
                scope_project: false,
                lane_git: None,
            }
        })
    }

    fn collect_rows(
        dir: &Path,
        expanded: &HashSet<PathBuf>,
        depth: usize,
        rows: &mut Vec<Row>,
    ) -> anyhow::Result<()> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        let mut entries: Vec<_> = read
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !HIDDEN_ENTRIES.contains(&name.as_ref())
            })
            .collect();
        entries.sort_by_key(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (!is_dir, e.file_name().to_ascii_lowercase())
        });

        for entry in entries {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let is_expanded = is_dir && expanded.contains(&path);
            rows.push(Row {
                name: entry.file_name().to_string_lossy().into_owned().into(),
                depth,
                is_dir,
                expanded: is_expanded,
                ignored: false,
                path: path.clone(),
            });
            if is_expanded {
                Self::collect_rows(&path, expanded, depth + 1, rows)?;
            }
        }

        Ok(())
    }

    fn mark_ignored_rows(root: &Path, rows: &mut [Row]) {
        let mut child = match Command::new("git")
            .args(["check-ignore", "--stdin", "-z"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return,
        };

        let paths = rows
            .iter()
            .filter_map(|row| row.path.strip_prefix(root).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let writer = child.stdin.take().map(|mut stdin| {
            std::thread::spawn(move || {
                for path in paths {
                    let _ = stdin.write_all(path.as_bytes());
                    let _ = stdin.write_all(&[0]);
                }
            })
        });

        let output = child.wait_with_output();
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        let Ok(output) = output else {
            return;
        };
        let ignored = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| root.join(String::from_utf8_lossy(path).as_ref()))
            .collect::<HashSet<_>>();
        for row in rows {
            row.ignored = ignored.contains(&row.path);
        }
    }

    fn reload(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        // Only surface the loading row when there is nothing to show yet
        // (first load or a root switch). On an expand/collapse the existing
        // rows stay put until the fresh ones swap in — inserting the loading
        // row above them made the whole tree jump down and back every tap.
        let root_changed = self.rows_root.as_deref() != Some(root.as_path());
        self.loading = root_changed || self.rows.is_empty();
        self.error = None;
        self.rows_root = Some(root.clone());
        self.load_seq = self.load_seq.wrapping_add(1);
        let seq = self.load_seq;
        let expanded = self.expanded.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut rows = Vec::new();
                    Self::collect_rows(&root, &expanded, 0, &mut rows)?;
                    Self::mark_ignored_rows(&root, &mut rows);
                    Ok::<_, anyhow::Error>(rows)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(rows) => {
                        this.rows = rows;
                        this.error = None;
                    }
                    Err(error) => {
                        this.rows.clear();
                        this.error = Some(format!("{error:#}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn validate_entry_name(name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!name.is_empty(), "Enter a name.");
        anyhow::ensure!(
            name != "." && name != "..",
            "That name is reserved by the filesystem."
        );
        anyhow::ensure!(
            Path::new(name).file_name() == Some(OsStr::new(name)),
            "Names cannot contain path separators."
        );
        Ok(())
    }

    fn submit_create(
        tree: Entity<Self>,
        parent: PathBuf,
        root: PathBuf,
        kind: NewEntryKind,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name = input.read(cx).value().trim().to_owned();
        let result = (|| {
            Self::validate_entry_name(&name)?;
            let target = parent.join(&name);
            match kind {
                NewEntryKind::File => {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)?;
                }
                NewEntryKind::Folder => std::fs::create_dir(&target)?,
            }
            Ok::<_, anyhow::Error>(target)
        })();

        match result {
            Ok(target) => {
                tree.update(cx, |this, cx| {
                    this.selected = Some(target);
                    this.expanded.insert(parent);
                    this.reload(root, cx);
                });
                window.close_dialog(cx);
            }
            Err(error) => {
                window.push_notification(Notification::error(error.to_string()), cx);
            }
        }
    }

    fn open_create_dialog(
        &mut self,
        parent: PathBuf,
        root: PathBuf,
        kind: NewEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, placeholder, confirm_label) = match kind {
            NewEntryKind::File => ("New File", "File name", "Create File"),
            NewEntryKind::Folder => ("New Folder", "Folder name", "Create Folder"),
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        input.update(cx, |input, cx| input.focus(window, cx));
        let tree = cx.entity().clone();
        let parent_label = parent.display().to_string();

        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            let enter_tree = tree.clone();
            let enter_parent = parent.clone();
            let enter_root = root.clone();
            let enter_input = input.clone();
            let save_tree = tree.clone();
            let save_parent = parent.clone();
            let save_root = root.clone();
            let save_input = input.clone();

            dialog
                .title(title)
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(Input::new(&input))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(dialog_cx))
                                .truncate()
                                .child(parent_label.clone()),
                        )
                        .capture_key_down(move |event, window, cx| {
                            if event.keystroke.key == "enter" {
                                cx.stop_propagation();
                                Self::submit_create(
                                    enter_tree.clone(),
                                    enter_parent.clone(),
                                    enter_root.clone(),
                                    kind,
                                    enter_input.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .footer(move |_, _, _, cx| {
                    let tree = save_tree.clone();
                    let parent = save_parent.clone();
                    let root = save_root.clone();
                    let input = save_input.clone();
                    vec![
                        crate::ui::style::dialog_neutral_button(
                            "file-entry-create-cancel",
                            "Cancel",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                        crate::ui::style::primary_button_compact(
                            "file-entry-create-confirm",
                            confirm_label,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            Self::submit_create(
                                tree.clone(),
                                parent.clone(),
                                root.clone(),
                                kind,
                                input.clone(),
                                window,
                                cx,
                            );
                        }),
                    ]
                })
        });
    }

    fn remap_path(path: &Path, from: &Path, to: &Path) -> PathBuf {
        path.strip_prefix(from)
            .map(|suffix| to.join(suffix))
            .unwrap_or_else(|_| path.to_path_buf())
    }

    fn decoration_for_kind(kind: ChangeKind) -> GitDecoration {
        match kind {
            ChangeKind::Conflicted => GitDecoration::Conflicted,
            ChangeKind::Deleted => GitDecoration::Deleted,
            ChangeKind::Modified | ChangeKind::Renamed => GitDecoration::Modified,
            ChangeKind::Added | ChangeKind::Untracked => GitDecoration::Created,
        }
    }

    fn decoration_rank(decoration: GitDecoration) -> u8 {
        match decoration {
            GitDecoration::Created => 1,
            GitDecoration::Modified => 2,
            GitDecoration::Deleted => 3,
            GitDecoration::Conflicted => 4,
        }
    }

    fn update_decoration(
        decorations: &mut HashMap<PathBuf, GitDecoration>,
        path: PathBuf,
        decoration: GitDecoration,
    ) {
        decorations
            .entry(path)
            .and_modify(|current| {
                if Self::decoration_rank(decoration) > Self::decoration_rank(*current) {
                    *current = decoration;
                }
            })
            .or_insert(decoration);
    }

    fn git_decorations(root: &Path, snapshot: &GitSnapshot) -> HashMap<PathBuf, GitDecoration> {
        let mut decorations = HashMap::new();
        for entry in &snapshot.entries {
            let mut strongest = None;
            for kind in [entry.staged, entry.unstaged].into_iter().flatten() {
                let decoration = Self::decoration_for_kind(kind);
                if strongest.is_none_or(|current| {
                    Self::decoration_rank(decoration) > Self::decoration_rank(current)
                }) {
                    strongest = Some(decoration);
                }
            }
            let Some(decoration) = strongest else {
                continue;
            };

            let path = root.join(&entry.path);
            Self::update_decoration(&mut decorations, path.clone(), decoration);
            let mut parent = path.parent();
            while let Some(directory) = parent.filter(|directory| *directory != root) {
                if !directory.starts_with(root) {
                    break;
                }
                Self::update_decoration(&mut decorations, directory.to_path_buf(), decoration);
                parent = directory.parent();
            }
        }
        decorations
    }

    fn decoration_color(decoration: GitDecoration, cx: &App) -> gpui::Hsla {
        match decoration {
            GitDecoration::Created => crate::ui::design::sage(cx),
            GitDecoration::Modified => crate::ui::design::amber(cx),
            GitDecoration::Deleted | GitDecoration::Conflicted => crate::ui::design::rose(cx),
        }
    }

    fn finish_inline_rename(
        &mut self,
        input_id: gpui::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self
            .renaming
            .as_ref()
            .filter(|rename| rename.input.entity_id() == input_id)
        else {
            return;
        };
        let path = rename.path.clone();
        let root = rename.root.clone();
        let input = rename.input.clone();
        let name = input.read(cx).value().trim().to_owned();
        let result = (|| {
            Self::validate_entry_name(&name)?;
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("This item cannot be renamed."))?;
            let target = parent.join(&name);
            if target == path {
                return Ok(target);
            }
            match std::fs::symlink_metadata(&target) {
                Ok(_) => anyhow::bail!("An item with that name already exists."),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            std::fs::rename(&path, &target)?;
            Ok::<_, anyhow::Error>(target)
        })();

        match result {
            Ok(target) => {
                self.renaming = None;
                self.expanded = self
                    .expanded
                    .iter()
                    .map(|expanded| Self::remap_path(expanded, &path, &target))
                    .collect();
                self.selected = Some(target.clone());
                if target == path {
                    cx.notify();
                } else {
                    self.reload(root, cx);
                }
            }
            Err(error) => {
                window.push_notification(Notification::error(error.to_string()), cx);
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        }
    }

    fn begin_inline_rename(
        &mut self,
        path: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current_name));
        let input_id = input.entity_id();
        cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.finish_inline_rename(input_id, window, cx);
                }
            },
        )
        .detach();

        self.selected = Some(path.clone());
        self.renaming = Some(InlineRename {
            path,
            root,
            input: input.clone(),
        });
        cx.notify();

        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
            window.dispatch_action(Box::new(SelectAll), cx);
        });
    }

    fn confirm_move_to_trash(
        &mut self,
        path: PathBuf,
        root: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let non_empty = is_dir
            && std::fs::read_dir(&path)
                .ok()
                .and_then(|mut entries| entries.next())
                .is_some();
        let message = if non_empty {
            format!("“{name}” is not empty. It and everything inside it will be moved to Trash.")
        } else {
            format!("“{name}” will be moved to Trash.")
        };
        let tree = cx.entity().clone();

        ConfirmDialog::new("Move to Trash?", message)
            .icon(IconName::Delete)
            .detail(path.display().to_string())
            .confirm_label("Move to Trash")
            .confirm_id("file-entry-trash-confirm")
            .on_confirm(move |_, cx| {
                tree.update(cx, |this, cx| {
                    this.start_move_to_trash(path.clone(), root.clone(), cx);
                });
            })
            .open(window, cx);
    }

    fn start_move_to_trash(&mut self, path: PathBuf, root: PathBuf, cx: &mut Context<Self>) {
        let task_path = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::open_with::move_to_trash(&task_path) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.expanded
                        .retain(|expanded| !expanded.starts_with(&path));
                    if this
                        .selected
                        .as_ref()
                        .is_some_and(|selected| selected.starts_with(&path))
                    {
                        this.selected = None;
                    }
                    this.reload(root, cx);
                }
                Err(error) => {
                    this.error = Some(format!("Could not move item to Trash: {error:#}").into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn build_row_menu(
        mut menu: PopupMenu,
        tree: Entity<Self>,
        path: PathBuf,
        root: PathBuf,
        is_dir: bool,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        tree.update(cx, |this, cx| {
            this.selected = Some(path.clone());
            cx.notify();
        });

        if is_dir {
            let create_tree = tree.clone();
            let create_parent = path.clone();
            let create_root = root.clone();
            menu = menu.item(
                PopupMenuItem::new("New File")
                    .icon(IconName::File)
                    .on_click(move |_, window, cx| {
                        create_tree.update(cx, |this, cx| {
                            this.open_create_dialog(
                                create_parent.clone(),
                                create_root.clone(),
                                NewEntryKind::File,
                                window,
                                cx,
                            );
                        });
                    }),
            );

            let create_tree = tree.clone();
            let create_parent = path.clone();
            let create_root = root.clone();
            menu = menu
                .item(
                    PopupMenuItem::new("New Folder")
                        .icon(IconName::Folder)
                        .on_click(move |_, window, cx| {
                            create_tree.update(cx, |this, cx| {
                                this.open_create_dialog(
                                    create_parent.clone(),
                                    create_root.clone(),
                                    NewEntryKind::Folder,
                                    window,
                                    cx,
                                );
                            });
                        }),
                )
                .separator();
        }

        let reveal_path = path.clone();
        menu = menu.item(
            PopupMenuItem::new("Reveal in Finder")
                .icon(IconName::FolderOpen)
                .on_click(move |_, _, _| {
                    crate::open_with::reveal_in_finder(&reveal_path);
                }),
        );

        if is_dir {
            let terminal_path = path.clone();
            menu = menu.item(
                PopupMenuItem::new("Open in Terminal")
                    .icon(IconName::SquareTerminal)
                    .on_click(move |_, _, _| {
                        crate::open_with::open_in_terminal(&terminal_path);
                    }),
            );
        } else {
            let default_path = path.clone();
            menu = menu.item(
                PopupMenuItem::new("Open in Default App")
                    .icon(IconName::ExternalLink)
                    .on_click(move |_, _, _| {
                        crate::open_with::open_default(&default_path);
                    }),
            );
        }

        let copy_path = path.clone();
        menu = menu.separator().item(
            PopupMenuItem::new("Copy Path")
                .icon(IconName::Copy)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        copy_path.to_string_lossy().into_owned(),
                    ));
                }),
        );

        let rename_tree = tree.clone();
        let rename_path = path.clone();
        let rename_root = root.clone();
        menu = menu.separator().item(
            PopupMenuItem::new("Rename")
                .icon(IconName::ALargeSmall)
                .on_click(move |_, window, cx| {
                    rename_tree.update(cx, |this, cx| {
                        this.begin_inline_rename(
                            rename_path.clone(),
                            rename_root.clone(),
                            window,
                            cx,
                        );
                    });
                }),
        );

        let trash_tree = tree;
        let trash_path = path;
        let trash_root = root;
        menu.item(
            PopupMenuItem::new("Move to Trash")
                .icon(Icon::new(IconName::Delete).text_color(crate::ui::design::rose(cx)))
                .on_click(move |_, window, cx| {
                    trash_tree.update(cx, |this, cx| {
                        this.confirm_move_to_trash(
                            trash_path.clone(),
                            trash_root.clone(),
                            is_dir,
                            window,
                            cx,
                        );
                    });
                }),
        )
    }

    fn build_root_menu(
        mut menu: PopupMenu,
        tree: Entity<Self>,
        root: PathBuf,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        tree.update(cx, |this, cx| {
            this.selected = None;
            cx.notify();
        });

        let create_tree = tree.clone();
        let create_parent = root.clone();
        let create_root = root.clone();
        menu = menu.item(
            PopupMenuItem::new("New File")
                .icon(IconName::File)
                .on_click(move |_, window, cx| {
                    create_tree.update(cx, |this, cx| {
                        this.open_create_dialog(
                            create_parent.clone(),
                            create_root.clone(),
                            NewEntryKind::File,
                            window,
                            cx,
                        );
                    });
                }),
        );

        let create_tree = tree.clone();
        let create_parent = root.clone();
        let create_root = root.clone();
        menu = menu
            .item(
                PopupMenuItem::new("New Folder")
                    .icon(IconName::Folder)
                    .on_click(move |_, window, cx| {
                        create_tree.update(cx, |this, cx| {
                            this.open_create_dialog(
                                create_parent.clone(),
                                create_root.clone(),
                                NewEntryKind::Folder,
                                window,
                                cx,
                            );
                        });
                    }),
            )
            .separator();

        let reveal_path = root.clone();
        menu = menu.item(
            PopupMenuItem::new("Reveal in Finder")
                .icon(IconName::FolderOpen)
                .on_click(move |_, _, _| {
                    crate::open_with::reveal_in_finder(&reveal_path);
                }),
        );

        let terminal_path = root.clone();
        menu = menu
            .item(
                PopupMenuItem::new("Open in Terminal")
                    .icon(IconName::SquareTerminal)
                    .on_click(move |_, _, _| {
                        crate::open_with::open_in_terminal(&terminal_path);
                    }),
            )
            .separator();

        let refresh_tree = tree.clone();
        let refresh_root = root.clone();
        menu = menu.item(
            PopupMenuItem::new("Refresh")
                .icon(IconName::Redo2)
                .on_click(move |_, _, cx| {
                    refresh_tree.update(cx, |this, cx| {
                        this.reload(refresh_root.clone(), cx);
                    });
                }),
        );

        let collapse_tree = tree;
        menu.item(
            PopupMenuItem::new("Collapse All")
                .icon(IconName::Minimize)
                .on_click(move |_, _, cx| {
                    collapse_tree.update(cx, |this, cx| {
                        this.expanded.clear();
                        this.reload(root.clone(), cx);
                    });
                }),
        )
    }
}

impl Render for FileTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((project, root)) = self
            .workspace
            .read(cx)
            .active_project()
            .map(|p| (p.id, p.path.clone()))
        else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_color(crate::ui::design::t3(cx))
                .child("No project selected")
                .into_any_element();
        };

        // Same rule as the git panel: the flip exists only while the open
        // agent is a Solo with a live lane, and defaults to the Solo.
        let lane_context = self
            .agents
            .read(cx)
            .explicitly_selected_agent(project)
            .filter(|agent| agent.is_active_solo())
            .and_then(|agent| {
                let lane = agent.lane_path.clone()?;
                let branch = agent.solo_branch.clone()?;
                lane.join(".git")
                    .exists()
                    .then_some((agent.id, lane, branch))
            });
        match &lane_context {
            Some((agent_id, lane, _)) => {
                if self.scope_agent != Some(*agent_id) {
                    self.scope_agent = Some(*agent_id);
                    self.scope_project = false;
                    self.lane_git = None;
                }
                if self
                    .lane_git
                    .as_ref()
                    .is_none_or(|(current, _)| current != agent_id)
                {
                    let git = cx.new(|cx| GitState::new_with_worktree_watcher(lane.clone(), cx));
                    cx.observe(&git, |_, _, cx| cx.notify()).detach();
                    self.lane_git = Some((*agent_id, git));
                }
            }
            None => {
                self.scope_agent = None;
                self.scope_project = false;
                self.lane_git = None;
            }
        }
        let lane_active = lane_context.is_some() && !self.scope_project;
        let project_branch: SharedString = self
            .git_states
            .read(cx)
            .get(project)
            .and_then(|git| git.read(cx).branch_label())
            .unwrap_or_else(|| "…".to_string())
            .into();
        let (repository_roots, active_repository) = if lane_active {
            (HashSet::new(), None)
        } else {
            let git_states = self.git_states.read(cx);
            let active = git_states.active_repository_path(project);
            let repositories = git_states
                .repositories(project)
                .into_iter()
                .filter_map(|git| {
                    let git = git.read(cx);
                    git.is_repo.then(|| git.repo_path.clone())
                })
                .collect::<HashSet<_>>();
            if repositories.len() > 1 {
                (repositories, active)
            } else {
                (HashSet::new(), active)
            }
        };
        let root = match &lane_context {
            Some((_, lane, _)) if lane_active => lane.clone(),
            _ => root,
        };

        if self.rows_root.as_ref() != Some(&root) {
            self.reload(root.clone(), cx);
        }

        let active_git = if lane_active {
            self.lane_git.as_ref().map(|(_, git)| git.clone())
        } else {
            self.git_states.read(cx).get(project)
        };
        let git_decorations = active_git
            .and_then(|git| {
                let git = git.read(cx);
                git.snapshot
                    .as_ref()
                    .map(|snapshot| Self::git_decorations(&git.repo_path, snapshot))
            })
            .unwrap_or_default();
        let rows = self.rows.clone();
        let renaming = self
            .renaming
            .as_ref()
            .map(|rename| (rename.path.clone(), rename.input.clone()));
        let loading = self.loading;
        let error = self.error.clone();
        let empty_space_tree = cx.entity().clone();
        let empty_space_root = root.clone();

        v_flex()
            .id("file-tree-scroll")
            .size_full()
            .p_1()
            .gap_0()
            .overflow_y_scroll()
            .when_some(lane_context, |list, (_, _, branch)| {
                let sky = crate::ui::design::sky(cx);
                list.child(
                    h_flex()
                        .w_full()
                        .px_1()
                        .py_1()
                        .when(lane_active, |row| row.bg(sky.opacity(0.07)))
                        .child(
                            crate::ui::style::segmented_container_quiet(cx)
                                .child(
                                    crate::ui::style::segment_with_leading(
                                        "files-scope-project",
                                        crate::ui::design::indicator::lucide_icon(
                                            lucide_icons::Icon::GitBranch,
                                            crate::ui::design::t3(cx),
                                            crate::ui::design::icon_sm(),
                                        )
                                        .into_any_element(),
                                        project_branch,
                                        !lane_active,
                                        cx,
                                    )
                                    .flex_1()
                                    .justify_center()
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scope_project = true;
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    crate::ui::style::segment_with_leading(
                                        "files-scope-lane",
                                        crate::ui::design::indicator::solo_icon(
                                            if lane_active {
                                                sky
                                            } else {
                                                crate::ui::design::t3(cx)
                                            },
                                            crate::ui::design::icon_sm(),
                                        )
                                        .into_any_element(),
                                        crate::ui::style::solo_slug_short(&branch, 10),
                                        lane_active,
                                        cx,
                                    )
                                    .flex_1()
                                    .justify_center()
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scope_project = false;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
            })
            .when(loading, |list| {
                list.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Loading files…"),
                )
            })
            .when_some(error, |list, error| {
                list.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .children(rows.into_iter().enumerate().map(|(ix, row)| {
                let icon = if row.is_dir {
                    if row.expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else {
                    IconName::File
                };
                let center = self.center.clone();
                let path = row.path.clone();
                let root = root.clone();
                let is_dir = row.is_dir;
                let is_repository_root = is_dir && repository_roots.contains(&row.path);
                let repository_icon_color = if active_repository.as_ref() == Some(&row.path) {
                    crate::ui::design::sky(cx).opacity(0.82)
                } else {
                    crate::ui::design::sky(cx).opacity(0.58)
                };
                let selected = self.selected.as_ref() == Some(&row.path);
                let name_color = git_decorations
                    .get(&row.path)
                    .copied()
                    .map(|decoration| Self::decoration_color(decoration, cx))
                    .unwrap_or_else(|| {
                        if row.ignored {
                            crate::ui::design::t4(cx)
                        } else if selected {
                            crate::ui::design::t1(cx)
                        } else {
                            crate::ui::design::t2(cx)
                        }
                    });
                let row_name = row.name.clone();
                let rename_input = renaming
                    .as_ref()
                    .filter(|(rename_path, _)| rename_path == &row.path)
                    .map(|(_, input)| input.clone());
                let menu_tree = cx.entity().clone();
                let menu_path = row.path.clone();
                let menu_root = root.clone();
                let name_element = if let Some(rename_input) = rename_input {
                    let cancel_tree = cx.entity().clone();
                    let rename_input_id = rename_input.entity_id();
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .capture_key_down(move |event, _, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                cancel_tree.update(cx, |this, cx| {
                                    if this.renaming.as_ref().is_some_and(|rename| {
                                        rename.input.entity_id() == rename_input_id
                                    }) {
                                        this.renaming = None;
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .child(Input::new(&rename_input).small().h(px(22.)))
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(crate::ui::design::text_body())
                        .text_color(name_color)
                        .truncate()
                        .child(row_name)
                        .into_any_element()
                };

                h_flex()
                    .id(("file-row", ix))
                    .w_full()
                    .pl(px(8. + row.depth as f32 * 14.))
                    .pr_2()
                    .py_0p5()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_xs())
                    .cursor_pointer()
                    .when(selected, |row| {
                        row.bg(crate::ui::design::surface_2(cx))
                            .text_color(crate::ui::design::t1(cx))
                    })
                    .when(!selected, |row| {
                        row.hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this
                            .renaming
                            .as_ref()
                            .is_some_and(|rename| rename.path == path)
                        {
                            return;
                        }
                        this.selected = Some(path.clone());
                        if is_dir {
                            if !this.expanded.remove(&path) {
                                this.expanded.insert(path.clone());
                            }
                            this.reload(root.clone(), cx);
                        } else {
                            center.update(cx, |center, cx| {
                                center.open_file(project, path.clone(), window, cx);
                            });
                        }
                    }))
                    .child(if is_repository_root {
                        crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::FolderGit2,
                            repository_icon_color,
                            crate::ui::design::icon(),
                        )
                        .into_any_element()
                    } else {
                        Icon::new(icon)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx))
                            .into_any_element()
                    })
                    .child(name_element)
                    .context_menu(move |menu, _, menu_cx| {
                        Self::build_row_menu(
                            menu,
                            menu_tree.clone(),
                            menu_path.clone(),
                            menu_root.clone(),
                            is_dir,
                            menu_cx,
                        )
                    })
            }))
            .child(
                div()
                    .id("file-tree-empty-space")
                    .w_full()
                    .flex_1()
                    .min_h(px(24.))
                    .context_menu(move |menu, _, menu_cx| {
                        Self::build_root_menu(
                            menu,
                            empty_space_tree.clone(),
                            empty_space_root.clone(),
                            menu_cx,
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::FileTree;
    use std::path::Path;

    #[test]
    fn entry_names_are_single_filesystem_components() {
        for valid in ["main.rs", ".env", "folder name"] {
            assert!(FileTree::validate_entry_name(valid).is_ok(), "{valid}");
        }
        for invalid in ["", ".", "..", "nested/file", "trailing/"] {
            assert!(FileTree::validate_entry_name(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn rename_remaps_descendants_without_touching_other_paths() {
        let from = Path::new("/project/old");
        let to = Path::new("/project/new");
        assert_eq!(
            FileTree::remap_path(Path::new("/project/old/src/lib.rs"), from, to),
            Path::new("/project/new/src/lib.rs")
        );
        assert_eq!(
            FileTree::remap_path(Path::new("/project/other"), from, to),
            Path::new("/project/other")
        );
    }
}
