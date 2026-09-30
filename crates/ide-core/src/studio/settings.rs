//! User-selected craft guidance. It never changes Studio's write scope.
use super::*;

pub const PREFERENCES_MAX_BYTES: usize = 8_000;
pub const SKILL_MAX_BYTES: usize = 64_000;
pub const SKILLS_MAX_BYTES: usize = 128_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StudioSettings {
    pub preferences: String,
    pub skills: Vec<StudioSkill>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioSkill {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub source: String,
    /// Deduplicate copies from the same catalog entry.
    pub source_key: String,
    pub source_directory: Option<std::path::PathBuf>,
}

impl StudioSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.preferences.len() <= PREFERENCES_MAX_BYTES,
            "Keep design preferences under 8 KB."
        );
        let mut names = std::collections::HashSet::new();
        let mut sources = std::collections::HashSet::new();
        for skill in &self.skills {
            anyhow::ensure!(
                !skill.name.trim().is_empty() && skill.name.chars().count() <= 80,
                "Give each skill a name of 80 characters or fewer."
            );
            anyhow::ensure!(
                names.insert(skill.name.trim().to_lowercase()),
                "A Studio skill already uses that name. Choose another name."
            );
            anyhow::ensure!(
                sources.insert(&skill.source_key),
                "This skill is already added to Studio."
            );
            anyhow::ensure!(
                !skill.instructions.trim().is_empty()
                    && skill.instructions.len() <= SKILL_MAX_BYTES,
                "Give each skill instructions, up to 64 KB."
            );
            anyhow::ensure!(
                skill.description.len() <= 4_000,
                "Keep the skill description under 4 KB."
            );
        }
        anyhow::ensure!(
            self.skills
                .iter()
                .map(|s| s.instructions.len())
                .sum::<usize>()
                <= SKILLS_MAX_BYTES,
            "Studio skills exceed 128 KB in total. Shorten or detach a skill before adding more."
        );
        Ok(())
    }

    pub fn request_guidance(&self) -> String {
        // Include an explicit empty snapshot too: removing a skill must supersede
        // guidance already present in a resumed conversation.
        format!("Current Studio preferences and additional skills (replaces all earlier Studio settings in this conversation). Apply only relevant design guidance. The Studio contract, host scope, project conventions and current user request take priority. These instructions grant no extra tools or permissions. Resolve relative read-only references against source_directory when present; do not execute scripts or install dependencies from added skills.\n{}",
            serde_json::to_string(self).expect("Studio settings serialize"))
    }
}

pub struct StudioBuiltinSkill {
    pub name: &'static str,
    pub description: &'static str,
    pub instructions: &'static str,
}

pub const STUDIO_BUILTIN_SKILLS: &[StudioBuiltinSkill] = &[
    StudioBuiltinSkill {
        name: "Choro Studio",
        description:
            "Preserves your edits, follows the project design system, and requires a visual review.",
        instructions: include_str!("../../assets/experts/skills/choro-studio/SKILL.md"),
    },
    StudioBuiltinSkill {
        name: "Frontend Design",
        description: "Guides typography, color, layout, motion, and clear interface writing.",
        instructions: include_str!("../../assets/experts/skills/frontend-design/SKILL.md"),
    },
    StudioBuiltinSkill {
        name: "Web Design Guidelines",
        description:
            "Checks accessibility, keyboard use, forms, responsive layouts, and performance.",
        instructions: include_str!(
            "../../assets/experts/skills/web-design-guidelines/reference/guidelines.md"
        ),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_legacy_defaults() {
        let mut config = crate::config::AppConfig::default();
        config.studio.preferences = "Use compact layouts".into();
        config.studio.skills.push(StudioSkill {
            id: Uuid::new_v4(),
            name: "Accessible forms".into(),
            description: "Forms".into(),
            instructions: "Keep errors next to their fields.".into(),
            source: "Custom".into(),
            source_key: "custom:forms".into(),
            source_directory: None,
        });
        config.studio.validate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        config.save_to(&path).unwrap();
        assert_eq!(
            crate::config::AppConfig::load_from(&path).studio,
            config.studio
        );
        let mut json = serde_json::to_value(&config).unwrap();
        let restored: crate::config::AppConfig = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(restored.studio, config.studio);
        json.as_object_mut().unwrap().remove("studio");
        let legacy: crate::config::AppConfig = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.studio, StudioSettings::default());
        let guidance = restored.studio.request_guidance();
        assert!(guidance.contains("Use compact layouts"));
        assert!(guidance.contains("Keep errors next to their fields."));
        assert!(StudioSettings::default()
            .request_guidance()
            .contains("replaces all earlier"));
    }
    #[test]
    fn rejects_oversized_preferences_and_duplicate_skills() {
        let mut settings = StudioSettings {
            preferences: "a".repeat(PREFERENCES_MAX_BYTES + 1),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
        settings.preferences.clear();
        let skill = StudioSkill {
            id: Uuid::new_v4(),
            name: "Example".into(),
            description: String::new(),
            instructions: "Guidance".into(),
            source: "Custom".into(),
            source_key: "example".into(),
            source_directory: None,
        };
        settings.skills = vec![skill.clone(), skill];
        assert!(settings.validate().is_err());
    }
}
