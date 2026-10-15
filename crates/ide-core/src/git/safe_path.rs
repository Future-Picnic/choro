//! Resolve a relative source path without traversing linked parents.
use std::{fs, path::{Component, Path, PathBuf}};
use anyhow::{ensure, Result};

pub fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Path must stay inside the root directory."
    );
    ensure!(
        !relative
            .components()
            .any(|c| matches!(c, Component::Normal(n) if n == ".git")),
        "Git metadata is outside this file scope."
    );
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if index + 1 < components.len() {
            match fs::symlink_metadata(&current) {
                Ok(metadata) => ensure!(
                    metadata.is_dir() && !metadata.is_symlink(),
                    "A parent directory is a symlink or file: {}",
                    current.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(current)
}

