//! Resolve a user-authored model name; never invent a fallback or fuzzy version.
use crate::{AgentKind, AgentModel};
use anyhow::{bail, ensure, Result};

fn tokens(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut digit = false;
    for c in text.to_lowercase().chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() {
            if !word.is_empty() && c.is_numeric() != digit {
                result.push(std::mem::take(&mut word));
            }
            word.push(c);
            digit = c.is_numeric();
        } else if !word.is_empty() {
            result.push(std::mem::take(&mut word));
        }
    }
    result
}

/// The lead must copy the requested name from the trusted submission, including
/// its spelling. A tool result or another agent cannot supply model authority.
pub fn contains_model_request(submission: &str, request: &str) -> bool {
    let requested = tokens(request);
    !requested.is_empty()
        && tokens(submission)
            .windows(requested.len())
            .any(|w| w == requested)
}

fn aliases(model: AgentModel) -> Vec<Vec<String>> {
    let mut aliases = vec![tokens(model.label()), tokens(model.short_label())];
    if let Some(cli) = model.cli_value() {
        aliases.push(tokens(cli));
    }
    if model.belongs_to(AgentKind::Claude) {
        let name = tokens(model.label());
        // Labels drop the vendor because the brand icon carries it, but people
        // still write "claude opus 5.5", so keep that spelling resolvable.
        if let Some(family) = name.first() {
            let mut vendor = vec!["claude".to_string()];
            vendor.extend(name.iter().cloned());
            aliases.push(vendor);
            // Family-only Claude requests can be ambiguous across offered versions.
            aliases.push(vec![family.clone()]);
        }
    }
    aliases
}

pub fn resolve_model_request(request: &str) -> Result<(AgentKind, AgentModel)> {
    ensure!(
        !request.trim().is_empty() && request.len() <= 120,
        "Provide a short model name copied from the user's request."
    );
    let input = tokens(request);
    let mut matches = Vec::new();
    for provider in [AgentKind::Codex, AgentKind::Claude] {
        for &model in AgentModel::models_for(provider) {
            let score = aliases(model)
                .iter()
                .filter(|a| a.len() == input.len())
                .filter_map(|alias| {
                    let mut total = 0;
                    for (left, right) in input.iter().zip(alias) {
                        // Numbers and short identifiers must match exactly. Never
                        // turn an unavailable version into another generation.
                        if left.chars().any(char::is_numeric) || right.chars().any(char::is_numeric)
                        {
                            if left != right {
                                return None;
                            }
                        } else {
                            total += super::names::typo_distance(left, right, 1)?;
                        }
                    }
                    (total <= 2).then_some(total)
                })
                .min();
            if let Some(score) = score {
                matches.push((score, provider, model));
            }
        }
    }
    let Some(best) = matches.iter().map(|m| m.0).min() else {
        bail!("Model ‘{request}’ is not recognized in Choro's Codex/Claude model catalog. Ask the user for an available model; do not substitute the lead's model.");
    };
    matches.retain(|m| m.0 == best);
    if matches.len() != 1 {
        bail!(
            "Model ‘{request}’ is ambiguous: {}. Ask the user which version to use.",
            matches
                .iter()
                .map(|m| m.2.label())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok((matches[0].1, matches[0].2))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_picker_names_ids_and_clear_typos_resolve() {
        for provider in [AgentKind::Codex, AgentKind::Claude] {
            for &model in AgentModel::models_for(provider) {
                for label in [
                    model.label(),
                    model.short_label(),
                    model.cli_value().unwrap(),
                ] {
                    assert_eq!(
                        resolve_model_request(label).unwrap(),
                        (provider, model),
                        "{label}"
                    );
                }
            }
        }
        assert_eq!(
            resolve_model_request("gpt6 asrta").unwrap().1,
            AgentModel::CodexGpt6Astra
        );
        assert_eq!(
            resolve_model_request("Sonnnet 5").unwrap().1,
            AgentModel::ClaudeSonnet
        );
        assert_eq!(
            resolve_model_request("fabel 5.1").unwrap().1,
            AgentModel::ClaudeFable51
        );
        // Labels no longer carry the vendor, but people still write it.
        for (request, expected) in [
            ("claude opus 5.5", AgentModel::ClaudeOpus55),
            ("Claude Sonnet 5", AgentModel::ClaudeSonnet),
            ("claude haiku 4.5", AgentModel::ClaudeHaiku45),
        ] {
            assert_eq!(resolve_model_request(request).unwrap().1, expected, "{request}");
        }
        // A bare family still has to be disambiguated rather than guessed.
        assert!(resolve_model_request("opus").is_err());
    }
    #[test]
    fn ambiguous_versions_unknowns_and_typos_in_numbers_never_fallback() {
        for name in [
            "Fable",
            "Opus",
            "GPT-6",
            "GPT-6.1 Astra",
            "Sonnet 9",
            "Gemini",
            "",
            "Slo",
        ] {
            assert!(resolve_model_request(name).is_err(), "{name}");
        }
        assert!(resolve_model_request("Fable")
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
    }
    #[test]
    fn model_authority_requires_the_users_actual_words_and_boundaries() {
        assert!(contains_model_request(
            "Delegate using GPT6 asrta and Sonnet 5",
            "gpt6 asrta"
        ));
        assert!(!contains_model_request(
            "Delegate using GPT6 asrta",
            "GPT6 Astra"
        ));
        assert!(!contains_model_request("Use solar styling", "Sol"));
        assert!(!contains_model_request("Delegate using Astra", "Sonnet 5"));
    }
}
