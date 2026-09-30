//! The "Memorized" chat card — nothing enters Choro's memory invisibly.
//!
//! The MCP `memory_save` tool writes rows in a separate process; the shared
//! DB is the only channel, so the preview poll hands fresh rows to
//! `surface_fresh_memorized_cards`, which drops a card into the saving
//! agent's open chat with a one-tap undo.

use super::*;
use crate::state::agent_chat::MemorizedCard;
use ide_core::local_store::StoredMemory;

impl CenterArea {
    /// Push a "Memorized" card for every new identity whose source chat is
    /// available. Rows wait for hydration instead of being skipped by a global
    /// timestamp cursor.
    pub(super) fn surface_fresh_memorized_cards(
        &mut self,
        memories: &[StoredMemory],
        cx: &mut Context<Self>,
    ) {
        for memory in memories {
            if self.memory_card_ids_seen.contains(&memory.id) {
                continue;
            }
            let Some(agent_id) = memory.source_agent_id else {
                // Hand-authored Settings memories do not belong in an agent chat.
                self.memory_card_ids_seen.insert(memory.id);
                continue;
            };
            if self.agents.read(cx).agent(agent_id).is_none() {
                self.memory_card_ids_seen.insert(memory.id);
                continue;
            }
            let card = MemorizedCard {
                memory_id: memory.id,
                text: memory.text.clone(),
                global: memory.is_global(),
                created_at: memory.created_at,
            };
            let mut surfaced = false;
            self.agent_chats.update(cx, |chats, cx| {
                let Some(session) = chats.sessions.get_mut(&agent_id) else {
                    return;
                };
                let duplicate = session.timeline.iter().any(|item| {
                    matches!(
                        item,
                        AgentChatTimelineItem::Memorized(existing)
                            if existing.memory_id == card.memory_id
                    )
                });
                if duplicate {
                    surfaced = true;
                    return;
                }
                session
                    .timeline
                    .push(AgentChatTimelineItem::Memorized(card.clone()));
                persist_timeline_item(agent_id, AgentChatTimelineItem::Memorized(card.clone()), cx);
                surfaced = true;
                cx.notify();
            });
            if surfaced {
                self.memory_card_ids_seen.insert(memory.id);
            }
        }
    }

    /// Undo from the card: delete the memory and remove the card itself.
    pub(super) fn undo_memorized(
        &mut self,
        agent_id: Uuid,
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
                    ide_core::local_store::LocalStore::open_default()
                        .and_then(|store| store.undo_memory(agent_id, memory_id))
                })
                .await;
            this.update(cx, |this, cx| {
                this.memory_undos_pending.remove(&memory_id);
                match result {
                    Ok(()) => {
                        this.agent_chats.update(cx, |chats, cx| {
                            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                                return;
                            };
                            session.timeline.retain(|item| {
                                !matches!(
                                    item,
                                    AgentChatTimelineItem::Memorized(card)
                                        if card.memory_id == memory_id
                                )
                            });
                            cx.notify();
                        });
                    }
                    Err(error) => {
                        this.agent_start_errors
                            .insert(agent_id, format!("Couldn't undo that memory: {error:#}"));
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// The card: check-marked head, the remembered sentence, scope chip, undo.
    pub(super) fn render_memorized_card(
        &self,
        agent_id: Uuid,
        card: &MemorizedCard,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let memory_id = card.memory_id;
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
                            .child(if card.global {
                                "· every project"
                            } else {
                                "· this project"
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::ghost_button_compact(
                            ("memorized-undo", memory_id.as_u128() as u64),
                            "Undo",
                        )
                        .disabled(self.memory_undos_pending.contains(&memory_id))
                        .tooltip("Forget this again")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.undo_memorized(agent_id, memory_id, cx);
                        })),
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
}
