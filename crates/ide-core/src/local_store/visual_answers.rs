//! Immutable presentation revisions and separately versioned local interaction state.
use super::*;
use crate::visual_answers::*;
use anyhow::ensure;

#[cfg(test)]
#[path = "visual_answers_tests.rs"]
mod tests;

pub(super) async fn ensure_schema(conn: &Connection) -> Result<()> {
    for sql in [
        "CREATE TABLE IF NOT EXISTS visual_answer_revisions (
          id TEXT NOT NULL, revision INTEGER NOT NULL, project_id TEXT NOT NULL,
          agent_id TEXT NOT NULL, document_json TEXT NOT NULL, interaction_json TEXT NOT NULL,
          created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
          PRIMARY KEY(id,revision),
          FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
          FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE)",
        "CREATE INDEX IF NOT EXISTS visual_answer_owner ON visual_answer_revisions(agent_id,updated_at)",
        "CREATE TABLE IF NOT EXISTS visual_answer_operations (
          agent_id TEXT NOT NULL, operation_key TEXT NOT NULL, request_json TEXT NOT NULL,
          answer_id TEXT NOT NULL, revision INTEGER NOT NULL,
          PRIMARY KEY(agent_id,operation_key),
          FOREIGN KEY(answer_id,revision) REFERENCES visual_answer_revisions(id,revision) ON DELETE CASCADE)",
    ] { conn.execute(sql, ()).await?; }
    Ok(())
}

async fn authorize(conn: &Connection, project: Uuid, agent: Uuid) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM agents WHERE id=?1 AND project_id=?2",
            (agent.to_string(), project.to_string()),
        )
        .await?;
    ensure!(
        rows.next().await?.is_some(),
        "Visual answer agent does not belong to the bound project"
    );
    Ok(())
}

async fn load(
    conn: &Connection,
    project: Uuid,
    agent: Uuid,
    id: Uuid,
    revision: Option<u64>,
) -> Result<VisualAnswer> {
    let mut rows = conn.query(
        "SELECT revision,document_json,interaction_json,created_at,updated_at FROM visual_answer_revisions
         WHERE id=?1 AND project_id=?2 AND agent_id=?3 AND (?4 IS NULL OR revision=?4)
         ORDER BY revision DESC LIMIT 1",
        params![id.to_string(), project.to_string(), agent.to_string(), revision.map(|r| r as i64)],
    ).await?;
    let row = rows
        .next()
        .await?
        .context("Visual answer is unavailable in this conversation")?;
    Ok(VisualAnswer {
        id,
        project_id: project,
        agent_id: agent,
        revision: i64_to_u64(row.get(0)?)?,
        document: serde_json::from_str(&row.get::<String>(1)?)?,
        interaction: serde_json::from_str(&row.get::<String>(2)?)?,
        created_at: i64_to_u64(row.get(3)?)?,
        updated_at: i64_to_u64(row.get(4)?)?,
    })
}

async fn insert(conn: &Connection, answer: &VisualAnswer) -> Result<()> {
    conn.execute("INSERT INTO visual_answer_revisions (id,revision,project_id,agent_id,document_json,interaction_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![answer.id.to_string(), answer.revision as i64, answer.project_id.to_string(), answer.agent_id.to_string(), serde_json::to_string(&answer.document)?, serde_json::to_string(&answer.interaction)?, u64_to_i64(answer.created_at)?, u64_to_i64(answer.updated_at)?]).await?;
    Ok(())
}

async fn write_interaction(conn: &Connection, answer: &mut VisualAnswer) -> Result<()> {
    answer.interaction.validate(&answer.document)?;
    answer.updated_at = unix_now();
    conn.execute("UPDATE visual_answer_revisions SET interaction_json=?1,updated_at=?2 WHERE id=?3 AND revision=?4 AND agent_id=?5",
        params![serde_json::to_string(&answer.interaction)?, u64_to_i64(answer.updated_at)?, answer.id.to_string(), answer.revision as i64, answer.agent_id.to_string()]).await?;
    Ok(())
}

async fn finish_transaction<T>(conn: &Connection, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => match conn.execute("COMMIT", ()).await {
            Ok(_) => Ok(value),
            Err(error) => {
                let _ = conn.execute("ROLLBACK", ()).await;
                Err(error.into())
            }
        },
        Err(error) => {
            let _ = conn.execute("ROLLBACK", ()).await;
            Err(error)
        }
    }
}

impl LocalStore {
    /// Publication never opens or changes the project Preview surface.
    pub fn present_visual_answer(
        &self,
        project: Uuid,
        agent: Uuid,
        input: VisualPresentInput,
    ) -> Result<VisualPublication> {
        input.validate()?;
        let request_json = serde_json::to_string(&input)?;
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                authorize(&conn, project, agent).await?;
                let mut rows = conn.query("SELECT request_json,answer_id,revision FROM visual_answer_operations WHERE agent_id=?1 AND operation_key=?2", (agent.to_string(), input.operation_key.clone())).await?;
                if let Some(row) = rows.next().await? {
                    ensure!(row.get::<String>(0)? == request_json, "operation_key was already used with different content; use a new key");
                    let id = parse_uuid(&row.get::<String>(1)?)?;
                    let revision = i64_to_u64(row.get(2)?)?;
                    drop(rows);
                    return Ok(VisualPublication { answer: load(&conn, project, agent, id, Some(revision)).await?, duplicate: true });
                }
                drop(rows);
                let now = unix_now();
                let answer = if let Some(id) = input.id {
                    let previous = load(&conn, project, agent, id, None).await?;
                    ensure!(Some(previous.revision) == input.expected_revision, "Visual answer revision conflict; call visual_read before updating");
                    let mut interaction = VisualInteraction { draft: previous.interaction.draft, collapsed: previous.interaction.collapsed, ..Default::default() };
                    interaction.selected_option_id = previous.interaction.selected_option_id.filter(|id| input.document.options.iter().any(|option| &option.id == id));
                    // Keep the local view when its dataset/column references still
                    // resolve; the renderer drops anything that no longer applies.
                    if previous.document.view_compatible_with(&input.document) {
                        if previous.document.openui_lang == input.document.openui_lang { interaction.scroll_top = previous.interaction.scroll_top; }
                        interaction.fields = previous.interaction.fields;
                    }
                    VisualAnswer { id, project_id: project, agent_id: agent, revision: previous.revision + 1, document: input.document, interaction, created_at: now, updated_at: now }
                } else {
                    VisualAnswer { id: Uuid::new_v4(), project_id: project, agent_id: agent, revision: 1, document: input.document, interaction: VisualInteraction::default(), created_at: now, updated_at: now }
                };
                insert(&conn, &answer).await?;
                conn.execute("INSERT INTO visual_answer_operations(agent_id,operation_key,request_json,answer_id,revision) VALUES(?1,?2,?3,?4,?5)", params![agent.to_string(), input.operation_key, request_json, answer.id.to_string(), answer.revision as i64]).await?;
                Ok(VisualPublication { answer, duplicate: false })
            }.await;
            finish_transaction(&conn, result).await
        })
    }

    pub fn read_visual_answer(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: Option<u64>,
    ) -> Result<VisualAnswer> {
        ensure!(
            revision.is_none_or(|v| v > 0 && v <= i64::MAX as u64),
            "Invalid visual answer revision"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            authorize(&conn, project, agent).await?;
            load(&conn, project, agent, id, revision).await
        })
    }

    /// Cheap publication detector for the host. Revision rows are only ever
    /// inserted, so (count, newest rowid) changes exactly when an answer is
    /// published or revised; interaction saves do not change it.
    pub fn visual_answer_stamp(&self, project: Uuid, agent: Uuid) -> Result<(u64, i64)> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            let mut rows = conn
                .query(
                    "SELECT COUNT(*), COALESCE(MAX(rowid),0) FROM visual_answer_revisions WHERE project_id=?1 AND agent_id=?2",
                    (project.to_string(), agent.to_string()),
                )
                .await?;
            let row = rows.next().await?.context("Visual answer stamp query returned no row")?;
            Ok((i64_to_u64(row.get(0)?)?, row.get::<i64>(1)?))
        })
    }

    /// Answer identities and latest revisions in latest-publication order,
    /// without loading documents. A revision to an older answer becomes the
    /// newest publication; interaction saves and idempotent retries never reorder it.
    pub fn visual_answer_heads(&self, project: Uuid, agent: Uuid) -> Result<Vec<VisualAnswerHead>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            authorize(&conn, project, agent).await?;
            let mut rows = conn
                .query(
                    "SELECT id, MAX(revision), MIN(created_at) FROM visual_answer_revisions
                     WHERE project_id=?1 AND agent_id=?2 GROUP BY id ORDER BY MAX(rowid)",
                    (project.to_string(), agent.to_string()),
                )
                .await?;
            let mut heads = Vec::new();
            while let Some(row) = rows.next().await? {
                heads.push(VisualAnswerHead {
                    id: parse_uuid(&row.get::<String>(0)?)?,
                    revision: i64_to_u64(row.get(1)?)?,
                    created_at: i64_to_u64(row.get(2)?)?,
                });
            }
            Ok(heads)
        })
    }

    /// Latest revisions only, ordered by their most recent interaction/publication.
    pub fn list_visual_answers(&self, project: Uuid, agent: Uuid) -> Result<Vec<VisualAnswer>> {
        self.rt.block_on(async {
            let conn = self.connect().await?;
            authorize(&conn, project, agent).await?;
            let mut rows = conn.query("SELECT id FROM visual_answer_revisions WHERE project_id=?1 AND agent_id=?2 GROUP BY id ORDER BY MAX(updated_at),MAX(rowid)", (project.to_string(), agent.to_string())).await?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next().await? { ids.push(parse_uuid(&row.get::<String>(0)?)?); }
            drop(rows);
            let mut answers = Vec::new();
            for id in ids { answers.push(load(&conn, project, agent, id, None).await?); }
            Ok(answers)
        })
    }

    /// Host-only: renderer messages cannot set submission status or overwrite a receipt.
    pub fn save_visual_interaction(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: u64,
        state: VisualInteraction,
    ) -> Result<VisualAnswer> {
        self.mutate_visual_interaction(project, agent, id, revision, true, |answer| {
            ensure!(
                state.revision == answer.interaction.revision,
                "Interaction revision conflict"
            );
            ensure!(
                state.submission == answer.interaction.submission
                    && state.submission_error == answer.interaction.submission_error,
                "Only the host may change submission status"
            );
            // A submitted revision is a historical record: it can still be
            // reopened, filtered and scrolled, but its decision is fixed.
            ensure!(
                answer.interaction.submission.is_none()
                    || (state.selected_option_id == answer.interaction.selected_option_id
                        && state.draft == answer.interaction.draft),
                "This revision has already been submitted or is sending"
            );
            state.validate(&answer.document)?;
            answer.interaction = state;
            Ok(())
        })
    }

    pub fn begin_visual_submission(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: u64,
        interaction_revision: u64,
        action: &str,
    ) -> Result<VisualAnswer> {
        self.begin_visual_submission_with_draft(
            project,
            agent,
            id,
            revision,
            interaction_revision,
            action,
            None,
        )
    }

    /// Records the composer note and reserves the receipt in one transaction,
    /// so a concurrent state save cannot separate the note from the send.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_visual_submission_with_draft(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: u64,
        interaction_revision: u64,
        action: &str,
        draft: Option<String>,
    ) -> Result<VisualAnswer> {
        self.mutate_visual_interaction(project, agent, id, revision, true, |answer| {
            ensure!(
                answer.interaction.revision == interaction_revision,
                "Interaction revision conflict"
            );
            ensure!(
                answer.interaction.submission.is_none(),
                "This choice is already sending or sent; do not resend"
            );
            if let Some(draft) = draft {
                answer.interaction.draft = draft;
            }
            let text = answer.interaction.submission_text(answer, action)?;
            answer.interaction.submission = Some(VisualSubmission {
                id: Uuid::new_v4(),
                action_id: action.into(),
                text,
                status: VisualSubmissionStatus::Sending,
            });
            answer.interaction.submission_error = None;
            Ok(())
        })
    }

    /// A definitely rejected dispatch is retryable; uncertain sends stay Sending across restart.
    pub fn finish_visual_submission(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: u64,
        submission_id: Uuid,
        accepted: bool,
    ) -> Result<VisualAnswer> {
        self.mutate_visual_interaction(project, agent, id, revision, false, |answer| {
            let pending = answer
                .interaction
                .submission
                .as_mut()
                .context("No visual submission is pending")?;
            ensure!(pending.id == submission_id, "Stale visual submission token");
            if pending.status == VisualSubmissionStatus::Sent {
                ensure!(accepted, "A sent receipt cannot be cleared");
                return Ok(());
            }
            if accepted {
                pending.status = VisualSubmissionStatus::Sent;
                answer.interaction.collapsed = true;
                // The receipt now holds the note; the draft is spent.
                answer.interaction.draft.clear();
            } else {
                answer.interaction.submission = None;
                answer.interaction.submission_error =
                    Some("The message was not accepted. Your choice is saved; try again.".into());
            }
            Ok(())
        })
    }

    fn mutate_visual_interaction(
        &self,
        project: Uuid,
        agent: Uuid,
        id: Uuid,
        revision: u64,
        latest_only: bool,
        operation: impl FnOnce(&mut VisualAnswer) -> Result<()>,
    ) -> Result<VisualAnswer> {
        ensure!(
            revision > 0 && revision < i64::MAX as u64,
            "Invalid visual answer revision"
        );
        self.rt.block_on(async {
            let conn = self.connect().await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            let result = async {
                authorize(&conn, project, agent).await?;
                let mut answer = load(
                    &conn,
                    project,
                    agent,
                    id,
                    if latest_only { None } else { Some(revision) },
                )
                .await?;
                ensure!(answer.revision == revision, "Stale visual answer revision");
                operation(&mut answer)?;
                answer.interaction.revision = answer
                    .interaction
                    .revision
                    .checked_add(1)
                    .context("Interaction revision overflow")?;
                write_interaction(&conn, &mut answer).await?;
                Ok(answer)
            }
            .await;
            finish_transaction(&conn, result).await
        })
    }
}
