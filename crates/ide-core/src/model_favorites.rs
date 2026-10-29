use serde::{Deserialize, Serialize};

use crate::AgentModel;

/// Stable identity independent of display labels, reasoning effort, and the
/// project whose OpenCode catalog happens to be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFavorite {
    BuiltIn(AgentModel),
    OpenCode(String),
    Codex(String),
}

impl ModelFavorite {
    pub fn new(model: AgentModel, external_id: Option<&str>) -> Self {
        if model == AgentModel::CodexDefault && external_id.is_some() {
            AgentModel::models_for(crate::AgentKind::Codex).iter()
                .find(|known| known.cli_value() == external_id).map(|known| Self::BuiltIn(*known))
                .unwrap_or_else(|| Self::Codex(external_id.unwrap().to_string()))
        } else if model == AgentModel::OpenCode {
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
    fn discovered_codex_favorites_preserve_legacy_keys_and_distinguish_future_models() {
        assert_eq!(ModelFavorite::new(AgentModel::CodexDefault, Some("gpt-6-sol")),
            ModelFavorite::BuiltIn(AgentModel::CodexGpt6Sol));
        let future = ModelFavorite::new(AgentModel::CodexDefault, Some("future-model"));
        assert_ne!(future, ModelFavorite::new(AgentModel::CodexDefault, Some("other-model")));
        assert_eq!(serde_json::from_str::<ModelFavorite>(&serde_json::to_string(&future).unwrap()).unwrap(), future);
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
