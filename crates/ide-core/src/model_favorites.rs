use serde::{Deserialize, Serialize};

use crate::AgentModel;

/// Stable identity independent of display labels, reasoning effort, and the
/// project whose OpenCode catalog happens to be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFavorite {
    BuiltIn(AgentModel),
    OpenCode(String),
}

impl ModelFavorite {
    pub fn new(model: AgentModel, external_id: Option<&str>) -> Self {
        if model == AgentModel::OpenCode {
            Self::OpenCode(
                external_id
                    .unwrap_or(crate::config::DEFAULT_OPENCODE_GENERATION_MODEL_ID)
                    .to_string(),
            )
        } else {
            Self::BuiltIn(model)
        }
    }
}

/// Preserve catalog order within each group. Favorites never introduce models
/// that the caller's provider, runtime, or availability rules have excluded.
pub fn favorites_first<T>(
    rows: &mut [T],
    favorites: &[ModelFavorite],
    key: impl Fn(&T) -> ModelFavorite,
) {
    rows.sort_by_key(|row| !favorites.contains(&key(row)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_identity_keeps_provider_qualified_ids_distinct() {
        let first = ModelFavorite::new(AgentModel::OpenCode, Some("anthropic/model"));
        let second = ModelFavorite::new(AgentModel::OpenCode, Some("other/model"));
        assert_ne!(first, second);
        assert_eq!(
            serde_json::from_str::<ModelFavorite>(&serde_json::to_string(&first).unwrap()).unwrap(),
            first
        );
    }

    #[test]
    fn favorites_move_first_without_adding_unavailable_models_or_reordering_peers() {
        let mut models = vec![
            AgentModel::CodexGpt56Sol,
            AgentModel::ClaudeHaiku45,
            AgentModel::CodexGpt6Astra,
        ];
        let favorites = vec![
            ModelFavorite::BuiltIn(AgentModel::CodexGpt6Astra),
            ModelFavorite::OpenCode("unavailable/model".into()),
        ];
        favorites_first(&mut models, &favorites, |model| {
            ModelFavorite::new(*model, None)
        });
        assert_eq!(
            models,
            vec![
                AgentModel::CodexGpt6Astra,
                AgentModel::CodexGpt56Sol,
                AgentModel::ClaudeHaiku45
            ]
        );
    }
}
