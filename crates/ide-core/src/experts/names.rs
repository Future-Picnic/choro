//! Resolve names from the trusted, literal user submission, never model output.
//! This only identifies the allowed team; the lead still plans and schedules it.
use super::{catalog, normalized_expert_name, ExpertProfile};
use anyhow::{bail, Result};
use uuid::Uuid;

pub fn expert_aliases(profile: &ExpertProfile) -> Vec<String> {
    catalog::catalog()
        .experts
        .iter()
        .find(|entry| {
            entry.id == profile.id
                && normalized_expert_name(&entry.name) == normalized_expert_name(&profile.name)
        })
        .map(|entry| entry.aliases.clone())
        .unwrap_or_default()
}

struct Mention {
    start: usize,
    end: usize,
    profile: usize,
    exact: bool,
}

fn words(text: &str) -> Vec<(usize, usize, &str)> {
    let mut result = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if c.is_alphanumeric() || c == '_' {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            result.push((s, i, &text[s..i]));
        }
    }
    result
}

/// Bounded optimal-string-alignment distance, including adjacent transpositions.
/// Short identifiers must be exact; arbitrary words are never fuzzy aliases.
fn typo_distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    if a == b {
        return Some(0);
    }
    if !a.is_ascii()
        || !b.is_ascii()
        || a.len().min(b.len()) < 5
        || a.len().max(b.len()) > 32
        || a.len().abs_diff(b.len()) > limit
    {
        return None;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    (d[a.len()][b.len()] <= limit).then_some(d[a.len()][b.len()])
}

/// Single-word role aliases must occur as recipients in a delegation request.
/// “Build the frontend” or “delegate frontend implementation to UI Designer”
/// must not authorize the Frontend Engineer merely by describing working scope.
fn alias_recipient(text: &str, tokens: &[(usize, usize, &str)], i: usize) -> bool {
    let prefix = &tokens[..i];
    if !prefix
        .iter()
        .any(|(_, _, w)| matches!(*w, "delegate" | "delegation" | "delegating"))
    {
        return false;
    }
    let Some((_, end, previous)) = prefix.last() else {
        return false;
    };
    matches!(
        *previous,
        "to" | "and" | "expert" | "experts" | "bandmate" | "bandmates"
    ) || text[*end..tokens[i].0].contains(',')
}

pub fn named_experts(text: &str, profiles: &[ExpertProfile]) -> Result<Vec<Uuid>> {
    let text = normalized_expert_name(text);
    let tokens = words(&text);
    let mut mentions = Vec::new();
    for (profile, p) in profiles.iter().enumerate().filter(|(_, p)| !p.archived) {
        let name = normalized_expert_name(&p.name);
        if name.is_empty() {
            continue;
        }
        // Exact names retain the existing whitespace and word-boundary behavior.
        for (start, _) in text.match_indices(&name) {
            let end = start + name.len();
            let boundary = |c: char| c.is_alphanumeric() || c == '_';
            if !text[..start].chars().next_back().is_some_and(boundary)
                && !text[end..].chars().next().is_some_and(boundary)
            {
                mentions.push(Mention {
                    start,
                    end,
                    profile,
                    exact: true,
                });
            }
        }
        let name_words = words(&name);
        // Keep an exact word as an anchor (e.g. UI/UX), allow at most two edits
        // in the rest of the full name. Never guess a short acronym or one-word name.
        if name_words.len() >= 2 {
            for window in tokens.windows(name_words.len()) {
                let start = window[0].0;
                let end = window.last().unwrap().1;
                let separators_ok = window.windows(2).all(|pair| {
                    text[pair[0].1..pair[1].0]
                        .chars()
                        .all(|c| c.is_whitespace() || c == '-')
                });
                if !separators_ok || !window.iter().zip(&name_words).any(|(a, b)| a.2 == b.2) {
                    continue;
                }
                let edits: Option<usize> = window
                    .iter()
                    .zip(&name_words)
                    .map(|(a, b)| typo_distance(a.2, b.2, if b.2.len() >= 6 { 2 } else { 1 }))
                    .sum();
                if edits.is_some_and(|d| d <= 2) {
                    mentions.push(Mention {
                        start,
                        end,
                        profile,
                        exact: edits == Some(0),
                    });
                }
            }
        }
        for alias in expert_aliases(p) {
            let alias = normalized_expert_name(&alias);
            for (i, &(start, end, word)) in tokens.iter().enumerate() {
                if alias_recipient(&text, &tokens, i) && typo_distance(word, &alias, 1).is_some() {
                    // A real profile name wins over a product alias at this span.
                    mentions.push(Mention {
                        start,
                        end,
                        profile,
                        exact: false,
                    });
                }
            }
        }
    }
    let maximal: Vec<_> = mentions
        .iter()
        .filter(|m| {
            !mentions.iter().any(|other| {
                other.start <= m.start
                    && other.end >= m.end
                    && (other.start < m.start || other.end > m.end)
            })
        })
        .collect();
    let preferred: Vec<_> = maximal
        .iter()
        .copied()
        .filter(|m| {
            m.exact
                || !maximal
                    .iter()
                    .any(|other| other.start == m.start && other.end == m.end && other.exact)
        })
        .collect();
    let mut ids = Vec::new();
    for mention in &preferred {
        if preferred.iter().any(|other| {
            other.profile != mention.profile
                && other.start < mention.end
                && mention.start < other.end
        }) {
            let names: Vec<_> = preferred
                .iter()
                .filter(|other| other.start < mention.end && mention.start < other.end)
                .map(|other| profiles[other.profile].name.as_str())
                .collect();
            bail!("Bandmate reference ‘{}’ is ambiguous ({}). Use the full name or select the Bandmate with /delegate.", &text[mention.start..mention.end], names.join(", "));
        }
        let p = &profiles[mention.profile];
        // An unavailable exact name must not redirect to another fuzzy match.
        if p.enabled && !ids.contains(&p.id) {
            ids.push(p.id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experts::tests::profile;

    #[test]
    fn screenshot_request_resolves_only_the_three_named_roles() {
        let profiles: Vec<_> = catalog::catalog()
            .experts
            .iter()
            .map(|p| p.profile())
            .collect();
        let request = "pleaws i want to create a simple html file of landiugne page new. for another coffed palace dont read what we have now jusat dd a new landinge page and please delegate expets ui designder fro deisgn, ux writie for texrt and frotnend for the build the code of front";
        let ids = named_experts(request, &profiles).unwrap();
        let mut names: Vec<_> = profiles
            .iter()
            .filter(|p| ids.contains(&p.id))
            .map(|p| p.name.as_str())
            .collect();
        names.sort();
        assert_eq!(names, ["Frontend Engineer", "UI Designer", "UX Writer"]);
    }

    #[test]
    fn bandmate_terms_resolve_recipients_without_changing_legacy_names() {
        let profiles: Vec<_> = catalog::catalog()
            .experts
            .iter()
            .map(|p| p.profile())
            .collect();
        let expected = named_experts("Delegate to Frontend Engineer", &profiles).unwrap();
        for noun in ["expert", "experts", "bandmate", "bandmates"] {
            assert_eq!(
                named_experts(&format!("Delegate to {noun} Frontend"), &profiles).unwrap(),
                expected
            );
        }
        assert!(
            named_experts("Review the frontend bandmate experience", &profiles)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn working_scope_does_not_authorize_role_aliases() {
        let profiles: Vec<_> = catalog::catalog()
            .experts
            .iter()
            .map(|p| p.profile())
            .collect();
        assert!(named_experts("Build the frontend and backend", &profiles)
            .unwrap()
            .is_empty());
        let ids = named_experts(
            "Implement the frontend and delegate design to UI Designer",
            &profiles,
        )
        .unwrap();
        assert_eq!(ids.len(), 1);
        let ids =
            named_experts("Delegate frontend implementation to UI Designer", &profiles).unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn ambiguous_typo_requires_clarification_and_exact_names_win() {
        let a = profile("UI Writer");
        let b = profile("UI Writter");
        assert!(named_experts("Delegate to UI Writie", &[a.clone(), b.clone()]).is_err());
        assert_eq!(
            named_experts("Delegate to UI Writer", &[a.clone(), b]).unwrap(),
            vec![a.id]
        );
    }

    #[test]
    fn unavailable_names_and_renamed_defaults_do_not_redirect() {
        let mut disabled = profile("UI Writer");
        disabled.enabled = false;
        assert!(
            named_experts("UI Writer", &[disabled, profile("UI Writter")])
                .unwrap()
                .is_empty()
        );
        let mut frontend = catalog::catalog()
            .experts
            .iter()
            .find(|p| p.name == "Frontend Engineer")
            .unwrap()
            .profile();
        frontend.name = "My Specialist".into();
        assert!(named_experts("Delegate to frontend", &[frontend])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn short_names_boundaries_and_non_ascii_names_remain_exact() {
        assert!(named_experts("build", &[profile("UI")]).unwrap().is_empty());
        assert!(named_experts("Ask UX", &[profile("UI")])
            .unwrap()
            .is_empty());
        let expert = profile("עיצוב ממשק");
        assert_eq!(
            named_experts("Ask עיצוב ממשק", &[expert.clone()]).unwrap(),
            vec![expert.id]
        );
    }
}
