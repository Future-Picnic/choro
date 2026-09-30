//! Bounded, source-addressed context shared by both provider adapters.
use crate::local_store::LocalStore;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn parent_context(store: &LocalStore, parent: Uuid, before: Option<i64>) -> Result<Value> {
    let agents = store.load_agents()?;
    let agent = agents
        .iter()
        .find(|a| a.id == parent)
        .context("Parent chat is unavailable")?;
    let messages = store.load_chat_messages(parent)?;
    let candidates = messages
        .iter()
        .filter(|m| {
            m.sequence < before.unwrap_or(i64::MAX)
                && matches!(m.role.as_str(), "user" | "assistant")
        })
        .collect::<Vec<_>>();
    let mut excerpts = candidates
        .iter()
        .rev()
        .take(12)
        .map(|m| {
            json!({
                "message_id": m.id, "sequence": m.sequence, "role": m.role,
                "text": m.text.chars().take(2000).collect::<String>(),
                "truncated": m.text.chars().count() > 2000
            })
        })
        .collect::<Vec<_>>();
    excerpts.reverse();
    let older = (candidates.len() > excerpts.len())
        .then(|| excerpts.first().and_then(|m| m["sequence"].as_i64()))
        .flatten();
    let summary = store.load_agent_summary(parent)?.map(|s| json!({
        "text": s.summary_text.chars().take(8000).collect::<String>(),
        "through_sequence": s.last_summarized_sequence,
        "truncated": s.summary_text.chars().count() > 8000,
        "authority": "Background evidence; the user's assignment and constraints take precedence."
    }));
    Ok(
        json!({"summary": summary, "excerpts": excerpts, "older_context_before": older,
        "linked_documents": agent.linked_docs,
        "omissions": "Only user and assistant text is included. Hidden reasoning is excluded. Optional background is bounded; use delegation_read with context_before to retrieve earlier source positions. Linked document and attachment references remain in the source messages."}),
    )
}
