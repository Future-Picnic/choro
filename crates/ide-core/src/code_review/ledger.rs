//! Recover explicit content evidence already saved by Choro's confirmed file
//! ledger (including native Bandmate integration). Never use observed rows.
use super::*;
use crate::{agent_changes::{ChangeKey, EvidenceKind, MutationEvidence}, local_store::StoredChatFileLedgerEntry};

#[derive(Deserialize)]
struct Projection {
    baseline_content: Option<String>, result_content: Option<String>,
    baseline_hash: Option<String>, result_hash: Option<String>,
    #[serde(default)] prior_segments: Vec<serde_json::Value>,
}

pub fn confirmed_ledger_evidence(entry: &StoredChatFileLedgerEntry, project: Uuid,
    root: &std::path::Path, existing: &[MutationEvidence]) -> Vec<MutationEvidence> {
    if entry.observed || excluded_review_path(&entry.path) { return vec![]; }
    let mut projections = Vec::new();
    let mut pending = if entry.segments_json.trim().is_empty() { vec![] }
        else { match serde_json::from_str::<Vec<serde_json::Value>>(&entry.segments_json) { Ok(items) => items, Err(_) => return vec![] } };
    if pending.is_empty() {
        pending.push(serde_json::json!({"baseline_content":entry.baseline_content,
            "result_content":entry.result_content,"baseline_hash":entry.baseline_hash,"result_hash":entry.result_hash}));
    }
    let mut visited = 0;
    while let Some(raw) = pending.pop() {
        visited += 1; if visited > 4096 { break; }
        let Ok(mut projection) = serde_json::from_value::<Projection>(raw) else { continue; };
        pending.append(&mut projection.prior_segments);
        if let Some((before, after)) = projection.baseline_content.zip(projection.result_content) {
            if before.len() > MAX_SOURCE_BYTES || after.len() > MAX_SOURCE_BYTES || before.contains('\0') || after.contains('\0') { continue; }
            let before_hash = content_hash(before.as_bytes()); let after_hash = content_hash(after.as_bytes());
            if projection.baseline_hash.as_ref().is_some_and(|hash| hash != &before_hash)
                || projection.result_hash.as_ref().is_some_and(|hash| hash != &after_hash)
                || before == after { continue; }
            if existing.iter().any(|e| e.key.path == entry.path &&
                (e.before_hash.as_ref() == Some(&before_hash) && e.after_hash.as_ref() == Some(&after_hash)
                    || e.before.as_ref() == Some(&before) && e.after.as_ref() == Some(&after))) { continue; }
            if projections.iter().any(|e: &MutationEvidence| e.before_hash.as_ref() == Some(&before_hash)
                && e.after_hash.as_ref() == Some(&after_hash)) { continue; }
            let first_typed_time = existing.iter().filter(|e| e.key.path == entry.path).map(|e| e.captured_at).min();
            projections.push(MutationEvidence { key:ChangeKey { project_id:project, root:root.into(), agent_id:entry.agent_id,
                generation:"confirmed-ledger".into(), turn_id:"recorded-content".into(),
                action_id:format!("ledger-{before_hash}-{after_hash}"), path:entry.path.clone() },
                kind:EvidenceKind::Contents, confirmed:true, additions:None,deletions:None,
                before_hash:Some(before_hash),after_hash:Some(after_hash),before:Some(before),after:Some(after),patch:None,
                // Legacy projections predate typed receipts. Exact content
                // replay must still verify the chain; this is not authorship.
                captured_at:first_typed_time.map(|t| t.saturating_sub(1)).unwrap_or(entry.updated_at.saturating_mul(1_000_000)) });
        }
    }
    projections.reverse();
    if let Some(first) = existing.iter().filter(|e| e.key.path == entry.path).map(|e| e.captured_at).min() {
        let start = first.saturating_sub(projections.len() as u64);
        for (index, receipt) in projections.iter_mut().enumerate() { receipt.captured_at = start.saturating_add(index as u64).min(first.saturating_sub(1)); }
    }
    projections
}
