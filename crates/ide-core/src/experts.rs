//! Saved Expert setups. A profile never owns a conversation or private memory.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AgentEffort, AgentKind, AgentModel};

pub mod catalog;
pub mod resources;

#[cfg(test)]
#[path = "experts/catalog_tests.rs"]
mod catalog_tests;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertAdditions {
    #[serde(default)]
    pub builtin_id: Option<String>,
    #[serde(default)]
    pub bundled_skills: Vec<String>,
    #[serde(default)]
    pub custom_skills: Vec<ExpertCustomSkill>,
    #[serde(default)]
    pub riff_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertCustomSkill {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenSkillFile {
    pub content: String,
    pub executable: bool,
}

pub fn normalized_expert_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertSkill {
    pub provider: AgentKind,
    pub name: String,
    pub source: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedExpertSkill {
    pub reference: ExpertSkill,
    pub content: String,
    pub sha256: String,
    /// Full bounded text package, portable across exports and provider sessions.
    #[serde(default)]
    pub files: BTreeMap<String, FrozenSkillFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertProfile {
    pub id: Uuid,
    pub revision: u64,
    pub name: String,
    pub description: String,
    pub provider: AgentKind,
    pub model: AgentModel,
    pub effort: AgentEffort,
    pub instructions: String,
    pub skills: Vec<ExpertSkill>,
    pub expected_outcome: String,
    pub enabled: bool,
    pub archived: bool,
    #[serde(default)]
    pub additions: ExpertAdditions,
}

impl ExpertProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !normalized_expert_name(&self.name).is_empty(),
            "Give the Bandmate a name."
        );
        ensure!(
            self.name.chars().count() <= 80,
            "Bandmate names may contain at most 80 characters."
        );
        ensure!(
            matches!(self.provider, AgentKind::Codex | AgentKind::Claude),
            "Bandmates support Codex and Claude chat runtimes."
        );
        ensure!(
            self.model.belongs_to(self.provider),
            "Choose a model from the Bandmate's provider."
        );
        ensure!(
            self.model.efforts().contains(&self.effort),
            "The selected model does not support this reasoning effort."
        );
        ensure!(
            !self.instructions.trim().is_empty(),
            "Describe the Bandmate's job."
        );
        ensure!(
            self.instructions.len() <= 64_000
                && self.expected_outcome.len() <= 16_000
                && self.description.len() <= 4_000,
            "Bandmate instructions are too long."
        );
        ensure!(
            self.skills.len()
                + self.additions.bundled_skills.len()
                + self.additions.custom_skills.len()
                + self.additions.riff_ids.len()
                <= 32,
            "Choose at most 32 skills for a Bandmate."
        );
        let mut sources = std::collections::HashSet::new();
        for skill in &self.skills {
            ensure!(
                sources.insert(&skill.source),
                "This skill source is selected more than once."
            );
            ensure!(
                skill.provider == self.provider && skill.source.is_absolute(),
                "Choose skills belonging to this Bandmate's provider."
            );
        }
        let mut bundled = std::collections::HashSet::new();
        for id in &self.additions.bundled_skills {
            ensure!(
                bundled.insert(id) && catalog::skill(id).is_some(),
                "A bundled skill is missing or selected more than once."
            );
        }
        let mut custom_ids = std::collections::HashSet::new();
        let mut custom_names = std::collections::HashSet::new();
        for skill in &self.additions.custom_skills {
            ensure!(
                custom_ids.insert(skill.id)
                    && custom_names.insert(normalized_expert_name(&skill.name)),
                "Custom skills must have distinct names and IDs within the Bandmate."
            );
            ensure!(
                !skill.name.trim().is_empty()
                    && skill.name.chars().count() <= 80
                    && !skill.instructions.trim().is_empty()
                    && skill.instructions.len() <= 64_000
                    && skill.description.len() <= 4_000,
                "Give each custom skill a name and bounded instructions."
            );
        }
        let mut riffs = std::collections::HashSet::new();
        ensure!(
            self.additions.riff_ids.iter().all(|id| riffs.insert(id)),
            "This Riff is selected more than once."
        );
        Ok(())
    }

    pub fn snapshot(&self) -> Result<ExpertSnapshot> {
        let config = crate::AppConfig::config_path();
        self.snapshot_at(
            config
                .parent()
                .context("Choro configuration has no directory")?,
        )
    }

    pub fn snapshot_at(&self, config_root: &Path) -> Result<ExpertSnapshot> {
        self.validate()?;
        ensure!(
            self.enabled && !self.archived,
            "This Bandmate is disabled or archived."
        );
        let mut skills = Vec::new();
        let mut content_bytes = 0usize;
        for reference in &self.skills {
            let metadata = std::fs::metadata(&reference.source).with_context(|| {
                format!(
                    "Required skill '{}' is unavailable at {}. Repair the Bandmate in Settings.",
                    reference.name,
                    reference.source.display()
                )
            })?;
            ensure!(
                metadata.is_file() && metadata.len() <= 256_000,
                "Required skill '{}' is not a supported skill file.",
                reference.name
            );
            let files = resources::capture(&reference.source)?;
            let content = files
                .get("SKILL.md")
                .context("Missing skill entrypoint")?
                .content
                .clone();
            content_bytes += content.len();
            ensure!(content_bytes<=128_000,"Selected skill instructions exceed the Bandmate context budget. Remove optional skills before starting.");
            skills.push(ResolvedExpertSkill {
                reference: reference.clone(),
                sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
                content,
                files,
            });
        }
        for id in &self.additions.bundled_skills {
            let entry = catalog::skill(id).context("Bundled skill is unavailable")?;
            let files = catalog::files(id)?;
            let content = files
                .get("SKILL.md")
                .context("Missing bundled entrypoint")?
                .content
                .clone();
            skills.push(ResolvedExpertSkill {
                reference: ExpertSkill {
                    provider: self.provider,
                    name: entry.title.clone(),
                    source: config_root
                        .join("expert-skill-cache")
                        .join(id)
                        .join("SKILL.md"),
                },
                sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
                content,
                files,
            });
        }
        for custom in &self.additions.custom_skills {
            skills.push(resolved_text(
                self.provider,
                &custom.name,
                &format!(
                    "When to use: {}\n\n{}",
                    custom.description, custom.instructions
                ),
                config_root,
            ));
        }
        if !self.additions.riff_ids.is_empty() {
            #[derive(Deserialize)]
            struct Riff {
                id: Uuid,
                name: String,
                instructions: String,
                #[serde(default = "riff_enabled")]
                enabled: bool,
            }
            fn riff_enabled() -> bool {
                true
            }
            #[derive(Deserialize)]
            struct Riffs {
                riffs: Vec<Riff>,
            }
            let path = config_root.join("choro_riffs.json");
            ensure!(
                std::fs::metadata(&path)
                    .context("Linked Riffs are unavailable. Repair the Bandmate in Settings.")?
                    .len()
                    <= 4 * 1024 * 1024,
                "Riff catalog is too large."
            );
            let riffs: Riffs = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            for id in &self.additions.riff_ids {
                let riff = riffs
                    .riffs
                    .iter()
                    .find(|r| r.id == *id && r.enabled)
                    .context(
                        "A linked Riff is missing or disabled. Repair the Bandmate in Settings.",
                    )?;
                skills.push(resolved_text(
                    self.provider,
                    &format!("Riff: {}", riff.name),
                    &riff.instructions,
                    config_root,
                ));
            }
        }
        ensure!(skills.iter().map(|s| s.content.len()).sum::<usize>() <= 128_000, "Selected skill instructions exceed the Bandmate context budget. Remove optional skills before starting.");
        ensure!(
            skills
                .iter()
                .map(|s| s.files.values().map(|f| f.content.len()).sum::<usize>())
                .sum::<usize>()
                <= 8 * 1024 * 1024,
            "Selected skill packages exceed the 8 MiB Bandmate resource budget."
        );
        Ok(ExpertSnapshot {
            profile: self.clone(),
            skills,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpertSnapshot {
    pub profile: ExpertProfile,
    pub skills: Vec<ResolvedExpertSkill>,
}

impl ExpertSnapshot {
    /// Rebuild frozen packages after import/restart before passing their paths to a provider.
    pub fn runtime_instructions(&self, cache: &Path) -> Result<String> {
        ensure!(
            self.skills.len() <= 32
                && self.skills.iter().map(|s| s.content.len()).sum::<usize>() <= 128_000,
            "Bandmate snapshot exceeds the skill instruction budget."
        );
        let mut bytes = 0usize;
        for skill in &self.skills {
            resources::validate_files(&skill.files)?;
            for resource in skill.files.values() {
                bytes = bytes
                    .checked_add(resource.content.len())
                    .context("Bandmate package size overflow")?;
                ensure!(
                    bytes <= 8 * 1024 * 1024,
                    "Bandmate snapshot exceeds the resource budget."
                );
            }
        }
        let mut resolved = self.clone();
        for skill in &mut resolved.skills {
            ensure!(
                format!("{:x}", Sha256::digest(skill.content.as_bytes())) == skill.sha256,
                "Bandmate skill snapshot failed its integrity check."
            );
            if !skill.files.is_empty() {
                ensure!(
                    skill
                        .files
                        .get("SKILL.md")
                        .is_some_and(|f| f.content == skill.content),
                    "Bandmate skill entrypoint disagrees with its frozen package."
                );
                skill.reference.source = resources::materialize(cache, &skill.files)?;
            }
        }
        Ok(resolved.instructions())
    }

    pub fn instructions(&self) -> String {
        let mut text = format!("Bandmate setup: {}\n{}\n\nExpected outcome:\n{}\n\nApply this setup within the user's task and existing project instructions and permissions.\n", self.profile.name, self.profile.instructions, self.profile.expected_outcome);
        for skill in &self.skills {
            text.push_str(&format!(
                "\nSelected skill: {} ({})\nResolve this skill's relative resource and script paths against its containing directory. Apply it only when relevant to this assignment; attached skills do not mandate running every workflow.\n{}\n",
                skill.reference.name,
                skill.reference.source.display(),
                skill.content
            ));
        }
        text
    }
}

fn resolved_text(
    provider: AgentKind,
    name: &str,
    content: &str,
    root: &Path,
) -> ResolvedExpertSkill {
    ResolvedExpertSkill {
        reference: ExpertSkill {
            provider,
            name: name.into(),
            source: root.join("expert-skill-cache/SKILL.md"),
        },
        content: content.into(),
        sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
        files: BTreeMap::from([(
            "SKILL.md".into(),
            FrozenSkillFile {
                content: content.into(),
                executable: false,
            },
        )]),
    }
}

mod names;
pub use names::{expert_aliases, named_experts};

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn profile(name: &str) -> ExpertProfile {
        let model = AgentModel::default_for(AgentKind::Codex);
        ExpertProfile {
            id: Uuid::new_v4(),
            revision: 1,
            name: name.into(),
            description: String::new(),
            provider: AgentKind::Codex,
            model,
            effort: model.default_effort(),
            instructions: "Implement the assigned scope".into(),
            skills: Vec::new(),
            expected_outcome: "A checked result".into(),
            enabled: true,
            archived: false,
            additions: Default::default(),
        }
    }
    #[test]
    fn names_have_boundaries_and_normalized_whitespace() {
        let ui = profile("UI Designer");
        let short = profile("UI");
        assert!(named_experts("build a list", &[short]).unwrap().is_empty());
        assert_eq!(
            named_experts("Ask UI  DESIGNER to help", &[ui.clone()]).unwrap(),
            vec![ui.id]
        );
    }
    #[test]
    fn longer_names_do_not_authorize_overlapping_profiles() {
        let ui = profile("UI");
        let designer = profile("UI Designer");
        let profiles = [ui.clone(), designer.clone()];
        assert_eq!(
            named_experts("Delegate design to UI Designer", &profiles).unwrap(),
            vec![designer.id]
        );
        let both = named_experts("Use UI Designer and ask UI to review", &profiles).unwrap();
        assert_eq!(both.len(), 2);
        assert!(both.contains(&ui.id));
        assert!(both.contains(&designer.id));
    }
    #[test]
    fn snapshots_survive_profile_edits_and_missing_skills_fail() {
        let mut expert = profile("Designer");
        let snapshot = expert.snapshot().unwrap();
        expert.instructions = "Changed".into();
        assert_ne!(snapshot.profile.instructions, expert.instructions);
        expert.skills.push(ExpertSkill {
            provider: expert.provider,
            name: "missing".into(),
            source: PathBuf::from("/choro-nonexistent-skill/SKILL.md"),
        });
        assert!(expert.snapshot().is_err());
    }
}
