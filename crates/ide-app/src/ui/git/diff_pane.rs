use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    div, list, prelude::FluentBuilder, px, App, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ListAlignment, ListState, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use gpui_component::{h_flex, v_flex, Icon, IconName, Sizable};
use ide_core::git::{FileDiff, LineOrigin};
use ide_core::ProjectId;
use uuid::Uuid;

use crate::state::GitState;
use crate::ui::center::CenterArea;

/// What a diff pane shows.
#[derive(Clone, PartialEq, Eq)]
pub enum DiffKind {
    /// All uncommitted changes vs HEAD.
    Project,
    /// One file: worktree-vs-index or index-vs-HEAD.
    File { path: PathBuf, staged: bool },
    /// What a commit introduced.
    Commit { sha: String },
    /// Explicit conversation paths, also applied to older saved snapshots.
    /// A missing snapshot never widens this to the entire project.
    ConversationFiles {
        repo_path: PathBuf,
        snapshot_id: Option<Uuid>,
        paths: Vec<PathBuf>,
    },
}

impl DiffKind {
    pub fn key(&self) -> String {
        match self {
            DiffKind::Project => "project-diff".into(),
            DiffKind::File { path, staged } => format!("file:{}:{staged}", path.display()),
            DiffKind::Commit { sha } => format!("commit:{sha}"),
            DiffKind::ConversationFiles {
                repo_path,
                snapshot_id,
                paths,
            } => {
                format!("conversation:{repo_path:?}:{snapshot_id:?}:{paths:?}")
            }
        }
    }

    pub fn key_for_repo(&self, repo: &Path) -> String {
        format!("repo:{}:{}", repo.display(), self.key())
    }
}

/// Editor-style diff view shown as a tab in the center area: collapsible
/// per-file sections with stage/unstage and open-file actions. Live-refreshes
/// while the working tree changes (except commit/snapshot diffs, which are static).
pub struct DiffPane {
    project: ProjectId,
    repo: PathBuf,
    kind: DiffKind,
    git: Option<Entity<GitState>>,
    center: WeakEntity<CenterArea>,
    diffs: Arc<Vec<FileDiff>>,
    collapsed: HashSet<PathBuf>,
    list_state: ListState,
    loading: bool,
}

#[derive(Clone, Copy)]
enum DiffRow {
    FileHeader(usize),
    Binary,
    Empty,
    Hunk {
        file: usize,
        hunk: usize,
    },
    Line {
        file: usize,
        hunk: usize,
        line: usize,
    },
}

impl DiffPane {
    pub fn new(
        project: ProjectId,
        repo: PathBuf,
        kind: DiffKind,
        git: Option<Entity<GitState>>,
        center: WeakEntity<CenterArea>,
        cx: &mut Context<Self>,
    ) -> Self {
        let repo = match &kind {
            DiffKind::ConversationFiles { repo_path, .. } => repo_path.clone(),
            _ => repo,
        };
        // Re-read while the working tree changes; historical diffs are immutable.
        if !matches!(
            kind,
            DiffKind::Commit { .. }
                | DiffKind::ConversationFiles {
                    snapshot_id: Some(_),
                    ..
                }
        ) {
            if let Some(git) = &git {
                cx.observe(git, |this: &mut Self, _, cx| this.reload(cx))
                    .detach();
            }
        }
        let mut pane = Self {
            project,
            repo,
            kind,
            git,
            center,
            diffs: Arc::new(Vec::new()),
            collapsed: HashSet::new(),
            list_state: ListState::new(0, ListAlignment::Top, px(600.)),
            loading: false,
        };
        pane.reload(cx);
        pane
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        let repo = self.repo.clone();
        let kind = self.kind.clone();
        cx.spawn(async move |this, cx| {
            let diffs = cx
                .background_executor()
                .spawn(async move {
                    match kind {
                        DiffKind::Project => ide_core::git::worktree_diffs(&repo),
                        DiffKind::File { path, staged } => {
                            ide_core::git::diff::diff_file(&repo, &path, staged).map(|d| vec![d])
                        }
                        DiffKind::Commit { sha } => ide_core::git::commit_diff(&repo, &sha),
                        DiffKind::ConversationFiles {
                            snapshot_id, paths, ..
                        } => {
                            if let Some(snapshot_id) = snapshot_id {
                                load_agent_snapshot_diffs(snapshot_id, &paths)
                            } else {
                                ide_core::git::workspace_worktree_diffs(&repo)
                                    .map(|diffs| filter_conversation_diffs(&repo, &paths, diffs))
                            }
                        }
                    }
                })
                .await;
            this.update(cx, |pane, cx| {
                pane.loading = false;
                match diffs {
                    Ok(diffs) => pane.diffs = Arc::new(diffs),
                    Err(error) => eprintln!("diff load failed: {error:#}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn file_stats(diff: &FileDiff) -> (usize, usize) {
        let mut add = 0;
        let mut remove = 0;
        for hunk in &diff.hunks {
            for line in &hunk.lines {
                match line.origin {
                    LineOrigin::Add => add += 1,
                    LineOrigin::Remove => remove += 1,
                    LineOrigin::Context => {}
                }
            }
        }
        (add, remove)
    }

    fn render_file_header(
        &self,
        ix: usize,
        diff: &FileDiff,
        pane: WeakEntity<Self>,
        cx: &App,
    ) -> impl IntoElement {
        let path = diff.path.clone();
        let collapsed = self.collapsed.contains(&path);
        let (add, remove) = Self::file_stats(diff);
        let file_name: SharedString = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let dir: SharedString = path
            .parent()
            .map(|p| p.display().to_string())
            .filter(|p| !p.is_empty())
            .map(|p| format!("{p}/"))
            .unwrap_or_default()
            .into();
        let toggle_path = path.clone();

        // Stage/unstage only makes sense for working-tree diffs.
        let stage_action: Option<(bool, Entity<GitState>)> = match (&self.kind, &self.git) {
            (DiffKind::Commit { .. } | DiffKind::ConversationFiles { .. }, _) | (_, None) => None,
            (DiffKind::File { staged, .. }, Some(git)) => Some((*staged, git.clone())),
            (DiffKind::Project, Some(git)) => {
                // In the project diff a file may have unstaged parts; offer Stage.
                Some((false, git.clone()))
            }
        };

        let open_center = self.center.clone();
        let open_path = self.repo.join(&path);
        let project = self.project;

        h_flex()
            .id(("diff-file-header", ix))
            .w_full()
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .bg(crate::ui::design::nav(cx))
            .border_b_1()
            .border_color(crate::ui::design::line(cx))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                pane.update(cx, |this, cx| {
                    if !this.collapsed.remove(&toggle_path) {
                        this.collapsed.insert(toggle_path.clone());
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
                .size(crate::ui::design::icon())
                .text_color(crate::ui::design::t3(cx)),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(file_name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .truncate()
                    .child(dir),
            )
            .when(add > 0, |row| {
                row.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::sage(cx))
                        .child(format!("+{add}")),
                )
            })
            .when(remove > 0, |row| {
                row.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(format!("−{remove}")),
                )
            })
            .when_some(stage_action, |row, (staged, git)| {
                let file = path.clone();
                row.child(
                    crate::ui::style::ghost_button_compact(
                        ("stage-file", ix),
                        if staged { "Unstage" } else { "Stage" },
                    )
                    .on_click(move |_, _, cx| {
                        let file = file.clone();
                        git.update(cx, |git, cx| {
                            if staged {
                                git.unstage(file, cx);
                            } else {
                                git.stage(file, cx);
                            }
                        });
                    }),
                )
            })
            .child(
                crate::ui::style::ghost_button_compact(("open-file", ix), "Open File").on_click(
                    move |_, window, cx| {
                        let path = open_path.clone();
                        open_center
                            .update(cx, |center, cx| {
                                center.open_file(project, path, window, cx);
                            })
                            .ok();
                    },
                ),
            )
    }

    fn rows(&self) -> Vec<DiffRow> {
        let mut rows = Vec::new();
        for (file, diff) in self.diffs.iter().enumerate() {
            rows.push(DiffRow::FileHeader(file));
            if self.collapsed.contains(&diff.path) {
                continue;
            }
            if diff.is_binary {
                rows.push(DiffRow::Binary);
            } else if diff.hunks.is_empty() {
                rows.push(DiffRow::Empty);
            } else {
                for (hunk, diff_hunk) in diff.hunks.iter().enumerate() {
                    rows.push(DiffRow::Hunk { file, hunk });
                    rows.extend((0..diff_hunk.lines.len()).map(|line| DiffRow::Line {
                        file,
                        hunk,
                        line,
                    }));
                }
            }
        }
        rows
    }

    fn render_row(&self, row: DiffRow, pane: WeakEntity<Self>, cx: &App) -> gpui::AnyElement {
        match row {
            DiffRow::FileHeader(file) => self
                .diffs
                .get(file)
                .map(|diff| {
                    self.render_file_header(file, diff, pane, cx)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
            DiffRow::Binary => div()
                .w_full()
                .p_3()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("Binary file — no text diff")
                .into_any_element(),
            DiffRow::Empty => div()
                .w_full()
                .p_3()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("No changes")
                .into_any_element(),
            DiffRow::Hunk { file, hunk } => self
                .diffs
                .get(file)
                .and_then(|diff| diff.hunks.get(hunk))
                .map(|hunk| {
                    div()
                        .w_full()
                        .px_2()
                        .py_0p5()
                        .bg(crate::ui::design::surface(cx))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .font_family(crate::ui::design::FONT_MONO)
                        .child(SharedString::from(hunk.header.clone()))
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element()),
            DiffRow::Line { file, hunk, line } => {
                let Some(line) = self
                    .diffs
                    .get(file)
                    .and_then(|diff| diff.hunks.get(hunk))
                    .and_then(|hunk| hunk.lines.get(line))
                else {
                    return div().into_any_element();
                };
                let (bg, marker) = match line.origin {
                    LineOrigin::Add => (Some(crate::ui::design::palette::diff_add_bg(cx)), "+"),
                    LineOrigin::Remove => {
                        (Some(crate::ui::design::palette::diff_remove_bg(cx)), "-")
                    }
                    LineOrigin::Context => (None, " "),
                };
                let old_no: SharedString = line
                    .old_no
                    .map(|n| n.to_string())
                    .unwrap_or_default()
                    .into();
                let new_no: SharedString = line
                    .new_no
                    .map(|n| n.to_string())
                    .unwrap_or_default()
                    .into();
                h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_size(crate::ui::design::text_ui())
                    .when_some(bg, |row, bg| row.bg(bg))
                    .child(
                        div()
                            .w(px(34.))
                            .flex_shrink_0()
                            .text_color(crate::ui::design::t3(cx))
                            .text_right()
                            .child(old_no),
                    )
                    .child(
                        div()
                            .w(px(34.))
                            .flex_shrink_0()
                            .text_color(crate::ui::design::t3(cx))
                            .text_right()
                            .child(new_no),
                    )
                    .child(
                        div()
                            .w(px(10.))
                            .flex_shrink_0()
                            .child(SharedString::from(marker)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .whitespace_nowrap()
                            .child(SharedString::from(line.text.clone())),
                    )
                    .into_any_element()
            }
        }
    }
}

fn load_agent_snapshot_diffs(
    snapshot_id: Uuid,
    paths: &[PathBuf],
) -> anyhow::Result<Vec<FileDiff>> {
    let snapshot = ide_core::local_store::LocalStore::open_default()?
        .load_agent_diff_snapshot(snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("diff snapshot {snapshot_id} not found"))?;
    Ok(filter_conversation_diffs(
        &snapshot.repo_path,
        paths,
        snapshot.files.into_iter().map(|file| file.diff).collect(),
    ))
}

fn filter_conversation_diffs(
    repo: &Path,
    paths: &[PathBuf],
    diffs: Vec<FileDiff>,
) -> Vec<FileDiff> {
    let allowed = paths
        .iter()
        .map(|path| normalize_snapshot_path(repo, path))
        .collect::<HashSet<_>>();
    diffs
        .into_iter()
        .filter(|diff| allowed.contains(&normalize_snapshot_path(repo, &diff.path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_diff_filters_old_snapshots_and_live_fallbacks_to_owned_paths() {
        let repo = Path::new("/repo");
        let diffs = vec![
            FileDiff {
                path: "src/ours.rs".into(),
                ..Default::default()
            },
            FileDiff {
                path: "other-agent.rs".into(),
                ..Default::default()
            },
            FileDiff {
                path: "generated.json".into(),
                ..Default::default()
            },
        ];
        let filtered =
            filter_conversation_diffs(repo, &["/repo/./src/ours.rs".into()], diffs.clone());
        assert_eq!(filtered, vec![diffs[0].clone()]);
        assert!(filter_conversation_diffs(repo, &[], diffs).is_empty());
    }

    #[test]
    fn conversation_diff_tab_keys_include_the_allowed_paths() {
        let kind = |paths| DiffKind::ConversationFiles {
            repo_path: "/repo".into(),
            snapshot_id: Some(Uuid::nil()),
            paths,
        };
        assert_ne!(
            kind(vec!["a.rs".into()]).key(),
            kind(vec!["b.rs".into()]).key()
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let path = std::ffi::OsString::from_vec(vec![b'a', 0xff]);
            assert!(!kind(vec![path.into()]).key().is_empty());
        }
    }

    #[test]
    fn diff_tab_keys_are_scoped_to_the_repository() {
        let kind = DiffKind::File {
            path: PathBuf::from("src/new.rs"),
            staged: false,
        };

        assert_ne!(
            kind.key_for_repo(Path::new("/project")),
            kind.key_for_repo(Path::new("/project-solo"))
        );
    }
}

fn normalize_snapshot_path(repo_path: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(repo_path).unwrap_or(path);
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

impl Render for DiffPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let all_collapsed =
            !self.diffs.is_empty() && self.diffs.iter().all(|d| self.collapsed.contains(&d.path));
        let total: (usize, usize) = self
            .diffs
            .iter()
            .map(Self::file_stats)
            .fold((0, 0), |acc, s| (acc.0 + s.0, acc.1 + s.1));
        let rows = Arc::new(self.rows());
        if self.list_state.item_count() != rows.len() {
            self.list_state.reset(rows.len());
        }
        let weak_pane = cx.entity().downgrade();
        let list_pane = cx.entity();

        v_flex()
            .size_full()
            // Pane toolbar: file count, +/- totals, expand/collapse all.
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!("{} files", self.diffs.len())),
                    )
                    .when(total.0 > 0, |bar| {
                        bar.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::sage(cx))
                                .child(format!("+{}", total.0)),
                        )
                    })
                    .when(total.1 > 0, |bar| {
                        bar.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::rose(cx))
                                .child(format!("−{}", total.1)),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::ghost_button_compact(
                            "toggle-collapse-all",
                            if all_collapsed {
                                "Expand All"
                            } else {
                                "Collapse All"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.diffs.iter().all(|d| this.collapsed.contains(&d.path)) {
                                this.collapsed.clear();
                            } else {
                                this.collapsed =
                                    this.diffs.iter().map(|d| d.path.clone()).collect();
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .id("diff-pane-scroll-wrap")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .when(self.diffs.is_empty(), |pane| {
                        pane.child(
                            v_flex()
                                .w_full()
                                .items_center()
                                .py_8()
                                .gap_2()
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    Icon::new(IconName::CircleCheck)
                                        .size(crate::ui::design::icon_xl())
                                        .text_color(crate::ui::design::sage(cx)),
                                )
                                .child(div().text_size(crate::ui::design::text_body()).child(
                                    if self.loading {
                                        "Loading…"
                                    } else {
                                        "No changes"
                                    },
                                )),
                        )
                    })
                    .when(!self.diffs.is_empty(), |pane| {
                        pane.child(
                            list(self.list_state.clone(), move |row, _, cx| {
                                let Some(row) = rows.get(row).copied() else {
                                    return div().into_any_element();
                                };
                                list_pane.read(cx).render_row(row, weak_pane.clone(), cx)
                            })
                            .size_full(),
                        )
                    }),
            )
    }
}
