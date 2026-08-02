//! Provider-neutral data exchanged by Choro's native Project Preview inspector.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualElementStyles {
    pub display: String,
    pub color: String,
    pub background_color: String,
    pub font_family: String,
    pub font_size: String,
    pub font_weight: String,
    pub border_radius: String,
    pub padding: String,
    pub margin: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualElementSelection {
    pub selector: String,
    pub tag_name: String,
    pub id: String,
    pub classes: Vec<String>,
    pub text: String,
    pub role: String,
    pub accessible_name: String,
    #[serde(rename = "pageURL")]
    pub page_url: String,
    pub page_title: String,
    pub rect: VisualElementRect,
    pub styles: VisualElementStyles,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAreaSelection {
    #[serde(rename = "pageURL")]
    pub page_url: String,
    pub page_title: String,
    pub rect: VisualElementRect,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReviewSubmission {
    pub id: Uuid,
    pub project_id: crate::ProjectId,
    pub agent_id: Uuid,
    pub preview_id: Option<Uuid>,
    pub url: String,
    pub comment: String,
    pub target_kind: String,
    pub element: Option<VisualElementSelection>,
    pub area: Option<VisualAreaSelection>,
    pub image_base64: String,
    pub created_at: u64,
}
