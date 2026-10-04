//! Ship is allowed with concurrent shared-tree work only after an explicit warning.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ActivityIdentity {
    agent_id: Uuid,
    backend_generation: u64,
    started_at: Option<u64>,
}

#[derive(Clone, Default)]
pub(super) struct ShipActivityConsent {
    acknowledged: HashSet<ActivityIdentity>,
}

#[derive(Clone)]
pub(super) struct ShipActivity {
    identity: ActivityIdentity,
    title: String,
}

fn uses_shared_project_files(agent: &AgentRecord) -> bool {
    !agent.is_active_solo()
        && agent
            .delegation
            .as_ref()
            .and_then(|binding| binding.workspace.as_deref())
            .is_none_or(|workspace| {
                workspace == agent.repository_root() || workspace == agent.project_path
            })
}

fn conflicting_activity(
    shipper: &AgentRecord,
    other: &AgentRecord,
    status: AgentChatStatus,
    started_at: Option<u64>,
    backend_generation: u64,
) -> Option<ShipActivity> {
    (shipper.id != other.id
        && shipper.project_id == other.project_id
        && uses_shared_project_files(shipper)
        && uses_shared_project_files(other)
        && matches!(
            status,
            AgentChatStatus::Running
                | AgentChatStatus::Cancelling
                | AgentChatStatus::WaitingForUser
        ))
    .then(|| ShipActivity {
        identity: ActivityIdentity {
            agent_id: other.id,
            backend_generation,
            started_at,
        },
        title: other.title.clone(),
    })
}

impl ShipActivityConsent {
    fn needs_confirmation(&self, activity: &[ShipActivity]) -> bool {
        activity
            .iter()
            .any(|a| !self.acknowledged.contains(&a.identity))
    }

    fn acknowledging(&self, activity: &[ShipActivity]) -> Self {
        let mut consent = self.clone();
        consent
            .acknowledged
            .extend(activity.iter().map(|a| a.identity.clone()));
        consent
    }
}

impl CenterArea {
    pub(super) fn shared_ship_activity(
        &self,
        agent_id: Uuid,
        cx: &App,
    ) -> Option<Vec<ShipActivity>> {
        let agents = self.agents.read(cx);
        let shipper = agents.agent(agent_id)?;
        let chats = self.agent_chats.read(cx);
        let mut activity = agents
            .iter_records()
            .filter_map(|other| {
                let session = chats.session(other.id)?;
                conflicting_activity(
                    shipper,
                    other,
                    session.status,
                    session.started_running_at,
                    chats.backend_generation(other.id),
                )
            })
            .collect::<Vec<_>>();
        activity.sort_by(|a, b| {
            a.title
                .cmp(&b.title)
                .then_with(|| a.identity.agent_id.cmp(&b.identity.agent_id))
        });
        Some(activity)
    }
}

/// Returns true when execution must stop. Cancel only closes this warning,
/// leaving the underlying Ship dialog and its inputs intact.
pub(super) fn confirm_if_needed<T: 'static>(
    center: &gpui::WeakEntity<CenterArea>,
    agent_id: Uuid,
    consent: &ShipActivityConsent,
    window: &mut Window,
    cx: &mut Context<T>,
    resume: impl Fn(&mut T, ShipActivityConsent, &mut Window, &mut Context<T>) + 'static,
) -> bool {
    let activity = center
        .upgrade()
        .and_then(|center| center.read(cx).shared_ship_activity(agent_id, cx));
    confirm_activity(activity, consent, window, cx, resume)
}

pub(super) fn confirm_activity<T: 'static>(
    activity: Option<Vec<ShipActivity>>,
    consent: &ShipActivityConsent,
    window: &mut Window,
    cx: &mut Context<T>,
    resume: impl Fn(&mut T, ShipActivityConsent, &mut Window, &mut Context<T>) + 'static,
) -> bool {
    let Some(activity) = activity else {
        window.push_notification(
            Notification::info("Could not check active agents. Reopen Ship and try again."),
            cx,
        );
        return true;
    };
    if !consent.needs_confirmation(&activity) {
        return false;
    }
    let consent = consent.acknowledging(&activity);
    let names = activity
        .iter()
        .map(|a| a.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let this = cx.entity().downgrade();
    ConfirmDialog::new(
        "Other agents are still working",
        format!("{names} are using this project's shared files. Shipping can change the branch and commit files while they are editing. Wait for them to finish, or continue anyway."),
    )
    .tone(ConfirmTone::Warning)
    .confirm_label("Continue anyway")
    .cancel_label("Wait")
    .confirm_id("confirm-ship-active-agents")
    .on_confirm(move |window, cx| {
        if let Some(this) = this.upgrade() {
            this.update(cx, |state, cx| resume(state, consent.clone(), window, cx));
        }
    })
    .open(window, cx);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{AgentAccessMode, AgentEffort, AgentKind, AgentModel};

    fn agent(project: ProjectId) -> AgentRecord {
        AgentRecord::new(
            project,
            PathBuf::from("/project"),
            "Working agent",
            "",
            AgentKind::Codex,
            AgentModel::CodexDefault,
            AgentEffort::Medium,
            AgentAccessMode::FullAccess,
        )
    }

    fn running(shipper: &AgentRecord, other: &AgentRecord) -> Option<ShipActivity> {
        conflicting_activity(shipper, other, AgentChatStatus::Running, Some(10), 1)
    }

    #[test]
    fn warning_applies_only_to_other_agents_in_the_same_project() {
        let shipper = agent(ProjectId(Uuid::new_v4()));
        let other = agent(shipper.project_id);
        assert!(running(&shipper, &other).is_some());
        assert!(running(&shipper, &shipper).is_none());
        assert!(running(&shipper, &agent(ProjectId(Uuid::new_v4()))).is_none());
    }

    #[test]
    fn solo_isolation_is_exempt_but_rejoined_agents_use_shared_files() {
        let shipper = agent(ProjectId(Uuid::new_v4()));
        let mut other = agent(shipper.project_id);
        other.solo_branch = Some("solo/feature".into());
        other.lane_path = Some(PathBuf::from("/lanes/feature"));
        assert!(running(&shipper, &other).is_none());
        assert!(running(&other, &shipper).is_none());
        other.solo_rejoined_branch = Some("main".into());
        assert!(
            running(&shipper, &other).is_some(),
            "retained cleanup folder is not isolation"
        );
    }

    #[test]
    fn cancelling_and_waiting_turns_still_need_confirmation() {
        let shipper = agent(ProjectId(Uuid::new_v4()));
        let other = agent(shipper.project_id);
        for status in [
            AgentChatStatus::Running,
            AgentChatStatus::Cancelling,
            AgentChatStatus::WaitingForUser,
        ] {
            assert!(conflicting_activity(&shipper, &other, status, Some(10), 1).is_some());
        }
        for status in [
            AgentChatStatus::Idle,
            AgentChatStatus::PlanReady,
            AgentChatStatus::Failed,
        ] {
            assert!(conflicting_activity(&shipper, &other, status, None, 1).is_none());
        }
    }

    #[test]
    fn managed_agents_in_shared_files_warn_but_isolated_workspaces_do_not() {
        let shipper = agent(ProjectId(Uuid::new_v4()));
        let mut other = agent(shipper.project_id);
        other.hidden_doc_assistant = true;
        other.delegation = Some(ide_core::delegation::DelegationBinding {
            run_id: Uuid::new_v4(),
            parent_agent_id: shipper.id,
            task_id: Some(Uuid::new_v4()),
            attempt_id: None,
            workspace: Some(PathBuf::from("/project")),
            task_kind: None,
        });
        assert!(running(&shipper, &other).is_some());
        other.delegation.as_mut().unwrap().workspace = Some(PathBuf::from("/isolated/task"));
        assert!(running(&shipper, &other).is_none());
    }

    #[test]
    fn accepting_current_activity_does_not_authorize_new_agents_or_turns() {
        let shipper = agent(ProjectId(Uuid::new_v4()));
        let other = agent(shipper.project_id);
        let current = vec![running(&shipper, &other).unwrap()];
        let consent = ShipActivityConsent::default();
        assert!(!consent.needs_confirmation(&[]));
        assert!(consent.needs_confirmation(&current));
        let accepted = consent.acknowledging(&current);
        assert!(!accepted.needs_confirmation(&current));
        let newcomer = running(&shipper, &agent(shipper.project_id)).unwrap();
        assert!(accepted.needs_confirmation(&[current[0].clone(), newcomer]));
        let restarted =
            conflicting_activity(&shipper, &other, AgentChatStatus::Running, Some(20), 1).unwrap();
        assert!(accepted.needs_confirmation(&[restarted]));
        let new_backend =
            conflicting_activity(&shipper, &other, AgentChatStatus::Running, Some(10), 2).unwrap();
        assert!(accepted.needs_confirmation(&[new_backend]));
        assert!(
            ShipActivityConsent::default().needs_confirmation(&current),
            "new Ship attempt asks again"
        );
    }
}
