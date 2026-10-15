//! Delegation archives contain conversation metadata, never project files.
use super::*;
use crate::delegation::DelegationRun;

pub(super) fn prepare_delegation_export(
    _store: &LocalStore, runs: &mut [DelegationRun],
) -> Result<Vec<PathBuf>> {
    for run in runs { run.close_legacy(); }
    Ok(Vec::new())
}

pub(super) fn restore_delegation_export(
    _store: &LocalStore, runs: &mut [DelegationRun], agents: &mut [AgentRecord],
) -> Result<()> {
    for run in runs.iter_mut() {
        run.close_legacy();
        if run.is_shared() && !run.status.terminal() {
            run.pause("Imported assignment — verify the existing working folder before Resume", true);
        }
    }
    for agent in agents {
        if let Some(binding) = agent.delegation.as_mut().filter(|b| runs.iter().any(|r| r.id == b.run_id && !r.is_shared())) {
            if binding.task_id.is_none() { agent.delegation = None; }
            else { binding.execution_version = 0; }
        }
    }
    Ok(())
}
