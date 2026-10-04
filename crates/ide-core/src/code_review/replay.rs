//! Collapse historical edits only when exact reverse replay proves their net
//! effect against the frozen source. No Git dirtiness establishes ownership.
use super::*;
use crate::{agent_changes::{EvidenceKind, MutationEvidence}, git::{FileDiff, LineOrigin}};
use anyhow::{ensure, Context, Result};

fn range(value: &str) -> Result<(usize, usize)> {
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Ok((start.parse()?, count.parse()?))
}

fn reverse_patch(current: &str, patch: &FileDiff) -> Result<String> {
    ensure!(!patch.is_binary && !patch.hunks.is_empty(), "Uninspectable patch");
    let mut lines: Vec<String> = current.split_terminator('\n').map(str::to_string).collect();
    let trailing = current.ends_with('\n');
    let mut replacements = Vec::new();
    for hunk in &patch.hunks {
        let parts: Vec<_> = hunk.header.split_whitespace().collect();
        ensure!(parts.len() >= 4 && parts[0] == "@@" && parts[3] == "@@", "Invalid hunk header");
        let (old_start, old_count) = range(parts[1].strip_prefix('-').context("Missing old range")?)?;
        let (new_start, new_count) = range(parts[2].strip_prefix('+').context("Missing new range")?)?;
        let start = if new_count == 0 { new_start } else { new_start.checked_sub(1).context("Invalid new start")? };
        let mut before = Vec::new(); let mut after = Vec::new();
        for line in &hunk.lines {
            let text = line.text.strip_suffix('\n').unwrap_or(&line.text);
            ensure!(!text.contains(['\n', '\0']), "Invalid patch line");
            if line.origin != LineOrigin::Add {
                ensure!(line.old_no.map(|n| n as usize) == Some(old_start + before.len()), "Old positions do not match header");
                before.push(text.to_string());
            } else { ensure!(line.old_no.is_none(), "Added line has an old position"); }
            if line.origin != LineOrigin::Remove {
                ensure!(line.new_no.map(|n| n as usize) == Some(new_start + after.len()), "New positions do not match header");
                after.push(text.to_string());
            } else { ensure!(line.new_no.is_none(), "Removed line has a new position"); }
        }
        ensure!(before.len() == old_count && after.len() == new_count, "Incomplete hunk");
        let end = start.checked_add(new_count).context("Hunk range overflow")?;
        ensure!(end <= lines.len() && lines[start..end] == after, "Patch does not match frozen source");
        replacements.push((start, end, before));
    }
    replacements.sort_by_key(|r| r.0);
    ensure!(replacements.windows(2).all(|w| w[0].1 <= w[1].0), "Overlapping hunks");
    for (start, end, before) in replacements.into_iter().rev() { lines.splice(start..end, before); }
    let mut result = lines.join("\n");
    if trailing && !lines.is_empty() { result.push('\n'); }
    ensure!(result.len() <= MAX_SOURCE_BYTES, "Replayed source exceeds per-file limit");
    Ok(result)
}

/// One net receipt per provable path; otherwise retain the original receipts.
/// Unrelated current lines remain identical on both sides of the net diff.
pub(super) fn collapse_current_edits(evidence: Vec<MutationEvidence>, source: &BTreeMap<PathBuf, String>,
    storage: &std::path::Path) -> Vec<MutationEvidence> {
    let mut groups = BTreeMap::<PathBuf, Vec<MutationEvidence>>::new();
    for receipt in evidence { groups.entry(receipt.key.path.clone()).or_default().push(receipt); }
    let mut result = Vec::new();
    for (path, mut receipts) in groups {
        receipts.sort_by_key(|e| e.captured_at);
        let mut missing = Vec::new();
        receipts.retain(|receipt| {
            let usable = receipt.before.is_some() && receipt.after.is_some() || receipt.patch.is_some();
            if !usable { missing.push(receipt.clone()); }
            usable
        });
        let replay = (|| -> Result<(MutationEvidence, usize)> {
            ensure!(receipts.len() > 1, "Single receipt needs no replay");
            let after = if let Some(hash) = source.get(&path) { read_review_blob(storage, hash)? }
                else { ensure!(receipts.last().is_some_and(|e| e.after.as_deref() == Some("")), "No frozen after-version"); String::new() };
            let mut before = after.clone();
            let mut applied = 0;
            for receipt in receipts.iter().rev() {
                let undo = (|| -> Result<String> {
                  if let Some((old, new)) = receipt.before.as_ref().zip(receipt.after.as_ref()) {
                    ensure!(new == &before, "Content chain has another writer or missing edit");
                    ensure!(receipt.before_hash.as_ref().is_none_or(|hash| hash == &content_hash(old.as_bytes()))
                        && receipt.after_hash.as_ref().is_none_or(|hash| hash == &content_hash(new.as_bytes())), "Content identity mismatch");
                    Ok(old.clone())
                } else {
                    let patch = receipt.patch.as_ref().context("Missing receipt contents")?;
                    ensure!(patch.path == path, "Patch path mismatch");
                    if let Some(hash) = &receipt.after_hash { ensure!(hash == &content_hash(before.as_bytes()), "After identity mismatch"); }
                    let previous = reverse_patch(&before, patch)?;
                    if let Some(hash) = &receipt.before_hash { ensure!(hash == &content_hash(previous.as_bytes()), "Before identity mismatch"); }
                    Ok(previous)
                }
                })();
                match undo {
                    Ok(previous) => { before = previous; applied += 1; }
                    Err(error) => { ensure!(applied > 1, "{error}"); break; }
                }
            }
            ensure!(before.len() <= MAX_SOURCE_BYTES, "Oversized baseline");
            let mut net = receipts[0].clone();
            net.key.action_id = format!("review-net-{}", content_hash(serde_json::to_string(&receipts[receipts.len()-applied..].iter().map(|r| &r.key).collect::<Vec<_>>())?.as_bytes()));
            net.kind = EvidenceKind::Contents;
            net.before_hash = Some(content_hash(before.as_bytes())); net.after_hash = Some(content_hash(after.as_bytes()));
            net.before = Some(before); net.after = Some(after); net.patch = None;
            net.captured_at = receipts.last().unwrap().captured_at;
            Ok((net, applied))
        })();
        match replay {
            Ok((net, applied)) => {
                // A gap keeps earlier receipts explicit, but does not force
                // every provable subsequent revision to be reviewed again.
                result.extend(receipts[..receipts.len()-applied].iter().cloned());
                result.push(net);
            }
            Err(_) => result.extend(receipts)
        }
        // Missing evidence is still an explicit skipped segment; it does not
        // make the inspectable edits disappear or establish clean coverage.
        result.extend(missing);
    }
    result
}
