//! Versioned, offline skill packages compiled into both the app and MCP binary.
use super::*;
use std::{collections::BTreeMap, sync::OnceLock};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/expert_assets.rs"));
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExpertCatalog {
    pub version: u64,
    pub researched_on: String,
    pub skills: Vec<CatalogSkill>,
    pub experts: Vec<CatalogExpert>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CatalogSkill {
    pub id: String,
    pub title: String,
    pub description: String,
    pub upstream: String,
    pub repository: String,
    pub upstream_revision: String,
    pub upstream_sha256: String,
    pub repository_stars: u64,
    pub license: String,
    pub edition: String,
    pub changes: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CatalogExpert {
    #[serde(default)]
    pub aliases: Vec<String>,
    pub id: Uuid,
    pub builtin_id: String,
    pub name: String,
    pub description: String,
    pub provider: AgentKind,
    pub model: AgentModel,
    pub effort: AgentEffort,
    pub skills: Vec<String>,
    pub instructions: String,
    pub expected_outcome: String,
}

pub fn catalog() -> &'static ExpertCatalog {
    static CATALOG: OnceLock<ExpertCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/experts/catalog.json"))
            .expect("validated built-in Bandmate catalog")
    })
}

pub fn skill(id: &str) -> Option<&'static CatalogSkill> {
    catalog().skills.iter().find(|s| s.id == id)
}

pub fn files(id: &str) -> Result<BTreeMap<String, FrozenSkillFile>> {
    ensure!(
        skill(id).is_some(),
        "Bundled skill '{id}' is unavailable. Repair the Bandmate in Settings."
    );
    let prefix = format!("skills/{id}/");
    let files = embedded::ASSETS
        .iter()
        .filter_map(|(path, content)| {
            path.strip_prefix(&prefix).map(|relative| {
                (
                    relative.to_owned(),
                    FrozenSkillFile {
                        content: (*content).to_owned(),
                        executable: false,
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        files.contains_key("SKILL.md"),
        "Bundled skill '{id}' has no entrypoint."
    );
    Ok(files)
}

impl CatalogExpert {
    pub fn profile(&self) -> ExpertProfile {
        ExpertProfile {
            id: self.id,
            revision: 0,
            name: self.name.clone(),
            description: self.description.clone(),
            provider: self.provider,
            model: self.model,
            effort: self.effort,
            instructions: self.instructions.clone(),
            skills: Vec::new(),
            expected_outcome: self.expected_outcome.clone(),
            enabled: true,
            archived: false,
            additions: ExpertAdditions {
                builtin_id: Some(self.builtin_id.clone()),
                bundled_skills: self.skills.clone(),
                ..Default::default()
            },
        }
    }
}
