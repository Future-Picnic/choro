//! Shared desktop/phone navigation membership. Expansion is deliberately absent.
use crate::{AgentStatus, Project, ProjectId, ProjectSection};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ProjectGroup {
    pub id: String,
    pub name: String,
    pub project_ids: Vec<ProjectId>,
}
pub fn project_groups(projects: &[Project], sections: &[ProjectSection]) -> Vec<ProjectGroup> {
    let mut groups = Vec::new();
    let favorites = projects
        .iter()
        .filter(|p| p.is_favorite)
        .map(|p| p.id)
        .collect::<Vec<_>>();
    if !favorites.is_empty() {
        groups.push(ProjectGroup {
            id: "favorites".into(),
            name: "Favorites".into(),
            project_ids: favorites,
        });
    }
    for section in sections {
        groups.push(ProjectGroup {
            id: section.id.0.to_string(),
            name: section.name.clone(),
            project_ids: projects
                .iter()
                .filter(|p| !p.is_favorite && p.section_id == Some(section.id))
                .map(|p| p.id)
                .collect(),
        });
    }
    let remaining = projects
        .iter()
        .filter(|p| !p.is_favorite && p.section_id.is_none())
        .map(|p| p.id)
        .collect::<Vec<_>>();
    if !remaining.is_empty() {
        groups.push(ProjectGroup {
            id: "projects".into(),
            name: "Projects".into(),
            project_ids: remaining,
        });
    }
    groups
}
pub fn includes_agent(
    status: AgentStatus,
    active_work: bool,
    working: bool,
    waiting: bool,
    expanded: bool,
) -> bool {
    if !active_work {
        return status == AgentStatus::InProgress;
    }
    !status.is_finished() && (working || waiting || (expanded && status == AgentStatus::InProgress))
}
/// Pins are the user's ongoing work list, so finished agents stay pinned until
/// the user unpins them. Waiting agents surface in Attention instead.
pub fn pinned_eligible(pinned: bool, waiting: bool) -> bool {
    pinned && !waiting
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_groups_preserve_membership_order_and_favorites_take_precedence() {
        let section = ProjectSection::new("Design partners");
        let mut favorite = Project::from_path("/fixture/favorite".into());
        favorite.is_favorite = true;
        favorite.section_id = Some(section.id);
        let mut grouped = Project::from_path("/fixture/grouped".into());
        grouped.section_id = Some(section.id);
        let plain = Project::from_path("/fixture/plain".into());
        let groups = project_groups(
            &[plain.clone(), grouped.clone(), favorite.clone()],
            &[section],
        );
        assert_eq!(
            groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["Favorites", "Design partners", "Projects"]
        );
        assert_eq!(groups[0].project_ids, vec![favorite.id]);
        assert_eq!(groups[1].project_ids, vec![grouped.id]);
        assert_eq!(groups[2].project_ids, vec![plain.id]);
    }
    #[test]
    fn attention_does_not_duplicate_pins_and_finished_agents_are_not_active() {
        assert!(!pinned_eligible(true, true));
        assert!(pinned_eligible(true, false));
        assert!(!pinned_eligible(false, false));
        assert!(!includes_agent(AgentStatus::Done, true, true, true, true));
        assert!(includes_agent(
            AgentStatus::InProgress,
            true,
            false,
            true,
            false
        ));
        assert!(!includes_agent(
            AgentStatus::InProgress,
            true,
            false,
            false,
            false
        ));
        assert!(includes_agent(
            AgentStatus::InProgress,
            true,
            false,
            false,
            true
        ));
    }
}
