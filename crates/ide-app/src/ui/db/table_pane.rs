use gpui::{
    div, prelude::FluentBuilder as _, px, AppContext as _, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    v_flex, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _,
};
use ide_core::{DbProvider, SqlHandle, TableColumn, TableFilter, TableRow};
use serde_json::Value;

const PAGE_SIZE: u64 = 50;

pub struct TablePane {
    handle: SqlHandle,
    provider: DbProvider,
    namespace: String,
    table: String,
    prod: bool,
    filter_input: Entity<InputState>,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    page: u64,
    has_more: bool,
    table_editable: bool,
    loading: bool,
    error: Option<SharedString>,
    load_seq: u64,
}

impl TablePane {
    pub fn new(
        handle: SqlHandle,
        namespace: String,
        table: String,
        prod: bool,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        handle.set_read_only(read_only);
        let provider = handle.provider();
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("filter: column=value; another=value")
        });
        cx.subscribe(
            &filter_input,
            |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.page = 0;
                    this.load(cx);
                }
            },
        )
        .detach();
        let mut pane = Self {
            handle,
            provider,
            namespace,
            table,
            prod,
            filter_input,
            columns: Vec::new(),
            rows: Vec::new(),
            page: 0,
            has_more: false,
            table_editable: false,
            loading: false,
            error: None,
            load_seq: 0,
        };
        pane.load(cx);
        pane
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let filters = match parse_filters(self.filter_input.read(cx).value().as_ref()) {
            Ok(filters) => filters,
            Err(error) => {
                self.error = Some(error.into());
                cx.notify();
                return;
            }
        };
        self.loading = true;
        self.error = None;
        self.load_seq += 1;
        let seq = self.load_seq;
        let handle = self.handle.clone();
        let namespace = self.namespace.clone();
        let table = self.table.clone();
        let offset = self.page * PAGE_SIZE;
        cx.spawn(async move |this, cx| {
            let result =
                cx.background_executor()
                    .spawn(async move {
                        handle.fetch_rows(&namespace, &table, &filters, offset, PAGE_SIZE)
                    })
                    .await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.columns = page.columns;
                        this.rows = page.rows;
                        this.has_more = page.has_more;
                        this.table_editable =
                            page.editable && this.columns.iter().any(|column| column.primary_key);
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_row_editor(&mut self, original: String, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| RowEditor::new(original.clone(), window, cx));
        let handle = self.handle.clone();
        let namespace = self.namespace.clone();
        let table = self.table.clone();
        let prod = self.prod;
        let pane = cx.entity().clone();
        let title: SharedString = format!(
            "Edit row — {namespace}.{table}{}",
            if prod { "  ⚠ PROD" } else { "" }
        )
        .into();
        window.open_dialog(cx, move |dialog, _, _| {
            let footer_editor = editor.clone();
            let footer_handle = handle.clone();
            let footer_namespace = namespace.clone();
            let footer_table = table.clone();
            let footer_original = original.clone();
            let footer_pane = pane.clone();
            dialog
                .w(px(760.))
                .title(title.clone())
                .child(editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = footer_editor.clone();
                    let handle = footer_handle.clone();
                    let namespace = footer_namespace.clone();
                    let table = footer_table.clone();
                    let original = footer_original.clone();
                    let pane = footer_pane.clone();
                    let invalid = editor.read(cx).parse_error.is_some();
                    let armed = editor.read(cx).prod_save_armed;
                    let save_label = if prod && armed {
                        "Confirm Save to PROD"
                    } else if prod {
                        "Save to PROD"
                    } else {
                        "Save"
                    };
                    let save = if prod {
                        crate::ui::style::danger_button_compact("db-row-save", save_label)
                    } else {
                        crate::ui::style::primary_button_compact("db-row-save", save_label, cx)
                    }
                    .icon(IconName::Check)
                    .disabled(invalid)
                    .on_click(move |_, window, cx| {
                        if editor.read(cx).parse_error.is_some() {
                            return;
                        }
                        if prod && !editor.read(cx).prod_save_armed {
                            editor.update(cx, |editor, cx| {
                                editor.prod_save_armed = true;
                                cx.notify();
                            });
                            return;
                        }
                        let edited = editor.read(cx).input.read(cx).value().to_string();
                        let handle = handle.clone();
                        let namespace = namespace.clone();
                        let table = table.clone();
                        let original = original.clone();
                        let pane = pane.clone();
                        window.close_dialog(cx);
                        pane.update(cx, |pane, cx| {
                            pane.loading = true;
                            cx.notify();
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_executor()
                                    .spawn(async move {
                                        handle.update_row(&namespace, &table, &original, &edited)
                                    })
                                    .await;
                                this.update(cx, |this, cx| match result {
                                    Ok(()) => this.load(cx),
                                    Err(error) => {
                                        this.loading = false;
                                        this.error = Some(format!("save failed: {error:#}").into());
                                        cx.notify();
                                    }
                                })
                                .ok();
                            })
                            .detach();
                        });
                    });
                    vec![
                        save,
                        crate::ui::style::dialog_neutral_button("db-row-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn render_row(&self, index: usize, row: &TableRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let value = serde_json::from_str::<Value>(&row.json).unwrap_or(Value::Null);
        let object = value.as_object();
        let original = row.json.clone();
        let editable = self.table_editable;
        let mut cells = h_flex().gap_0().items_start();
        for column in &self.columns {
            let value = object
                .and_then(|object| object.get(&column.name))
                .map(display_value)
                .unwrap_or_else(|| "null".into());
            cells = cells.child(
                v_flex()
                    .w(px(190.))
                    .min_w(px(190.))
                    .px_3()
                    .py_2()
                    .gap_1()
                    .border_r_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.5))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(column.name.clone()),
                            )
                            .when(column.primary_key, |header| {
                                header.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::amber(cx))
                                        .child("PK"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t1(cx))
                            .child(value),
                    ),
            );
        }
        h_flex()
            .id(("db-table-row", index))
            .w_full()
            .min_w(px(0.))
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.5))
            .child(
                div()
                    .id(("db-row-cells-scroll", index))
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_x_scroll()
                    .child(cells),
            )
            .child(
                div()
                    .w(px(52.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::ui::style::header_icon_button(
                            ("db-edit-table-row", index),
                            IconName::Inspector,
                            cx,
                        )
                        .tooltip(if editable {
                            "Edit row"
                        } else {
                            "A primary key is required to edit rows"
                        })
                        .disabled(!editable)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_row_editor(original.clone(), window, cx);
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

impl Render for TablePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let red = crate::ui::design::rose(cx);
        let header = h_flex()
            .w_full()
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .bg(crate::ui::design::base(cx))
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(super::provider_brand_mark(self.provider, 22.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{}.{}", self.namespace, self.table)),
                    )
                    .when(self.prod, |row| {
                        row.child(
                            div()
                                .px_1p5()
                                .rounded(crate::ui::design::r_xs())
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(FontWeight::BOLD)
                                .bg(red.opacity(0.85))
                                .text_color(crate::ui::design::on_accent(cx))
                                .child("PROD"),
                        )
                    })
                    .when(self.handle.is_read_only(), |row| {
                        row.child(
                            div()
                                .px_1p5()
                                .rounded(crate::ui::design::r_xs())
                                .text_size(crate::ui::design::text_ui())
                                .bg(crate::ui::design::nav(cx))
                                .text_color(crate::ui::design::t3(cx))
                                .child("READ ONLY"),
                        )
                    }),
            )
            .child(div().flex_1().child(Input::new(&self.filter_input).small()))
            .child(
                crate::ui::style::header_icon_button("db-table-apply-filter", IconName::Search, cx)
                    .tooltip("Apply filter")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = 0;
                        this.load(cx);
                    })),
            )
            .child(
                crate::ui::style::refresh_icon_button("db-table-refresh", cx)
                    .tooltip("Reload")
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
            .child(
                crate::ui::style::header_icon_button("db-table-prev", IconName::ChevronLeft, cx)
                    .tooltip("Previous page")
                    .disabled(self.page == 0 || self.loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = this.page.saturating_sub(1);
                        this.load(cx);
                    })),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(format!("Page {}", self.page + 1)),
            )
            .child(
                crate::ui::style::header_icon_button("db-table-next", IconName::ChevronRight, cx)
                    .tooltip("Next page")
                    .disabled(!self.has_more || self.loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page += 1;
                        this.load(cx);
                    })),
            );

        let content = if self.loading && self.rows.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new())
                .into_any_element()
        } else if let Some(error) = &self.error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .px_8()
                .child(Icon::new(IconName::CircleX).size_6().text_color(red))
                .child(
                    div()
                        .text_color(red)
                        .whitespace_normal()
                        .child(error.clone()),
                )
                .into_any_element()
        } else if self.rows.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(crate::ui::design::t3(cx))
                .child("No rows match this filter")
                .into_any_element()
        } else {
            let rows = self
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| self.render_row(index, row, cx))
                .collect::<Vec<_>>();
            v_flex()
                .id("db-table-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .children(rows)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .child(header)
            .when(self.loading && !self.rows.is_empty(), |pane| {
                pane.child(div().h(px(2.)).w_full().bg(crate::ui::design::accent(cx)))
            })
            .child(content)
    }
}

struct RowEditor {
    input: Entity<InputState>,
    parse_error: Option<SharedString>,
    prod_save_armed: bool,
}

impl RowEditor {
    fn new(json: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .default_value(json.clone())
        });
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let value = this.input.read(cx).value().to_string();
                this.parse_error = validate_row_json(&value).err().map(Into::into);
                this.prod_save_armed = false;
                cx.notify();
            }
        })
        .detach();
        Self {
            input,
            parse_error: validate_row_json(&json).err().map(Into::into),
            prod_save_armed: false,
        }
    }
}

impl Render for RowEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(720.))
            .h(px(520.))
            .gap_2()
            .when_some(self.parse_error.clone(), |view, error| {
                view.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(self.prod_save_armed, |view| {
                view.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child("Click Confirm Save to PROD to write this row."),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(Input::new(&self.input).h_full()),
            )
    }
}

fn parse_filters(input: &str) -> Result<Vec<TableFilter>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    input
        .split(';')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let (column, value) = part
                .split_once('=')
                .ok_or_else(|| format!("Invalid filter '{part}'. Use column=value"))?;
            let column = column.trim();
            if column.is_empty() {
                return Err("Filter column cannot be empty".into());
            }
            Ok(TableFilter {
                column: column.into(),
                value: value.trim().into(),
            })
        })
        .collect()
}

fn validate_row_json(json: &str) -> Result<(), String> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => Err("Row must be a JSON object".into()),
        Err(error) => Err(format!("Invalid JSON: {error}")),
    }
}

fn display_value(value: &Value) -> String {
    let value = match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    };
    let mut chars = value.chars();
    let clipped = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_safe_filter_syntax() {
        let filters = parse_filters("status=active; email=@example.com").unwrap();
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].column, "status");
        assert_eq!(filters[1].value, "@example.com");
        assert!(parse_filters("broken").is_err());
    }
}
