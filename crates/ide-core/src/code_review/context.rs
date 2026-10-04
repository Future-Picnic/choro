//! Compact entry context, with lossless paginated access to archived evidence.
use super::*;
use anyhow::{ensure, Result};
use serde_json::{json, Value};

/// Identify verification commands, without executing them or treating a
/// mention in a search/script body as evidence of a completed check.
fn command_title(check: &ReviewCheck) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(&check.description) {
        return value["command"].as_str().map(str::to_string);
    }
    let title = check.description.split_once("title: ")?.1;
    serde_json::Deserializer::from_str(title).into_iter::<String>().next()?.ok()
}

fn verification_command(command: &str) -> bool {
    let mut command = command.trim();
    for prefix in ["/bin/zsh -lc ","/bin/bash -lc ","/bin/sh -c ","zsh -lc ","bash -lc "] {
        if let Some(inner) = command.strip_prefix(prefix) { command = inner.trim().trim_matches(['\'', '"']); break; }
    }
    let words: Vec<_> = command.split_whitespace().skip_while(|word| word.contains('=') && !word.starts_with('-')).collect();
    let first = words.first().map(|w| w.trim_matches(['\'', '"'])).unwrap_or("");
    let second = words.get(1).copied().unwrap_or("");
    match first {
        "cargo" => matches!(second, "test"|"check"|"build"|"clippy"|"fmt"),
        "npm"|"pnpm"|"yarn"|"bun" => matches!(second,"test"|"lint"|"check"|"build")
            || second == "run" && words.get(2).is_some_and(|w| matches!(*w,"test"|"lint"|"check"|"build"|"typecheck")),
        "pytest"|"mypy"|"ruff" => true,
        "python"|"python3" => second == "-m" && words.get(2).is_some_and(|w| matches!(*w,"pytest"|"unittest")),
        "go"|"dotnet" => second == "test",
        "node" => second == "--test",
        "git" => second == "diff" && words.contains(&"--check"),
        _ => first.starts_with("./scripts/") && ["test","check","verify","lint","release-macos"].iter().any(|name| first.contains(name)),
    }
}

pub fn review_context(run: &ReviewRun, input: &ReviewInput, section: &str, page: usize) -> Result<Value> {
    validate_input(run, input)?;
    if section != "overview" {
        let entries: Vec<Value> = match section {
            "decisions" => input.requirements.decisions.iter().map(|s| json!(s)).collect(),
            "checks" => input.requirements.checks.iter().map(|s| json!(s)).collect(),
            "files" => run.files.iter().map(|f| json!(f)).collect(),
            "omissions" => input.omitted_source.iter().map(|(path, reason)| json!({"path":path,"reason":reason})).collect(),
            _ => anyhow::bail!("Unknown review context section"),
        };
        // Variable pages preserve every entry; never silently truncate a plan,
        // a command result, or evidence ranges to fit the opening response.
        let mut pages = vec![Vec::new()];
        let mut bytes = 0;
        for entry in entries {
            let size = serde_json::to_vec(&entry)?.len();
            if !pages.last().unwrap().is_empty() && (bytes + size > 40_000 || pages.last().unwrap().len() == 10) {
                pages.push(Vec::new()); bytes = 0;
            }
            bytes += size; pages.last_mut().unwrap().push(entry);
        }
        ensure!(page < pages.len(), "Invalid context page");
        return Ok(json!({"section":section,"page":page,"total_pages":pages.len(),"entries":pages[page]}));
    }
    ensure!(page == 0, "Overview has only page zero");
    let files: Vec<_> = run.files.iter().map(|f| json!({"id":f.id,"path":f.path,
        "change_kind":f.change_kind,"status":f.status,"diff_pages":f.diff_pages,
        "consumed_pages":f.consumed_pages,"skip_reason":f.skip_reason})).collect();
    let recent_decisions: Vec<_> = input.requirements.decisions.iter().enumerate()
        .rev().take(3).map(|(index, text)| json!({"index":index,"preview":text.chars().take(500).collect::<String>()})).collect();
    let mut seen = BTreeSet::new();
    let recent_checks: Vec<_> = input.requirements.checks.iter().enumerate()
        .rev().filter(|(_, check)| command_title(check).is_some_and(|title|
            verification_command(&title) && seen.insert(title)))
        .take(8).map(|(index, check)| json!({"index":index,
            "preview":check.description.chars().take(500).collect::<String>(),
            "provenance":check.provenance,"limitation":check.limitation})).collect();
    Ok(json!({"version":REVIEW_VERSION,"run_id":run.id,"snapshot_id":run.snapshot_id,
        "deadline_at":run.deadline_at,"seconds_remaining":run.deadline_at.saturating_sub(review_now()),
        "requirements":{"user_requirements":input.requirements.user_requirements,
            "project_rules":input.requirements.project_rules,"supplementary_guidance":input.requirements.supplementary_guidance},
        "decisions":{"count":input.requirements.decisions.len(),"recent_previews":recent_decisions,
            "read":"review_context section=decisions; read relevant recorded plans and decisions before judging intent"},
        "checks":{"archived_command_records":input.requirements.checks.len(),"recent_verification_previews":recent_checks,
            "read":"review_context section=checks; previews are not proof of passing checks"},
        "files":files,"batches":input.batches,"patch_only_files":input.patch_only_files,
        "pending_candidates":run.candidates,"limitations":run.limitations,
        "omitted_source_count":input.omitted_source.len(),
        "omitted_source_details":"review_context section=omissions returns excluded or unavailable source paths and reasons",
        "file_details":"review_context section=files returns full ranges and content identities",
        "accounting":"Consume every assigned diff page and explicitly report each file complete. This accounts for coverage, not proof that no bugs exist."}))
}
