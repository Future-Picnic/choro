use gpui::{Context, Entity, EventEmitter};
use ide_core::config::GenerationAgent;
use ide_core::local_store::{LocalStore, StoredQuickAskExchange};
use ide_core::ProjectId;
use std::path::PathBuf;
use uuid::Uuid;

use crate::state::Workspace;
use crate::ui::center::attachment_helpers::{
    prompt_with_attached_files, split_prompt_attached_files,
};
use crate::voice::{answer_quick_ask, VoiceConversationTurn};

const SESSION_CONTEXT_EXCHANGES: usize = 8;
const SESSION_CONTEXT_CHARS: usize = 12_000;
const MAX_SESSION_IMAGES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickAskScope {
    General,
    Project(ProjectId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickAskPhase {
    Idle,
    Thinking,
}

#[derive(Clone, Debug)]
pub enum QuickAskEvent {
    Completed,
    Failed {
        question: String,
        attachments: Vec<PathBuf>,
    },
}

pub struct QuickAskState {
    workspace: Entity<Workspace>,
    history: Vec<StoredQuickAskExchange>,
    session: Vec<StoredQuickAskExchange>,
    pending_question: Option<String>,
    session_id: Uuid,
    scope: QuickAskScope,
    agent: GenerationAgent,
    phase: QuickAskPhase,
    error: Option<String>,
    request_generation: u64,
}

impl EventEmitter<QuickAskEvent> for QuickAskState {}

impl QuickAskState {
    pub fn load(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let (scope, agent) = {
            let workspace = workspace.read(cx);
            (
                workspace
                    .active
                    .map(QuickAskScope::Project)
                    .unwrap_or(QuickAskScope::General),
                workspace.quick_ask_agent.clone(),
            )
        };
        let history = LocalStore::open_default()
            .and_then(|store| store.load_quick_ask_exchanges())
            .unwrap_or_else(|error| {
                eprintln!("failed to load Quick Ask history: {error:#}");
                Vec::new()
            });
        Self {
            workspace,
            history,
            session: Vec::new(),
            pending_question: None,
            session_id: Uuid::new_v4(),
            scope,
            agent,
            phase: QuickAskPhase::Idle,
            error: None,
            request_generation: 0,
        }
    }

    pub fn history(&self) -> &[StoredQuickAskExchange] {
        &self.history
    }

    pub fn session(&self) -> &[StoredQuickAskExchange] {
        &self.session
    }

    pub fn pending_question(&self) -> Option<&str> {
        self.pending_question.as_deref()
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn scope(&self) -> QuickAskScope {
        self.scope
    }

    pub fn agent(&self) -> &GenerationAgent {
        &self.agent
    }

    pub fn phase(&self) -> QuickAskPhase {
        self.phase
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn begin_session(&mut self, cx: &mut Context<Self>) {
        let (scope, agent) = {
            let workspace = self.workspace.read(cx);
            (
                workspace
                    .active
                    .map(QuickAskScope::Project)
                    .unwrap_or(QuickAskScope::General),
                workspace.quick_ask_agent.clone(),
            )
        };
        self.scope = scope;
        self.agent = agent;
        self.reset_session();
        cx.notify();
    }

    /// Explicitly restore one archived conversation into the disposable panel.
    /// History is never ambient model context: only this user action rehydrates
    /// the turns, and a normal Quick Ask launch still starts fresh.
    pub fn continue_conversation(&mut self, session_id: Uuid, cx: &mut Context<Self>) -> bool {
        let session = exchanges_for_session(&self.history, session_id);
        let Some(latest) = session.last() else {
            return false;
        };
        let (scope, agent) = {
            let workspace = self.workspace.read(cx);
            let scope = latest
                .project_id
                .filter(|project_id| {
                    workspace
                        .projects
                        .iter()
                        .any(|project| project.id == *project_id)
                })
                .map(QuickAskScope::Project)
                .unwrap_or(QuickAskScope::General);
            (scope, workspace.quick_ask_agent.clone())
        };

        self.request_generation = self.request_generation.wrapping_add(1);
        self.session_id = session_id;
        self.session = session;
        self.pending_question = None;
        self.scope = scope;
        self.agent = agent;
        self.phase = QuickAskPhase::Idle;
        self.error = None;
        cx.notify();
        true
    }

    pub fn set_scope(&mut self, scope: QuickAskScope, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.reset_session();
        cx.notify();
    }

    pub fn set_agent(&mut self, agent: GenerationAgent, cx: &mut Context<Self>) {
        let agent = agent.normalized();
        if self.agent == agent {
            return;
        }
        self.agent = agent;
        self.error = None;
        cx.notify();
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        LocalStore::open_default()?.clear_quick_ask_exchanges()?;
        self.history.clear();
        self.reset_session();
        cx.notify();
        Ok(())
    }

    pub fn submit(&mut self, question: String, attachments: Vec<PathBuf>, cx: &mut Context<Self>) {
        let question = question.trim().to_string();
        if (question.is_empty() && attachments.is_empty()) || self.phase == QuickAskPhase::Thinking
        {
            return;
        }
        let question = if question.is_empty() {
            "What should I know about these images?".to_string()
        } else {
            question
        };
        // Persist the same attachment block used by agent chat. The canonical
        // renderer can then show sent images without a Quick Ask-only path.
        let stored_question = prompt_with_attached_files(&question, &attachments);
        let project = match self.scope {
            QuickAskScope::General => None,
            QuickAskScope::Project(project_id) => self
                .workspace
                .read(cx)
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| (project.id, project.name.clone(), project.path.clone())),
        };
        if matches!(self.scope, QuickAskScope::Project(_)) && project.is_none() {
            self.scope = QuickAskScope::General;
            self.reset_session();
        }
        let conversation = bounded_session_context(&self.session);
        let generation_images = session_image_paths(&self.session, &attachments);
        let session_id = self.session_id;
        let generation_agent = self.agent.clone();
        let failed_question = question.clone();
        let failed_attachments = attachments.clone();
        let generation_project = project.clone();
        let generation_question = question.clone();
        let request_agent = generation_agent.clone();
        self.phase = QuickAskPhase::Thinking;
        self.error = None;
        self.pending_question = Some(stored_question.clone());
        self.request_generation = self.request_generation.wrapping_add(1);
        let request_generation = self.request_generation;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let project_ref = generation_project
                        .as_ref()
                        .map(|(_, name, path)| (name.as_str(), path.as_path()));
                    answer_quick_ask(
                        &request_agent,
                        project_ref,
                        &conversation,
                        &generation_question,
                        &generation_images,
                    )
                })
                .await;

            let _ = this.update(cx, |state, cx| {
                if state.request_generation != request_generation || state.session_id != session_id
                {
                    return;
                }
                // Persistence happens only after this current-request gate.
                // A reset or scope change therefore cannot leave a stale
                // exchange behind, even temporarily.
                state.phase = QuickAskPhase::Idle;
                state.pending_question = None;
                match result {
                    Ok(answer) => {
                        let stored = LocalStore::open_default().and_then(|store| {
                            store.save_quick_ask_exchange(
                                session_id,
                                project.as_ref().map(|(id, name, _)| (*id, name.as_str())),
                                &stored_question,
                                &answer,
                                generation_agent.provider.label(),
                                generation_agent.model_label(),
                            )
                        });
                        match stored {
                            Ok(exchange) => {
                                state.error = None;
                                state.session.push(exchange.clone());
                                state.history.insert(0, exchange);
                                cx.emit(QuickAskEvent::Completed);
                            }
                            Err(error) => {
                                state.error = Some(format!("{error:#}"));
                                cx.emit(QuickAskEvent::Failed {
                                    question: failed_question.clone(),
                                    attachments: failed_attachments.clone(),
                                });
                            }
                        }
                    }
                    Err(error) => {
                        state.error = Some(format!("{error:#}"));
                        cx.emit(QuickAskEvent::Failed {
                            question: failed_question.clone(),
                            attachments: failed_attachments.clone(),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reset_session(&mut self) {
        self.request_generation = self.request_generation.wrapping_add(1);
        self.session_id = Uuid::new_v4();
        self.session.clear();
        self.pending_question = None;
        self.phase = QuickAskPhase::Idle;
        self.error = None;
    }
}

pub(crate) fn quick_ask_question_text(question: &str) -> String {
    let (text, attached_files) = split_prompt_attached_files(question);
    if !attached_files.is_empty() {
        return text;
    }

    // Questions saved by the first attachment implementation used Markdown
    // image syntax. Keep those archives readable while all new turns use the
    // shared agent-chat format.
    question
        .split_once("\n\n![Attached image ")
        .map(|(text, _)| text.to_string())
        .unwrap_or_else(|| question.to_string())
}

fn attached_image_paths(question: &str) -> Vec<PathBuf> {
    let (_, attached_files) = split_prompt_attached_files(question);
    if !attached_files.is_empty() {
        return attached_files;
    }

    question
        .lines()
        .filter_map(|line| {
            let (_, destination) = line.split_once("](<")?;
            let path = destination.strip_suffix(">)")?;
            line.starts_with("![Attached image ")
                .then(|| PathBuf::from(path))
        })
        .collect()
}

pub(crate) fn quick_ask_question_for_agent_chat(question: &str) -> String {
    prompt_with_attached_files(
        &quick_ask_question_text(question),
        &attached_image_paths(question),
    )
}

fn session_image_paths(session: &[StoredQuickAskExchange], current: &[PathBuf]) -> Vec<PathBuf> {
    let mut images = Vec::new();
    for path in current.iter().rev() {
        if images.len() == MAX_SESSION_IMAGES {
            break;
        }
        if !images.iter().any(|existing| existing == path) {
            images.push(path.clone());
        }
    }
    for path in session
        .iter()
        .rev()
        .flat_map(|exchange| attached_image_paths(&exchange.question))
    {
        if images.len() == MAX_SESSION_IMAGES {
            break;
        }
        if path.is_file() && !images.iter().any(|existing| existing == &path) {
            images.push(path);
        }
    }
    images.reverse();
    images
}

fn bounded_session_context(exchanges: &[StoredQuickAskExchange]) -> Vec<VoiceConversationTurn> {
    let mut selected = Vec::<(String, String)>::new();
    let mut remaining = SESSION_CONTEXT_CHARS;
    for exchange in exchanges.iter().rev().take(SESSION_CONTEXT_EXCHANGES) {
        let question_chars = exchange.question.chars().count();
        let answer_chars = exchange.answer.chars().count();
        let cost = question_chars.saturating_add(answer_chars);
        if cost <= remaining {
            selected.push((exchange.question.clone(), exchange.answer.clone()));
            remaining -= cost;
            continue;
        }
        if selected.is_empty() && remaining >= 2 {
            let total = cost.max(1);
            let question_budget =
                (remaining.saturating_mul(question_chars) / total).clamp(1, remaining - 1);
            let answer_budget = remaining - question_budget;
            selected.push((
                exchange.question.chars().take(question_budget).collect(),
                exchange.answer.chars().take(answer_budget).collect(),
            ));
        }
        break;
    }
    selected.reverse();
    selected
        .into_iter()
        .flat_map(|(question, answer)| {
            [
                VoiceConversationTurn::new("user", question),
                VoiceConversationTurn::new("assistant", answer),
            ]
        })
        .collect()
}

fn exchanges_for_session(
    history: &[StoredQuickAskExchange],
    session_id: Uuid,
) -> Vec<StoredQuickAskExchange> {
    let mut exchanges = history
        .iter()
        .filter(|exchange| exchange.session_id == session_id)
        .cloned()
        .collect::<Vec<_>>();
    // Global history is newest-first; model context and conversation rendering
    // are chronological.
    exchanges.reverse();
    exchanges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(question: &str, answer: &str) -> StoredQuickAskExchange {
        StoredQuickAskExchange {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            project_id: None,
            project_name: None,
            question: question.to_string(),
            answer: answer.to_string(),
            provider: "Codex".to_string(),
            model_label: "GPT-5.6 Luna".to_string(),
            created_at: 1,
        }
    }

    #[test]
    fn session_context_is_bounded_to_recent_exchanges() {
        let exchanges = (0..12)
            .map(|index| exchange(&format!("question {index}"), "answer"))
            .collect::<Vec<_>>();
        let context = bounded_session_context(&exchanges);
        assert_eq!(context.len(), SESSION_CONTEXT_EXCHANGES * 2);
        assert_eq!(context[0].text, "question 4");
    }

    #[test]
    fn session_context_never_exceeds_the_character_budget() {
        let exchanges = vec![exchange(
            &"q".repeat(SESSION_CONTEXT_CHARS),
            &"a".repeat(SESSION_CONTEXT_CHARS),
        )];
        let context = bounded_session_context(&exchanges);
        assert_eq!(context.len(), 2);
        assert_eq!(
            context
                .iter()
                .map(|turn| turn.text.chars().count())
                .sum::<usize>(),
            SESSION_CONTEXT_CHARS
        );
    }

    #[test]
    fn archived_conversation_restores_in_chronological_order() {
        let session_id = Uuid::new_v4();
        let other_session = Uuid::new_v4();
        let mut newest = exchange("second question", "second answer");
        newest.session_id = session_id;
        newest.created_at = 2;
        let mut oldest = exchange("first question", "first answer");
        oldest.session_id = session_id;
        oldest.created_at = 1;
        let mut unrelated = exchange("other", "answer");
        unrelated.session_id = other_session;

        let restored = exchanges_for_session(&[newest, unrelated, oldest], session_id);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].question, "first question");
        assert_eq!(restored[1].question, "second question");
    }

    #[test]
    fn attached_images_round_trip_through_the_persisted_question() {
        let attachments = vec![
            PathBuf::from("/tmp/first image.png"),
            PathBuf::from("/tmp/second.webp"),
        ];
        let question = prompt_with_attached_files("Compare these", &attachments);

        assert!(question.contains("\n\nAttached files:\n"));
        assert_eq!(quick_ask_question_text(&question), "Compare these");
        assert_eq!(attached_image_paths(&question), attachments);
    }

    #[test]
    fn legacy_attached_images_are_normalized_for_agent_chat_rendering() {
        let legacy = "Compare these\n\n![Attached image 1](</tmp/first image.png>)";
        let normalized = quick_ask_question_for_agent_chat(legacy);
        let (text, attachments) = split_prompt_attached_files(&normalized);

        assert_eq!(text, "Compare these");
        assert_eq!(attachments, vec![PathBuf::from("/tmp/first image.png")]);
    }

    #[test]
    fn current_images_are_kept_for_generation_even_before_persistence() {
        let attachments = vec![PathBuf::from("/missing/current.png")];

        assert_eq!(session_image_paths(&[], &attachments), attachments);
    }
}
