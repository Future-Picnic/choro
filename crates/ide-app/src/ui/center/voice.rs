use super::*;

impl CenterArea {
    pub fn selected_voice_dictation_target(
        &self,
        cx: &App,
    ) -> Option<crate::voice::VoiceDictationTarget> {
        if let Some(agent_id) = self.open_voice_chat_id(cx) {
            return Some(crate::voice::VoiceDictationTarget::Agent(agent_id));
        }
        let (project, _) = self.active_project(cx)?;
        if self
            .new_agent_composer
            .as_ref()
            .is_some_and(|composer| composer.project == project)
        {
            return Some(crate::voice::VoiceDictationTarget::NewAgent(project));
        }
        self.agents
            .read(cx)
            .explicitly_selected_agent(project)
            .filter(|agent| agent.runtime == ide_core::AgentRuntimeKind::Chat)
            .map(|agent| crate::voice::VoiceDictationTarget::Agent(agent.id))
    }

    pub fn open_voice_chat_id(&self, cx: &App) -> Option<Uuid> {
        let (project, _) = self.active_project(cx)?;
        if self.view_mode != CenterMode::Agents
            || self
                .new_agent_composer
                .as_ref()
                .is_some_and(|composer| composer.project == project)
        {
            return None;
        }
        self.agents
            .read(cx)
            .selected_agent(project)
            .filter(|agent| agent.runtime == ide_core::AgentRuntimeKind::Chat)
            .map(|agent| agent.id)
    }

    pub fn apply_voice_decision(
        &mut self,
        decision: crate::voice::VoiceDecision,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::PresentResponse {
                text: decision.speech.clone(),
            });
        cx.notify();
        match decision.action {
            crate::voice::VoiceAction::None => {}
            crate::voice::VoiceAction::FocusChat { agent_id }
            | crate::voice::VoiceAction::ShowPendingInput { agent_id } => {
                self.focus_voice_chat(agent_id, cx);
            }
            crate::voice::VoiceAction::StopRun { agent_id } => {
                self.focus_voice_chat(agent_id, cx);
                self.request_agent_chat_stop(agent_id, cx);
            }
            crate::voice::VoiceAction::SendPrompt { agent_id, prompt } => {
                let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
                    return;
                };
                self.focus_voice_chat(agent_id, cx);
                let mode = self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .map(|session| session.interaction_mode)
                    .unwrap_or(AgentInteractionMode::Default);
                self.dispatch_agent_chat_submission_with_agent(
                    &agent,
                    prompt.clone(),
                    Some(prompt),
                    Vec::new(),
                    mode,
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub fn queue_voice_dictation(
        &mut self,
        target: crate::voice::VoiceDictationTarget,
        text: String,
        insert_at_cursor: bool,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::Write {
                target,
                text,
                insert_at_cursor,
            });
        cx.notify();
    }

    pub fn queue_voice_project_plan(
        &mut self,
        project: ProjectId,
        prompt: String,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::CreatePlan { project, prompt });
        cx.notify();
    }

    pub fn queue_voice_agent(
        &mut self,
        project: ProjectId,
        prompt: String,
        send: bool,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::CreateAgent {
                project,
                prompt,
                send,
            });
        cx.notify();
    }

    pub fn queue_voice_draft_send(
        &mut self,
        target: crate::voice::VoiceDictationTarget,
        fallback_text: String,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::Send {
                target,
                fallback_text,
            });
        cx.notify();
    }

    pub fn queue_voice_draft_discard(
        &mut self,
        agent_id: Uuid,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.voice_composer_pending
            .push_back(VoiceComposerAction::Discard { agent_id, text });
        cx.notify();
    }

    fn focus_voice_chat(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_active(agent.project_id, cx)
        });
        self.agents.update(cx, |agents, cx| {
            agents.select(agent.project_id, agent_id, cx)
        });
        self.stash_new_agent_composer();
        self.set_view_mode(CenterMode::Agents, cx);
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    pub(super) fn apply_pending_voice_composer_actions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(action) = self.voice_composer_pending.pop_front() {
            let action = match action {
                VoiceComposerAction::PresentResponse { text } => {
                    window.push_notification(
                        Notification::info(text)
                            .title("Assistant")
                            .id::<ProjectTalkResponseNotification>()
                            .autohide(false),
                        cx,
                    );
                    continue;
                }
                VoiceComposerAction::CreatePlan { project, prompt } => {
                    self.open_new_agent_composer_for_project(project, window, cx);
                    if let Some(composer) = self.new_agent_composer.as_mut() {
                        composer.prompt.update(cx, |input, cx| {
                            input.set_value(prompt.clone(), window, cx);
                            input.set_cursor_position(
                                input_position_for_byte_offset(&prompt, prompt.len()),
                                window,
                                cx,
                            );
                            input.focus(window, cx);
                        });
                        composer.interaction_mode = AgentInteractionMode::Plan;
                        composer.error = None;
                    }
                    self.start_new_agent_composer(window, cx);
                    continue;
                }
                VoiceComposerAction::CreateAgent {
                    project,
                    prompt,
                    send,
                } => {
                    self.open_new_agent_composer_for_project(project, window, cx);
                    if let Some(composer) = self.new_agent_composer.as_mut() {
                        composer.prompt.update(cx, |input, cx| {
                            input.set_value(prompt.clone(), window, cx);
                            input.set_cursor_position(
                                input_position_for_byte_offset(&prompt, prompt.len()),
                                window,
                                cx,
                            );
                            input.focus(window, cx);
                        });
                        composer.interaction_mode = AgentInteractionMode::Default;
                        composer.error = None;
                    }
                    if send {
                        self.start_new_agent_composer(window, cx);
                    }
                    continue;
                }
                action => action,
            };
            let target = match &action {
                VoiceComposerAction::PresentResponse { .. } => {
                    unreachable!("Project Talk responses are presented before chat actions")
                }
                VoiceComposerAction::CreatePlan { .. } => {
                    unreachable!("plan handoff is handled before chat actions")
                }
                VoiceComposerAction::CreateAgent { .. } => {
                    unreachable!("agent handoff is handled before chat actions")
                }
                VoiceComposerAction::Write { target, .. }
                | VoiceComposerAction::Send { target, .. } => *target,
                VoiceComposerAction::Discard { agent_id, .. } => {
                    crate::voice::VoiceDictationTarget::Agent(*agent_id)
                }
            };

            if let crate::voice::VoiceDictationTarget::NewAgent(project) = target {
                let Some(input) = self
                    .new_agent_composer
                    .as_ref()
                    .filter(|composer| composer.project == project)
                    .map(|composer| composer.prompt.clone())
                else {
                    continue;
                };
                match action {
                    VoiceComposerAction::Write {
                        text,
                        insert_at_cursor,
                        ..
                    } => write_voice_text(&input, text, insert_at_cursor, window, cx),
                    VoiceComposerAction::Send { fallback_text, .. } => {
                        if input.read(cx).value().trim().is_empty() {
                            input
                                .update(cx, |input, cx| input.set_value(fallback_text, window, cx));
                        }
                        self.start_new_agent_composer(window, cx);
                    }
                    VoiceComposerAction::PresentResponse { .. }
                    | VoiceComposerAction::CreatePlan { .. }
                    | VoiceComposerAction::CreateAgent { .. }
                    | VoiceComposerAction::Discard { .. } => {
                        unreachable!("new-agent dictation only writes or sends")
                    }
                }
                continue;
            }

            let crate::voice::VoiceDictationTarget::Agent(agent_id) = target else {
                unreachable!("new-agent dictation is handled before chat dictation")
            };
            let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
                continue;
            };
            self.focus_voice_chat(agent.id, cx);
            let input = self.agent_chat_input(&agent, "Message agent…", window, cx);
            match action {
                VoiceComposerAction::PresentResponse { .. } => {
                    unreachable!("Project Talk responses are presented before chat actions")
                }
                VoiceComposerAction::CreatePlan { .. } => {
                    unreachable!("plan handoff is handled before chat actions")
                }
                VoiceComposerAction::CreateAgent { .. } => {
                    unreachable!("agent handoff is handled before chat actions")
                }
                VoiceComposerAction::Write {
                    text,
                    insert_at_cursor,
                    ..
                } => write_voice_text(&input, text, insert_at_cursor, window, cx),
                VoiceComposerAction::Send { fallback_text, .. } => {
                    if input.read(cx).value().trim().is_empty() {
                        input.update(cx, |input, cx| input.set_value(fallback_text, window, cx));
                    }
                    self.submit_agent_chat_message(&agent, input, false, window, cx);
                }
                VoiceComposerAction::Discard { text, .. } => {
                    let current = input.read(cx).value().to_string();
                    if let Some(next) = remove_untouched_voice_draft(&current, &text) {
                        input.update(cx, |input, cx| input.set_value(next, window, cx));
                    }
                }
            }
        }
    }
}

fn write_voice_text(
    input: &Entity<InputState>,
    text: String,
    insert_at_cursor: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if insert_at_cursor {
        input.update(cx, |input, cx| {
            let current = input.value();
            let insertion = voice_insertion_text(&current, input.cursor(), &text);
            input.replace(insertion, window, cx);
            input.focus(window, cx);
        });
    } else {
        input.update(cx, |input, cx| {
            let current = input.value();
            let existing = current.trim_end();
            let combined = if existing.is_empty() {
                text
            } else {
                format!("{existing} {text}")
            };
            input.set_value(combined, window, cx);
        });
    }
}

fn voice_insertion_text(current: &str, cursor: usize, transcript: &str) -> String {
    let transcript = transcript.trim();
    if transcript.is_empty() {
        return String::new();
    }
    let cursor = cursor.min(current.len());
    let before = current.get(..cursor).unwrap_or(current);
    let after = current.get(cursor..).unwrap_or_default();
    let first = transcript.chars().next();
    let last = transcript.chars().next_back();
    let needs_leading_space = before
        .chars()
        .next_back()
        .is_some_and(|character| !character.is_whitespace())
        && !first.is_some_and(|character| {
            matches!(
                character,
                ',' | '.' | '!' | '?' | ';' | ':' | ')' | ']' | '}'
            )
        });
    let needs_trailing_space = after
        .chars()
        .next()
        .is_some_and(|character| !character.is_whitespace())
        && !last.is_some_and(|character| matches!(character, '(' | '[' | '{'));

    format!(
        "{}{}{}",
        if needs_leading_space { " " } else { "" },
        transcript,
        if needs_trailing_space { " " } else { "" }
    )
}

fn remove_untouched_voice_draft(current: &str, voice_draft: &str) -> Option<String> {
    let current = current.trim_end();
    let voice_draft = voice_draft.trim();
    if current == voice_draft {
        return Some(String::new());
    }
    let prefix = current.strip_suffix(voice_draft)?.trim_end();
    (!prefix.is_empty()).then(|| prefix.to_string())
}

#[cfg(test)]
mod tests {
    use super::{remove_untouched_voice_draft, voice_insertion_text};

    #[test]
    fn removes_only_an_untouched_voice_suffix() {
        assert_eq!(
            remove_untouched_voice_draft("existing text voice draft", "voice draft"),
            Some("existing text".to_string())
        );
        assert_eq!(
            remove_untouched_voice_draft("voice draft edited", "voice draft"),
            None
        );
    }

    #[test]
    fn inserts_dictation_at_the_cursor_with_natural_spacing() {
        assert_eq!(voice_insertion_text("", 0, "hello"), "hello");
        assert_eq!(voice_insertion_text("fix", 3, "the test"), " the test");
        assert_eq!(voice_insertion_text("fixlater", 3, "this"), " this ");
        assert_eq!(voice_insertion_text("hello", 5, ", world"), ", world");
    }
}
