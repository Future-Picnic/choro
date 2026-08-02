use std::collections::{BTreeSet, HashSet};
use std::hash::{Hash, Hasher};

use gpui::{
    div, prelude::FluentBuilder, px, AppContext, Context, ElementId, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    v_flex, Disableable, Icon, IconName, Sizable,
};
use ide_core::{DbProvider, MongoHandle};
use serde_json::Value;

const PAGE_SIZE: u64 = 20;

/// One rendered document: clipped preview for fast layout, full JSON and
/// `_id` kept aside for the edit dialog.
struct DocCard {
    display: SharedString,
    clipped: bool,
    full: SharedString,
    tree: Option<Value>,
    /// Canonical extended JSON of `_id`; None = not editable.
    id: Option<String>,
}

/// Rendering caps per document card: laying out huge JSON blobs is what
/// makes scrolling crawl, so clip what's displayed (data stays intact).
const MAX_DOC_LINES: usize = 24;
const MAX_DOC_CHARS: usize = 2_500;
const MAX_SCALAR_CHARS: usize = 180;
const MAX_EXPANDED_ARRAY_ITEMS: usize = 200;
const MAX_FILTER_FIELDS: usize = 48;
const MAX_FILTER_FIELD_DEPTH: usize = 2;
const MAX_QUICK_FILTER_ROWS: usize = 8;

struct QuickFilterRow {
    field: String,
    value_input: Entity<InputState>,
}

#[derive(Clone, Copy)]
enum FieldPickerTarget {
    Add,
    Row(usize),
}

fn is_extjson_wrapper(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 1
        && map.keys().next().is_some_and(|key| {
            matches!(
                key.as_str(),
                "$oid"
                    | "$date"
                    | "$numberInt"
                    | "$numberLong"
                    | "$numberDouble"
                    | "$numberDecimal"
            )
        })
}

fn collect_filter_fields(
    value: &Value,
    prefix: Option<&str>,
    depth: usize,
    fields: &mut BTreeSet<String>,
) {
    if fields.len() >= MAX_FILTER_FIELDS {
        return;
    }

    let Value::Object(map) = value else {
        return;
    };
    if is_extjson_wrapper(map) {
        return;
    }

    for (key, child) in map {
        if key.starts_with('$') {
            continue;
        }

        let path = match prefix {
            Some(prefix) => format!("{prefix}.{key}"),
            None => key.clone(),
        };
        fields.insert(path.clone());

        if fields.len() >= MAX_FILTER_FIELDS {
            break;
        }

        if depth < MAX_FILTER_FIELD_DEPTH {
            collect_filter_fields(child, Some(&path), depth + 1, fields);
        }
    }
}

fn quick_filter_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }

    serde_json::from_str(trimmed).unwrap_or_else(|_| Value::String(trimmed.to_string()))
}

fn quick_filter_json(current_filter: &str, filters: &[(String, String)]) -> String {
    let mut quick_object = serde_json::Map::new();
    for (field, raw_value) in filters {
        quick_object.insert(field.clone(), quick_filter_value(raw_value));
    }
    if quick_object.is_empty() {
        return current_filter.trim().to_string();
    }

    let quick_json =
        serde_json::to_string(&Value::Object(quick_object.clone())).unwrap_or_else(|_| "{}".into());

    let trimmed = current_filter.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return quick_json;
    }

    let mut object = serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| match value {
            Value::Object(object) => Some(object),
            _ => None,
        });

    if let Some(mut object) = object.take() {
        for (field, raw_value) in filters {
            object.insert(field.clone(), quick_filter_value(raw_value));
        }
        return serde_json::to_string(&Value::Object(object)).unwrap_or(quick_json);
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        format!(r#"{{"$and":[{trimmed},{quick_json}]}}"#)
    } else {
        quick_json
    }
}

fn clip_for_display(doc: &str) -> (String, bool) {
    let mut clipped = String::with_capacity(doc.len().min(MAX_DOC_CHARS + 16));
    let mut truncated = false;
    for (ix, line) in doc.lines().enumerate() {
        if ix >= MAX_DOC_LINES || clipped.len() + line.len() > MAX_DOC_CHARS {
            truncated = true;
            break;
        }
        if ix > 0 {
            clipped.push('\n');
        }
        // Very long single lines (embedded blobs/arrays) also kill layout.
        if line.chars().count() > 200 {
            clipped.extend(line.chars().take(200));
            clipped.push('…');
            truncated = true;
        } else {
            clipped.push_str(line);
        }
    }
    (clipped, truncated)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut clipped: String = value.chars().take(max_chars).collect();
    clipped.push('…');
    clipped
}

fn quote_string(value: &str) -> String {
    serde_json::to_string(&truncate_chars(value, MAX_SCALAR_CHARS))
        .unwrap_or_else(|_| "\"<string>\"".into())
}

fn number_wrapper(map: &serde_json::Map<String, Value>) -> Option<String> {
    for key in [
        "$numberInt",
        "$numberLong",
        "$numberDouble",
        "$numberDecimal",
    ] {
        if let Some(number) = map.get(key).and_then(Value::as_str) {
            return Some(number.to_string());
        }
    }
    None
}

#[derive(Clone, Copy)]
enum ValueTone {
    Muted,
    String,
    Number,
    ObjectId,
    Date,
    Keyword,
}

fn scalar_value(value: &Value) -> Option<(String, ValueTone)> {
    match value {
        Value::Null => Some(("null".into(), ValueTone::Keyword)),
        Value::Bool(value) => Some((value.to_string(), ValueTone::Keyword)),
        Value::Number(value) => Some((value.to_string(), ValueTone::Number)),
        Value::String(value) => Some((quote_string(value), ValueTone::String)),
        Value::Array(_) => None,
        Value::Object(map) => {
            if let Some(oid) = map.get("$oid").and_then(Value::as_str) {
                return Some((format!("ObjectId('{oid}')"), ValueTone::ObjectId));
            }

            if let Some(date) = map.get("$date") {
                let label = match date {
                    Value::String(date) => truncate_chars(date, MAX_SCALAR_CHARS),
                    Value::Object(map) => number_wrapper(map)
                        .map(|millis| format!("Date({millis})"))
                        .unwrap_or_else(|| "Date".into()),
                    _ => "Date".into(),
                };
                return Some((label, ValueTone::Date));
            }

            number_wrapper(map).map(|number| (number, ValueTone::Number))
        }
    }
}

fn container_label(value: &Value) -> Option<String> {
    if scalar_value(value).is_some() {
        return None;
    }

    match value {
        Value::Array(values) => Some(format!("Array ({})", values.len())),
        Value::Object(map) => Some(format!("Object ({})", map.len())),
        _ => None,
    }
}

fn doc_tree_key(page: u64, ix: usize, card: &DocCard) -> String {
    card.id
        .clone()
        .unwrap_or_else(|| format!("page:{page}:doc:{ix}"))
}

fn child_path(parent: &str, segment: impl AsRef<str>) -> String {
    if parent.is_empty() {
        segment.as_ref().to_string()
    } else {
        format!("{parent}/{}", segment.as_ref())
    }
}

fn stable_id(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn tone_color<T>(tone: ValueTone, cx: &Context<T>) -> gpui::Hsla {
    match tone {
        ValueTone::Muted => crate::ui::design::t3(cx),
        ValueTone::String => crate::ui::design::sage(cx),
        ValueTone::Number | ValueTone::Date => crate::ui::design::accent(cx),
        ValueTone::ObjectId => crate::ui::design::rose(cx),
        ValueTone::Keyword => crate::ui::design::amber(cx),
    }
}

fn json_value_text<T>(value: &Value, cx: &Context<T>) -> gpui::Div {
    if let Some((text, tone)) = scalar_value(value) {
        div()
            .text_color(tone_color(tone, cx))
            .child(SharedString::from(text))
    } else {
        div()
            .text_color(tone_color(ValueTone::Muted, cx))
            .child(SharedString::from(
                container_label(value).unwrap_or_else(|| "<value>".into()),
            ))
    }
}

fn json_tree_row<T: 'static>(
    expanded_nodes: &HashSet<String>,
    node_key: String,
    label: SharedString,
    value: &Value,
    depth: usize,
    cx: &mut Context<T>,
    expansion_accessor: fn(&mut T) -> &mut HashSet<String>,
) -> impl IntoElement {
    let is_container = container_label(value).is_some();
    let expanded = is_container && expanded_nodes.contains(&node_key);
    let toggle_key = node_key.clone();

    h_flex()
        .id(("db-doc-tree-row", stable_id(&node_key)))
        .w_full()
        .pl(px(6. + depth as f32 * 18.))
        .pr_2()
        .py_0p5()
        .gap_1p5()
        .items_center()
        .rounded(crate::ui::design::r_xs())
        .when(is_container, |row| {
            row.cursor_pointer()
                .hover(|style| style.bg(crate::ui::design::hover(cx).opacity(0.5)))
                .on_click(cx.listener(move |this: &mut T, _, _, cx| {
                    cx.stop_propagation();
                    let expanded_nodes = expansion_accessor(this);
                    if !expanded_nodes.remove(&toggle_key) {
                        expanded_nodes.insert(toggle_key.clone());
                    }
                    cx.notify();
                }))
        })
        .child(if is_container {
            Icon::new(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(crate::ui::design::icon_sm())
            .text_color(crate::ui::design::t3(cx))
            .into_any_element()
        } else {
            div().w(px(12.)).into_any_element()
        })
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t1(cx).opacity(0.9))
                .child(label),
        )
        .child(div().text_color(crate::ui::design::t3(cx)).child(":"))
        .child(json_value_text(value, cx))
}

#[allow(clippy::too_many_arguments)]
fn render_json_tree_node<T: 'static>(
    expanded_nodes: &HashSet<String>,
    doc_key: &str,
    path: &str,
    label: SharedString,
    value: &Value,
    depth: usize,
    cx: &mut Context<T>,
    expansion_accessor: fn(&mut T) -> &mut HashSet<String>,
) -> gpui::Div {
    let node_key = format!("{doc_key}:{path}");
    let expanded = container_label(value).is_some() && expanded_nodes.contains(&node_key);
    let mut node = v_flex().w_full().child(json_tree_row(
        expanded_nodes,
        node_key,
        label,
        value,
        depth,
        cx,
        expansion_accessor,
    ));

    if !expanded {
        return node;
    }

    match value {
        Value::Object(map) if scalar_value(value).is_none() => {
            for (key, child) in map {
                let child_path = child_path(path, key);
                node = node.child(render_json_tree_node(
                    expanded_nodes,
                    doc_key,
                    &child_path,
                    key.clone().into(),
                    child,
                    depth + 1,
                    cx,
                    expansion_accessor,
                ));
            }
        }
        Value::Array(values) => {
            for (ix, child) in values.iter().take(MAX_EXPANDED_ARRAY_ITEMS).enumerate() {
                let label = format!("[{ix}]");
                let child_path = child_path(path, &label);
                node = node.child(render_json_tree_node(
                    expanded_nodes,
                    doc_key,
                    &child_path,
                    label.clone().into(),
                    child,
                    depth + 1,
                    cx,
                    expansion_accessor,
                ));
            }
            let remaining = values.len().saturating_sub(MAX_EXPANDED_ARRAY_ITEMS);
            if remaining > 0 {
                node = node.child(
                    h_flex()
                        .pl(px(6. + (depth + 1) as f32 * 18.))
                        .py_0p5()
                        .text_color(crate::ui::design::t3(cx))
                        .child(SharedString::from(format!(
                            "… {remaining} more items hidden"
                        ))),
                );
            }
        }
        _ => {}
    }

    node
}

fn render_json_tree<T: 'static>(
    expanded_nodes: &HashSet<String>,
    doc_key: &str,
    value: &Value,
    cx: &mut Context<T>,
    expansion_accessor: fn(&mut T) -> &mut HashSet<String>,
) -> gpui::Div {
    let mut tree = v_flex()
        .w_full()
        .font_family(crate::ui::design::FONT_MONO)
        .text_size(crate::ui::design::text_ui());

    match value {
        Value::Object(map) if scalar_value(value).is_none() => {
            for (key, child) in map {
                tree = tree.child(render_json_tree_node(
                    expanded_nodes,
                    doc_key,
                    key,
                    key.clone().into(),
                    child,
                    0,
                    cx,
                    expansion_accessor,
                ));
            }
        }
        Value::Array(values) => {
            for (ix, child) in values.iter().take(MAX_EXPANDED_ARRAY_ITEMS).enumerate() {
                let label = format!("[{ix}]");
                let path = label.clone();
                tree = tree.child(render_json_tree_node(
                    expanded_nodes,
                    doc_key,
                    &path,
                    label.into(),
                    child,
                    0,
                    cx,
                    expansion_accessor,
                ));
            }
            let remaining = values.len().saturating_sub(MAX_EXPANDED_ARRAY_ITEMS);
            if remaining > 0 {
                tree = tree.child(
                    h_flex()
                        .pl(px(24.))
                        .py_0p5()
                        .text_color(crate::ui::design::t3(cx))
                        .child(SharedString::from(format!(
                            "… {remaining} more items hidden"
                        ))),
                );
            }
        }
        _ => {
            tree = tree.child(render_json_tree_node(
                expanded_nodes,
                doc_key,
                "value",
                "value".into(),
                value,
                0,
                cx,
                expansion_accessor,
            ));
        }
    }

    tree
}

/// Center tab: a page of documents from one collection, with a Mongo
/// query filter, prev/next paging, and guarded JSON editing.
pub struct CollectionPane {
    handle: MongoHandle,
    db: String,
    collection: String,
    prod: bool,
    filter_input: Entity<InputState>,
    quick_filter_rows: Vec<QuickFilterRow>,
    docs: Vec<DocCard>,
    expanded_tree_nodes: HashSet<String>,
    total: u64,
    page: u64,
    loading: bool,
    error: Option<SharedString>,
    /// Generation counter so stale background loads are dropped.
    load_seq: u64,
}

impl CollectionPane {
    pub fn new(
        handle: MongoHandle,
        db: String,
        collection: String,
        prod: bool,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        handle.set_read_only(read_only);
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(r#"filter, e.g. {email: "a@example.com"}"#)
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
            db,
            collection,
            prod,
            filter_input,
            quick_filter_rows: Vec::new(),
            docs: Vec::new(),
            expanded_tree_nodes: HashSet::new(),
            total: 0,
            page: 0,
            loading: false,
            error: None,
            load_seq: 0,
        };
        pane.load(cx);
        pane
    }

    fn available_filter_fields(&self) -> Vec<String> {
        let mut fields = BTreeSet::new();
        for card in &self.docs {
            if let Some(tree) = &card.tree {
                collect_filter_fields(tree, None, 0, &mut fields);
            }
            if fields.len() >= MAX_FILTER_FIELDS {
                break;
            }
        }
        if fields.is_empty() {
            fields.insert("_id".into());
        }
        fields.into_iter().take(MAX_FILTER_FIELDS).collect()
    }

    fn add_quick_filter(&mut self, field: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_filter_rows.len() >= MAX_QUICK_FILTER_ROWS {
            return;
        }
        let value_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("value, e.g. active"));
        self.quick_filter_rows
            .push(QuickFilterRow { field, value_input });
        cx.notify();
    }

    fn set_quick_filter_field(&mut self, index: usize, field: String, cx: &mut Context<Self>) {
        if let Some(row) = self.quick_filter_rows.get_mut(index) {
            row.field = field;
        }
        cx.notify();
    }

    fn remove_quick_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.quick_filter_rows.len() {
            self.quick_filter_rows.remove(index);
        }
        cx.notify();
    }

    fn apply_quick_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filters = self
            .quick_filter_rows
            .iter()
            .map(|row| {
                (
                    row.field.clone(),
                    row.value_input.read(cx).value().to_string(),
                )
            })
            .collect::<Vec<_>>();
        if filters.is_empty() {
            return;
        }
        let current_filter = self.filter_input.read(cx).value().to_string();
        let next_filter = quick_filter_json(&current_filter, &filters);
        self.filter_input
            .update(cx, |input, cx| input.set_value(next_filter, window, cx));
        self.page = 0;
        self.load(cx);
    }

    fn field_picker_button(
        id: impl Into<ElementId>,
        label: SharedString,
        icon: Option<IconName>,
        fields: Vec<String>,
        selected: Option<String>,
        pane: Entity<Self>,
        target: FieldPickerTarget,
        disabled: bool,
    ) -> impl IntoElement {
        let mut button = crate::ui::style::secondary_button_compact(id, label)
            .dropdown_caret(true)
            .disabled(disabled);
        if let Some(icon) = icon {
            button = button.icon(icon);
        }

        button.dropdown_menu(move |mut menu, window, _| {
            menu = menu.min_w(px(220.)).max_h(px(320.)).scrollable(true);
            if fields.is_empty() {
                return menu.item(PopupMenuItem::new("No fields on current page").disabled(true));
            }

            menu = menu.item(PopupMenuItem::label("Fields on current page"));
            for field in fields.iter().cloned() {
                let checked = selected.as_deref() == Some(field.as_str());
                let field_for_click = field.clone();
                let pane = pane.clone();
                menu = menu.item(PopupMenuItem::new(field).checked(checked).on_click(
                    window.listener_for(
                        &pane,
                        move |this: &mut Self, _, window, cx| match target {
                            FieldPickerTarget::Add => {
                                this.add_quick_filter(field_for_click.clone(), window, cx);
                            }
                            FieldPickerTarget::Row(index) => {
                                this.set_quick_filter_field(index, field_for_click.clone(), cx);
                            }
                        },
                    ),
                ));
            }
            menu
        })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.load_seq += 1;
        let seq = self.load_seq;
        let handle = self.handle.clone();
        let db = self.db.clone();
        let collection = self.collection.clone();
        let filter = self.filter_input.read(cx).value().to_string();
        let skip = self.page * PAGE_SIZE;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    handle.find_docs(&db, &collection, &filter, skip, PAGE_SIZE as i64)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.expanded_tree_nodes.clear();
                        this.docs = page
                            .docs
                            .into_iter()
                            .map(|entry| {
                                let (display, clipped) = clip_for_display(&entry.json);
                                let tree = serde_json::from_str::<Value>(&entry.json).ok();
                                DocCard {
                                    display: display.into(),
                                    clipped,
                                    full: entry.json.into(),
                                    tree,
                                    id: entry.id,
                                }
                            })
                            .collect();
                        this.total = page.total;
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

    /// Opens the full document in a JSON editor dialog; Save writes back
    /// via replace_one and reloads the page.
    fn open_doc_editor(
        &mut self,
        id: String,
        full_json: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_component::WindowExt;
        let handle = self.handle.clone();
        let db = self.db.clone();
        let collection = self.collection.clone();
        let prod = self.prod;
        let pane = cx.entity().clone();
        let title: SharedString = format!(
            "Edit document — {db}.{collection}{}",
            if prod { "  ⚠ PROD" } else { "" }
        )
        .into();

        let editor = cx.new(|cx| DocEditorView::new(full_json.clone(), window, cx));

        window.open_dialog(cx, move |dialog, _, _| {
            let editor = editor.clone();
            let save_editor = editor.clone();
            let handle = handle.clone();
            let db = db.clone();
            let collection = collection.clone();
            let id = id.clone();
            let pane = pane.clone();
            dialog
                .w(px(940.))
                .title(title.clone())
                .child(editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let handle = handle.clone();
                    let db = db.clone();
                    let collection = collection.clone();
                    let id = id.clone();
                    let pane = pane.clone();
                    let invalid_json = editor.read(cx).parse_error.is_some();
                    let prod_save_armed = editor.read(cx).prod_save_armed;
                    let save_label = if prod && prod_save_armed {
                        "Confirm Save to PROD"
                    } else if prod {
                        "Save to PROD"
                    } else {
                        "Save"
                    };
                    let save = if prod {
                        crate::ui::style::danger_button_compact("db-doc-save", save_label)
                    } else {
                        crate::ui::style::primary_button_compact("db-doc-save", save_label, cx)
                    }
                    .icon(IconName::Check)
                    .disabled(invalid_json)
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
                        let input = editor.read(cx).input.clone();
                        let new_json = input.read(cx).value().to_string();
                        let handle = handle.clone();
                        let db = db.clone();
                        let collection = collection.clone();
                        let id = id.clone();
                        let pane = pane.clone();
                        window.close_dialog(cx);
                        pane.update(cx, |pane_ref, cx| {
                            pane_ref.loading = true;
                            cx.notify();
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_executor()
                                    .spawn(async move {
                                        handle.replace_doc(&db, &collection, &id, &new_json)
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
                        crate::ui::style::dialog_neutral_button("db-doc-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn page_label(&self) -> String {
        if self.total == 0 {
            return "0 documents".into();
        }
        let start = self.page * PAGE_SIZE + 1;
        let end = (self.page * PAGE_SIZE + self.docs.len() as u64).min(self.total);
        format!("{start}–{end} of {}", self.total)
    }

    fn last_page(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            (self.total - 1) / PAGE_SIZE
        }
    }

    fn expanded_nodes(this: &mut Self) -> &mut HashSet<String> {
        &mut this.expanded_tree_nodes
    }
}

struct DocEditorView {
    input: Entity<InputState>,
    tree: Option<Value>,
    parse_error: Option<SharedString>,
    expanded_tree_nodes: HashSet<String>,
    prod_save_armed: bool,
}

impl DocEditorView {
    fn new(full_json: SharedString, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .default_value(full_json.to_string())
        });
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_tree(cx);
            }
        })
        .detach();

        let (tree, parse_error) = Self::parse_json(&full_json);
        Self {
            input,
            tree,
            parse_error,
            expanded_tree_nodes: HashSet::new(),
            prod_save_armed: false,
        }
    }

    fn parse_json(json: &str) -> (Option<Value>, Option<SharedString>) {
        match serde_json::from_str::<Value>(json) {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(format!("Invalid JSON: {error}").into())),
        }
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let json = self.input.read(cx).value().to_string();
        let (tree, parse_error) = Self::parse_json(&json);
        self.tree = tree;
        self.parse_error = parse_error;
        self.expanded_tree_nodes.clear();
        self.prod_save_armed = false;
        cx.notify();
    }

    fn expanded_nodes(this: &mut Self) -> &mut HashSet<String> {
        &mut this.expanded_tree_nodes
    }
}

impl Render for DocEditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tree_content = if let Some(tree) = &self.tree {
            render_json_tree(
                &self.expanded_tree_nodes,
                "edit",
                tree,
                cx,
                Self::expanded_nodes,
            )
            .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .px_4()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::rose(cx))
                .child(
                    self.parse_error
                        .clone()
                        .unwrap_or_else(|| "Invalid JSON".into()),
                )
                .into_any_element()
        };

        h_flex()
            .w(px(900.))
            .h(px(560.))
            .gap_3()
            .child(
                v_flex()
                    .w(px(410.))
                    .h_full()
                    .border_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.65))
                    .rounded(crate::ui::design::r_sm())
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(crate::ui::design::line(cx).opacity(0.65))
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("FORMATTED"),
                    )
                    .child(
                        v_flex()
                            .id("db-doc-edit-tree-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p_2()
                            .child(tree_content),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("JSON"),
                    )
                    .when(self.prod_save_armed, |col| {
                        col.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::rose(cx))
                                .child("Click Confirm Save to PROD to write this document."),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .child(Input::new(&self.input).h_full()),
                    ),
            )
    }
}

impl Render for CollectionPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let red = crate::ui::design::rose(cx);
        let fields = self.available_filter_fields();
        let pane = cx.entity().clone();
        let controls = h_flex()
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
                    .child(super::provider_brand_mark(DbProvider::MongoDb, 22.))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(format!(
                                "{}.{}",
                                self.db, self.collection
                            ))),
                    )
                    .when(self.prod, |bar| {
                        bar.child(
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
                    .when(self.handle.is_read_only(), |bar| {
                        bar.child(
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
            .child(Self::field_picker_button(
                "db-add-filter",
                "Add filter".into(),
                Some(IconName::Plus),
                fields.clone(),
                None,
                pane.clone(),
                FieldPickerTarget::Add,
                self.quick_filter_rows.len() >= MAX_QUICK_FILTER_ROWS,
            ))
            .child(
                crate::ui::style::header_icon_button("db-apply-filter", IconName::Search, cx)
                    .tooltip("Apply filter (Enter)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = 0;
                        this.load(cx);
                    })),
            )
            .child(
                crate::ui::style::refresh_icon_button("db-refresh", cx)
                    .tooltip("Reload")
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
            .child(
                crate::ui::style::header_icon_button("db-prev-page", IconName::ChevronLeft, cx)
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
                    .child(SharedString::from(self.page_label())),
            )
            .child(
                crate::ui::style::header_icon_button("db-next-page", IconName::ChevronRight, cx)
                    .tooltip("Next page")
                    .disabled(self.page >= self.last_page() || self.loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page += 1;
                        this.load(cx);
                    })),
            );
        let mut quick_filters = v_flex().w_full().px_3().pb_2().gap_1p5();
        for (index, row) in self.quick_filter_rows.iter().enumerate() {
            quick_filters = quick_filters.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(42.))
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("Filter"),
                    )
                    .child(Self::field_picker_button(
                        ElementId::named_usize("db-quick-filter-field", index),
                        row.field.clone().into(),
                        None,
                        fields.clone(),
                        Some(row.field.clone()),
                        pane.clone(),
                        FieldPickerTarget::Row(index),
                        false,
                    ))
                    .child(
                        div()
                            .w(px(260.))
                            .child(Input::new(&row.value_input).small()),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            ElementId::named_usize("db-quick-filter-remove", index),
                            IconName::Close,
                            cx,
                        )
                        .tooltip("Remove filter")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_quick_filter(index, cx);
                        })),
                    ),
            );
        }
        quick_filters = quick_filters.child(
            h_flex().w_full().justify_end().child(
                crate::ui::style::accent_button_compact(
                    "db-quick-filter-apply",
                    "Apply filters",
                    cx,
                )
                .disabled(self.quick_filter_rows.is_empty())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.apply_quick_filters(window, cx);
                })),
            ),
        );
        let header = v_flex()
            .w_full()
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.18))
            .child(controls)
            .when(!self.quick_filter_rows.is_empty(), |header| {
                header.child(quick_filters)
            });

        let body: gpui::AnyElement = if self.loading {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new())
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Querying…"),
                )
                .into_any_element()
        } else if let Some(error) = &self.error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .px_8()
                .child(
                    gpui_component::Icon::new(IconName::TriangleAlert)
                        .size(crate::ui::design::icon_xl())
                        .text_color(red),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(error.clone()),
                )
                .child(
                    crate::ui::style::refresh_button("db-retry", "Retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .into_any_element()
        } else if self.docs.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("No documents match")
                .into_any_element()
        } else {
            v_flex()
                .id("db-doc-list")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p_2()
                .gap_2()
                .children(self.docs.iter().enumerate().map(|(ix, card)| {
                    let doc_key = doc_tree_key(self.page, ix, card);
                    let editable = !self.handle.is_read_only() && card.id.is_some();
                    let edit_id = card.id.clone();
                    let edit_full = card.full.clone();
                    div()
                        .id(("db-doc", ix))
                        .w_full()
                        .p_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::line(cx).opacity(0.6))
                        .bg(crate::ui::design::nav(cx).opacity(0.4))
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_ui())
                        .whitespace_normal()
                        .when(editable, |c| {
                            c.hover(|style| {
                                style
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::design::hover(cx).opacity(0.6))
                            })
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if let Some(id) = edit_id.clone() {
                                        this.open_doc_editor(id, edit_full.clone(), window, cx);
                                    }
                                },
                            ))
                        })
                        .child(if let Some(tree) = &card.tree {
                            render_json_tree(
                                &self.expanded_tree_nodes,
                                &doc_key,
                                tree,
                                cx,
                                Self::expanded_nodes,
                            )
                            .into_any_element()
                        } else {
                            div()
                                .font_family(crate::ui::design::FONT_MONO)
                                .text_size(crate::ui::design::text_ui())
                                .whitespace_normal()
                                .child(card.display.clone())
                                .into_any_element()
                        })
                        .when(card.tree.is_none() && card.clipped, |c| {
                            c.child(
                                div()
                                    .pt_1()
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("… clipped — click to view & edit the full document"),
                            )
                        })
                }))
                .into_any_element()
        };

        v_flex().size_full().child(header).child(body)
    }
}
