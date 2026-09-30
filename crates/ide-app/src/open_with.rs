//! "Open With" — hand project paths to macOS apps and Finder.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One installed app the project can be opened in.
#[derive(Clone)]
pub struct ExternalApp {
    /// Menu label.
    pub label: &'static str,
    /// Name passed to `open -a`; None opens in Finder.
    pub app: Option<&'static str>,
}

/// (label, `open -a` name, .app bundle name) — checked against the standard
/// application directories.
const CANDIDATES: &[(&str, &str)] = &[
    ("VS Code", "Visual Studio Code"),
    ("Cursor", "Cursor"),
    ("Zed", "Zed"),
    ("Sublime Text", "Sublime Text"),
    ("IntelliJ IDEA", "IntelliJ IDEA"),
    ("WebStorm", "WebStorm"),
    ("Xcode", "Xcode"),
    ("Android Studio", "Android Studio"),
    ("Warp", "Warp"),
    ("iTerm", "iTerm"),
    ("Terminal", "Terminal"),
];

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// Apps actually installed on this machine, plus Finder (always available).
pub fn available_apps() -> Vec<ExternalApp> {
    let dirs = app_dirs();
    let mut apps: Vec<ExternalApp> = CANDIDATES
        .iter()
        .filter(|(_, app)| {
            dirs.iter()
                .any(|dir| dir.join(format!("{app}.app")).exists())
        })
        .map(|(label, app)| ExternalApp {
            label,
            app: Some(app),
        })
        .collect();
    apps.push(ExternalApp {
        label: "Finder",
        app: None,
    });
    apps
}

/// Opens `path` in the app (or Finder when `app` is None). Fire-and-forget.
pub fn open_in(app: Option<&str>, path: &str) {
    let mut cmd = Command::new("open");
    if let Some(app) = app {
        cmd.arg("-a").arg(app);
    }
    cmd.arg("--");
    cmd.arg(path);
    if let Err(error) = cmd.spawn() {
        eprintln!("open with failed: {error}");
    }
}

/// Reveals a file or directory in Finder without opening it.
pub fn reveal_in_finder(path: &Path) {
    let mut cmd = Command::new("open");
    cmd.arg("-R").arg("--").arg(path);
    if let Err(error) = cmd.spawn() {
        eprintln!("reveal in Finder failed: {error}");
    }
}

/// Opens a file using the app registered for its type.
pub fn open_default(path: &Path) {
    let mut cmd = Command::new("open");
    cmd.arg("--").arg(path);
    if let Err(error) = cmd.spawn() {
        eprintln!("open in default app failed: {error}");
    }
}

/// Opens Terminal with `path` as its working directory.
pub fn open_in_terminal(path: &Path) {
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg("Terminal").arg("--").arg(path);
    if let Err(error) = cmd.spawn() {
        eprintln!("open in Terminal failed: {error}");
    }
}

/// Moves a path to the user's Trash through Finder.
///
/// Finder handles name collisions and volumes correctly, which a direct move
/// into `~/.Trash` would not.
pub fn move_to_trash(path: &Path) -> anyhow::Result<()> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg("on run argv")
        .arg("-e")
        .arg("tell application \"Finder\"")
        .arg("-e")
        .arg("delete POSIX file (item 1 of argv)")
        .arg("-e")
        .arg("end tell")
        .arg("-e")
        .arg("end run")
        .arg("--")
        .arg(path)
        .status()?;
    anyhow::ensure!(status.success(), "Finder could not move the item to Trash");
    Ok(())
}
