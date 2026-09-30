use super::*;
use crate::delegation::{workspace, DelegationRun};

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(root)?;
    anyhow::ensure!(
        !meta.file_type().is_symlink(),
        "Delegation archive contains an unexpected symlink: {}",
        root.display()
    );
    if meta.is_dir() {
        for entry in fs::read_dir(root)? {
            collect_files(&entry?.path(), out)?;
        }
    } else if meta.is_file() {
        out.push(root.to_path_buf());
    }
    Ok(())
}
fn git_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    // History and index only. Never archive hooks, remotes/credentials, caches,
    // ignored environment files, or arbitrary executable Git configuration.
    for name in ["objects", "refs", "HEAD", "index", "packed-refs", "shallow"] {
        collect_files(&root.join(".git").join(name), files)?;
    }
    Ok(())
}

pub(super) fn prepare_delegation_export(
    store: &LocalStore,
    runs: &mut [DelegationRun],
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let managed = store.app_data_dir().join("delegation");
    for run in runs {
        let storage = managed.join(run.id.to_string());
        for task in &mut run.tasks {
            for a in &mut task.attempts {
                if a.workspace.is_dir() && !a.working_copy_cleaned {
                    anyhow::ensure!(
                        a.workspace.starts_with(&storage),
                        "A delegated workspace is outside Choro's managed storage."
                    );
                    a.archive_snapshot = Some(workspace::capture(&a.workspace, &storage)?);
                    git_files(&a.workspace, &mut files)?;
                }
                if let Some(result) = &a.completed_snapshot {
                    let path = storage.join(format!("result-{}", result.id));
                    git_files(&path, &mut files)?;
                }
            }
            if let Some(op) = &mut task.integration {
                if op.resolution_dir.is_dir() {
                    op.archive_snapshot = Some(workspace::capture(&op.resolution_dir, &storage)?);
                    git_files(&op.resolution_dir, &mut files)?;
                }
            }
        }
        collect_files(&storage.join("blobs"), &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

pub(super) fn restore_delegation_export(
    store: &LocalStore,
    runs: &mut [DelegationRun],
    agents: &mut [AgentRecord],
) -> Result<()> {
    for run in runs {
        let storage = store
            .app_data_dir()
            .join("delegation")
            .join(run.id.to_string());
        for task in &mut run.tasks {
            for a in &mut task.attempts {
                a.workspace = storage.join(format!("attempt-{}", a.id));
                for snapshot in [
                    &mut a.snapshot,
                    &mut a.completed_snapshot,
                    &mut a.archive_snapshot,
                ]
                .into_iter()
                .flatten()
                {
                    snapshot.storage = storage.clone();
                }
                if let Some(snapshot) = &a.archive_snapshot {
                    workspace::restore_archived_files(snapshot, &a.workspace)?;
                }
                if let Some(snapshot) = &a.completed_snapshot {
                    workspace::restore_archived_files(
                        snapshot,
                        &storage.join(format!("result-{}", snapshot.id)),
                    )?;
                }
            }
            if let Some(a) = task.attempt() {
                if let Some(agent) = agents.iter_mut().find(|agent| agent.id == a.child_agent_id) {
                    if let Some(binding) = &mut agent.delegation {
                        binding.workspace = Some(a.workspace.clone());
                    }
                }
            }
            if let Some(op) = &mut task.integration {
                op.storage = storage.clone();
                op.resolution_dir = storage.join(format!("integration-{}", op.id));
                if let Some(snapshot) = &mut op.archive_snapshot {
                    snapshot.storage = storage.clone();
                    workspace::restore_archived_files(snapshot, &op.resolution_dir)?;
                }
                op.save_journal()?;
            }
        }
        run.pause(
            "Imported task — verify repository paths and provider sessions before Resume",
            true,
        );
    }
    Ok(())
}
