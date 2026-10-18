//! Ship-linked GitHub Actions rows. Each row is keyed by its watch, so a merge
//! never shares identity with the conversation or an assignment that shipped it.
use std::collections::HashMap;

use gpui::{px, AnyElement, App, Hsla, IntoElement, Styled};
use gpui_component::IconName;
use ide_core::ship_actions::ShipActionsWatch;
use ide_core::ProjectId;
use uuid::Uuid;

use super::{CompanionAnimation, CompanionItem};
use crate::ui::center::ship_actions_model::{companion_row, ActionsTone, CompanionActionsRow};
use crate::ui::design;

pub(super) fn actions_items<'a>(
    watches: impl IntoIterator<Item = &'a ShipActionsWatch>,
    project_names: &HashMap<ProjectId, String>,
) -> Vec<CompanionItem> {
    watches
        .into_iter()
        .filter_map(|watch| {
            // Watches for removed projects are not polled; leave them out too.
            let project_name = project_names.get(&watch.project_id)?;
            actions_item(watch, project_name)
        })
        .collect()
}

fn actions_item(watch: &ShipActionsWatch, project_name: &str) -> Option<CompanionItem> {
    let row = companion_row(watch)?;
    let origin = watch.latest_origin()?;
    Some(CompanionItem {
        project_id: watch.project_id,
        agent_id: origin.agent_id,
        assignment_id: None,
        title: format!("GitHub Actions · PR #{}", watch.pr_number),
        project_name: project_name.to_owned(),
        status: row.status.clone(),
        animation: actions_animation(&row),
        reviewing: false,
        created_at: watch.updated_at,
        acknowledge_on_open: false,
        actions: Some(row),
    })
}

/// Avatar pose for a row. Muted completions (skipped, cancelled) and offline
/// polling stay calm rather than celebrating or spinning.
pub(super) fn actions_animation(row: &CompanionActionsRow) -> CompanionAnimation {
    if row.attention {
        CompanionAnimation::NeedsAttention
    } else if row.spinning {
        CompanionAnimation::Working
    } else if row.tone == ActionsTone::Success {
        CompanionAnimation::Done
    } else {
        CompanionAnimation::Idle
    }
}

pub(super) fn actions_tone_color(tone: ActionsTone, cx: &App) -> Hsla {
    match tone {
        ActionsTone::Pending => design::amber(cx),
        ActionsTone::Success => design::sage(cx),
        ActionsTone::Failure => design::rose(cx),
        // The companion's glass needs more contrast than a card's t3.
        ActionsTone::Muted => design::t2(cx),
    }
}

/// Spinner only while GitHub is still polled; otherwise a still glyph whose
/// shape, not just its color, carries the state.
pub(super) fn actions_status_glyph(row: &CompanionActionsRow, seed: Uuid, cx: &App) -> AnyElement {
    let color = actions_tone_color(row.tone, cx);
    if row.spinning {
        return crate::ui::logo_spinner::delegation_spinner(
            12.,
            "companion-actions",
            seed.as_u128() as usize,
            color,
        );
    }
    let icon = match row.tone {
        ActionsTone::Failure => IconName::CircleX,
        ActionsTone::Success => IconName::CircleCheck,
        ActionsTone::Pending => IconName::TriangleAlert,
        ActionsTone::Muted => IconName::Minus,
    };
    gpui_component::Icon::new(icon)
        .size(px(12.))
        .text_color(color)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::companion::primary_animation;
    use ide_core::ship_actions::{GithubActionRun, ShipActionsOrigin, TrackingPhase};
    use uuid::Uuid;

    fn watch(
        project: ProjectId,
        agent: Uuid,
        status: &str,
        conclusion: Option<&str>,
    ) -> ShipActionsWatch {
        ShipActionsWatch {
            id: Uuid::new_v4(),
            revision: 1,
            project_id: project,
            project_name: "Stored".into(),
            host: "github.com".into(),
            repository: "owner/repo".into(),
            repository_path: "/tmp/repo".into(),
            pr_number: 12,
            pr_url: "https://github.com/owner/repo/pull/12".into(),
            base_branch: "main".into(),
            merge_sha: Some("abc".into()),
            merged_at: Some(100),
            monitoring_started_at: Some(100),
            phase: TrackingPhase::Tracking,
            origins: vec![ShipActionsOrigin {
                agent_id: agent,
                ship_id: "ship".into(),
                agent_title: "Agent".into(),
                created_at: 10,
            }],
            runs: vec![GithubActionRun {
                id: 1,
                workflow_id: 1,
                name: "Deploy".into(),
                event: "push".into(),
                head_sha: "abc".into(),
                head_branch: "main".into(),
                attempt: 1,
                status: status.into(),
                conclusion: conclusion.map(str::to_owned),
                created_at: 100,
                updated_at: 110,
                started_at: Some(101),
                html_url: "https://github.com/owner/repo/actions/runs/1".into(),
            }],
            discovery_seconds: 0,
            last_checked_at: Some(100),
            refresh_error: None,
            error_count: 0,
            next_poll_at: 100,
            stable_terminal_scans: 0,
            delivered_revision: None,
            acknowledged_revision: None,
            created_at: 90,
            updated_at: 100,
        }
    }

    #[test]
    fn two_merges_from_one_conversation_keep_separate_rows() {
        let project = ProjectId(Uuid::new_v4());
        let agent = Uuid::new_v4();
        let names = HashMap::from([(project, "Choro".to_string())]);
        let first = watch(project, agent, "in_progress", None);
        let second = ShipActionsWatch {
            pr_number: 13,
            ..watch(project, agent, "completed", Some("failure"))
        };
        let items = actions_items([&first, &second], &names);
        assert_eq!(items.len(), 2);
        let ids: Vec<_> = items
            .iter()
            .map(|item| item.actions.as_ref().unwrap().watch_id)
            .collect();
        assert_eq!(ids, vec![first.id, second.id]);
        assert!(items
            .iter()
            .all(|item| item.agent_id == agent && item.assignment_id.is_none()));
        assert!(items.iter().all(|item| !item.acknowledge_on_open));
        assert_eq!(items[1].title, "GitHub Actions · PR #13");
        assert_eq!(items[0].project_name, "Choro");
    }

    #[test]
    fn removed_projects_have_no_rows() {
        let w = watch(
            ProjectId(Uuid::new_v4()),
            Uuid::new_v4(),
            "in_progress",
            None,
        );
        assert!(actions_items([&w], &HashMap::new()).is_empty());
    }

    #[test]
    fn failures_lead_then_agent_work_then_running_actions_then_completions() {
        let project = ProjectId(Uuid::new_v4());
        let names = HashMap::from([(project, "Choro".to_string())]);
        let running = watch(project, Uuid::new_v4(), "in_progress", None);
        let failed = watch(project, Uuid::new_v4(), "completed", Some("failure"));
        let mut done = watch(project, Uuid::new_v4(), "completed", Some("success"));
        done.phase = TrackingPhase::Settled;
        let mut items = actions_items([&done, &running, &failed], &names);
        let agent_work = super::super::tests::item(CompanionAnimation::Working);
        let agent_done = super::super::tests::item(CompanionAnimation::Done);
        items.push(agent_work.clone());
        items.push(agent_done.clone());
        items.sort_by_key(CompanionItem::priority);
        let order: Vec<_> = items
            .iter()
            .map(|item| (item.animation, item.actions.is_some()))
            .collect();
        assert_eq!(order[0], (CompanionAnimation::NeedsAttention, true));
        assert_eq!(order[1], (CompanionAnimation::Working, false));
        assert_eq!(order[2], (CompanionAnimation::Working, true));
        assert_eq!(order[3].0, CompanionAnimation::Done);
        assert_eq!(order[4].0, CompanionAnimation::Done);

        // An old completion never hides live work in the avatar.
        assert_eq!(
            primary_animation(&[agent_done.clone(), agent_work], None),
            CompanionAnimation::Working
        );
        let running_actions = actions_items([&running], &names);
        assert_eq!(
            primary_animation(&[agent_done, running_actions[0].clone()], None),
            CompanionAnimation::Working
        );
    }

    #[test]
    fn cancelled_or_offline_rows_do_not_celebrate_or_spin() {
        let project = ProjectId(Uuid::new_v4());
        let names = HashMap::from([(project, "Choro".to_string())]);
        let mut cancelled = watch(project, Uuid::new_v4(), "completed", Some("cancelled"));
        cancelled.phase = TrackingPhase::Settled;
        let item = actions_items([&cancelled], &names).remove(0);
        assert_eq!(item.animation, CompanionAnimation::Idle);
        assert_eq!(item.actions.as_ref().unwrap().tone, ActionsTone::Muted);

        let mut offline = watch(project, Uuid::new_v4(), "in_progress", None);
        offline.refresh_error = Some("offline".into());
        let item = actions_items([&offline], &names).remove(0);
        assert_eq!(item.animation, CompanionAnimation::Idle);
        assert_eq!(item.status, "Unable to refresh");
    }
}
