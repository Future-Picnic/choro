//! Codex supplies model identities and capabilities. Static enum variants are
//! retained for old records, never used as evidence that an account has access.
use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::AgentEffort;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexModel {
    pub id: String,
    pub name: String,
    pub efforts: Vec<AgentEffort>,
    pub default_effort: Option<AgentEffort>,
    pub is_default: bool,
}

impl CodexModel {
    pub fn normalize_effort(&self, requested: AgentEffort) -> Option<AgentEffort> {
        self.efforts
            .contains(&requested)
            .then_some(requested)
            .or(self
                .default_effort
                .filter(|effort| self.efforts.contains(effort)))
            .or_else(|| self.efforts.first().copied())
    }

    pub fn variants(&self) -> Vec<String> {
        self.efforts
            .iter()
            .map(|effort| effort.cli_value().to_string())
            .collect()
    }
}

pub fn parse_model_page(page: &Value) -> Result<(Vec<CodexModel>, Option<String>)> {
    let entries = page
        .get("data")
        .and_then(Value::as_array)
        .context("Codex returned an invalid model catalog. Update Codex and refresh models.")?;
    let mut models = Vec::new();
    for entry in entries {
        if entry.get("hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let id = entry
            .get("model")
            .or_else(|| entry.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .context("Codex returned a model without an identifier.")?;
        let name = entry
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(id);
        let efforts = entry
            .get("supportedReasoningEfforts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|effort| {
                AgentEffort::from_cli_value(effort.get("reasoningEffort")?.as_str()?)
            })
            .collect::<Vec<_>>();
        let default_effort = entry
            .get("defaultReasoningEffort")
            .and_then(Value::as_str)
            .and_then(AgentEffort::from_cli_value);
        models.push(CodexModel {
            id: id.into(),
            name: name.into(),
            efforts,
            default_effort,
            is_default: entry
                .get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let cursor = match page.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) if !cursor.is_empty() => Some(cursor.clone()),
        _ => bail!("Codex returned an invalid model catalog cursor."),
    };
    Ok((models, cursor))
}

/// An unavailable saved choice resolves to the provider's recommendation, not
/// to a guessed older model. The caller must report and persist the resolution.
pub fn resolve_model<'a>(
    models: &'a [CodexModel],
    requested: Option<&str>,
) -> Result<&'a CodexModel> {
    models.iter().find(|model| Some(model.id.as_str()) == requested)
        .or_else(|| models.iter().find(|model| model.is_default))
        .or_else(|| models.first())
        .context("Codex returned no available models. Check your Codex sign-in, update Codex, and refresh models.")
}

pub fn model_unavailable(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("model_not_found")
        || (lower.contains("model")
            && (lower.contains("not supported")
                || lower.contains("not found")
                || lower.contains("does not exist")
                || lower.contains("do not have access")))
}

pub fn readable_error(message: &str) -> String {
    // Some app-server releases wrap a JSON HTTP error inside error.message.
    let decoded = serde_json::from_str::<Value>(message).ok();
    let detail = decoded
        .as_ref()
        .and_then(|error| {
            error
                .pointer("/error/message")
                .or_else(|| error.get("message"))
                .and_then(Value::as_str)
        })
        .unwrap_or(message);
    let lower = detail.to_ascii_lowercase();
    let recovery = if model_unavailable(message) {
        Some("Choose Automatic or another model in the model picker and resend your message. If models are missing, update Codex and refresh models.")
    } else if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("refresh token")
        || lower.contains("not logged in")
    {
        Some("Sign in again with `codex login`, then retry in this conversation.")
    } else {
        None
    };
    let detail = crate::redact_sensitive_text(detail);
    match recovery {
        Some(recovery) => format!("{detail}\n\n{recovery}"),
        None => detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn future_models_keep_wire_ids_and_provider_capabilities() {
        let (models, cursor) = parse_model_page(&json!({"data": [
            {"model":"future-model", "displayName":"Future", "isDefault":true,
             "supportedReasoningEfforts":[{"reasoningEffort":"low"}], "defaultReasoningEffort":"low"},
            {"model":"hidden-model", "hidden":true}
        ], "nextCursor":"next"})).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "future-model");
        assert_eq!(
            models[0].normalize_effort(AgentEffort::Max),
            Some(AgentEffort::Low)
        );
        assert_eq!(cursor.as_deref(), Some("next"));
        assert_eq!(
            resolve_model(&models, Some("retired-model")).unwrap().id,
            "future-model"
        );
    }

    #[test]
    fn empty_or_invalid_catalogs_never_invent_models_or_efforts() {
        assert!(parse_model_page(&json!({})).is_err());
        assert!(parse_model_page(&json!({"data":[{}]})).is_err());
        assert!(resolve_model(&[], None).is_err());
        let (models, _) = parse_model_page(&json!({"data":[{"model":"no-reasoning"}]})).unwrap();
        assert_eq!(models[0].normalize_effort(AgentEffort::High), None);
    }

    #[test]
    fn explicit_choices_win_over_defaults() {
        let (models, _) = parse_model_page(&json!({"data":[
            {"model":"default", "isDefault":true}, {"model":"chosen"}
        ]}))
        .unwrap();
        assert_eq!(resolve_model(&models, Some("chosen")).unwrap().id, "chosen");
    }

    #[test]
    fn nested_provider_errors_explain_recovery() {
        let text = readable_error(
            r#"{"type":"error","error":{"message":"The 'future-model' model is not supported when using Codex with a ChatGPT account."}}"#,
        );
        assert!(text.starts_with("The 'future-model'"));
        assert!(text.contains("Choose Automatic"));
        assert!(!text.contains("\"type\""));
    }
}
