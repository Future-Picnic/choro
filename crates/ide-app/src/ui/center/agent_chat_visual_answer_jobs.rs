//! Ordered LocalStore writes for one conversation's Visual answers: renderer
//! state saves, submissions and receipt outcomes run one at a time, so their
//! compare-and-set revisions never race.
use ide_core::visual_answers::VisualAnswer;

use super::agent_chat_visual_answer::{
    apply_renderer_state, merge_client_state, submission_display, SendTarget, VisualJob,
};
use super::agent_chat_visual_answer_flow::accept_saved;
use super::visual_answer_surface::VisualRendererState;
use super::*;

impl CenterArea {
    /// Apply validated renderer state to the cached answer and save it.
    pub(super) fn apply_visual_state(
        &mut self,
        agent_id: Uuid,
        answer_id: Uuid,
        state: VisualRendererState,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let Some(answer) = entry.answers.get_mut(&answer_id) else {
            return;
        };
        let Some(interaction) = apply_renderer_state(answer, state) else {
            return;
        };
        if interaction == answer.interaction {
            return;
        }
        let changed_selection =
            interaction.selected_option_id != answer.interaction.selected_option_id;
        answer.interaction = interaction;
        entry.state_version += 1;
        self.queue_visual_job(agent_id, VisualJob::Persist(answer_id), cx);
        if changed_selection {
            cx.notify();
        }
    }

    pub(super) fn queue_visual_job(
        &mut self,
        agent_id: Uuid,
        job: VisualJob,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if let VisualJob::Persist(id) = &job {
            // Coalesce: one queued persist per answer writes the latest state.
            if !entry.dirty.insert(*id) {
                return;
            }
        }
        entry.jobs.push_back(job);
        if !entry.working {
            entry.working = true;
            self.run_visual_jobs(agent_id, cx);
        }
    }

    /// Store writes for one conversation run strictly in order, so a state
    /// save and a submission can never race each other's compare-and-set.
    pub(super) fn run_visual_jobs(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(project_id) = self.agents.read(cx).agent(agent_id).map(|a| a.project_id.0) else {
            return;
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let Some(job) = entry.jobs.pop_front() else {
            entry.working = false;
            return;
        };
        match job {
            VisualJob::Persist(answer_id) => {
                entry.dirty.remove(&answer_id);
                let Some(answer) = entry.answers.get(&answer_id).cloned() else {
                    return self.run_visual_jobs(agent_id, cx);
                };
                self.run_visual_persist(agent_id, project_id, answer, cx);
            }
            VisualJob::Submit {
                target,
                action_id,
                draft,
                display,
                window,
            } => {
                self.run_visual_submit(
                    agent_id, project_id, target, action_id, draft, display, None, window, cx,
                );
            }
            VisualJob::Resend {
                answer_id,
                revision,
                submission_id,
                action_id,
                window,
            } => {
                let display = entry
                    .answers
                    .get(&answer_id)
                    .map(|answer| submission_display(answer, &action_id, &answer.interaction.draft))
                    .unwrap_or_default();
                let target = SendTarget {
                    answer_id,
                    revision,
                    surface: entry.surface,
                };
                self.run_visual_submit(
                    agent_id,
                    project_id,
                    target,
                    action_id,
                    String::new(),
                    display,
                    Some(submission_id),
                    Some(window),
                    cx,
                );
            }
            VisualJob::Finish {
                answer_id,
                revision,
                submission_id,
                accepted,
            } => self.run_visual_finish(
                agent_id,
                project_id,
                answer_id,
                revision,
                submission_id,
                accepted,
                cx,
            ),
        }
    }

    fn run_visual_persist(
        &mut self,
        agent_id: Uuid,
        project_id: Uuid,
        answer: VisualAnswer,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let store = Self::visual_store()?;
                    let attempt = store.save_visual_interaction(
                        project_id,
                        agent_id,
                        answer.id,
                        answer.revision,
                        answer.interaction.clone(),
                    );
                    match attempt {
                        Ok(saved) => Ok(saved),
                        // Another writer (a submission outcome) moved on:
                        // reapply the client fields to its record once.
                        Err(_) => {
                            let latest =
                                store.read_visual_answer(project_id, agent_id, answer.id, None)?;
                            anyhow::ensure!(
                                latest.revision == answer.revision,
                                "A newer revision replaced this answer"
                            );
                            let merged =
                                merge_client_state(&latest.interaction, &answer.interaction);
                            store.save_visual_interaction(
                                project_id,
                                agent_id,
                                answer.id,
                                answer.revision,
                                merged,
                            )
                        }
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => {
                        this.accept_saved_visual_answer(agent_id, saved);
                        if let Some(entry) = this.visual_answers.get_mut(&agent_id) {
                            if entry.note_error.take().is_some() {
                                cx.notify();
                            }
                        }
                    }
                    // Say so: filters, selection or the note may not survive
                    // a restart until a later save succeeds.
                    Err(error) => {
                        if let Some(entry) = this.visual_answers.get_mut(&agent_id) {
                            entry.note_error = Some(
                                format!(
                                    "Choro couldn’t save this visual answer’s state: {error:#}"
                                )
                                .into(),
                            );
                        }
                        cx.notify();
                    }
                }
                this.run_visual_jobs(agent_id, cx);
            })
            .ok();
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn run_visual_finish(
        &mut self,
        agent_id: Uuid,
        project_id: Uuid,
        answer_id: Uuid,
        revision: u64,
        submission_id: Uuid,
        accepted: bool,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let finished = cx
                .background_executor()
                .spawn(async move {
                    Self::visual_store()?.finish_visual_submission(
                        project_id,
                        agent_id,
                        answer_id,
                        revision,
                        submission_id,
                        accepted,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                match finished {
                    Ok(saved) => this.accept_saved_visual_answer(agent_id, saved),
                    Err(error) => {
                        eprintln!("visual answer receipt was not updated: {error:#}")
                    }
                }
                this.settle_visual_send(agent_id, answer_id, accepted);
                this.remeasure_agent_chat_list(agent_id);
                cx.notify();
                this.run_visual_jobs(agent_id, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Local client fields are the source of truth: edits made while this
    /// write was in flight (a note typed during a save) are never replaced
    /// by the older values it carried. See `accept_saved`.
    pub(super) fn accept_saved_visual_answer(&mut self, agent_id: Uuid, saved: VisualAnswer) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if let Some(next) = accept_saved(entry.answers.get(&saved.id), saved) {
            entry.answers.insert(next.id, next);
        }
    }
}
