use anyhow::Result;
use ide_core::{
    doc_assistant::{self, DocAssistantRole},
    local_store::LocalStore,
    AgentRecord, AppConfig,
};
use serde_json::json;

fn main() -> Result<()> {
    let store = LocalStore::open_default()?;
    let workspace = store.load_workspace_config(AppConfig::load())?;
    let agents = store.load_agents()?;
    let backfill = backfill_transcripts(&store, &agents)?;
    let diff_backfill = store.backfill_agent_diff_snapshots()?;

    let mut message_count = 0usize;
    let mut timeline_count = 0usize;
    let mut attachment_count = 0usize;
    let mut cli_resume_count = 0usize;
    let mut chat_resume_count = 0usize;

    for agent in &agents {
        message_count += store.load_chat_messages(agent.id)?.len();
        timeline_count += store.load_timeline_events(agent.id)?.len();
        attachment_count += store.load_attachments(agent.id)?.len();
        if agent.cli_session_id.is_some() {
            cli_resume_count += 1;
        }
        if agent.chat_session_id.is_some() {
            chat_resume_count += 1;
        }
    }

    println!("root={}", store.root().display());
    println!("db={}", store.db_path().display());
    println!("projects={}", workspace.projects.len());
    println!("agents={}", agents.len());
    println!("chat_messages={message_count}");
    println!("timeline_events={timeline_count}");
    println!("attachments={attachment_count}");
    println!("agents_with_cli_resume={cli_resume_count}");
    println!("agents_with_chat_resume={chat_resume_count}");
    println!("backfilled_agents={}", backfill.agents);
    println!("backfilled_messages={}", backfill.messages);
    println!("backfilled_timeline_events={}", backfill.timeline_events);
    println!(
        "backfilled_diff_snapshots={} scanned={} worktree={} commits={}",
        diff_backfill.backfilled_events,
        diff_backfill.scanned_events,
        diff_backfill.worktree_matches,
        diff_backfill.commit_matches,
    );

    Ok(())
}

#[derive(Default)]
struct BackfillCounts {
    agents: usize,
    messages: usize,
    timeline_events: usize,
}

fn backfill_transcripts(store: &LocalStore, agents: &[AgentRecord]) -> Result<BackfillCounts> {
    let mut counts = BackfillCounts::default();
    for agent in agents {
        if !store.load_timeline_events(agent.id)?.is_empty() {
            continue;
        }
        let Some(session_id) = agent
            .chat_session_id
            .as_deref()
            .or(agent.cli_session_id.as_deref())
        else {
            continue;
        };
        let messages =
            doc_assistant::read_chat_messages(agent.provider, agent.runtime_path(), session_id);
        if messages.is_empty() {
            continue;
        }

        let mut event_index = 0usize;
        let mut wrote_any = false;
        for (message_index, message) in messages.into_iter().enumerate() {
            let created_at = agent.created_at.saturating_add(message_index as u64);
            match message.role {
                DocAssistantRole::User => {
                    let text = message.text;
                    write_message_event(store, agent, "user", text, created_at)?;
                    counts.messages += 1;
                    counts.timeline_events += 1;
                    event_index += 1;
                    wrote_any = true;
                }
                DocAssistantRole::Assistant => {
                    let (cleaned, plan) = split_tagged_block(&message.text, "proposed_plan");
                    let (cleaned, review) = split_tagged_block(&cleaned, "code_review");
                    if !cleaned.trim().is_empty() {
                        write_message_event(store, agent, "assistant", cleaned, created_at)?;
                        counts.messages += 1;
                        counts.timeline_events += 1;
                        event_index += 1;
                        wrote_any = true;
                    }
                    if let Some(markdown) = plan {
                        let id = format!("resumed-plan-{message_index}");
                        let payload = json!({
                            "type": "proposed_plan",
                            "id": id,
                            "markdown": markdown,
                            "expanded": false,
                            "implemented_at": agent.updated_at,
                        });
                        store.upsert_timeline_event(
                            agent.id,
                            "proposed_plan",
                            Some(format!("proposed_plan:resumed-plan-{message_index}")),
                            payload.to_string(),
                            created_at.saturating_add(event_index as u64),
                        )?;
                        counts.timeline_events += 1;
                        event_index += 1;
                        wrote_any = true;
                    }
                    if let Some(markdown) = review {
                        let id = format!("resumed-review-{message_index}");
                        let payload = json!({
                            "type": "code_review",
                            "id": id,
                            "markdown": markdown,
                            "expanded": false,
                        });
                        store.upsert_timeline_event(
                            agent.id,
                            "code_review",
                            Some(format!("code_review:resumed-review-{message_index}")),
                            payload.to_string(),
                            created_at.saturating_add(event_index as u64),
                        )?;
                        counts.timeline_events += 1;
                        event_index += 1;
                        wrote_any = true;
                    }
                }
            }
        }
        if !agent.changed_files.is_empty() {
            let files = agent
                .changed_files
                .iter()
                .map(|file| {
                    json!({
                        "path": file.path.to_string_lossy(),
                        "additions": file.additions,
                        "deletions": file.deletions,
                    })
                })
                .collect::<Vec<_>>();
            let payload = json!({
                "type": "changed_files",
                "files": files,
            });
            let key = format!(
                "changed_files:{:016x}",
                stable_hash(payload.to_string().as_bytes())
            );
            store.upsert_timeline_event(
                agent.id,
                "changed_files",
                Some(key),
                payload.to_string(),
                agent.updated_at,
            )?;
            counts.timeline_events += 1;
            wrote_any = true;
        }
        if wrote_any {
            counts.agents += 1;
        }
    }
    Ok(counts)
}

fn write_message_event(
    store: &LocalStore,
    agent: &AgentRecord,
    role: &str,
    text: String,
    created_at: u64,
) -> Result<()> {
    store.upsert_chat_message(agent.id, role, text.clone(), created_at, None)?;
    let payload = json!({
        "type": "message",
        "role": role,
        "text": text,
        "created_at": created_at,
        "backend_message_id": null,
    });
    let key = format!(
        "message:{role}:{created_at}:{:016x}",
        stable_hash(payload["text"].as_str().unwrap_or_default().as_bytes())
    );
    store.upsert_timeline_event(
        agent.id,
        "message",
        Some(key),
        payload.to_string(),
        created_at,
    )?;
    Ok(())
}

fn split_tagged_block(text: &str, tag: &str) -> (String, Option<String>) {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");
    let Some(open) = text.find(&open_tag) else {
        return (text.to_string(), None);
    };
    let after_open = open + open_tag.len();
    let Some(close_rel) = text[after_open..].find(&close_tag) else {
        return (text.to_string(), None);
    };
    let close = after_open + close_rel;
    let block = text[after_open..close].trim();
    let before = text[..open].trim();
    let after = text[close + close_tag.len()..].trim();
    let cleaned = match (before.is_empty(), after.is_empty()) {
        (true, true) => String::new(),
        (false, true) => before.to_string(),
        (true, false) => after.to_string(),
        (false, false) => format!("{before}\n\n{after}"),
    };
    (cleaned, (!block.is_empty()).then(|| block.to_string()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
