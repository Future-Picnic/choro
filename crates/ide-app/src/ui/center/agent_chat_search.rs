use super::*;
use crate::actions::OpenAgentChatSearch;

gpui::actions!(
    agent_chat_search,
    [
        AgentChatSearchNext,
        AgentChatSearchPrevious,
        CloseAgentChatSearch
    ]
);

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(140);
const LIVE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);
const MAX_SEARCH_RESULTS: usize = 500;
const SEARCH_CANDIDATE_PAGE_SIZE: usize = 200;

/// Workspace fallback for Find when focus is on the chat header or has just
/// returned from the command palette. Only installed while a chat is visible.
pub(crate) const WORKSPACE_CONTEXT: &str = "AgentChatWorkspace";

pub(crate) fn bindings() -> Vec<gpui::KeyBinding> {
    vec![
        gpui::KeyBinding::new(
            "enter",
            AgentChatSearchNext,
            Some("AgentChatSearch > Input"),
        ),
        gpui::KeyBinding::new(
            "shift-enter",
            AgentChatSearchPrevious,
            Some("AgentChatSearch > Input"),
        ),
        gpui::KeyBinding::new("cmd-g", AgentChatSearchNext, Some("AgentChat")),
        gpui::KeyBinding::new("cmd-shift-g", AgentChatSearchPrevious, Some("AgentChat")),
        gpui::KeyBinding::new(
            "escape",
            CloseAgentChatSearch,
            Some("AgentChatSearch || AgentChatSearch > Input"),
        ),
    ]
}

#[derive(Clone, Debug)]
struct AgentChatSearchMatch {
    sequence: i64,
    message: AgentChatMessage,
}

struct AgentChatSearchOutcome {
    results: Vec<AgentChatSearchMatch>,
    truncated: bool,
}

pub(super) struct AgentChatSearchState {
    agent_id: Uuid,
    input: Entity<InputState>,
    query: String,
    results: Vec<AgentChatSearchMatch>,
    selected: usize,
    searching: bool,
    truncated: bool,
    error: Option<String>,
    generation: u64,
    cancellation_generation: Arc<std::sync::atomic::AtomicU64>,
    live_generation: u64,
    navigation_pending: bool,
}

fn contains_query(text: &str, query: &str) -> bool {
    crate::state::agent_chat::fold_search_text(text).contains(query)
}

fn push_search_match(
    results: &mut Vec<AgentChatSearchMatch>,
    sequence: i64,
    message: AgentChatMessage,
    folded_query: &str,
) {
    if crate::state::agent_chat::searchable_message_text(&message)
        .is_some_and(|text| contains_query(&text, folded_query))
    {
        results.push(AgentChatSearchMatch { sequence, message });
    }
}

fn search_persisted_agent_chat(
    agent_id: Uuid,
    query: &str,
    cancellation_generation: &std::sync::atomic::AtomicU64,
    generation: u64,
) -> anyhow::Result<Option<AgentChatSearchOutcome>> {
    let folded_query = crate::state::agent_chat::fold_search_text(query);
    let store = ide_core::local_store::LocalStore::open_default()?;
    let mut results = Vec::new();
    let mut pending_assistants = Vec::new();
    let mut before_sequence = None;

    loop {
        if cancellation_generation.load(std::sync::atomic::Ordering::Relaxed) != generation {
            return Ok(None);
        }
        let page = store.search_timeline_message_candidates_page(
            agent_id,
            &folded_query,
            crate::state::agent_chat::TIMELINE_SEARCH_TEXT_VERSION,
            before_sequence,
            SEARCH_CANDIDATE_PAGE_SIZE,
        )?;

        for event in page.events.into_iter().rev() {
            if cancellation_generation.load(std::sync::atomic::Ordering::Relaxed) != generation {
                return Ok(None);
            }
            let Some(AgentChatTimelineItem::Message(message)) =
                timeline_item_from_store_event(&event)
            else {
                continue;
            };
            match &message {
                AgentChatMessage::Assistant { .. } => {
                    pending_assistants.push((event.sequence, message));
                }
                AgentChatMessage::User { text, .. } => {
                    if !crate::state::agent_chat::search_turn_is_hidden(text) {
                        for (sequence, assistant) in pending_assistants.drain(..) {
                            push_search_match(&mut results, sequence, assistant, &folded_query);
                        }
                        push_search_match(&mut results, event.sequence, message, &folded_query);
                    } else {
                        pending_assistants.clear();
                    }
                }
                AgentChatMessage::Thought { .. } => {}
            }
            if results.len() > MAX_SEARCH_RESULTS {
                break;
            }
        }

        if results.len() > MAX_SEARCH_RESULTS {
            break;
        }
        if !page.has_more {
            for (sequence, assistant) in pending_assistants.drain(..) {
                push_search_match(&mut results, sequence, assistant, &folded_query);
                if results.len() > MAX_SEARCH_RESULTS {
                    break;
                }
            }
            break;
        }
        let Some(oldest_sequence) = page.oldest_sequence else {
            break;
        };
        before_sequence = Some(oldest_sequence);
    }

    let truncated = results.len() > MAX_SEARCH_RESULTS;
    results.truncate(MAX_SEARCH_RESULTS);
    Ok(Some(AgentChatSearchOutcome { results, truncated }))
}

fn latest_turn_search_snapshot(session: &AgentChatSession) -> (Vec<AgentChatMessage>, bool) {
    let start = session
        .timeline
        .iter()
        .rposition(|item| {
            matches!(
                item,
                AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
            )
        })
        .unwrap_or(0);
    let messages = session.timeline[start..]
        .iter()
        .filter_map(|item| match item {
            AgentChatTimelineItem::Message(message @ AgentChatMessage::User { .. })
            | AgentChatTimelineItem::Message(message @ AgentChatMessage::Assistant { .. }) => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let hidden = messages.first().is_some_and(|message| {
        matches!(
            message,
            AgentChatMessage::User { text, .. }
                if crate::state::agent_chat::search_turn_is_hidden(text)
        )
    });
    (messages, hidden)
}

pub(super) fn chat_search_highlight_style(cx: &App) -> gpui::HighlightStyle {
    gpui::HighlightStyle {
        color: Some(crate::ui::design::t1(cx)),
        background_color: Some(crate::ui::design::amber(cx).opacity(0.34)),
        ..Default::default()
    }
}

pub(super) fn chat_search_highlights(
    text: &str,
    query: Option<&str>,
    cx: &App,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    if text.is_ascii() && query.is_ascii() {
        let mut start = 0;
        while start + query.len() <= text.len() {
            let Some(relative) = text[start..]
                .as_bytes()
                .windows(query.len())
                .position(|candidate| candidate.eq_ignore_ascii_case(query.as_bytes()))
            else {
                break;
            };
            let match_start = start + relative;
            let match_end = match_start + query.len();
            ranges.push(match_start..match_end);
            start = match_end;
        }
    } else {
        let folded_query = crate::state::agent_chat::fold_search_text(query);
        if folded_query.is_empty() {
            return Vec::new();
        }
        let mut folded_text = String::new();
        let mut source_spans = Vec::new();
        for (source_start, ch) in text.char_indices() {
            let source_end = source_start + ch.len_utf8();
            let folded_char = crate::state::agent_chat::fold_search_text(&ch.to_string());
            for folded_ch in folded_char.chars() {
                let folded_start = folded_text.len();
                folded_text.push(folded_ch);
                source_spans.push((folded_start..folded_text.len(), source_start..source_end));
            }
        }

        let mut folded_offset = 0;
        while folded_offset < folded_text.len() {
            let Some(relative) = folded_text[folded_offset..].find(&folded_query) else {
                break;
            };
            let match_start = folded_offset + relative;
            let match_end = match_start + folded_query.len();
            let start_span = source_spans.partition_point(|(folded, _)| folded.end <= match_start);
            let end_span = source_spans.partition_point(|(folded, _)| folded.end < match_end);
            let source_start = source_spans.get(start_span).map(|(_, source)| source.start);
            let source_end = source_spans.get(end_span).map(|(_, source)| source.end);
            if let (Some(source_start), Some(source_end)) = (source_start, source_end) {
                let range = source_start..source_end;
                if ranges.last() != Some(&range) {
                    ranges.push(range);
                }
            }
            folded_offset = match_end;
        }
    }
    let style = chat_search_highlight_style(cx);
    ranges.into_iter().map(|range| (range, style)).collect()
}

fn same_search_message(left: &AgentChatMessage, right: &AgentChatMessage) -> bool {
    match (left, right) {
        (
            AgentChatMessage::User {
                text: left_text,
                created_at: left_created,
                ..
            },
            AgentChatMessage::User {
                text: right_text,
                created_at: right_created,
                ..
            },
        ) => left_created == right_created && left_text == right_text,
        (
            AgentChatMessage::Assistant {
                message_id: left_id,
                text: left_text,
                created_at: left_created,
            },
            AgentChatMessage::Assistant {
                message_id: right_id,
                text: right_text,
                created_at: right_created,
            },
        ) => match (left_id, right_id) {
            (Some(left_id), Some(right_id)) => left_id == right_id,
            _ => {
                left_created == right_created
                    && (left_text == right_text
                        || left_text.starts_with(right_text)
                        || right_text.starts_with(left_text))
            }
        },
        _ => false,
    }
}

impl CenterArea {
    pub(crate) fn visible_agent_chat_search_target(&self, cx: &App) -> Option<Uuid> {
        if self.view_mode != CenterMode::Agents {
            return None;
        }
        let (project, _) = self.active_project(cx)?;
        if self
            .new_agent_composer
            .as_ref()
            .is_some_and(|composer| composer.project == project)
        {
            return None;
        }
        self.agents
            .read(cx)
            .selected_agent(project)
            .filter(|agent| agent.runtime == AgentRuntimeKind::Chat)
            .map(|agent| agent.id)
    }

    pub(crate) fn open_selected_agent_chat_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent_id) = self.visible_agent_chat_search_target(cx) else {
            return false;
        };
        self.open_agent_chat_search(agent_id, window, cx);
        true
    }

    fn focus_agent_chat_search_after_render(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.on_next_frame(window, move |this, window, cx| {
            if this.visible_agent_chat_search_target(cx) != Some(agent_id) {
                return;
            }
            let input = this
                .agent_chat_search
                .as_ref()
                .filter(|search| search.agent_id == agent_id)
                .map(|search| search.input.clone());
            if let Some(input) = input {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        });
    }

    pub(super) fn open_agent_chat_search(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .agent_chat_search
            .as_ref()
            .is_some_and(|search| search.agent_id == agent_id)
        {
            cx.notify();
            self.focus_agent_chat_search_after_render(agent_id, window, cx);
            return;
        }

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Find in conversation…")
                .default_value("")
        });
        cx.subscribe(&input, move |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_agent_chat_search(agent_id, cx);
            }
        })
        .detach();

        if let Some(previous) = self.agent_chat_search.as_ref() {
            previous
                .cancellation_generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.agent_chat_search = Some(AgentChatSearchState {
            agent_id,
            input: input.clone(),
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            searching: false,
            truncated: false,
            error: None,
            generation: 0,
            cancellation_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            live_generation: 0,
            navigation_pending: false,
        });
        cx.notify();
        self.focus_agent_chat_search_after_render(agent_id, window, cx);
    }

    fn close_agent_chat_search(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .agent_chat_search
            .as_ref()
            .is_some_and(|search| search.agent_id == agent_id)
        {
            return;
        }
        if let Some(search) = self.agent_chat_search.take() {
            search
                .cancellation_generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
        cx.on_next_frame(window, move |this, window, cx| {
            if this.agent_chat_search.is_some()
                || this.visible_agent_chat_search_target(cx) != Some(agent_id)
            {
                return;
            }
            if let Some(input) = this.agent_chat_inputs.get(&agent_id).cloned() {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        });
    }

    fn schedule_agent_chat_search(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(search) = self
            .agent_chat_search
            .as_mut()
            .filter(|search| search.agent_id == agent_id)
        else {
            return;
        };
        let query = search.input.read(cx).value().trim().to_string();
        search.generation = search.generation.wrapping_add(1);
        search
            .cancellation_generation
            .store(search.generation, std::sync::atomic::Ordering::Relaxed);
        search.live_generation = search.live_generation.wrapping_add(1);
        let generation = search.generation;
        let cancellation_generation = search.cancellation_generation.clone();
        search.query = query.clone();
        search.results.clear();
        search.selected = 0;
        search.truncated = false;
        search.error = None;
        search.navigation_pending = false;
        if query.is_empty() {
            search.searching = false;
            cx.notify();
            return;
        }
        search.searching = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let is_current = this
                .update(cx, |this, _| {
                    this.agent_chat_search.as_ref().is_some_and(|search| {
                        search.agent_id == agent_id && search.generation == generation
                    })
                })
                .unwrap_or(false);
            if !is_current {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    search_persisted_agent_chat(
                        agent_id,
                        &query,
                        &cancellation_generation,
                        generation,
                    )
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                let Some(search) = this.agent_chat_search.as_mut().filter(|search| {
                    search.agent_id == agent_id && search.generation == generation
                }) else {
                    return;
                };
                search.searching = false;
                match result {
                    Ok(Some(outcome)) => {
                        search.truncated = outcome.truncated;
                        search.results = outcome.results;
                        search.selected = 0;
                        search.navigation_pending = !search.results.is_empty();
                    }
                    Err(error) => {
                        search.error = Some(error.to_string());
                    }
                    Ok(None) => return,
                }
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.schedule_agent_chat_search_live_refresh(cx);
            });
        })
        .detach();

        self.schedule_agent_chat_search_live_refresh(cx);
    }

    pub(super) fn schedule_agent_chat_search_live_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(search) = self
            .agent_chat_search
            .as_mut()
            .filter(|search| !search.query.is_empty())
        else {
            return;
        };
        search.live_generation = search.live_generation.wrapping_add(1);
        let live_generation = search.live_generation;
        let agent_id = search.agent_id;
        let query = search.query.clone();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIVE_SEARCH_DEBOUNCE).await;
            let snapshot = this
                .update(cx, |this, cx| {
                    let is_current = this.agent_chat_search.as_ref().is_some_and(|search| {
                        search.agent_id == agent_id
                            && search.query == query
                            && search.live_generation == live_generation
                    });
                    if !is_current {
                        return None;
                    }
                    this.agent_chats
                        .read(cx)
                        .session(agent_id)
                        .map(latest_turn_search_snapshot)
                })
                .ok()
                .flatten();
            let Some((candidates, hidden)) = snapshot else {
                return;
            };
            let folded_query = crate::state::agent_chat::fold_search_text(&query);
            let candidates_for_filter = candidates.clone();
            let matching = cx
                .background_executor()
                .spawn(async move {
                    if hidden {
                        Vec::new()
                    } else {
                        candidates_for_filter
                            .into_iter()
                            .rev()
                            .filter(|message| {
                                crate::state::agent_chat::searchable_message_text(message)
                                    .is_some_and(|text| contains_query(&text, &folded_query))
                            })
                            .collect::<Vec<_>>()
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                let Some(search) = this.agent_chat_search.as_mut().filter(|search| {
                    search.agent_id == agent_id
                        && search.query == query
                        && search.live_generation == live_generation
                }) else {
                    return;
                };
                let previous_target = search
                    .results
                    .get(search.selected)
                    .map(|result| result.message.clone());
                let previously_empty = search.results.is_empty();
                search.results.retain(|result| {
                    !candidates
                        .iter()
                        .any(|candidate| same_search_message(candidate, &result.message))
                });

                let mut live_results = matching
                    .into_iter()
                    .enumerate()
                    .map(|(index, message)| AgentChatSearchMatch {
                        sequence: i64::MAX.saturating_sub(index as i64),
                        message,
                    })
                    .collect::<Vec<_>>();
                live_results.append(&mut search.results);
                if live_results.len() > MAX_SEARCH_RESULTS {
                    search.truncated = true;
                    live_results.truncate(MAX_SEARCH_RESULTS);
                }
                search.results = live_results;
                search.selected = previous_target
                    .as_ref()
                    .and_then(|target| {
                        search
                            .results
                            .iter()
                            .position(|result| same_search_message(target, &result.message))
                    })
                    .unwrap_or(0);
                if previously_empty && !search.results.is_empty() {
                    search.navigation_pending = true;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn move_agent_chat_search_selection(
        &mut self,
        agent_id: Uuid,
        delta: isize,
        cx: &mut Context<Self>,
    ) {
        let Some(search) = self
            .agent_chat_search
            .as_mut()
            .filter(|search| search.agent_id == agent_id)
        else {
            return;
        };
        let len = search.results.len();
        if len == 0 {
            return;
        }
        search.selected = (search.selected as isize + delta).rem_euclid(len as isize) as usize;
        search.navigation_pending = true;
        cx.notify();
    }

    pub(super) fn agent_chat_search_query(&self, agent_id: Uuid) -> Option<String> {
        self.agent_chat_search
            .as_ref()
            .filter(|search| search.agent_id == agent_id && !search.query.is_empty())
            .map(|search| search.query.clone())
    }

    pub(super) fn agent_chat_search_message_is_current(
        &self,
        agent_id: Uuid,
        message: &AgentChatMessage,
    ) -> bool {
        self.agent_chat_search
            .as_ref()
            .filter(|search| search.agent_id == agent_id)
            .and_then(|search| search.results.get(search.selected))
            .is_some_and(|target| same_search_message(message, &target.message))
    }

    pub(super) fn reconcile_agent_chat_search_navigation(
        &mut self,
        agent: &AgentRecord,
        session: &AgentChatSession,
        rows: &[AgentChatRow],
        display_order: &[usize],
        list_state: &ListState,
        top_down: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .agent_chat_search
            .as_ref()
            .filter(|search| search.agent_id == agent.id && search.navigation_pending)
            .and_then(|search| search.results.get(search.selected))
            .cloned()
        else {
            return;
        };

        let timeline_index = session.timeline.iter().position(|item| {
            matches!(item, AgentChatTimelineItem::Message(message) if same_search_message(message, &target.message))
        });
        if let Some(timeline_index) = timeline_index {
            let chronological_row = rows.iter().position(
                |row| matches!(row, AgentChatRow::TimelineItem(index) if *index == timeline_index),
            );
            if let Some(list_index) = chronological_row.and_then(|chronological_row| {
                display_order
                    .iter()
                    .position(|source_index| *source_index == chronological_row)
            }) {
                if matches!(target.message, AgentChatMessage::User { .. }) {
                    self.agent_chat_expanded_user_messages
                        .insert((agent.id, timeline_index));
                }
                if list_index < list_state.item_count() {
                    list_state.splice(list_index..list_index + 1, 1);
                    list_state.scroll_to(gpui::ListOffset {
                        item_ix: list_index,
                        offset_in_item: px(0.),
                    });
                }
                let latest_index = if top_down {
                    0
                } else {
                    rows.len().saturating_sub(1)
                };
                self.agent_chat_scrolled_up
                    .insert(agent.id, list_index != latest_index);
                if let Some(search) = self.agent_chat_search.as_mut() {
                    search.navigation_pending = false;
                }
                return;
            }
        }

        let can_load_older = self
            .agent_chat_history
            .get(&agent.id)
            .is_some_and(|history| {
                history.has_more
                    && !history.failed
                    && history
                        .oldest_sequence
                        .is_some_and(|oldest| oldest > target.sequence)
            });
        if can_load_older {
            self.load_older_agent_chat_history(agent.id, cx);
        } else if let Some(search) = self.agent_chat_search.as_mut() {
            search.navigation_pending = false;
        }
    }

    pub(super) fn render_agent_chat_search(
        &self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let search = self
            .agent_chat_search
            .as_ref()
            .filter(|search| search.agent_id == agent_id)?;
        let input = search.input.clone();
        let has_results = !search.results.is_empty();
        let status = if search.searching {
            "Searching…".to_string()
        } else if search.query.is_empty() {
            String::new()
        } else if search.error.is_some() {
            "Search unavailable".to_string()
        } else if search.results.is_empty() {
            "No matches".to_string()
        } else {
            format!(
                "{} of {}{}",
                search.selected + 1,
                search.results.len(),
                if search.truncated { "+" } else { "" }
            )
        };
        let error_tooltip = search.error.clone();

        Some(
            h_flex()
                .key_context("AgentChatSearch")
                .flex_none()
                .w_full()
                .h(px(42.))
                .px_3()
                .gap_1()
                .items_center()
                .justify_end()
                .border_b_1()
                .border_color(crate::ui::design::line(cx))
                .bg(crate::ui::design::surface(cx))
                .child(
                    div()
                        .w(px(260.))
                        .child(Input::new(&input).small().prefix(IconName::Search)),
                )
                .child(
                    div()
                        .id(("agent-chat-search-status", agent_id.as_u128() as u64))
                        .w(px(88.))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .when_some(error_tooltip, |label, error| {
                            label.tooltip(move |window, cx| {
                                Tooltip::new(error.clone()).build(window, cx)
                            })
                        })
                        .child(status),
                )
                .child(
                    crate::ui::style::header_icon_button(
                        ("agent-chat-search-previous", agent_id.as_u128() as u64),
                        IconName::ChevronUp,
                        cx,
                    )
                    .disabled(!has_results)
                    .tooltip("Previous match (⇧↩)")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.move_agent_chat_search_selection(agent_id, -1, cx);
                    })),
                )
                .child(
                    crate::ui::style::header_icon_button(
                        ("agent-chat-search-next", agent_id.as_u128() as u64),
                        IconName::ChevronDown,
                        cx,
                    )
                    .disabled(!has_results)
                    .tooltip("Next match (↩)")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.move_agent_chat_search_selection(agent_id, 1, cx);
                    })),
                )
                .child(
                    crate::ui::style::header_icon_button(
                        ("agent-chat-search-close", agent_id.as_u128() as u64),
                        IconName::Close,
                        cx,
                    )
                    .tooltip("Close find (Esc)")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_agent_chat_search(agent_id, window, cx);
                    })),
                )
                .into_any_element(),
        )
    }

    pub(super) fn bind_agent_chat_search_actions(
        &self,
        element: gpui::Div,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        element
            .key_context("AgentChat")
            .on_action(
                cx.listener(move |this, _: &OpenAgentChatSearch, window, cx| {
                    this.open_agent_chat_search(agent_id, window, cx);
                }),
            )
            .on_action(cx.listener(move |this, _: &AgentChatSearchNext, _, cx| {
                this.move_agent_chat_search_selection(agent_id, 1, cx);
            }))
            .on_action(
                cx.listener(move |this, _: &AgentChatSearchPrevious, _, cx| {
                    this.move_agent_chat_search_selection(agent_id, -1, cx);
                }),
            )
            .on_action(
                cx.listener(move |this, _: &CloseAgentChatSearch, window, cx| {
                    this.close_agent_chat_search(agent_id, window, cx);
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_find_instead_of_dispatching_to_the_search_input() {
        let mut keymap = gpui::Keymap::new(vec![gpui::KeyBinding::new(
            "escape",
            gpui_component::input::Escape,
            Some("Input"),
        )]);
        keymap.add_bindings(bindings());
        let contexts = [
            "Root",
            WORKSPACE_CONTEXT,
            "AgentChat",
            "AgentChatSearch",
            "Input",
        ]
        .map(|context| gpui::KeyContext::parse(context).unwrap());
        let (resolved, _) =
            keymap.bindings_for_input(&[gpui::Keystroke::parse("escape").unwrap()], &contexts);
        assert!(resolved
            .first()
            .is_some_and(|binding| binding.action().as_any().is::<CloseAgentChatSearch>()));
    }

    #[test]
    fn message_identity_survives_normalized_assistant_text() {
        let stored = AgentChatMessage::Assistant {
            message_id: Some("turn-7".into()),
            text: "raw tagged response".into(),
            created_at: 42,
        };
        let rendered = AgentChatMessage::Assistant {
            message_id: Some("turn-7".into()),
            text: "response".into(),
            created_at: 42,
        };
        assert!(same_search_message(&stored, &rendered));

        let partial = AgentChatMessage::Assistant {
            message_id: None,
            text: "streamed res".into(),
            created_at: 84,
        };
        let complete = AgentChatMessage::Assistant {
            message_id: None,
            text: "streamed response".into(),
            created_at: 84,
        };
        assert!(same_search_message(&partial, &complete));
    }

    #[test]
    fn search_text_uses_the_visible_user_prompt() {
        let message = AgentChatMessage::User {
            text: "hidden context".into(),
            display_text: Some("Visible question".into()),
            tags: Vec::new(),
            created_at: 1,
        };
        assert_eq!(
            crate::state::agent_chat::searchable_message_text(&message).as_deref(),
            Some("Visible question")
        );
    }

    #[test]
    fn hidden_turn_drops_its_assistant_candidates() {
        let timeline = AgentChatSession {
            agent_id: Uuid::nil(),
            title: String::new(),
            chat_session_id: None,
            cli_session_id: None,
            hidden_from_notifications: false,
            is_compacting: false,
            status: AgentChatStatus::Idle,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: Vec::new(),
            timeline: vec![
                AgentChatTimelineItem::Message(AgentChatMessage::User {
                    text: format!("{REVIEW_CHECKLIST_REQUEST_MARKER}\nSource turn: 1"),
                    display_text: None,
                    tags: Vec::new(),
                    created_at: 1,
                }),
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id: Some("maintenance".into()),
                    text: "secret checklist output".into(),
                    created_at: 2,
                }),
            ],
            queued_turns: Vec::new(),
            work_log: Vec::new(),
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            changed_files: Default::default(),
            usage: None,
            started_running_at: None,
            last_activity_at: 2,
        };

        let (messages, hidden) = latest_turn_search_snapshot(&timeline);
        assert_eq!(messages.len(), 2);
        assert!(hidden);
    }
}
