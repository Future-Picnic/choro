use super::*;
use anyhow::Context as _;

#[derive(Clone)]
struct PublishFileChoice {
    name: String,
    included: bool,
}

pub(super) struct PublishRepositoryDialog {
    git: Entity<GitState>,
    repository_name: Entity<InputState>,
    private: bool,
    offers_file_selection: bool,
    files: Vec<PublishFileChoice>,
    error: Option<String>,
}

impl PublishRepositoryDialog {
    fn new(git: Entity<GitState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repo_path = git.read(cx).repo_path.clone();
        let default_name = repo_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(sanitize_repository_name)
            .unwrap_or_default();
        let repository_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Repository name")
                .default_value(default_name)
        });
        cx.subscribe(&repository_name, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        let offers_file_selection = !repo_path.join(".gitignore").exists();
        let files = if offers_file_selection {
            top_level_publish_choices(&repo_path)
        } else {
            Vec::new()
        };

        Self {
            git,
            repository_name,
            private: true,
            offers_file_selection,
            files,
            error: None,
        }
    }

    fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repository_name =
            sanitize_repository_name(self.repository_name.read(cx).value().trim());
        if repository_name.is_empty() {
            self.error = Some("Enter a repository name".into());
            cx.notify();
            return;
        }
        if self.offers_file_selection && !self.files.iter().any(|file| file.included) {
            self.error = Some("Select at least one file or folder to publish".into());
            cx.notify();
            return;
        }

        let excluded: Vec<String> = self
            .files
            .iter()
            .filter(|file| !file.included)
            .map(|file| file.name.clone())
            .collect();
        let private = self.private;
        self.git.update(cx, move |git, cx| {
            git.run_repository_setup(
                move |repo| {
                    publish_repository_to_github(&repo, &repository_name, private, &excluded)
                },
                cx,
            );
        });
        window.close_dialog(cx);
    }
}

impl Render for PublishRepositoryDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = cx.entity();
        let repository_name =
            sanitize_repository_name(self.repository_name.read(cx).value().trim());
        let has_included_file = self.files.iter().any(|file| file.included);
        let can_publish =
            !repository_name.is_empty() && (!self.offers_file_selection || has_included_file);
        let visibility_label = if self.private {
            "Private repository"
        } else {
            "Public repository"
        };

        v_flex()
            .w_full()
            .gap_4()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("Repository name"),
                    )
                    .child(Input::new(&self.repository_name)),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("Visibility"),
                    )
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "publish-repository-visibility",
                            visibility_label,
                            cx,
                        )
                        .w_full()
                        .dropdown_caret(true)
                        .dropdown_menu({
                            let private_dialog = dialog.clone();
                            let public_dialog = dialog.clone();
                            let private = self.private;
                            move |menu, window, _| {
                                menu.item(
                                    PopupMenuItem::new("Private repository")
                                        .checked(private)
                                        .on_click(window.listener_for(
                                            &private_dialog,
                                            |this, _, _, cx| {
                                                this.private = true;
                                                this.error = None;
                                                cx.notify();
                                            },
                                        )),
                                )
                                .item(
                                    PopupMenuItem::new("Public repository")
                                        .checked(!private)
                                        .on_click(window.listener_for(
                                            &public_dialog,
                                            |this, _, _, cx| {
                                                this.private = false;
                                                this.error = None;
                                                cx.notify();
                                            },
                                        )),
                                )
                            }
                        }),
                    ),
            )
            .when(self.offers_file_selection, |content| {
                content.child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Files to include"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Excluded items are added to a new .gitignore."),
                                ),
                        )
                        .child(
                            v_flex()
                                .id("publish-files-scroll")
                                .w_full()
                                .max_h(px(184.))
                                .overflow_y_scroll()
                                .rounded(crate::ui::design::r_sm())
                                .border_1()
                                .border_color(crate::ui::design::line(cx))
                                .children(self.files.iter().enumerate().map(|(index, file)| {
                                    let included = file.included;
                                    let name = SharedString::from(file.name.clone());
                                    h_flex()
                                        .id(SharedString::from(format!(
                                            "publish-file-choice-{index}"
                                        )))
                                        .w_full()
                                        .items_center()
                                        .gap_2()
                                        .px_3()
                                        .py_1p5()
                                        .cursor_pointer()
                                        .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(file) = this.files.get_mut(index) {
                                                file.included = !file.included;
                                                this.error = None;
                                                cx.notify();
                                            }
                                        }))
                                        .child(crate::ui::style::checkbox(
                                            SharedString::from(format!(
                                                "publish-file-checkbox-{index}"
                                            )),
                                            included,
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t1(cx))
                                                .child(name),
                                        )
                                })),
                        ),
                )
            })
            .when_some(self.error.clone(), |content, error| {
                content.child(
                    div()
                        .w_full()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.45))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(SharedString::from(error)),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "cancel-publish-repository",
                            "Cancel",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::ui::style::primary_button_compact(
                            "confirm-publish-repository",
                            "Publish Repository",
                            cx,
                        )
                        .icon(IconName::GitHub)
                        .disabled(!can_publish)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.publish(window, cx);
                        })),
                    ),
            )
    }
}

pub(super) fn open_publish_repository_dialog(
    git: Entity<GitState>,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = cx.new(|cx| PublishRepositoryDialog::new(git, window, cx));
    let input = dialog.read(cx).repository_name.clone();
    window.open_dialog(cx, move |dialog_view, _, _| {
        dialog_view
            .title("Publish to GitHub")
            .w(px(520.))
            .overlay_closable(false)
            .child(dialog.clone())
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

fn top_level_publish_choices(repo: &Path) -> Vec<PublishFileChoice> {
    let mut files: Vec<_> = std::fs::read_dir(repo)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != ".git")
        .map(|name| PublishFileChoice {
            name,
            included: true,
        })
        .collect();
    files.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    files
}

fn sensitive_publish_paths(repo: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, found: &mut Vec<PathBuf>) {
        if found.len() >= 20 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            if found.len() >= 20 {
                break;
            }
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            if relative
                .components()
                .next()
                .is_some_and(|part| part.as_os_str() == ".git")
            {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // Do not follow directory symlinks outside the chosen project.
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                // These trees are generated locally, can contain thousands of
                // files, and are excluded before the initial publish. Do not
                // recursively scan their downloaded contents for project
                // secrets.
                if ide_core::git::is_generated_tool_path(relative) {
                    continue;
                }
                visit(root, &path, found);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase);
            let sensitive_name = name == ".env"
                || name.starts_with(".env.")
                || matches!(
                    name.as_str(),
                    "id_rsa"
                        | "id_dsa"
                        | "id_ecdsa"
                        | "id_ed25519"
                        | "credentials.json"
                        | "service-account.json"
                )
                || extension
                    .as_deref()
                    .is_some_and(|extension| matches!(extension, "pem" | "key" | "p12" | "pfx"));
            if sensitive_name {
                found.push(relative.to_path_buf());
            }
        }
    }

    let mut found = Vec::new();
    visit(repo, repo, &mut found);
    found.sort();
    found
}

fn git_path_is_ignored(repo: &Path, relative: &Path) -> bool {
    let _git_permit = ide_core::git::BackgroundGitPermit::acquire();
    Command::new("git")
        .args(["check-ignore", "--quiet", "--no-index", "--"])
        .arg(relative)
        .current_dir(repo)
        .status()
        .is_ok_and(|status| status.success())
}

fn sanitize_repository_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn publish_repository_to_github(
    repo: &Path,
    repository_name: &str,
    private: bool,
    excluded: &[String],
) -> anyhow::Result<Option<String>> {
    write_publish_gitignore(repo, excluded)?;

    git_output(repo, &["-c", "init.defaultBranch=main", "init"])?;
    ide_core::git::ensure_local_dependency_excludes(repo)
        .context("Failed to install Choro's local generated-file safety rules")?;
    let exposed_secrets = sensitive_publish_paths(repo)
        .into_iter()
        .filter(|path| !git_path_is_ignored(repo, path))
        .collect::<Vec<_>>();
    if !exposed_secrets.is_empty() {
        let names = exposed_secrets
            .iter()
            .take(8)
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Potential secret files would be published: {names}. Add them to .gitignore, then try again."
        );
    }
    git_output(repo, &["add", "--all"])?;
    git_output(repo, &["commit", "--message", "first commit"])?;

    let source = repo.to_string_lossy().to_string();
    let visibility = if private { "--private" } else { "--public" };
    let output = gh_command()?
        .args([
            "repo",
            "create",
            repository_name,
            "--source",
            &source,
            visibility,
            "--remote",
            "origin",
            "--push",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("Failed to run GitHub CLI")?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(if error.is_empty() {
            "Failed to publish repository with GitHub CLI".to_string()
        } else {
            error
        });
    }

    Ok(Some(format!("Published {repository_name} to GitHub")))
}

fn write_publish_gitignore(repo: &Path, excluded: &[String]) -> anyhow::Result<()> {
    if excluded.is_empty() {
        return Ok(());
    }

    let gitignore_path = repo.join(".gitignore");
    if gitignore_path.exists() {
        anyhow::bail!(
            ".gitignore was created while publishing; review it and try again so Choro does not overwrite it"
        );
    }
    let contents: String = excluded.iter().map(|name| format!("/{name}\n")).collect();
    std::fs::write(&gitignore_path, contents)
        .with_context(|| format!("Failed to write {}", gitignore_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        sanitize_repository_name, sensitive_publish_paths, top_level_publish_choices,
        write_publish_gitignore,
    };

    #[test]
    fn repository_name_matches_vscode_style_sanitizing() {
        assert_eq!(sanitize_repository_name(" My App! "), "My-App-");
        assert_eq!(sanitize_repository_name("choro_tools.v2"), "choro_tools.v2");
    }

    #[test]
    fn publish_choices_are_sorted_and_git_metadata_is_hidden() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(".git")).unwrap();
        std::fs::write(directory.path().join("zeta.txt"), "z").unwrap();
        std::fs::write(directory.path().join("Alpha.txt"), "a").unwrap();

        let choices = top_level_publish_choices(directory.path());
        let names: Vec<_> = choices.into_iter().map(|choice| choice.name).collect();
        assert_eq!(names, ["Alpha.txt", "zeta.txt"]);
    }

    #[test]
    fn excluded_items_create_a_root_anchored_gitignore_without_clobbering() {
        let directory = tempfile::tempdir().unwrap();
        let excluded = vec!["node_modules".to_string(), "local.env".to_string()];

        write_publish_gitignore(directory.path(), &excluded).unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join(".gitignore")).unwrap(),
            "/node_modules\n/local.env\n"
        );
        assert!(write_publish_gitignore(directory.path(), &excluded).is_err());
    }

    #[test]
    fn nested_secret_files_are_detected_without_following_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("config/nested")).unwrap();
        std::fs::write(
            directory.path().join("config/nested/.env.production"),
            "TOKEN=x",
        )
        .unwrap();
        std::fs::write(directory.path().join("config/private.pem"), "private").unwrap();
        std::fs::write(directory.path().join("config/public.json"), "{}").unwrap();

        let paths = sensitive_publish_paths(directory.path());
        assert_eq!(
            paths,
            [
                PathBuf::from("config/nested/.env.production"),
                PathBuf::from("config/private.pem"),
            ]
        );
    }
}
