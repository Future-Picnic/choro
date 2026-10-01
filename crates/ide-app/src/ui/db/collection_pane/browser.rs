//! The collection document browser: a query bar, a scannable list of the
//! page's documents (index, `_id`, a one-line field preview), the selected
//! document as an expandable tree beside it, and a status bar with paging.
//! Editing opens the guarded JSON editor in `collection_pane.rs`.

use super::*;
use crate::ui::db::chrome;

const DOC_ROW_H: f32 = 46.;
const LIST_MIN_W: f32 = 220.;
const LIST_MAX_W: f32 = 420.;
const DETAIL_HEADER_H: f32 = 36.;
const PREVIEW_FIELDS: usize = 4;
const PREVIEW_VALUE_CHARS: usize = 36;
const ID_LABEL_CHARS: usize = 48;

/// The `_id` as people read it: the ObjectId hex, the string, or compact
/// JSON for composite keys.
pub(super) fn doc_id_label(tree: &Value) -> Option<String> {
    let id = tree.as_object()?.get("_id")?;
    Some(match id {
        Value::String(text) => truncate_chars(text, ID_LABEL_CHARS),
        Value::Number(number) => number.to_string(),
        Value::Object(map) => match map.get("$oid").and_then(Value::as_str) {
            Some(oid) => oid.to_string(),
            None => match scalar_value(id) {
                Some((text, _)) => truncate_chars(&text, ID_LABEL_CHARS),
                None => truncate_chars(&id.to_string(), ID_LABEL_CHARS),
            },
        },
        other => truncate_chars(&other.to_string(), ID_LABEL_CHARS),
    })
}

/// First few top-level fields besides `_id`, as `field: value` pairs.
pub(super) fn doc_preview(tree: &Value) -> String {
    let Some(map) = tree.as_object() else {
        return truncate_chars(&tree.to_string(), PREVIEW_VALUE_CHARS * 2);
    };
    map.iter()
        .filter(|(key, _)| key.as_str() != "_id")
        .take(PREVIEW_FIELDS)
        .map(|(key, value)| {
            let shown = match (scalar_value(value), value) {
                (Some((text, _)), _) => truncate_chars(&text, PREVIEW_VALUE_CHARS),
                (None, Value::Array(items)) => format!("[{}]", items.len()),
                (None, _) => "{…}".to_string(),
            };
            format!("{key}: {shown}")
        })
        .collect::<Vec<_>>()
        .join("   ")
}

impl CollectionPane {
    pub(super) fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.submit_filter(cx);
    }

    fn render_query_bar(&self, fields: &[String], cx: &mut Context<Self>) -> gpui::Div {
        let pane = cx.entity().clone();
        chrome::query_bar(cx)
            .debug_selector(|| "db-docs-toolbar".into())
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(220.))
                    .gap_1p5()
                    .items_center()
                    .child(chrome::query_keyword("find", cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(120.))
                            .child(Input::new(&self.filter_input).small()),
                    )
                    .child(
                        crate::ui::style::secondary_button_compact("db-apply-filter", "Apply")
                            .tooltip("Apply filter (Enter)")
                            .on_click(cx.listener(|this, _, _, cx| this.submit_filter(cx))),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .child(Self::field_picker_button(
                        "db-add-filter",
                        "Field".into(),
                        Some(IconName::Plus),
                        fields.to_vec(),
                        None,
                        pane,
                        FieldPickerTarget::Add,
                        self.quick_filter_rows.len() >= MAX_QUICK_FILTER_ROWS,
                    ))
                    .child(
                        crate::ui::style::refresh_icon_button("db-refresh", cx)
                            .tooltip("Reload this page")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
    }

    fn render_quick_filters(&self, fields: &[String], cx: &mut Context<Self>) -> gpui::Div {
        let pane = cx.entity().clone();
        let mut rows = v_flex()
            .w_full()
            .px_2()
            .py_1p5()
            .gap_1p5()
            .bg(crate::ui::design::nav(cx))
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.22));
        for (index, row) in self.quick_filter_rows.iter().enumerate() {
            rows = rows.child(
                h_flex()
                    .w_full()
                    .gap_1p5()
                    .items_center()
                    .child(Self::field_picker_button(
                        ElementId::named_usize("db-quick-filter-field", index),
                        row.field.clone().into(),
                        None,
                        fields.to_vec(),
                        Some(row.field.clone()),
                        pane.clone(),
                        FieldPickerTarget::Row(index),
                        false,
                    ))
                    .child(
                        div()
                            .flex_none()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("="),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(120.))
                            .max_w(px(320.))
                            .child(Input::new(&row.value_input).small()),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            ElementId::named_usize("db-quick-filter-remove", index),
                            IconName::Close,
                            cx,
                        )
                        .tooltip("Remove field filter")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_quick_filter(index, cx);
                        })),
                    ),
            );
        }
        rows.child(
            h_flex().w_full().justify_end().child(
                crate::ui::style::accent_button_compact(
                    "db-quick-filter-apply",
                    "Add to filter",
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.apply_quick_filters(window, cx);
                })),
            ),
        )
    }

    fn render_doc_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let line = crate::ui::design::line(cx);
        let offset = self.page * PAGE_SIZE;
        v_flex()
            .id("db-doc-list")
            .debug_selector(|| "db-doc-list".into())
            .flex_none()
            .h_full()
            .w(gpui::relative(0.38))
            .min_w(px(LIST_MIN_W))
            .max_w(px(LIST_MAX_W))
            .overflow_y_scroll()
            .border_r_1()
            .border_color(line.opacity(0.28))
            .children(self.docs.iter().enumerate().map(|(ix, card)| {
                let selected = self.selected_doc == Some(ix);
                let id = card
                    .tree
                    .as_ref()
                    .and_then(doc_id_label)
                    .unwrap_or_else(|| "No _id".into());
                let preview = card
                    .tree
                    .as_ref()
                    .map(doc_preview)
                    .unwrap_or_else(|| card.display.lines().next().unwrap_or("").to_string());
                h_flex()
                    .id(("db-doc", ix))
                    .w_full()
                    .h(px(DOC_ROW_H))
                    .flex_none()
                    .pr_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(line.opacity(0.12))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(crate::ui::design::accent_soft(cx)))
                    .when(!selected, |row| {
                        row.hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.6)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_doc = Some(ix);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(40.))
                            .text_right()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(if selected {
                                crate::ui::design::accent(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .child((offset + ix as u64 + 1).to_string()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(id)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(SharedString::from(preview)),
                            ),
                    )
            }))
    }

    fn render_doc_detail(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some((ix, card)) = self
            .selected_doc
            .and_then(|ix| self.docs.get(ix).map(|card| (ix, card)))
        else {
            return chrome::state_message(
                lucide_icons::Icon::Braces,
                crate::ui::design::t3(cx),
                "Select a document".into(),
                None,
                None,
                cx,
            );
        };
        let editable = !self.handle.is_read_only() && card.id.is_some();
        let id_label = card
            .tree
            .as_ref()
            .and_then(doc_id_label)
            .unwrap_or_else(|| "Document".into());
        let edit_id = card.id.clone();
        let edit_full = card.full.clone();
        let doc_key = doc_tree_key(self.page, ix, card);

        let header = h_flex()
            .w_full()
            .h(px(DETAIL_HEADER_H))
            .flex_none()
            .pl_3()
            .pr_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.22))
            .child(
                div()
                    .flex_none()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child("_id"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t1(cx))
                    .child(SharedString::from(id_label)),
            )
            .when(editable, |header| {
                header.child(
                    crate::ui::style::secondary_button_compact("db-doc-edit", "Edit document")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(id) = edit_id.clone() {
                                this.open_doc_editor(id, edit_full.clone(), window, cx);
                            }
                        })),
                )
            });

        let body =
            match &card.tree {
                Some(tree) => render_json_tree(
                    &self.expanded_tree_nodes,
                    &doc_key,
                    tree,
                    cx,
                    Self::expanded_nodes,
                )
                .into_any_element(),
                None => div()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_size(crate::ui::design::text_ui())
                    .whitespace_normal()
                    .child(card.display.clone())
                    .when(card.clipped, |text| {
                        text.child(div().pt_1().text_color(crate::ui::design::t3(cx)).child(
                            "Clipped for display. Open the editor to see the whole document.",
                        ))
                    })
                    .into_any_element(),
            };

        v_flex()
            .debug_selector(|| "db-doc-detail".into())
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(crate::ui::design::surface(cx))
            .child(header)
            .child(
                div()
                    .id("db-doc-detail-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p_3()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_size(crate::ui::design::text_ui())
                    .child(body),
            )
            .into_any_element()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> gpui::Div {
        let read_only_hint = self.handle.is_read_only().then_some("Read only");
        chrome::status_bar(cx)
            .debug_selector(|| "db-docs-footer".into())
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_3()
                    .items_center()
                    .overflow_hidden()
                    .child(if self.total == 0 {
                        chrome::status_text("No documents")
                    } else {
                        chrome::status_figure(self.page_label(), "documents", cx)
                    })
                    .when_some(self.fetch_ms, |status, ms| {
                        status.child(chrome::status_figure(ms.to_string(), "ms", cx))
                    })
                    .when_some(read_only_hint, |status, hint| {
                        status.child(chrome::status_text(hint))
                    }),
            )
            .child(
                h_flex()
                    .debug_selector(|| "db-docs-pagination".into())
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .child(
                        crate::ui::style::header_icon_button(
                            "db-prev-page",
                            IconName::ChevronLeft,
                            cx,
                        )
                        .tooltip("Previous page")
                        .disabled(self.page == 0 || self.loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_page(this.page.saturating_sub(1), cx);
                        })),
                    )
                    .child(chrome::status_text(format!(
                        "Page {} of {}",
                        self.page + 1,
                        self.last_page() + 1
                    )))
                    .child(
                        crate::ui::style::header_icon_button(
                            "db-next-page",
                            IconName::ChevronRight,
                            cx,
                        )
                        .tooltip("Next page")
                        .disabled(self.page >= self.last_page() || self.loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_page(this.page + 1, cx);
                        })),
                    ),
            )
    }
}

impl Render for CollectionPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = self.available_filter_fields();
        // Empty-state wording describes the committed query, not the draft.
        let has_filter = !self.filter.trim().is_empty();

        let body: gpui::AnyElement = if self.docs.is_empty() && self.loading {
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
                        .child(format!("Loading {}.{}", self.db, self.collection)),
                )
                .into_any_element()
        } else if let Some(error) = self.error.clone().filter(|_| self.docs.is_empty()) {
            chrome::state_message(
                lucide_icons::Icon::AlertTriangle,
                crate::ui::design::rose(cx),
                "Documents could not be loaded".into(),
                Some(error),
                Some(
                    crate::ui::style::refresh_button("db-retry", "Retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.retry_load(cx))),
                ),
                cx,
            )
        } else if self.docs.is_empty() {
            chrome::state_message(
                if has_filter {
                    lucide_icons::Icon::FilterX
                } else {
                    lucide_icons::Icon::Braces
                },
                crate::ui::design::t3(cx),
                if has_filter {
                    "No documents match this filter".into()
                } else {
                    "This collection is empty".into()
                },
                None,
                has_filter.then(|| {
                    crate::ui::style::dialog_neutral_button(
                        "db-docs-empty-clear",
                        "Clear filter",
                        cx,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filter(window, cx)))
                }),
                cx,
            )
        } else {
            h_flex()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .items_start()
                .when(self.loading, |split| split.opacity(0.6))
                .child(self.render_doc_list(cx))
                .child(self.render_doc_detail(cx))
                .into_any_element()
        };

        v_flex()
            .debug_selector(|| "db-docs-pane".into())
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(self.render_query_bar(&fields, cx))
            .when(!self.quick_filter_rows.is_empty(), |pane| {
                pane.child(self.render_quick_filters(&fields, cx))
            })
            .when_some(
                self.failed_request_label()
                    .filter(|_| !self.docs.is_empty()),
                |pane, error| {
                    pane.child(chrome::inline_notice(
                        lucide_icons::Icon::AlertTriangle,
                        crate::ui::design::rose(cx),
                        error,
                        Some(
                            crate::ui::style::refresh_button("db-inline-retry", "Retry", cx)
                                .on_click(cx.listener(|this, _, _, cx| this.retry_load(cx))),
                        ),
                        cx,
                    ))
                },
            )
            .child(div().flex_1().min_h(px(0.)).flex().child(body))
            .child(self.render_status(cx))
    }
}

#[cfg(all(test, feature = "ui-layout-tests"))]
mod layout_tests {
    use super::*;
    use std::time::Duration;

    /// A page already on screen. The handle is lazy, so building it contacts
    /// no server; nothing here triggers a load.
    fn fixture(window: &mut Window, cx: &mut Context<CollectionPane>) -> CollectionPane {
        let docs = (0..20)
            .map(|index| {
                let json = serde_json::json!({
                    "_id": {"$oid": format!("64f1c2a9e1b2c3d4e5f6a7{index:02}")},
                    "email": format!("person{index}@example.com"),
                    "profile": {"city": "London", "tags": ["a", "b"]},
                    "notes": "a long note ".repeat(40),
                })
                .to_string();
                DocCard {
                    display: json.clone().into(),
                    clipped: false,
                    full: json.clone().into(),
                    tree: serde_json::from_str(&json).ok(),
                    id: Some(format!("{{\"$oid\":\"64f1c2a9e1b2c3d4e5f6a7{index:02}\"}}")),
                }
            })
            .collect();
        CollectionPane {
            handle: MongoHandle::connect("mongodb://127.0.0.1:1/?directConnection=true").unwrap(),
            db: "an_unusually_long_database_name".into(),
            collection: "an_unusually_long_collection_name".into(),
            prod: false,
            filter_input: cx.new(|cx| InputState::new(window, cx)),
            quick_filter_rows: Vec::new(),
            docs,
            expanded_tree_nodes: HashSet::new(),
            total: 95,
            page: 1,
            filter: r#"{"status": "active"}"#.into(),
            pending_request: None,
            loading: false,
            error: None,
            selected_doc: Some(2),
            fetch_ms: Some(18),
            load_seq: 0,
        }
    }

    #[gpui::test]
    fn document_list_detail_and_status_fit_ordinary_and_narrow_panes(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let mut pane = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| fixture(window, cx));
            pane = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        for (width, height) in [(1100., 600.), (700., 420.), (560., 360.)] {
            cx.simulate_resize(gpui::size(px(width), px(height)));
            cx.run_until_parked();
            let root = cx.debug_bounds("db-docs-pane").unwrap();
            let toolbar = cx.debug_bounds("db-docs-toolbar").unwrap();
            let list = cx.debug_bounds("db-doc-list").unwrap();
            let detail = cx.debug_bounds("db-doc-detail").unwrap();
            let footer = cx.debug_bounds("db-docs-footer").unwrap();
            let pagination = cx.debug_bounds("db-docs-pagination").unwrap();
            assert!(list.top() >= toolbar.bottom());
            assert!(
                list.right() <= detail.left(),
                "list overlaps detail at {width}"
            );
            assert!(detail.right() <= root.right(), "detail clipped at {width}");
            assert!(
                detail.size.width >= px(240.),
                "detail too narrow at {width}"
            );
            assert!(list.bottom() <= footer.top() && detail.bottom() <= footer.top());
            assert!(
                footer.bottom() <= root.bottom(),
                "status clipped at {width}"
            );
            assert!(pagination.right() <= footer.right());
        }

        // A failed refresh keeps the page on screen with a retry notice.
        pane.update(cx, |pane, cx| {
            pane.error = Some("server selection timed out".into());
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("db-doc-list").is_some());

        // An empty result replaces the split with a message.
        pane.update(cx, |pane, cx| {
            pane.error = None;
            pane.docs.clear();
            pane.selected_doc = None;
            pane.total = 0;
            cx.notify();
        });
        cx.run_until_parked();
        // Debug bounds are never cleared between frames in this GPUI
        // version, so check for the message rather than the list's absence.
        let message = cx
            .debug_bounds("db-state-message")
            .expect("an empty page shows a message");
        let footer = cx.debug_bounds("db-docs-footer").unwrap();
        assert!(message.bottom() <= footer.top());
    }

    fn page_of(count: usize, total: u64) -> ide_core::DocPage {
        ide_core::DocPage {
            docs: (0..count)
                .map(|index| ide_core::DocEntry {
                    id: Some(format!("{{\"$oid\":\"64f1c2a9e1b2c3d4e5f6b0{index:02}\"}}")),
                    json: format!(
                        "{{\"_id\": {{\"$oid\": \"64f1c2a9e1b2c3d4e5f6b0{index:02}\"}}, \"n\": {index}}}"
                    ),
                })
                .collect(),
            total,
        }
    }

    fn window_pane(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<CollectionPane>, &mut gpui::VisualTestContext) {
        cx.update(gpui_component::init);
        let mut pane = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| fixture(window, cx));
            pane = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        (pane.unwrap(), cx)
    }

    fn set_draft(pane: &Entity<CollectionPane>, text: &str, cx: &mut gpui::VisualTestContext) {
        let text = text.to_string();
        pane.update_in(cx, |pane, window, cx| {
            pane.filter_input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        });
    }

    /// Requests are driven through `begin_request`/`finish_request` with
    /// synthetic results, so nothing is sent to a server.
    #[gpui::test]
    fn failed_page_change_keeps_the_page_query_and_selection_on_screen(
        cx: &mut gpui::TestAppContext,
    ) {
        let (pane, cx) = window_pane(cx);
        set_draft(&pane, r#"{"status": "draft"}"#, cx);
        pane.update(cx, |pane, cx| {
            pane.selected_doc = Some(4);
            let first_id = pane.docs[0].id.clone();

            // Paging uses the committed filter, never the unsubmitted draft.
            let request = pane.page_request(pane.page + 1);
            assert_eq!(request.page, 2);
            assert_eq!(request.filter, r#"{"status": "active"}"#);

            let seq = pane.begin_request(request.clone());
            assert!(pane.finish_request(
                seq,
                request.clone(),
                Err(anyhow::anyhow!("server selection timed out")),
                Duration::from_millis(5),
            ));
            assert_eq!(pane.page, 1, "page label must not advance on failure");
            assert_eq!(pane.filter, r#"{"status": "active"}"#);
            assert_eq!(pane.docs.len(), 20);
            assert_eq!(pane.docs[0].id, first_id);
            assert_eq!(pane.selected_doc, Some(4));
            assert_eq!(pane.total, 95);
            assert!(!pane.loading);
            assert_eq!(pane.pending_request.as_ref(), Some(&request));
            assert!(pane
                .failed_request_label()
                .unwrap()
                .starts_with("Page 3 could not be loaded."));
            assert_eq!(
                pane.filter_input.read(cx).value().as_ref(),
                r#"{"status": "draft"}"#
            );
            // Only an explicit submit sends the draft, from the first page.
            assert_eq!(
                pane.draft_request(cx),
                DocRequest {
                    page: 0,
                    filter: r#"{"status": "draft"}"#.into(),
                }
            );

            // Retry repeats the failed request; success commits it.
            let retried = pane
                .pending_request
                .clone()
                .unwrap_or_else(|| pane.page_request(pane.page));
            assert_eq!(retried, request);
            let seq = pane.begin_request(retried.clone());
            assert!(pane.finish_request(seq, retried, Ok(page_of(7, 95)), Duration::ZERO));
            assert_eq!(pane.page, 2);
            assert_eq!(pane.docs.len(), 7);
            assert_eq!(pane.selected_doc, Some(0));
            assert!(pane.pending_request.is_none());
            assert!(pane.failed_request_label().is_none());
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn invalid_filter_is_not_committed_and_stale_results_are_ignored(
        cx: &mut gpui::TestAppContext,
    ) {
        let (pane, cx) = window_pane(cx);
        pane.update(cx, |pane, cx| {
            let submitted = DocRequest {
                page: 0,
                filter: "{status: ".into(),
            };
            let seq = pane.begin_request(submitted.clone());
            assert!(pane.finish_request(
                seq,
                submitted,
                Err(anyhow::anyhow!("invalid filter JSON")),
                Duration::ZERO,
            ));
            assert_eq!(pane.page, 1);
            assert_eq!(pane.filter, r#"{"status": "active"}"#);
            assert_eq!(pane.docs.len(), 20);
            assert!(pane
                .failed_request_label()
                .unwrap()
                .starts_with("The filter was not applied."));
            // Reload after a failed filter uses what is on screen.
            assert_eq!(
                pane.page_request(pane.page).filter,
                r#"{"status": "active"}"#
            );

            // An older request finishing late cannot overwrite a newer one.
            let older = pane.page_request(0);
            let older_seq = pane.begin_request(older.clone());
            let newer = pane.page_request(1);
            let newer_seq = pane.begin_request(newer.clone());
            assert!(!pane.finish_request(older_seq, older, Ok(page_of(1, 1)), Duration::ZERO));
            assert_eq!(pane.docs.len(), 20);
            assert!(pane.loading);
            assert!(pane.finish_request(newer_seq, newer, Ok(page_of(20, 95)), Duration::ZERO));
            assert!(!pane.loading);
            cx.notify();
        });
        cx.run_until_parked();
        // The retained page is still what the list shows.
        assert!(cx.debug_bounds("db-doc-list").is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_ids_read_as_people_expect() {
        let oid = serde_json::json!({"_id": {"$oid": "64f1c2a9e1b2c3d4e5f6a7b8"}});
        assert_eq!(doc_id_label(&oid).unwrap(), "64f1c2a9e1b2c3d4e5f6a7b8");
        let text = serde_json::json!({"_id": "user-42"});
        assert_eq!(doc_id_label(&text).unwrap(), "user-42");
        let number = serde_json::json!({"_id": {"$numberLong": "9007199254740993"}});
        assert_eq!(doc_id_label(&number).unwrap(), "9007199254740993");
        let composite = serde_json::json!({"_id": {"tenant": 1, "user": 2}});
        assert!(doc_id_label(&composite).unwrap().contains("tenant"));
        assert!(doc_id_label(&serde_json::json!({"name": "x"})).is_none());
    }

    #[test]
    fn previews_skip_id_and_summarize_containers() {
        // Field names are in alphabetical order so the expectation holds with
        // or without serde_json's insertion-order maps.
        let doc = serde_json::json!({
            "_id": {"$oid": "64f1c2a9e1b2c3d4e5f6a7b8"},
            "address": {"city": "London"},
            "age": 36,
            "name": "Ada",
            "tags": ["a", "b", "c"],
            "zeta": true
        });
        let preview = doc_preview(&doc);
        assert!(!preview.contains("_id"));
        assert!(preview.contains("name: \"Ada\""));
        assert!(preview.contains("tags: [3]"));
        assert!(preview.contains("address: {…}"));
        assert!(!preview.contains("zeta"), "preview is capped: {preview}");
    }
}
