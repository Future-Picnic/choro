use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use anyhow::{bail, Context as _, Result};
use ide_core::config::AppConfig;
use ide_core::local_store::LocalStore;
use ide_core::{Project, ProjectReferenceKind, ScriptPreset};
use serde::{Deserialize, Serialize};

const MANIFEST_JSON: &str = include_str!("../assets/onboarding/manifest.json");
const ONBOARDING_VERSION_FILE: &str = "onboarding-version";
const ONBOARDING_COMPLETED_FILE: &str = "onboarding-completed";
const ONBOARDING_PROGRESS_FILE: &str = "onboarding-progress.json";

static ONBOARDING_ENABLED: AtomicBool = AtomicBool::new(false);

const PLAYGROUND_FILES: &[(&str, &[u8])] = &[
    (
        "index.html",
        include_bytes!("../assets/onboarding/playground/index.html"),
    ),
    // The finished Field Guide. The starter task's whole job is to copy this
    // into `index.html`, so it has to ship *into the project* — a file that only
    // exists in our source tree is a file the agent correctly refuses to invent.
    (
        "showcase.html",
        include_bytes!("../assets/onboarding/playground/showcase.html"),
    ),
    (
        "styles.css",
        include_bytes!("../assets/onboarding/playground/styles.css"),
    ),
    (
        "scripts/serve.py",
        include_bytes!("../assets/onboarding/playground/scripts/serve.py"),
    ),
    (
        "choro_docs/project-story.choro",
        include_bytes!("../assets/onboarding/playground/choro_docs/project-story.choro"),
    ),
    (
        "README.md",
        include_bytes!("../assets/onboarding/playground/README.md"),
    ),
    (
        ".gitignore",
        include_bytes!("../assets/onboarding/playground/.gitignore"),
    ),
    (
        ".choro-tour/home.html",
        include_bytes!("../assets/onboarding/playground/.choro-tour/home.html"),
    ),
    (
        ".choro-tour/story.html",
        include_bytes!("../assets/onboarding/playground/.choro-tour/story.html"),
    ),
    (
        ".choro-tour/story-complete.html",
        include_bytes!("../assets/onboarding/playground/.choro-tour/story-complete.html"),
    ),
    (
        ".choro-tour/site.css",
        include_bytes!("../assets/onboarding/playground/.choro-tour/site.css"),
    ),
    (
        "assets/choro.png",
        include_bytes!("../assets/app-icon/app-icon-1024.png"),
    ),
];

#[derive(Clone, Deserialize)]
pub struct OnboardingManifest {
    pub version: u32,
    pub project: PlaygroundProject,
    pub first_agent: FirstAgent,
    pub starter_task: StarterTask,
}

#[derive(Clone, Deserialize)]
pub struct PlaygroundProject {
    pub name: String,
    pub folder: String,
    pub icon: String,
    pub icon_color: String,
    pub scripts: Vec<PlaygroundScript>,
    pub preview: PlaygroundPreview,
}

#[derive(Clone, Deserialize)]
pub struct PlaygroundScript {
    pub name: String,
    pub command: String,
}

#[derive(Clone, Deserialize)]
pub struct PlaygroundPreview {
    pub title: String,
    pub url: String,
    pub notes: String,
}

#[derive(Clone, Deserialize)]
pub struct StarterTask {
    pub title: String,
    pub description: String,
}

#[derive(Clone, Deserialize)]
pub struct FirstAgent {
    pub prompt: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct OnboardingProgress {
    pub phase: String,
    /// Legacy single-provider value retained so interrupted older tours resume.
    pub provider: Option<String>,
    #[serde(default)]
    pub providers: Vec<String>,
    pub stack: Vec<String>,
    pub active_agent: Option<String>,
    pub agent_was_active: bool,
}

pub fn enabled() -> bool {
    ONBOARDING_ENABLED.load(Ordering::Relaxed)
}

pub fn manifest() -> &'static OnboardingManifest {
    static MANIFEST: OnceLock<OnboardingManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON).expect("bundled onboarding manifest must be valid")
    })
}

/// Where the playground lives on disk, once `prepare` has run.
pub fn playground_root() -> PathBuf {
    AppConfig::config_root()
        .join("playground")
        .join(&manifest().project.folder)
}

/// Drop the user's picked stack where the served page can fetch it. Written by
/// the tour when they leave the stack question; the finished page reads it and
/// draws their tools pulled into one place. Best-effort — a missing file just
/// means the page shows nothing there.
pub fn write_stack(slugs: &[&str]) {
    let json = format!(
        "{{\"tools\":[{}]}}",
        slugs
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let path = playground_root().join("stack.json");
    let _ = fs::write(path, json);
}

pub fn prepare() -> Result<()> {
    let root = AppConfig::config_root();
    let forced = std::env::var_os("CHORO_ONBOARDING").is_some();
    if forced && std::env::var_os("CHORO_ONBOARDING_RESET").is_some() && root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("failed to reset onboarding data at {}", root.display()))?;
    }

    if !forced && !should_start_for_normal_install(&root) {
        ONBOARDING_ENABLED.store(false, Ordering::Relaxed);
        mark_completed_at(&root)?;
        return Ok(());
    }

    ONBOARDING_ENABLED.store(true, Ordering::Relaxed);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create onboarding root {}", root.display()))?;

    // Write the active marker before creating the workspace. If setup is ever
    // interrupted halfway through, the next launch retries onboarding instead
    // of mistaking the partially-created data for an existing installation.
    fs::write(
        root.join(ONBOARDING_VERSION_FILE),
        manifest().version.to_string(),
    )?;

    let manifest = manifest();
    let store = LocalStore::open_default()?;
    let existing = store.load_workspace_config(AppConfig::load())?;
    if !existing.projects.is_empty() {
        return Ok(());
    }

    let project_root = root.join("playground").join(&manifest.project.folder);
    write_playground(&project_root)?;
    initialize_git_repository(&project_root)?;

    let mut project = Project::from_path(project_root);
    project.name = manifest.project.name.clone();
    project.icon = manifest.project.icon.clone();
    project.icon_color = manifest.project.icon_color.clone();
    project.presets = manifest
        .project
        .scripts
        .iter()
        .map(|script| ScriptPreset::new(script.name.clone(), script.command.clone()))
        .collect();

    let project_id = project.id;
    let mut config = AppConfig::default();
    config.projects.push(project);
    config.active_project = Some(project_id);
    config.expanded_projects.push(project_id);
    store.save_workspace_config(&config)?;
    config.save()?;

    store.create_project_reference(
        project_id,
        ProjectReferenceKind::Url,
        manifest.project.preview.title.clone(),
        manifest.project.preview.url.clone(),
        manifest.project.preview.notes.clone(),
        None,
    )?;
    store.create_personal_task(
        project_id,
        manifest.starter_task.title.clone(),
        manifest.starter_task.description.clone(),
    )?;

    Ok(())
}

/// Finish the first-run experience permanently for this app data directory.
/// The dedicated onboarding bundle resets its isolated directory on launch, so
/// it remains replayable even though completion uses the same production path.
pub fn mark_completed() -> Result<()> {
    let root = AppConfig::config_root();
    mark_completed_at(&root)?;
    let _ = fs::remove_file(root.join(ONBOARDING_PROGRESS_FILE));
    ONBOARDING_ENABLED.store(false, Ordering::Relaxed);
    Ok(())
}

pub fn load_progress() -> Option<OnboardingProgress> {
    load_progress_at(&AppConfig::config_root())
}

pub fn save_progress(progress: &OnboardingProgress) -> Result<()> {
    save_progress_at(&AppConfig::config_root(), progress)
}

fn load_progress_at(root: &Path) -> Option<OnboardingProgress> {
    let json = fs::read_to_string(root.join(ONBOARDING_PROGRESS_FILE)).ok()?;
    serde_json::from_str(&json).ok()
}

fn save_progress_at(root: &Path, progress: &OnboardingProgress) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create app data root {}", root.display()))?;
    let path = root.join(ONBOARDING_PROGRESS_FILE);
    let temp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(progress)?;
    fs::write(&temp, json)
        .with_context(|| format!("failed to write onboarding progress at {}", temp.display()))?;
    fs::rename(&temp, &path)
        .with_context(|| format!("failed to save onboarding progress at {}", path.display()))
}

fn mark_completed_at(root: &Path) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create app data root {}", root.display()))?;
    fs::write(
        root.join(ONBOARDING_COMPLETED_FILE),
        manifest().version.to_string(),
    )
    .with_context(|| {
        format!(
            "failed to record onboarding completion at {}",
            root.display()
        )
    })
}

fn should_start_for_normal_install(root: &Path) -> bool {
    if root.join(ONBOARDING_COMPLETED_FILE).exists() {
        return false;
    }
    if root.join(ONBOARDING_VERSION_FILE).exists() {
        return true;
    }

    // Existing Choro users can have either the legacy JSON workspace, the
    // current SQLite store, or both. Never surprise either group with a tour
    // after an update. A truly clean installation has neither file.
    !root.join("config.json").exists() && !root.join("state.db").exists()
}

fn write_playground(root: &Path) -> Result<()> {
    for (relative, contents) in PLAYGROUND_FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn initialize_git_repository(root: &Path) -> Result<()> {
    run_git(root, &["-c", "init.defaultBranch=main", "init", "-q"])?;
    run_git(root, &["add", "."])?;
    run_git(
        root,
        &[
            "-c",
            "user.name=Choro Playground",
            "-c",
            "user.email=playground@choro.local",
            "commit",
            "-qm",
            "Start Choro Playground",
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_normal_install_starts_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        assert!(should_start_for_normal_install(dir.path()));
    }

    #[test]
    fn existing_json_install_skips_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();
        assert!(!should_start_for_normal_install(dir.path()));
    }

    #[test]
    fn existing_database_install_skips_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("state.db"), []).unwrap();
        assert!(!should_start_for_normal_install(dir.path()));
    }

    #[test]
    fn interrupted_onboarding_resumes_even_after_workspace_creation() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();
        fs::write(dir.path().join(ONBOARDING_VERSION_FILE), "1").unwrap();
        assert!(should_start_for_normal_install(dir.path()));
    }

    #[test]
    fn completed_onboarding_never_reopens() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ONBOARDING_VERSION_FILE), "1").unwrap();
        fs::write(dir.path().join(ONBOARDING_COMPLETED_FILE), "1").unwrap();
        assert!(!should_start_for_normal_install(dir.path()));
    }

    #[test]
    fn onboarding_progress_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let progress = OnboardingProgress {
            phase: "wait_first_agent".into(),
            provider: Some("codex".into()),
            providers: vec!["codex".into(), "claude".into()],
            stack: vec!["git".into(), "docs".into()],
            active_agent: Some("70b37033-47f1-41f6-aa9d-e30021831e9b".into()),
            agent_was_active: true,
        };

        save_progress_at(dir.path(), &progress).unwrap();
        let restored = load_progress_at(dir.path()).unwrap();
        assert_eq!(restored.phase, progress.phase);
        assert_eq!(restored.provider, progress.provider);
        assert_eq!(restored.providers, progress.providers);
        assert_eq!(restored.stack, progress.stack);
        assert_eq!(restored.active_agent, progress.active_agent);
        assert!(restored.agent_was_active);
    }
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run git in {}", root.display()))?;
    if !status.success() {
        bail!(
            "git command failed in {}: git {}",
            root.display(),
            args.join(" ")
        );
    }
    Ok(())
}
