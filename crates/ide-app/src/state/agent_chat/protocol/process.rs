use super::*;

pub(super) fn terminate_child_process(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }

    #[cfg(unix)]
    {
        let process_group = child.id() as libc::pid_t;
        let _ = signal_process_group(process_group, libc::SIGHUP);
        std::thread::sleep(Duration::from_millis(120));
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = signal_process_group(process_group, libc::SIGTERM);
        std::thread::sleep(Duration::from_millis(120));
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = signal_process_group(process_group, libc::SIGKILL);
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
pub(super) fn signal_process_group(
    process_group: libc::pid_t,
    signal: libc::c_int,
) -> std::io::Result<()> {
    if process_group <= 1 {
        return Ok(());
    }
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

pub(super) fn find_executable(name: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(&command_path_env()) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    for candidate in common_executable_candidates(name) {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

pub(super) fn find_codex_app_server_executable() -> Option<PathBuf> {
    for candidate in codex_app_server_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    find_executable("codex")
}

pub(super) fn command_path_env() -> String {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let mut entries = Vec::new();

        if let Ok(path) = std::env::var("PATH") {
            push_path_entries(&mut entries, &path);
        }

        for candidate in common_path_entries() {
            push_existing_path(&mut entries, candidate);
        }

        let seed_path = entries
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(":");
        if let Some(shell_path) = login_shell_path(&seed_path) {
            push_path_entries(&mut entries, &shell_path);
        }

        entries
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(":")
    })
    .clone()
}

pub(super) fn login_shell_path(seed_path: &str) -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = Command::new(shell)
        .env("PATH", seed_path)
        .args(["-lc", "printf '%s' \"$PATH\""])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
}

pub(super) fn push_path_entries(entries: &mut Vec<PathBuf>, path: &str) {
    for entry in std::env::split_paths(path) {
        push_existing_path(entries, entry);
    }
}

pub(super) fn push_existing_path(entries: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.is_dir() || entries.iter().any(|entry| entry == &path) {
        return;
    }
    entries.push(path);
}

pub(super) fn common_executable_candidates(name: &str) -> Vec<PathBuf> {
    common_path_entries()
        .into_iter()
        .map(|path| path.join(name))
        .collect()
}

pub(super) fn codex_app_server_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = home_dir() {
        candidates.push(home.join(".codex/plugins/.plugin-appserver/codex"));
        candidates.push(home.join(".codex/packages/standalone/current/bin/codex"));
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".superset/bin/codex"));
    }
    candidates.push(PathBuf::from(
        "/Applications/Codex.app/Contents/Resources/codex",
    ));
    candidates
}

pub(super) fn common_path_entries() -> Vec<PathBuf> {
    let mut entries = vec![
        PathBuf::from("/Applications/Codex.app/Contents/Resources"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ];

    if let Some(home) = home_dir() {
        entries.push(home.join(".opencode/bin"));
        entries.push(home.join(".codex/plugins/.plugin-appserver"));
        entries.push(home.join(".codex/packages/standalone/current/bin"));
        entries.push(home.join(".local/bin"));
        entries.push(home.join(".superset/bin"));
        entries.push(home.join(".npm-global/bin"));
        entries.push(home.join(".bun/bin"));
        entries.push(home.join("Library/pnpm"));
        entries.extend(nvm_bin_paths(&home));
    }

    entries
}

pub(super) fn nvm_bin_paths(home: &Path) -> Vec<PathBuf> {
    let node_versions = home.join(".nvm/versions/node");
    let Ok(version_dirs) = std::fs::read_dir(&node_versions) else {
        return Vec::new();
    };

    let mut bins = version_dirs
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("bin"))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    bins.sort_by(|left, right| right.cmp(left));

    let default_prefix = std::fs::read_to_string(home.join(".nvm/alias/default"))
        .ok()
        .map(|default| default.trim().to_string())
        .filter(|default| !default.is_empty())
        .map(|default| {
            if default.starts_with('v') {
                default
            } else {
                format!("v{default}")
            }
        });

    if let Some(default_prefix) = default_prefix {
        bins.sort_by_key(|path| {
            !path
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&default_prefix))
        });
    }

    bins
}

pub(super) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| {
            let user = std::env::var("USER").ok()?;
            Command::new("/usr/bin/dscl")
                .args([".", "-read", &format!("/Users/{user}"), "NFSHomeDirectory"])
                .output()
                .ok()
                .and_then(|output| {
                    output.status.success().then(|| {
                        String::from_utf8_lossy(&output.stdout)
                            .split_whitespace()
                            .last()
                            .map(PathBuf::from)
                    })?
                })
        })
        .filter(|path| path.is_absolute())
}
