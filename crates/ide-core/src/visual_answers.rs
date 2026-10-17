//! Provider-neutral, local snapshots for the composer Visual answer surface.
//! This contract carries data and presentation, never credentials or executable tools.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const MAX_VISUAL_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_VISUAL_STATE_BYTES: usize = 64 * 1024;
pub const MAX_TABLE_ROWS: usize = 1_000;
pub const MAX_CHART_POINTS: usize = 500;
pub const MAX_CHART_SERIES: usize = 8;
pub const MAX_DESIGN_OPTIONS: usize = 4;
/// Bound for the host-written receipt, kept separate from client state so a
/// full-size draft can always be submitted.
pub const MAX_VISUAL_RECEIPT_BYTES: usize = 48 * 1024;
/// Readable description of the current local view (search, sort, hidden
/// series) that the renderer stores in `fields.summary` for follow-ups.
pub const MAX_VISUAL_VIEW_SUMMARY_BYTES: usize = 2 * 1024;
/// Composer refinements are always available, even when the agent declared no
/// explicit `refine` action.
pub const IMPLICIT_REFINE_ACTION: &str = "choro_refine";

/// Gates tool advertisement, provider instructions and the composer surface.
/// The bundled renderer, Preview arbitration and native submission are
/// integrated; set false to withdraw the feature without leaving invisible answers.
pub const VISUAL_ANSWERS_READY: bool = true;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualPresentInput {
    pub operation_key: String,
    pub id: Option<Uuid>,
    pub expected_revision: Option<u64>,
    pub document: VisualDocument,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualDocument {
    pub title: String,
    pub summary: String,
    pub kind: VisualKind,
    pub openui_lang: String,
    #[serde(default)]
    pub datasets: BTreeMap<String, VisualDataset>,
    #[serde(default)]
    pub sources: Vec<VisualSource>,
    #[serde(default)]
    pub options: Vec<VisualOption>,
    #[serde(default)]
    pub actions: Vec<VisualAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualKind {
    Table,
    LineChart,
    BarChart,
    Summary,
    DesignOptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualDataset {
    pub columns: Vec<VisualColumn>,
    pub rows: Vec<BTreeMap<String, Value>>,
    /// Numeric columns plotted as series. Empty for table-only datasets.
    #[serde(default)]
    pub series: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualColumn {
    pub key: String,
    pub label: String,
    pub kind: VisualColumnKind,
    pub unit: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualColumnKind {
    Text,
    Number,
    Boolean,
    Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualSource {
    pub label: String,
    pub reference: Option<String>,
    pub retrieved_at: Option<u64>,
    pub time_range: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualOption {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Display only, inside a host-owned sandbox. Never loaded as the host page.
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualAction {
    pub id: String,
    pub label: String,
    pub kind: VisualActionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualActionKind {
    Select,
    Refine,
    FollowUp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualInteraction {
    pub revision: u64,
    pub selected_option_id: Option<String>,
    pub draft: String,
    /// OpenUI's opaque local field/filter state, restored without interpretation.
    pub fields: Value,
    pub scroll_top: f64,
    pub collapsed: bool,
    /// The user detached this revision from the composer: it stays in the
    /// transcript (with its selection and note) and reopens only when asked.
    /// A new revision starts attached. Absent in state saved before detach existed.
    #[serde(default)]
    pub detached: bool,
    pub submission: Option<VisualSubmission>,
    pub submission_error: Option<String>,
}

impl Default for VisualInteraction {
    fn default() -> Self {
        Self {
            revision: 0,
            selected_option_id: None,
            draft: String::new(),
            fields: serde_json::json!({}),
            scroll_top: 0.0,
            collapsed: false,
            detached: false,
            submission: None,
            submission_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualSubmission {
    pub id: Uuid,
    pub action_id: String,
    pub text: String,
    pub status: VisualSubmissionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualSubmissionStatus {
    Sending,
    Sent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualAnswer {
    pub id: Uuid,
    pub project_id: Uuid,
    pub agent_id: Uuid,
    pub revision: u64,
    pub document: VisualDocument,
    pub interaction: VisualInteraction,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Identity and latest revision of an answer, without its document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualAnswerHead {
    pub id: Uuid,
    pub revision: u64,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualPublication {
    pub answer: VisualAnswer,
    pub duplicate: bool,
}

fn bounded(text: &str, name: &str, max: usize, required: bool) -> Result<()> {
    ensure!(
        !required || !text.trim().is_empty(),
        "{name} must not be empty"
    );
    ensure!(
        text.len() <= max && !text.contains('\0'),
        "{name} is too long or contains NUL"
    );
    Ok(())
}

fn identifier(text: &str, name: &str) -> Result<()> {
    ensure!(
        !text.is_empty()
            && text.len() <= 128
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "{name} must use 1–128 ASCII letters, digits, underscores or hyphens"
    );
    Ok(())
}

impl VisualPresentInput {
    pub fn validate(&self) -> Result<()> {
        bounded(&self.operation_key, "operation_key", 200, true)?;
        ensure!(
            self.id.is_some() == self.expected_revision.is_some(),
            "Updating requires both id and expected_revision; omit both when creating"
        );
        ensure!(
            self.expected_revision
                .is_none_or(|v| v > 0 && v < i64::MAX as u64),
            "expected_revision is invalid"
        );
        ensure!(serde_json::to_vec(self)?.len() <= MAX_VISUAL_BYTES, "Visual answer exceeds 2 MiB; reduce the data or presentation without silently truncating it");
        self.document.validate()
    }
}

impl VisualDocument {
    /// Only an explicit choice belongs beside the composer. Informational
    /// tables/charts and optional follow-ups belong in the conversation.
    /// Use structured document metadata, never the title or explanatory prose.
    pub fn requires_user_response(&self) -> bool {
        !self.options.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_VISUAL_BYTES,
            "Visual answer exceeds 2 MiB"
        );
        bounded(&self.title, "title", 200, true)?;
        bounded(&self.summary, "summary", 8_192, true)?;
        bounded(&self.openui_lang, "openui_lang", MAX_VISUAL_BYTES, true)?;
        ensure!(
            self.datasets.len() <= 16 && self.sources.len() <= 32,
            "Use at most 16 datasets and 32 sources"
        );
        for (name, dataset) in &self.datasets {
            identifier(name, "dataset name")?;
            dataset.validate()?;
        }
        ensure!(
            self.datasets.is_empty() || !self.sources.is_empty(),
            "Data snapshots require at least one source label; mark sample data explicitly"
        );
        for source in &self.sources {
            bounded(&source.label, "source label", 512, true)?;
            if let Some(value) = &source.reference {
                bounded(value, "source reference", 2_048, false)?;
            }
            if let Some(value) = &source.time_range {
                bounded(value, "time range", 512, false)?;
            }
        }
        ensure!(
            self.options.len() <= MAX_DESIGN_OPTIONS,
            "Use at most four design alternatives"
        );
        ensure!(
            self.kind != VisualKind::DesignOptions || (2..=4).contains(&self.options.len()),
            "Design choices require two to four alternatives"
        );
        let mut options = BTreeSet::new();
        for option in &self.options {
            identifier(&option.id, "option id")?;
            ensure!(options.insert(&option.id), "Option IDs must be unique");
            bounded(&option.title, "option title", 200, true)?;
            bounded(&option.description, "option description", 2_048, false)?;
            bounded(&option.html, "option HTML", 256 * 1024, true)?;
        }
        let mut actions = BTreeSet::new();
        ensure!(self.actions.len() <= 16, "Use at most 16 follow-up actions");
        for action in &self.actions {
            identifier(&action.id, "action id")?;
            ensure!(actions.insert(&action.id), "Action IDs must be unique");
            bounded(&action.label, "action label", 200, true)?;
            ensure!(
                action.kind != VisualActionKind::Select || !self.options.is_empty(),
                "Selection actions require design alternatives"
            );
        }
        if matches!(self.kind, VisualKind::LineChart | VisualKind::BarChart) {
            ensure!(
                self.datasets.values().any(|d| !d.series.is_empty()),
                "Charts require a dataset with declared numeric series"
            );
        }
        Ok(())
    }

    /// Saved filters and sorting refer to dataset names and column keys. A
    /// revision that keeps those (and their types) can keep the local view.
    pub fn view_compatible_with(&self, other: &VisualDocument) -> bool {
        self.datasets.len() == other.datasets.len()
            && self.datasets.iter().all(|(name, dataset)| {
                other.datasets.get(name).is_some_and(|next| {
                    dataset.columns.len() == next.columns.len()
                        && dataset
                            .columns
                            .iter()
                            .zip(&next.columns)
                            .all(|(a, b)| a.key == b.key && a.kind == b.kind)
                })
            })
    }
}

impl VisualDataset {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.columns.is_empty() && self.columns.len() <= 64,
            "A dataset requires 1–64 columns"
        );
        ensure!(
            self.rows.len() <= MAX_TABLE_ROWS,
            "Dataset exceeds 1,000 rows; supply an explicit aggregate or smaller scope"
        );
        ensure!(
            self.series.len() <= MAX_CHART_SERIES,
            "Chart exceeds eight series"
        );
        ensure!(
            self.series.is_empty() || self.rows.len() <= MAX_CHART_POINTS,
            "Chart exceeds 500 points per series; provide an explicit aggregate"
        );
        let mut columns = BTreeMap::new();
        for column in &self.columns {
            identifier(&column.key, "column key")?;
            bounded(&column.label, "column label", 200, true)?;
            if let Some(unit) = &column.unit {
                bounded(unit, "unit", 100, false)?;
            }
            ensure!(
                columns.insert(&column.key, column.kind).is_none(),
                "Column keys must be unique"
            );
        }
        let mut series = BTreeSet::new();
        for key in &self.series {
            ensure!(
                series.insert(key) && columns.get(key) == Some(&VisualColumnKind::Number),
                "Chart series must name unique numeric columns"
            );
        }
        for row in &self.rows {
            ensure!(
                row.len() == columns.len(),
                "Every row must contain all declared columns; use null for missing values"
            );
            for (key, value) in row {
                let kind = columns
                    .get(key)
                    .ok_or_else(|| anyhow::anyhow!("Row contains undeclared column {key}"))?;
                let valid = value.is_null()
                    || match kind {
                        VisualColumnKind::Number => value.is_number(),
                        VisualColumnKind::Boolean => value.is_boolean(),
                        VisualColumnKind::Text | VisualColumnKind::Date => value.is_string(),
                    };
                ensure!(valid, "Value in {key} does not match its column type");
            }
        }
        Ok(())
    }
}

impl VisualInteraction {
    pub fn validate(&self, document: &VisualDocument) -> Result<()> {
        bounded(&self.draft, "draft", 32 * 1024, false)?;
        ensure!(self.fields.is_object(), "OpenUI state must be an object");
        ensure!(
            self.scroll_top.is_finite() && self.scroll_top >= 0.0,
            "Invalid scroll position"
        );
        ensure!(
            self.revision < i64::MAX as u64,
            "Interaction revision overflow"
        );
        ensure!(
            self.selected_option_id
                .as_ref()
                .is_none_or(|id| document.options.iter().any(|o| &o.id == id)),
            "Selected option is not in this answer revision"
        );
        // Client state and the host receipt have separate bounds: the receipt
        // repeats the draft, so a combined limit could reject a valid send.
        let client = VisualInteraction {
            submission: None,
            ..self.clone()
        };
        ensure!(
            serde_json::to_vec(&client)?.len() <= MAX_VISUAL_STATE_BYTES,
            "Interaction state exceeds 64 KiB"
        );
        ensure!(
            self.submission
                .as_ref()
                .is_none_or(|s| s.text.len() <= MAX_VISUAL_RECEIPT_BYTES),
            "Visual answer receipt exceeds 48 KiB"
        );
        Ok(())
    }

    /// The renderer's readable description of the current local view.
    pub fn view_summary(&self) -> Option<&str> {
        let summary = self.fields.get("summary")?.as_str()?.trim();
        if summary.is_empty() {
            return None;
        }
        let mut end = summary.len().min(MAX_VISUAL_VIEW_SUMMARY_BYTES);
        while !summary.is_char_boundary(end) {
            end -= 1;
        }
        Some(&summary[..end])
    }

    pub fn submission_text(&self, answer: &VisualAnswer, action_id: &str) -> Result<String> {
        self.validate(&answer.document)?;
        let implicit = VisualAction {
            id: IMPLICIT_REFINE_ACTION.into(),
            label: "Refine".into(),
            kind: VisualActionKind::Refine,
        };
        let action = answer
            .document
            .actions
            .iter()
            .find(|a| a.id == action_id)
            .or((action_id == IMPLICIT_REFINE_ACTION).then_some(&implicit))
            .ok_or_else(|| anyhow::anyhow!("Unknown visual-answer action"))?;
        let decision = match action.kind {
            VisualActionKind::Select => {
                let option = answer
                    .document
                    .options
                    .iter()
                    .find(|o| Some(&o.id) == self.selected_option_id.as_ref())
                    .ok_or_else(|| anyhow::anyhow!("Select an alternative before submitting"))?;
                format!(
                    "Use the {} direction.\n{}",
                    option.title, option.description
                )
            }
            VisualActionKind::Refine => {
                ensure!(
                    !self.draft.trim().is_empty(),
                    "Describe what you want to refine"
                );
                "Refine this visual answer.".into()
            }
            VisualActionKind::FollowUp => action.label.clone(),
        };
        let notes = self.draft.trim();
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("\n\n{notes}")
        };
        let view = self
            .view_summary()
            .map(|summary| format!("\nCurrent view: {summary}"))
            .unwrap_or_default();
        let text = format!("{decision}{notes}\n\n[Choro visual answer: {}; revision {}; action {}; selected option: {}]{view}\nTo revise it, call visual_read and then visual_present with this id. Continue in this conversation's current mode and permissions.", answer.id, answer.revision, action.id, self.selected_option_id.as_deref().unwrap_or("none"));
        ensure!(
            text.len() <= MAX_VISUAL_RECEIPT_BYTES,
            "This message is too long to send; shorten the note"
        );
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn dataset() -> VisualDataset {
        VisualDataset {
            columns: vec![
                VisualColumn {
                    key: "day".into(),
                    label: "Day".into(),
                    kind: VisualColumnKind::Text,
                    unit: None,
                },
                VisualColumn {
                    key: "value".into(),
                    label: "Signups".into(),
                    kind: VisualColumnKind::Number,
                    unit: Some("people".into()),
                },
            ],
            rows: vec![BTreeMap::from([
                ("day".into(), Value::from("Monday")),
                ("value".into(), Value::Null),
            ])],
            series: vec!["value".into()],
        }
    }
    #[test]
    fn missing_data_stays_null_and_types_are_checked() {
        let mut data = dataset();
        data.validate().unwrap();
        assert!(data.rows[0]["value"].is_null());
        data.rows[0].insert("value".into(), Value::from("unknown"));
        assert!(data.validate().is_err());
        data.rows[0].remove("value");
        assert!(data.validate().is_err());
    }
    #[test]
    fn charts_and_tables_have_different_explicit_limits() {
        let mut data = dataset();
        data.rows = vec![data.rows[0].clone(); MAX_CHART_POINTS + 1];
        assert!(data.validate().unwrap_err().to_string().contains("500"));
        data.series.clear();
        data.validate().unwrap();
        data.rows = vec![data.rows[0].clone(); MAX_TABLE_ROWS + 1];
        assert!(data.validate().unwrap_err().to_string().contains("1,000"));
    }
    #[test]
    fn series_and_column_identities_are_validated() {
        let mut data = dataset();
        data.series.push("value".into());
        assert!(data.validate().is_err());
        data.series = vec!["day".into()];
        assert!(data.validate().is_err());
        data.series.clear();
        data.columns.push(data.columns[0].clone());
        assert!(data.validate().is_err());
    }

    #[test]
    fn publication_requires_provenance_and_matching_update_identifiers() {
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title": "Sample signups", "summary": "Explicit sample data", "kind": "line_chart",
            "openui_lang": "root = TextContent(\"Sample signups\")",
            "datasets": { "signups": dataset() }
        }))
        .unwrap();
        assert!(document
            .validate()
            .unwrap_err()
            .to_string()
            .contains("source"));
        let mut document = document;
        document.sources.push(VisualSource {
            label: "Sample data".into(),
            reference: None,
            retrieved_at: None,
            time_range: Some("Monday".into()),
        });
        document.validate().unwrap();
        let mut input = VisualPresentInput {
            operation_key: "create".into(),
            id: Some(Uuid::new_v4()),
            expected_revision: None,
            document,
        };
        assert!(input.validate().is_err());
        input.expected_revision = Some(1);
        input.validate().unwrap();
        input.document.openui_lang = "x".repeat(MAX_VISUAL_BYTES);
        assert!(input.validate().unwrap_err().to_string().contains("2 MiB"));
    }

    #[test]
    fn series_count_and_saved_state_limits_are_enforced() {
        let mut data = dataset();
        data.series = vec!["value".into(); MAX_CHART_SERIES + 1];
        assert!(data
            .validate()
            .unwrap_err()
            .to_string()
            .contains("eight series"));
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title":"Answer", "summary":"A plain answer", "kind":"summary", "openui_lang":"root = TextContent(\"Answer\")"
        })).unwrap();
        let state = VisualInteraction {
            selected_option_id: Some("invented".into()),
            ..Default::default()
        };
        assert!(state.validate(&document).is_err());
        let state = VisualInteraction {
            fields: serde_json::json!({"large":"x".repeat(MAX_VISUAL_STATE_BYTES)}),
            ..Default::default()
        };
        assert!(state
            .validate(&document)
            .unwrap_err()
            .to_string()
            .contains("64 KiB"));
        let state = VisualInteraction {
            scroll_top: f64::INFINITY,
            ..Default::default()
        };
        assert!(state.validate(&document).is_err());
    }

    fn table_answer() -> VisualAnswer {
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title":"Projects", "summary":"Sample projects", "kind":"table",
            "openui_lang":"root = Answer([t])\nt = DataTable(\"signups\")",
            "datasets": { "signups": dataset() },
            "sources": [{"label":"Sample data"}]
        }))
        .unwrap();
        VisualAnswer {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            revision: 3,
            document,
            interaction: VisualInteraction::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn only_explicit_choices_require_a_composer_response() {
        let mut document = table_answer().document;
        document.title = "Choose how to read this comparison".into();
        for kind in [
            VisualKind::Table,
            VisualKind::LineChart,
            VisualKind::BarChart,
            VisualKind::Summary,
        ] {
            document.kind = kind;
            document.validate().unwrap();
            assert!(!document.requires_user_response());
        }
        document.actions = vec![
            VisualAction {
                id: "last_week".into(),
                label: "Show last week".into(),
                kind: VisualActionKind::FollowUp,
            },
            VisualAction {
                id: "refine".into(),
                label: "Refine this result".into(),
                kind: VisualActionKind::Refine,
            },
        ];
        assert!(!document.requires_user_response());
        document.kind = VisualKind::DesignOptions;
        document.options = ["solid", "outline"]
            .into_iter()
            .map(|id| VisualOption {
                id: id.into(),
                title: id.into(),
                description: String::new(),
                html: format!("<button>{id}</button>"),
            })
            .collect();
        document.actions.push(VisualAction {
            id: "use".into(),
            label: "Use this style".into(),
            kind: VisualActionKind::Select,
        });
        document.validate().unwrap();
        assert!(document.requires_user_response());
    }

    #[test]
    fn composer_refinement_carries_the_current_view_without_a_declared_action() {
        let answer = table_answer();
        let state = VisualInteraction {
            draft: "Only show the blocked ones".into(),
            fields: serde_json::json!({"summary":"Table signups: search \"Mon\"; sorted by Signups descending; 1 of 1 rows shown."}),
            ..Default::default()
        };
        let text = state
            .submission_text(&answer, IMPLICIT_REFINE_ACTION)
            .unwrap();
        assert!(text.contains("Only show the blocked ones"));
        assert!(text.contains("revision 3"));
        assert!(text.contains("Current view: Table signups: search \"Mon\""));
        assert!(state.submission_text(&answer, "invented").is_err());
        let empty = VisualInteraction::default();
        assert!(empty
            .submission_text(&answer, IMPLICIT_REFINE_ACTION)
            .is_err());
        let long = VisualInteraction {
            fields: serde_json::json!({"summary":"é".repeat(MAX_VISUAL_VIEW_SUMMARY_BYTES)}),
            ..Default::default()
        };
        assert!(long.view_summary().unwrap().len() <= MAX_VISUAL_VIEW_SUMMARY_BYTES);
    }

    #[test]
    fn a_maximum_draft_still_fits_once_it_is_repeated_in_the_receipt() {
        let answer = table_answer();
        let mut state = VisualInteraction {
            draft: "x".repeat(32 * 1024),
            ..Default::default()
        };
        state.validate(&answer.document).unwrap();
        let text = state
            .submission_text(&answer, IMPLICIT_REFINE_ACTION)
            .unwrap();
        state.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: IMPLICIT_REFINE_ACTION.into(),
            text,
            status: VisualSubmissionStatus::Sending,
        });
        state.validate(&answer.document).unwrap();
    }

    #[test]
    fn view_state_is_compatible_only_while_dataset_columns_are_unchanged() {
        let answer = table_answer();
        let mut next = answer.document.clone();
        next.title = "Renamed".into();
        next.datasets.get_mut("signups").unwrap().rows.clear();
        assert!(answer.document.view_compatible_with(&next));
        next.datasets.get_mut("signups").unwrap().columns[1].kind = VisualColumnKind::Text;
        assert!(!answer.document.view_compatible_with(&next));
        let mut renamed = answer.document.clone();
        let data = renamed.datasets.remove("signups").unwrap();
        renamed.datasets.insert("daily".into(), data);
        assert!(!answer.document.view_compatible_with(&renamed));
    }
}
