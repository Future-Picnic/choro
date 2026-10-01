//! A dense, read-first relational grid: one sticky header row carrying each
//! column's name, type and key role, aligned monospace cells, and widths
//! derived from the loaded page so every row shares one horizontal plane.
//! Column sizing follows DBFlux's data table (header vs. longest value,
//! clamped), adapted to Choro's tokens.

use gpui::{
    div, prelude::FluentBuilder as _, px, App, Div, FontWeight, Hsla, InteractiveElement as _,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};
use ide_core::{TableColumn, TableRow};
use serde_json::Value;

pub(super) const ROW_H: f32 = 28.;
pub(super) const HEADER_H: f32 = 40.;
pub(super) const GUTTER_W: f32 = 52.;
pub(super) const EDIT_W: f32 = 36.;
const CELL_PAD_X: f32 = 10.;
const MIN_COL_W: f32 = 76.;
const MAX_COL_W: f32 = 360.;
/// Menlo advances ~0.6em; at the 12.5px UI size that is ~7.5px per glyph.
const MONO_CHAR_W: f32 = 7.5;
const UI_CHAR_W: f32 = 7.0;
const TYPE_CHAR_W: f32 = 6.6;
const KEY_ICON_W: f32 = 16.;
/// Longest value, in characters, that still widens its column.
const WIDTH_SAMPLE_CHARS: usize = 44;
const CELL_DISPLAY_CHARS: usize = 200;
const CELL_TOOLTIP_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CellAlign {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct GridColumn {
    pub name: SharedString,
    pub data_type: SharedString,
    pub primary_key: bool,
    pub width: f32,
    pub align: CellAlign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CellValue {
    Null,
    Missing,
    Text(SharedString),
    Number(SharedString),
    Bool(bool),
    Json(SharedString),
}

impl CellValue {
    fn from_json(value: Option<&Value>) -> Self {
        match value {
            None => Self::Missing,
            Some(Value::Null) => Self::Null,
            Some(Value::Bool(value)) => Self::Bool(*value),
            Some(Value::Number(value)) => Self::Number(value.to_string().into()),
            Some(Value::String(value)) => Self::Text(single_line(value).into()),
            Some(other) => Self::Json(other.to_string().into()),
        }
    }

    fn display(&self) -> SharedString {
        match self {
            Self::Null => "NULL".into(),
            Self::Missing => "".into(),
            Self::Bool(value) => if *value { "true" } else { "false" }.into(),
            Self::Number(text) | Self::Text(text) | Self::Json(text) => {
                clip(text, CELL_DISPLAY_CHARS).into()
            }
        }
    }

    fn char_len(&self) -> usize {
        match self {
            Self::Null => 4,
            Self::Missing => 0,
            Self::Bool(value) => {
                if *value {
                    4
                } else {
                    5
                }
            }
            Self::Number(text) | Self::Text(text) | Self::Json(text) => text.chars().count(),
        }
    }

    /// The value as the row inspector shows it: unclipped up to the tooltip
    /// cap, so long text and JSON can be read without opening an editor.
    pub(super) fn inspector_text(&self) -> SharedString {
        match self {
            Self::Null => "NULL".into(),
            Self::Missing => "Not returned".into(),
            Self::Bool(value) => if *value { "true" } else { "false" }.into(),
            Self::Number(text) | Self::Text(text) | Self::Json(text) => {
                clip(text, CELL_TOOLTIP_CHARS).into()
            }
        }
    }

    pub(super) fn is_absent(&self) -> bool {
        matches!(self, Self::Null | Self::Missing)
    }

    /// Full text worth a tooltip when the cell may be visually truncated.
    fn tooltip(&self, width: f32) -> Option<SharedString> {
        let fits = (self.char_len() as f32) * MONO_CHAR_W + CELL_PAD_X * 2. <= width;
        match self {
            Self::Text(text) | Self::Json(text) | Self::Number(text) if !fits => {
                Some(clip(text, CELL_TOOLTIP_CHARS).into())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct GridRow {
    pub cells: Vec<CellValue>,
    /// The backend's row JSON, the identity used for keyed edits.
    pub original: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct GridModel {
    pub columns: Vec<GridColumn>,
    pub rows: Vec<GridRow>,
}

impl GridModel {
    pub fn build(columns: &[TableColumn], rows: &[TableRow]) -> Self {
        let parsed = rows
            .iter()
            .map(|row| {
                let value = serde_json::from_str::<Value>(&row.json).unwrap_or(Value::Null);
                let cells = columns
                    .iter()
                    .map(|column| {
                        CellValue::from_json(
                            value
                                .as_object()
                                .and_then(|object| object.get(&column.name)),
                        )
                    })
                    .collect::<Vec<_>>();
                GridRow {
                    cells,
                    original: row.json.clone(),
                }
            })
            .collect::<Vec<_>>();
        let grid_columns = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let values = parsed.iter().filter_map(|row| row.cells.get(index));
                grid_column(column, values)
            })
            .collect();
        Self {
            columns: grid_columns,
            rows: parsed,
        }
    }

    /// Width of the data columns alone.
    pub fn columns_width(&self) -> f32 {
        self.columns.iter().map(|column| column.width).sum()
    }
}

fn grid_column<'a>(
    column: &TableColumn,
    values: impl Iterator<Item = &'a CellValue>,
) -> GridColumn {
    let mut longest = 0usize;
    let mut non_null = 0usize;
    let mut numeric = 0usize;
    for value in values {
        longest = longest.max(value.char_len().min(WIDTH_SAMPLE_CHARS));
        match value {
            CellValue::Null | CellValue::Missing => {}
            CellValue::Number(_) => {
                non_null += 1;
                numeric += 1;
            }
            _ => non_null += 1,
        }
    }
    let name_w = column.name.chars().count() as f32 * UI_CHAR_W
        + if column.primary_key { KEY_ICON_W } else { 0. };
    let type_w = column.data_type.chars().count().min(28) as f32 * TYPE_CHAR_W;
    let content_w = longest as f32 * MONO_CHAR_W;
    let width = (name_w.max(type_w).max(content_w) + CELL_PAD_X * 2.)
        .clamp(MIN_COL_W, MAX_COL_W)
        .ceil();
    let numeric_column =
        is_numeric_type(&column.data_type) || (non_null > 0 && numeric == non_null);
    GridColumn {
        name: column.name.clone().into(),
        data_type: column.data_type.clone().into(),
        primary_key: column.primary_key,
        width,
        align: if numeric_column {
            CellAlign::End
        } else {
            CellAlign::Start
        },
    }
}

/// Numeric SQL and ClickHouse type names, ignoring `Nullable(..)` wrappers,
/// precision arguments and `unsigned`.
pub(super) fn is_numeric_type(data_type: &str) -> bool {
    let lower = data_type.trim().to_ascii_lowercase();
    let lower = lower
        .strip_prefix("nullable(")
        .map(|inner| inner.trim_end_matches(')'))
        .unwrap_or(&lower);
    if lower.starts_with("interval") {
        return false;
    }
    const NUMERIC: [&str; 16] = [
        "int",
        "smallint",
        "bigint",
        "tinyint",
        "mediumint",
        "integer",
        "numeric",
        "decimal",
        "float",
        "double",
        "real",
        "serial",
        "bigserial",
        "smallserial",
        "money",
        "uint",
    ];
    NUMERIC.iter().any(|prefix| lower.starts_with(prefix))
}

fn single_line(value: &str) -> String {
    if value.contains(['\n', '\r', '\t']) {
        value
            .chars()
            .map(|character| match character {
                '\n' | '\r' | '\t' => ' ',
                other => other,
            })
            .collect()
    } else {
        value.to_string()
    }
}

fn clip(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let clipped = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

/// One header cell: the column name (with a key glyph for primary keys) above
/// its declared type.
pub(super) fn header_cell(column: &GridColumn, cx: &App) -> Div {
    v_flex()
        .flex_none()
        .w(px(column.width))
        .h_full()
        .px(px(CELL_PAD_X))
        .justify_center()
        .gap_0p5()
        .border_r_1()
        .border_color(crate::ui::design::line(cx).opacity(0.28))
        .child(
            h_flex()
                .w_full()
                .min_w(px(0.))
                .gap_1()
                .items_center()
                .when(column.align == CellAlign::End, |row| row.justify_end())
                .when(column.primary_key, |row| {
                    row.child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::KeyRound,
                        crate::ui::design::amber(cx),
                        crate::ui::design::icon_sm(),
                    ))
                })
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx))
                        .child(column.name.clone()),
                ),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .when(column.align == CellAlign::End, |row| row.text_right())
                .font_family(crate::ui::design::FONT_MONO)
                .text_size(crate::ui::design::text_label())
                .text_color(crate::ui::design::t3(cx))
                .child(column.data_type.clone()),
        )
}

/// One body cell. NULL is set in italics at a muted tone so it can never be
/// mistaken for the string "NULL" or an empty string.
pub(super) fn body_cell(
    id: (usize, usize),
    column: &GridColumn,
    value: &CellValue,
    cx: &App,
) -> Stateful<Div> {
    let color = value_color(value, cx);
    let tooltip = value.tooltip(column.width);
    div()
        .id(("db-grid-cell", id.0 * 10_000 + id.1))
        .flex_none()
        .w(px(column.width))
        .h_full()
        .px(px(CELL_PAD_X))
        .flex()
        .items_center()
        .when(column.align == CellAlign::End, |cell| cell.justify_end())
        .border_r_1()
        .border_color(crate::ui::design::line(cx).opacity(0.14))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .font_family(crate::ui::design::FONT_MONO)
                .text_size(crate::ui::design::text_ui())
                .text_color(color)
                .when(matches!(value, CellValue::Null), |text| text.italic())
                .child(value.display()),
        )
        .when_some(tooltip, |cell, text| {
            cell.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
        })
}

pub(super) fn value_color(value: &CellValue, cx: &App) -> Hsla {
    match value {
        CellValue::Null | CellValue::Missing => crate::ui::design::t3(cx).opacity(0.8),
        CellValue::Json(_) => crate::ui::design::t2(cx),
        _ => crate::ui::design::t1(cx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str, primary_key: bool) -> TableColumn {
        TableColumn {
            name: name.into(),
            data_type: data_type.into(),
            nullable: !primary_key,
            editable: true,
            primary_key,
        }
    }

    #[test]
    fn grid_aligns_numbers_and_keeps_null_distinct_from_missing() {
        let columns = [
            column("id", "bigint", true),
            column("email", "text", false),
            column("score", "", false),
        ];
        let rows = [
            TableRow {
                json: r#"{"id":1,"email":"a@example.com","score":9.5}"#.into(),
            },
            TableRow {
                json: r#"{"id":2,"email":null}"#.into(),
            },
        ];
        let grid = GridModel::build(&columns, &rows);
        assert_eq!(grid.columns[0].align, CellAlign::End);
        assert_eq!(grid.columns[1].align, CellAlign::Start);
        assert_eq!(
            grid.columns[2].align,
            CellAlign::End,
            "inferred from values"
        );
        assert_eq!(grid.rows[1].cells[1], CellValue::Null);
        assert_eq!(grid.rows[1].cells[2], CellValue::Missing);
        assert_eq!(grid.rows[0].original, rows[0].json);
    }

    #[test]
    fn column_widths_follow_content_within_bounds() {
        let columns = [
            column("a", "text", false),
            column("payload", "jsonb", false),
        ];
        let long = "x".repeat(400);
        let rows = [TableRow {
            json: format!(r#"{{"a":"b","payload":"{long}"}}"#),
        }];
        let grid = GridModel::build(&columns, &rows);
        assert_eq!(grid.columns[0].width, MIN_COL_W);
        assert!(grid.columns[1].width <= MAX_COL_W);
        assert!(grid.columns[1].width > grid.columns[0].width);
        assert_eq!(
            grid.columns_width(),
            grid.columns[0].width + grid.columns[1].width
        );
    }

    #[test]
    fn numeric_type_detection_handles_dialects() {
        for numeric in [
            "INTEGER",
            "numeric(10,2)",
            "Nullable(UInt64)",
            "Float64",
            "double precision",
        ] {
            assert!(is_numeric_type(numeric), "{numeric}");
        }
        for other in ["interval", "text", "point", "timestamp", "Nullable(String)"] {
            assert!(!is_numeric_type(other), "{other}");
        }
    }

    #[test]
    fn multiline_text_renders_on_one_line_and_long_values_clip() {
        let value = CellValue::from_json(Some(&Value::String("a\nb\tc".into())));
        assert_eq!(value.display().as_ref(), "a b c");
        let long = CellValue::Text("y".repeat(500).into());
        assert!(long.display().ends_with('…'));
        assert!(long.tooltip(120.).is_some());
        assert!(CellValue::Text("short".into()).tooltip(200.).is_none());
    }

    #[test]
    fn inspector_text_shows_more_than_the_cell_and_names_absent_values() {
        let long = CellValue::Text("z".repeat(500).into());
        assert_eq!(long.inspector_text().chars().count(), 500);
        assert_eq!(CellValue::Null.inspector_text().as_ref(), "NULL");
        assert_eq!(CellValue::Missing.inspector_text().as_ref(), "Not returned");
        assert!(CellValue::Null.is_absent());
        assert!(!CellValue::Bool(false).is_absent());
    }
}
