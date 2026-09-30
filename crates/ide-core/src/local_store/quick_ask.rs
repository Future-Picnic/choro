use super::*;

pub(super) async fn insert_quick_ask_exchange_async(
    conn: &Connection,
    session_id: Uuid,
    project: Option<(ProjectId, &str)>,
    question: &str,
    answer: &str,
    provider: &str,
    model_label: &str,
) -> Result<StoredQuickAskExchange> {
    let question = question.trim();
    let answer = answer.trim();
    let provider = provider.trim();
    let model_label = model_label.trim();
    anyhow::ensure!(!question.is_empty(), "Quick Ask question is empty");
    anyhow::ensure!(!answer.is_empty(), "Quick Ask answer is empty");
    anyhow::ensure!(!provider.is_empty(), "Quick Ask provider is empty");
    anyhow::ensure!(!model_label.is_empty(), "Quick Ask model is empty");

    let exchange = StoredQuickAskExchange {
        id: Uuid::new_v4(),
        session_id,
        project_id: project.map(|(id, _)| id),
        project_name: project.map(|(_, name)| name.trim().to_string()),
        question: question.to_string(),
        answer: answer.to_string(),
        provider: provider.to_string(),
        model_label: model_label.to_string(),
        created_at: unix_now(),
    };
    conn.execute(
        "INSERT INTO quick_ask_exchanges
         (id, session_id, project_id, project_name, question, answer, provider,
          model_label, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            exchange.id.to_string(),
            exchange.session_id.to_string(),
            exchange.project_id.map(|id| id.0.to_string()),
            exchange.project_name.clone(),
            exchange.question.clone(),
            exchange.answer.clone(),
            exchange.provider.clone(),
            exchange.model_label.clone(),
            u64_to_i64(exchange.created_at)?,
        ],
    )
    .await?;
    Ok(exchange)
}

pub(super) async fn load_quick_ask_exchanges_async(
    conn: &Connection,
) -> Result<Vec<StoredQuickAskExchange>> {
    let mut rows = conn
        .query(
            "SELECT id, session_id, project_id, project_name, question, answer,
                    provider, model_label, created_at
             FROM quick_ask_exchanges
             ORDER BY created_at DESC, rowid DESC",
            (),
        )
        .await?;
    let mut exchanges = Vec::new();
    while let Some(row) = rows.next().await? {
        exchanges.push(StoredQuickAskExchange {
            id: parse_uuid(&row.get::<String>(0)?)?,
            session_id: parse_uuid(&row.get::<String>(1)?)?,
            project_id: opt_text(&row, 2)?
                .map(|value| parse_uuid(&value).map(ProjectId))
                .transpose()?,
            project_name: opt_text(&row, 3)?,
            question: row.get(4)?,
            answer: row.get(5)?,
            provider: row.get(6)?,
            model_label: row.get(7)?,
            created_at: i64_to_u64(row.get(8)?)?,
        });
    }
    Ok(exchanges)
}

pub(super) async fn clear_quick_ask_exchanges_async(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM quick_ask_exchanges", ()).await?;
    Ok(())
}
