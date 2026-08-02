//! "Remember this?" — memory proposals distilled from user decisions.
//!
//! When a decision carries real user text (plan feedback, a typed answer), a
//! background pass asks the generation agent whether it reveals ONE durable
//! preference. The default answer is no. When it says yes, a quiet card asks
//! the user to keep it for this project or everywhere — nothing is saved
//! without that tap, and dismissed proposals persist invisibly so the same
//! rule is never proposed twice.

use std::hash::{Hash, Hasher};

use super::*;
use crate::state::agent_chat::{MemoryProposalCard, MemoryProposalStatus};
use crate::ui::git::git_panel::{distill_memory_proposal, MemoryDecisionContext};

/// Below this many trimmed characters a decision is "yes do it" noise, not a
/// preference worth a distillation run.
const MIN_SIGNAL_CHARS: usize = 15;

fn proposal_element_seed(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

impl CenterArea {
    pub(super) fn maybe_propose_memory_from_plan_feedback(
        &mut self,
        agent_id: Uuid,
        feedback: String,
        plan_markdown: String,
        cx: &mut Context<Self>,
    ) {
        if feedback.trim().chars().count() < MIN_SIGNAL_CHARS {
            return;
        }
        self.spawn_memory_distillation(
            agent_id,
            MemoryDecisionContext::PlanFeedback {
                feedback,
                plan_markdown,
            },
            "plan_feedback",
            cx,
        );
    }

    pub(super) fn maybe_propose_memory_from_question_answers(
        &mut self,
        agent_id: Uuid,
        pairs: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let pairs: Vec<(String, String)> = pairs
            .into_iter()
            .filter(|(_, answer)| answer.trim().chars().count() >= MIN_SIGNAL_CHARS)
            .collect();
        if pairs.is_empty() {
            return;
        }
        self.spawn_memory_distillation(
            agent_id,
            MemoryDecisionContext::QuestionAnswers { pairs },
            "question_answer",
            cx,
        );
    }

    fn spawn_memory_distillation(
        &mut self,
        agent_id: Uuid,
        decision: MemoryDecisionContext,
        source: &'static str,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.read(cx).memory_proposals_enabled {
            return;
        }
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        if self.memory_distills_inflight.contains(&agent_id) {
            return;
        }
        // One open question per agent: while a proposal awaits a decision, new
        // signals are dropped rather than queued.
        let (has_pending, prior_proposal_texts) = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| {
                let mut has_pending = false;
                let mut texts = Vec::new();
                for item in &session.timeline {
                    if let AgentChatTimelineItem::MemoryProposal(card) = item {
                        has_pending |= card.status == MemoryProposalStatus::Pending;
                        texts.push(card.text.clone());
                    }
                }
                (has_pending, texts)
            })
            .unwrap_or((false, Vec::new()));
        if has_pending {
            return;
        }
        let project_id = agent.project_id;
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "this".to_string());
        let working_directory = agent.runtime_path().to_path_buf();
        let generation_agent = self.workspace.read(cx).generation_agent.clone();

        self.memory_distills_inflight.insert(agent_id);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut known = ide_core::local_store::LocalStore::open_default()
                        .and_then(|store| store.load_memories_for_project(project_id))
                        .map(|memories| {
                            memories
                                .iter()
                                .map(|memory| memory.text.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    known.extend(prior_proposal_texts);
                    distill_memory_proposal(
                        &generation_agent,
                        &working_directory,
                        &project_name,
                        &decision,
                        &known,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.memory_distills_inflight.remove(&agent_id);
                let proposal = match result {
                    Ok(Some(proposal)) => proposal,
                    Ok(None) => return,
                    Err(error) => {
                        // Suggestions must never surface generation noise.
                        eprintln!("memory proposal distillation failed: {error:#}");
                        return;
                    }
                };
                if !this.workspace.read(cx).memory_proposals_enabled {
                    return;
                }
                if this.agents.read(cx).agent(agent_id).is_none() {
                    return;
                }
                let card = MemoryProposalCard {
                    id: format!("mp-{}", Uuid::new_v4()),
                    text: proposal.text,
                    why: proposal.why,
                    suggested_global: proposal.global,
                    source: source.to_string(),
                    status: MemoryProposalStatus::Pending,
                    created_at: unix_now_secs(),
                };
                let mut timeline_to_persist = None;
                this.agent_chats.update(cx, |chats, cx| {
                    let Some(session) = chats.sessions.get_mut(&agent_id) else {
                        return;
                    };
                    let already_open = session.timeline.iter().any(|item| {
                        matches!(
                            item,
                            AgentChatTimelineItem::MemoryProposal(existing)
                                if existing.status == MemoryProposalStatus::Pending
                        )
                    });
                    if already_open {
                        return;
                    }
                    session
                        .timeline
                        .push(AgentChatTimelineItem::MemoryProposal(card));
                    timeline_to_persist = Some(session.timeline.clone());
                    cx.notify();
                });
                if let Some(timeline) = timeline_to_persist {
                    if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                        eprintln!("failed to persist memory proposal: {error:#}");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn accept_memory_proposal(
        &mut self,
        agent_id: Uuid,
        proposal_id: String,
        global: bool,
        cx: &mut Context<Self>,
    ) {
        if !self
            .memory_proposal_accepts_pending
            .insert(proposal_id.clone())
        {
            return;
        }
        self.memory_proposal_errors.remove(&proposal_id);
        // Scope belongs to the agent's project, never the currently focused one.
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            self.memory_proposal_accepts_pending.remove(&proposal_id);
            return;
        };
        let text = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| {
                session.timeline.iter().find_map(|item| match item {
                    AgentChatTimelineItem::MemoryProposal(card)
                        if card.id == proposal_id
                            && card.status == MemoryProposalStatus::Pending =>
                    {
                        Some(card.text.clone())
                    }
                    _ => None,
                })
            });
        let Some(text) = text else {
            self.memory_proposal_accepts_pending.remove(&proposal_id);
            return;
        };
        let project_id = agent.project_id;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let scope = if global { "global" } else { "project" };
                    // `source_agent_id: None` — the user accepted this, so it
                    // is user-authored: globals stay injectable and the
                    // Memorized-card poll will not double-card it.
                    ide_core::local_store::LocalStore::open_default().and_then(|store| {
                        store.save_memory(scope, (!global).then_some(project_id), &text, None)
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                this.memory_proposal_accepts_pending.remove(&proposal_id);
                match result {
                    Ok(memory) => {
                        this.memory_card_ids_seen.insert(memory.id);
                        this.set_memory_proposal_status(
                            agent_id,
                            &proposal_id,
                            MemoryProposalStatus::Accepted {
                                memory_id: memory.id,
                                global,
                            },
                            cx,
                        );
                    }
                    Err(error) => {
                        this.memory_proposal_errors
                            .insert(proposal_id.clone(), format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn dismiss_memory_proposal(
        &mut self,
        agent_id: Uuid,
        proposal_id: String,
        cx: &mut Context<Self>,
    ) {
        self.memory_proposal_errors.remove(&proposal_id);
        // Dismissed cards persist (invisibly) — deleting the timeline row
        // would resurrect the proposal on rehydration and lose dedupe.
        self.set_memory_proposal_status(
            agent_id,
            &proposal_id,
            MemoryProposalStatus::Dismissed,
            cx,
        );
    }

    pub(super) fn undo_memory_proposal(
        &mut self,
        agent_id: Uuid,
        proposal_id: String,
        memory_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if !self.memory_undos_pending.insert(memory_id) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Accepted proposals are user-authored (`source_agent_id`
                    // None), so `undo_memory`'s agent match cannot apply.
                    ide_core::local_store::LocalStore::open_default()
                        .and_then(|store| store.delete_memory(memory_id))
                })
                .await;
            this.update(cx, |this, cx| {
                this.memory_undos_pending.remove(&memory_id);
                match result {
                    Ok(()) => {
                        this.set_memory_proposal_status(
                            agent_id,
                            &proposal_id,
                            MemoryProposalStatus::Pending,
                            cx,
                        );
                    }
                    Err(error) => {
                        this.memory_proposal_errors
                            .insert(proposal_id.clone(), format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_memory_proposal_status(
        &mut self,
        agent_id: Uuid,
        proposal_id: &str,
        status: MemoryProposalStatus,
        cx: &mut Context<Self>,
    ) {
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::MemoryProposal(card) = item {
                    if card.id == proposal_id {
                        card.status = status.clone();
                        timeline_to_persist = Some(session.timeline.clone());
                        break;
                    }
                }
            }
            cx.notify();
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist memory proposal status: {error:#}");
            }
        }
    }

    /// The card: Pending asks the question, Accepted mirrors the Memorized
    /// card with undo, Dismissed renders nothing.
    pub(super) fn render_memory_proposal_card(
        &self,
        agent_id: Uuid,
        card: &MemoryProposalCard,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match &card.status {
            MemoryProposalStatus::Dismissed => div().into_any_element(),
            MemoryProposalStatus::Accepted { memory_id, global } => {
                let memory_id = *memory_id;
                let global = *global;
                let proposal_id = card.id.clone();
                let seed = proposal_element_seed(&card.id);
                crate::ui::style::chat_card(cx)
                    .child(
                        crate::ui::style::chat_card_head(cx)
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Brain,
                                crate::ui::design::sage(cx),
                                crate::ui::design::icon_sm(),
                            ))
                            .child("Memorized")
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(if global {
                                        "· every project"
                                    } else {
                                        "· this project"
                                    }),
                            )
                            .child(div().flex_1())
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("memory-proposal-undo", seed),
                                    "Undo",
                                )
                                .disabled(self.memory_undos_pending.contains(&memory_id))
                                .tooltip("Forget this again")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.undo_memory_proposal(
                                            agent_id,
                                            proposal_id.clone(),
                                            memory_id,
                                            cx,
                                        );
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child(card.text.clone()),
                    )
                    .into_any_element()
            }
            MemoryProposalStatus::Pending => {
                let seed = proposal_element_seed(&card.id);
                let busy = self.memory_proposal_accepts_pending.contains(&card.id);
                let error = self.memory_proposal_errors.get(&card.id).cloned();
                let dismiss_id = card.id.clone();
                let scope_button = |global: bool, cx: &mut Context<Self>| {
                    let suggested = global == card.suggested_global;
                    let label = if global {
                        "Every project"
                    } else {
                        "This project"
                    };
                    let id = ("memory-proposal-accept", seed.wrapping_add(global as u64));
                    let button = if suggested {
                        crate::ui::style::accent_button_compact(id, label, cx)
                    } else {
                        crate::ui::style::secondary_button_compact(id, label)
                    };
                    let proposal_id = card.id.clone();
                    button
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.accept_memory_proposal(agent_id, proposal_id.clone(), global, cx);
                        }))
                };
                let project_button = scope_button(false, cx);
                let global_button = scope_button(true, cx);
                crate::ui::style::chat_card(cx)
                    .child(
                        crate::ui::style::chat_card_head(cx)
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Sparkles,
                                crate::ui::design::sage(cx),
                                crate::ui::design::icon_sm(),
                            ))
                            .child("Remember this?")
                            .child(div().flex_1())
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("memory-proposal-dismiss", seed),
                                    "Dismiss",
                                )
                                .disabled(busy)
                                .tooltip("Don't remember, don't ask about this again")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.dismiss_memory_proposal(
                                            agent_id,
                                            dismiss_id.clone(),
                                            cx,
                                        );
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t2(cx))
                                    .child(card.text.clone()),
                            )
                            .when(!card.why.is_empty(), |body| {
                                body.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t4(cx))
                                        .child(card.why.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(project_button)
                            .child(global_button)
                            .when_some(error, |row, error| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(error),
                                )
                            }),
                    )
                    .into_any_element()
            }
        }
    }
}
