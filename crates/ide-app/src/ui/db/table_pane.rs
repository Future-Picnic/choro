use gpui::{
    div, prelude::FluentBuilder as _, px, AppContext as _, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::{Scrollbar, ScrollbarAxis},
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex, Disableable as _, IconName, Sizable as _, WindowExt as _,
};
use ide_core::{DbObject, DbObjectKind, DbProvider, SqlHandle, TableFilter};

use super::chrome;
use super::data_grid::{self, GridModel, EDIT_W, GUTTER_W, HEADER_H, ROW_H};
use super::row_editor::RowEditor;

#[path = "table_pane/inspector.rs"]
mod inspector;

const PAGE_SIZE: u64 = 50;

#[cfg(all(test, feature = "ui-layout-tests"))]
#[path = "table_pane/layout_tests.rs"]
mod layout_tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PageRequest {
    page: u64,
    filters: Vec<TableFilter>,
}

pub struct TablePane {
    handle: SqlHandle,
    provider: DbProvider,
    namespace: String,
    table: String,
    object_kind: DbObjectKind,
    prod: bool,
    filter_input: Entity<InputState>,
    grid: GridModel,
    page: u64,
    has_more: bool,
    table_editable: bool,
    loading: bool,
    loaded_once: bool,
    error: Option<SharedString>,
    filter_error: Option<SharedString>,
    filters: Vec<TableFilter>,
    pending_request: Option<PageRequest>,
    selected_row: Option<usize>,
    /// Whether a selected row is shown in the side inspector.
    inspector_open: bool,
    /// Round-trip time of the page on screen.
    fetch_ms: Option<u128>,
    load_seq: u64,
    h_scroll: ScrollHandle,
    v_scroll: ScrollHandle,
}

impl TablePane {
    pub fn new(
        handle: SqlHandle,
        namespace: String,
        object: DbObject,
        prod: bool,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        handle.set_read_only(read_only);
        let provider = handle.provider();
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter rows: column=value; other=value")
        });
        cx.subscribe(
            &filter_input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => this.apply_filter(cx),
                InputEvent::Change => {
                    // Re-render so the clear action tracks the draft filter.
                    this.filter_error = None;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        let mut pane = Self {
            handle,
            provider,
            namespace,
            table: object.name,
            object_kind: object.kind,
            prod,
            filter_input,
            grid: GridModel::default(),
            page: 0,
            has_more: false,
            table_editable: false,
            loading: false,
            loaded_once: false,
            error: None,
            filter_error: None,
            filters: Vec::new(),
            pending_request: None,
            selected_row: None,
            inspector_open: true,
            fetch_ms: None,
            load_seq: 0,
            h_scroll: ScrollHandle::new(),
            v_scroll: ScrollHandle::new(),
        };
        pane.load(cx);
        pane
    }

    fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let columns = self
            .grid
            .columns
            .iter()
            .map(|column| column.name.to_string())
            .collect::<Vec<_>>();
        match parse_filters(self.filter_input.read(cx).value().as_ref(), &columns) {
            Ok(filters) => self.load_request(PageRequest { page: 0, filters }, cx),
            Err(error) => {
                self.filter_error = Some(error.into());
                cx.notify();
            }
        }
    }

    fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.filter_error = None;
        self.apply_filter(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_page(self.page, cx);
    }

    fn load_page(&mut self, page: u64, cx: &mut Context<Self>) {
        self.load_request(
            PageRequest {
                page,
                filters: self.filters.clone(),
            },
            cx,
        );
    }

    fn retry_load(&mut self, cx: &mut Context<Self>) {
        let request = self.pending_request.clone().unwrap_or(PageRequest {
            page: self.page,
            filters: self.filters.clone(),
        });
        self.load_request(request, cx);
    }

    fn load_request(&mut self, request: PageRequest, cx: &mut Context<Self>) {
        // Draft input is committed only by Apply/Enter. A failed request keeps
        // the prior grid, its row numbering, and its applied filters intact.
        self.pending_request = Some(request.clone());
        self.loading = true;
        self.error = None;
        self.filter_error = None;
        self.load_seq += 1;
        let seq = self.load_seq;
        let handle = self.handle.clone();
        let namespace = self.namespace.clone();
        let table = self.table.clone();
        let offset = request.page * PAGE_SIZE;
        let filters = request.filters.clone();
        cx.spawn(async move |this, cx| {
            let (result, elapsed) = cx
                .background_executor()
                .spawn(async move {
                    let started = std::time::Instant::now();
                    let result = handle.fetch_rows(&namespace, &table, &filters, offset, PAGE_SIZE);
                    (result, started.elapsed())
                })
                .await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.fetch_ms = Some(elapsed.as_millis());
                        this.grid = GridModel::build(&page.columns, &page.rows);
                        this.has_more = page.has_more;
                        this.table_editable =
                            page.editable && page.columns.iter().any(|column| column.primary_key);
                        this.page = request.page;
                        this.filters = request.filters;
                        this.pending_request = None;
                        this.loaded_once = true;
                        this.selected_row = None;
                        this.v_scroll.set_offset(gpui::point(px(0.), px(0.)));
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

    fn save_row(&mut self, original: String, edited: String, cx: &mut Context<Self>) {
        self.loading = true;
        self.load_seq += 1;
        let seq = self.load_seq;
        let handle = self.handle.clone();
        let namespace = self.namespace.clone();
        let table = self.table.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { handle.update_row(&namespace, &table, &original, &edited) })
                .await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                match result {
                    Ok(()) => this.load(cx),
                    Err(error) => {
                        this.loading = false;
                        this.error = Some(format!("Save failed: {error:#}").into());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_row_editor(&mut self, original: String, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| RowEditor::new(original.clone(), window, cx));
        let prod = self.prod;
        let pane = cx.entity().clone();
        let title: SharedString = if prod {
            format!("Edit row in production: {}.{}", self.namespace, self.table)
        } else {
            format!("Edit row: {}.{}", self.namespace, self.table)
        }
        .into();
        window.open_dialog(cx, move |dialog, _, _| {
            let footer_editor = editor.clone();
            let footer_original = original.clone();
            let footer_pane = pane.clone();
            dialog
                .w(px(760.))
                .title(title.clone())
                .child(editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = footer_editor.clone();
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
                        let original = original.clone();
                        window.close_dialog(cx);
                        pane.update(cx, |pane, cx| pane.save_row(original, edited, cx));
                    });
                    vec![
                        save,
                        crate::ui::style::dialog_neutral_button("db-row-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui::Div {
        let has_filter = !self.filter_input.read(cx).value().trim().is_empty();
        let inspecting = self.inspector_open && self.selected_row.is_some();
        chrome::query_bar(cx)
            .debug_selector(|| "db-grid-toolbar".into())
            .child(
                h_flex()
                    .debug_selector(|| "db-grid-filter-controls".into())
                    .flex_1()
                    .min_w(px(220.))
                    .gap_1p5()
                    .items_center()
                    .child(chrome::query_keyword("WHERE", cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(120.))
                            .child(Input::new(&self.filter_input).small()),
                    )
                    .when(has_filter, |controls| {
                        controls.child(
                            crate::ui::style::header_icon_button(
                                "db-table-clear-filter",
                                IconName::Close,
                                cx,
                            )
                            .tooltip("Clear filter")
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.clear_filter(window, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        crate::ui::style::secondary_button_compact(
                            "db-table-apply-filter",
                            "Apply",
                        )
                        .tooltip("Apply filter (Enter)")
                        .on_click(cx.listener(|this, _, _, cx| this.apply_filter(cx))),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .child(
                        crate::ui::style::refresh_icon_button("db-table-refresh", cx)
                            .tooltip("Reload this page")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .child(
                        crate::ui::style::header_lucide_icon_button(
                            "db-table-inspector-toggle",
                            if inspecting {
                                lucide_icons::Icon::PanelRightClose
                            } else {
                                lucide_icons::Icon::PanelRight
                            },
                            cx,
                        )
                        .tooltip(if self.inspector_open {
                            "Hide row inspector"
                        } else {
                            "Show row inspector"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.inspector_open = !this.inspector_open;
                            cx.notify();
                        })),
                    ),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> gpui::Div {
        let first = self.page * PAGE_SIZE + 1;
        let shown = self.grid.rows.len() as u64;
        let columns = self.grid.columns.len();
        let edit_hint = if self.object_kind == DbObjectKind::View {
            Some("Views are read-only".to_string())
        } else if !self.provider.capabilities().row_editing {
            Some(format!(
                "{} rows are read-only in Choro",
                self.provider.display_name()
            ))
        } else if self.handle.is_read_only() {
            None
        } else if self.table_editable {
            Some("Edit rows by primary key".to_string())
        } else if self.loaded_once {
            Some("No primary key, rows are read-only".to_string())
        } else {
            None
        };
        chrome::status_bar(cx)
            .debug_selector(|| "db-grid-footer".into())
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_3()
                    .items_center()
                    .overflow_hidden()
                    .child(if shown == 0 {
                        chrome::status_text("No rows")
                    } else {
                        chrome::status_figure(format!("{first}–{}", first + shown - 1), "rows", cx)
                    })
                    .child(chrome::status_figure(
                        columns.to_string(),
                        if columns == 1 { "column" } else { "columns" },
                        cx,
                    ))
                    .when(!self.filters.is_empty(), |status| {
                        status.child(chrome::status_figure(
                            self.filters.len().to_string(),
                            if self.filters.len() == 1 {
                                "filter"
                            } else {
                                "filters"
                            },
                            cx,
                        ))
                    })
                    .when_some(self.fetch_ms, |status, ms| {
                        status.child(chrome::status_figure(ms.to_string(), "ms", cx))
                    })
                    .when_some(edit_hint, |status, text| {
                        status.child(
                            div()
                                .id("db-table-edit-hint")
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .child(text.clone())
                                .tooltip(move |window, cx| {
                                    Tooltip::new(text.clone()).build(window, cx)
                                }),
                        )
                    }),
            )
            .child(
                h_flex()
                    .debug_selector(|| "db-grid-pagination".into())
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .child(
                        crate::ui::style::header_icon_button(
                            "db-table-prev",
                            IconName::ChevronLeft,
                            cx,
                        )
                        .tooltip("Previous page")
                        .disabled(self.page == 0 || self.loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_page(this.page.saturating_sub(1), cx);
                        })),
                    )
                    .child(chrome::status_text(format!("Page {}", self.page + 1)))
                    .child(
                        crate::ui::style::header_icon_button(
                            "db-table-next",
                            IconName::ChevronRight,
                            cx,
                        )
                        .tooltip("Next page")
                        .disabled(!self.has_more || self.loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_page(this.page.saturating_add(1), cx);
                        })),
                    ),
            )
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let editable = self.table_editable && !self.handle.is_read_only();
        let lead_w = GUTTER_W + if editable { EDIT_W } else { 0. };
        let content_w = lead_w + self.grid.columns_width();
        let line = crate::ui::design::line(cx);

        let header = h_flex()
            .debug_selector(|| "db-grid-header".into())
            .flex_none()
            .w_full()
            .h(px(HEADER_H))
            .bg(crate::ui::design::nav(cx))
            .border_b_1()
            .border_color(line.opacity(0.36))
            .child(
                div()
                    .flex_none()
                    .w(px(lead_w))
                    .h_full()
                    .border_r_1()
                    .border_color(line.opacity(0.28)),
            )
            .children(
                self.grid
                    .columns
                    .iter()
                    .map(|column| data_grid::header_cell(column, cx)),
            );

        let offset = self.page * PAGE_SIZE;
        let rows = self
            .grid
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let selected = self.selected_row == Some(index);
                let original = row.original.clone();
                h_flex()
                    .id(("db-table-row", index))
                    .when(index == 0, |row| {
                        row.debug_selector(|| "db-grid-first-row".into())
                    })
                    .flex_none()
                    .w_full()
                    .h(px(ROW_H))
                    .border_b_1()
                    .border_color(line.opacity(0.14))
                    .when(selected, |row| row.bg(crate::ui::design::accent_soft(cx)))
                    .when(!selected && index % 2 == 1, |row| {
                        row.bg(crate::ui::design::nav(cx).opacity(0.45))
                    })
                    .when(!selected, |row| {
                        row.hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.6)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_row = if this.selected_row == Some(index) {
                            None
                        } else {
                            Some(index)
                        };
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(GUTTER_W))
                            .h_full()
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_end()
                            .border_r_1()
                            .border_color(line.opacity(0.28))
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(if selected {
                                crate::ui::design::accent(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .child((offset + index as u64 + 1).to_string()),
                    )
                    .when(editable, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .w(px(EDIT_W))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_r_1()
                                .border_color(line.opacity(0.28))
                                .child(
                                    crate::ui::style::header_icon_button(
                                        ("db-edit-table-row", index),
                                        IconName::Inspector,
                                        cx,
                                    )
                                    .tooltip("Edit row")
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.open_row_editor(original.clone(), window, cx);
                                        },
                                    )),
                                ),
                        )
                    })
                    .children(self.grid.columns.iter().zip(&row.cells).enumerate().map(
                        |(column_index, (column, value))| {
                            data_grid::body_cell((index, column_index), column, value, cx)
                        },
                    ))
            })
            .collect::<Vec<_>>();

        let body = div()
            .debug_selector(|| "db-grid-body".into())
            .id("db-table-body")
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.v_scroll)
            .map(|mut body| {
                body.style().restrict_scroll_to_axis = Some(true);
                body
            })
            .when(rows.is_empty(), |body| {
                body.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child("This table has no rows."),
                )
            })
            .child(v_flex().w_full().children(rows));

        div()
            .id("db-table-grid")
            .debug_selector(|| "db-table-grid".into())
            .relative()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .when(self.loading, |grid| grid.opacity(0.6))
            .child(
                div()
                    .id("db-table-hscroll")
                    .size_full()
                    .flex()
                    .flex_row()
                    .overflow_x_scroll()
                    .track_scroll(&self.h_scroll)
                    .map(|mut area| {
                        area.style().restrict_scroll_to_axis = Some(true);
                        area
                    })
                    .child(
                        v_flex()
                            .flex_none()
                            .h_full()
                            .w(px(content_w))
                            .min_w_full()
                            .child(header)
                            .child(body),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(HEADER_H))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(
                        Scrollbar::new(&self.v_scroll)
                            .id("db-table-vbar")
                            .axis(ScrollbarAxis::Vertical),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(
                        Scrollbar::new(&self.h_scroll)
                            .id("db-table-hbar")
                            .axis(ScrollbarAxis::Horizontal),
                    ),
            )
            .into_any_element()
    }

    fn render_message(
        &self,
        icon: lucide_icons::Icon,
        tone: gpui::Hsla,
        title: SharedString,
        detail: Option<SharedString>,
        action: Option<gpui_component::button::Button>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        chrome::state_message(icon, tone, title, detail, action, cx)
    }
}

impl Render for TablePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.loading && !self.loaded_once {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new())
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(format!("Loading {}.{}", self.namespace, self.table)),
                )
                .into_any_element()
        } else if let Some(error) = self.error.clone().filter(|_| !self.loaded_once) {
            let retry = crate::ui::style::refresh_button("db-table-retry", "Retry", cx)
                .on_click(cx.listener(|this, _, _, cx| this.retry_load(cx)));
            self.render_message(
                lucide_icons::Icon::AlertTriangle,
                crate::ui::design::rose(cx),
                "Rows could not be loaded".into(),
                Some(error),
                Some(retry),
                cx,
            )
        } else if self.grid.rows.is_empty() && !self.filters.is_empty() {
            let clear =
                crate::ui::style::dialog_neutral_button("db-table-empty-clear", "Clear filter", cx)
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filter(window, cx)));
            self.render_message(
                lucide_icons::Icon::FilterX,
                crate::ui::design::t3(cx),
                "No rows match this filter".into(),
                Some("Filters compare each column to an exact value.".into()),
                Some(clear),
                cx,
            )
        } else if self.grid.rows.is_empty() && self.page > 0 {
            let back = crate::ui::style::dialog_neutral_button(
                "db-table-empty-first-page",
                "Back to first page",
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.load_page(0, cx);
            }));
            self.render_message(
                lucide_icons::Icon::TableProperties,
                crate::ui::design::t3(cx),
                "No more rows".into(),
                None,
                Some(back),
                cx,
            )
        } else if self.grid.rows.is_empty() && self.grid.columns.is_empty() {
            self.render_message(
                lucide_icons::Icon::TableProperties,
                crate::ui::design::t3(cx),
                "This table is empty".into(),
                None,
                None,
                cx,
            )
        } else {
            self.render_grid(cx)
        };
        let inspector = self
            .selected_row
            .filter(|_| self.inspector_open)
            .and_then(|index| self.render_inspector(index, cx));

        v_flex()
            .debug_selector(|| "db-grid-pane".into())
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(self.render_toolbar(cx))
            .when_some(
                self.error.clone().filter(|_| self.loaded_once),
                |pane, error| {
                    pane.child(chrome::inline_notice(
                        lucide_icons::Icon::AlertTriangle,
                        crate::ui::design::rose(cx),
                        error,
                        Some(
                            crate::ui::style::refresh_button("db-table-inline-retry", "Retry", cx)
                                .on_click(cx.listener(|this, _, _, cx| this.retry_load(cx))),
                        ),
                        cx,
                    ))
                },
            )
            .when_some(self.filter_error.clone(), |pane, error| {
                pane.child(chrome::inline_notice(
                    lucide_icons::Icon::CircleX,
                    crate::ui::design::rose(cx),
                    error,
                    None,
                    cx,
                ))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .items_start()
                    .child(v_flex().flex_1().min_w(px(0.)).h_full().child(content))
                    .children(inspector),
            )
            .when(self.loaded_once, |pane| pane.child(self.render_footer(cx)))
    }
}

/// Parses `column=value; other=value`. When the table's columns are known,
/// unknown names are rejected before a query is sent.
fn parse_filters(input: &str, known_columns: &[String]) -> Result<Vec<TableFilter>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    input
        .split(';')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let (column, value) = part.split_once('=').ok_or_else(|| {
                format!("Use column=value to filter. \"{}\" has no =.", part.trim())
            })?;
            let column = column.trim();
            if column.is_empty() {
                return Err("Add a column name before =.".into());
            }
            if !known_columns.is_empty() && !known_columns.iter().any(|known| known == column) {
                return Err(format!(
                    "No column named \"{column}\". Columns: {}.",
                    column_list(known_columns)
                ));
            }
            Ok(TableFilter {
                column: column.into(),
                value: value.trim().into(),
            })
        })
        .collect()
}

fn column_list(columns: &[String]) -> String {
    const SHOWN: usize = 8;
    let listed = columns
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if columns.len() > SHOWN {
        format!("{listed}, and {} more", columns.len() - SHOWN)
    } else {
        listed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_safe_filter_syntax() {
        let filters = parse_filters("status=active; email=@example.com", &[]).unwrap();
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].column, "status");
        assert_eq!(filters[1].value, "@example.com");
        assert!(parse_filters("broken", &[]).is_err());
    }

    #[test]
    fn rejects_unknown_columns_once_columns_are_known() {
        let known = vec!["id".to_string(), "status".to_string()];
        assert!(parse_filters("status=active", &known).is_ok());
        let error = parse_filters("state=active", &known).unwrap_err();
        assert!(error.contains("state"));
        assert!(error.contains("id, status"));
    }

    #[test]
    fn long_column_lists_are_summarized() {
        let columns = (0..12).map(|index| format!("c{index}")).collect::<Vec<_>>();
        assert!(column_list(&columns).ends_with("and 4 more"));
    }
}
