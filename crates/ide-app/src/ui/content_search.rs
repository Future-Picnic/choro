use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    actions, div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, Icon, IconName, Selectable, Sizable, WindowExt,
};
use ide_core::search::{
    search_project_content, ContentSearchQuery, ContentSearchResult, ContentSearchSummary,
};
use ide_core::ProjectId;

use crate::state::Workspace;
use crate::ui::center::CenterArea;
use crate::ui::{design, palette_ui};

actions!(
    content_search,
    [
        ContentSearchNext,
        ContentSearchPrevious,
        ContentSearchConfirm
    ]
);

const CONTEXT: &str = "ContentSearch";
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(180);

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("down", ContentSearchNext, Some("ContentSearch > Input")),
        KeyBinding::new("up", ContentSearchPrevious, Some("ContentSearch > Input")),
        KeyBinding::new("enter", ContentSearchConfirm, Some("ContentSearch > Input")),
        KeyBinding::new(
            "secondary-enter",
            ContentSearchConfirm,
            Some("ContentSearch > Input"),
        ),
    ]
}

pub struct ContentSearch {
    center: Entity<CenterArea>,
    project: ProjectId,
    project_name: SharedString,
    root: PathBuf,
    input: Entity<InputState>,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    summary: ContentSearchSummary,
    loading: bool,
    error: Option<String>,
    selected: usize,
    search_id: u64,
    scroll: ScrollHandle,
}

impl ContentSearch {
    pub fn open(
        workspace: Entity<Workspace>,
        center: Entity<CenterArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(project) = workspace.read(cx).active_project().cloned() else {
            eprintln!("content search: no active project");
            return;
        };

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Find in files...")
                .default_value("")
        });
        let search = cx.new(|cx| {
            cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.selected = 0;
                    this.schedule_search(cx);
                }
            })
            .detach();
            Self {
                center,
                project: project.id,
                project_name: project.name.into(),
                root: project.path,
                input,
                case_sensitive: false,
                whole_word: false,
                regex: false,
                summary: ContentSearchSummary::default(),
                loading: false,
                error: None,
                selected: 0,
                search_id: 0,
                scroll: ScrollHandle::new(),
            }
        });

        let dialog_search = search.clone();
        palette_ui::soften_overlay(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            palette_ui::styled_dialog(dialog, palette_ui::DIALOG_W_WIDE, cx)
                .child(dialog_search.clone())
        });

        let input = search.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn query_text(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    fn query(&self, cx: &App) -> ContentSearchQuery {
        ContentSearchQuery {
            text: self.query_text(cx),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            regex: self.regex,
        }
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self.search_id += 1;
        let search_id = self.search_id;
        let query = self.query(cx);
        if query.is_empty() {
            self.summary = ContentSearchSummary::default();
            self.loading = false;
            self.error = None;
            cx.notify();
            return;
        }

        self.loading = true;
        self.error = None;
        cx.notify();

        let root = self.root.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let result = cx
                .background_executor()
                .spawn(async move { search_project_content(root, &query) })
                .await;

            this.update(cx, |this, cx| {
                if this.search_id != search_id {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(summary) => {
                        this.summary = summary;
                        this.error = None;
                    }
                    Err(error) => {
                        this.summary = ContentSearchSummary::default();
                        this.error = Some(error.to_string());
                    }
                }
                this.clamp_selection();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn clamp_selection(&mut self) {
        let len = self.summary.results.len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        let len = self.summary.results.len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1) % len;
        cx.notify();
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) {
        let len = self.summary.results.len();
        if len == 0 {
            return;
        }
        self.selected = if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
        cx.notify();
    }

    fn confirm_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(result) = self.summary.results.get(self.selected).cloned() else {
            return;
        };
        self.open_result(result, window, cx);
    }

    fn open_result(
        &mut self,
        result: ContentSearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.center.update(cx, |center, cx| {
            center.open_file_at(
                self.project,
                result.path,
                result.line_number,
                result.column,
                window,
                cx,
            );
        });
        palette_ui::close(window, cx);
    }

    fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        self.schedule_search(cx);
    }

    fn toggle_whole_word(&mut self, cx: &mut Context<Self>) {
        self.whole_word = !self.whole_word;
        self.schedule_search(cx);
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.regex = !self.regex;
        self.schedule_search(cx);
    }

    fn match_count(&self) -> usize {
        self.summary
            .results
            .iter()
            .map(|result| result.match_ranges.len())
            .sum()
    }

    fn file_count(&self) -> usize {
        self.summary
            .results
            .iter()
            .map(|result| result.rel_path.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    fn status_text(&self, cx: &App) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if self.query_text(cx).is_empty() {
            return "Type to search file contents".to_string();
        }
        if self.loading {
            return "Searching...".to_string();
        }
        if self.summary.results.is_empty() {
            return "No content matches".to_string();
        }

        let suffix = if self.summary.truncated { "+" } else { "" };
        format!(
            "{}{} matches in {} files",
            self.match_count(),
            suffix,
            self.file_count()
        )
    }

    /// Quiet per-file section label — the path replaces both a divider and a
    /// per-row path column, matching the palette group labels.
    fn render_file_label(&self, rel_path: String, cx: &App) -> impl IntoElement {
        h_flex()
            .flex_none()
            .px(px(palette_ui::ROW_PAD_X))
            .pt(px(10.))
            .pb(px(3.))
            .gap(px(6.))
            .items_center()
            .child(
                Icon::new(IconName::File)
                    .size(design::icon_sm())
                    .text_color(design::t4(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .font_family(design::FONT_MONO)
                    .text_size(design::text_file())
                    .text_color(design::t4(cx))
                    .child(rel_path),
            )
    }

    fn render_result_row(
        &self,
        ix: usize,
        result: ContentSearchResult,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = ix == self.selected;
        let line_label = format!("{}:{}", result.line_number, result.column);
        // Deep indentation would push the match out of the preview; show the
        // trimmed line and shift the highlight ranges accordingly.
        let trimmed_start = result.line_preview.len() - result.line_preview.trim_start().len();
        let preview = result.line_preview.trim_start().to_string();
        let ranges: Vec<_> = result
            .match_ranges
            .iter()
            .map(|range| {
                range.start.saturating_sub(trimmed_start)..range.end.saturating_sub(trimmed_start)
            })
            .collect();
        let kind = result.clone();

        palette_ui::row(selected, cx)
            .id(("content-search-row", ix))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_result(kind.clone(), window, cx);
            }))
            .child(
                div()
                    .w(px(58.))
                    .flex_none()
                    .font_family(design::FONT_MONO)
                    .text_size(design::text_file())
                    .text_color(design::t4(cx))
                    .child(line_label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(design::FONT_MONO)
                    .text_size(design::text_file())
                    .truncate()
                    .child(palette_ui::highlighted_ranges(preview, ranges, cx)),
            )
    }
}

impl Render for ContentSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.clamp_selection();
        let status = self.status_text(cx);
        let has_error = self.error.is_some();
        let mut rows = Vec::new();
        let mut last_path = None::<String>;
        for (ix, result) in self.summary.results.iter().cloned().enumerate() {
            if last_path.as_deref() != Some(result.rel_path.as_str()) {
                last_path = Some(result.rel_path.clone());
                rows.push(
                    self.render_file_label(result.rel_path.clone(), cx)
                        .into_any_element(),
                );
            }
            if ix == self.selected {
                self.scroll.scroll_to_item(rows.len());
            }
            rows.push(self.render_result_row(ix, result, cx).into_any_element());
        }

        palette_ui::frame(cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &ContentSearchNext, _, cx| {
                this.select_next(cx);
            }))
            .on_action(cx.listener(|this, _: &ContentSearchPrevious, _, cx| {
                this.select_previous(cx);
            }))
            .on_action(cx.listener(|this, _: &ContentSearchConfirm, window, cx| {
                this.confirm_selected(window, cx);
            }))
            .child(
                palette_ui::header_band()
                    .child(
                        Icon::new(IconName::Search)
                            .size(design::icon())
                            .text_color(design::t3(cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&self.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false),
                        ),
                    )
                    .child(
                        Button::new("content-search-case")
                            .ghost()
                            .small()
                            .compact()
                            .icon(IconName::CaseSensitive)
                            .selected(self.case_sensitive)
                            .tooltip("Case sensitive")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_case_sensitive(cx);
                            })),
                    )
                    .child(
                        Button::new("content-search-word")
                            .ghost()
                            .small()
                            .compact()
                            .icon(IconName::ALargeSmall)
                            .selected(self.whole_word)
                            .tooltip("Whole word")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_whole_word(cx);
                            })),
                    )
                    .child(
                        Button::new("content-search-regex")
                            .ghost()
                            .small()
                            .compact()
                            .icon(IconName::Asterisk)
                            .selected(self.regex)
                            .tooltip("Regex")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_regex(cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("content-search-results")
                    .track_scroll(&self.scroll)
                    .max_h(px(430.))
                    .min_h(px(96.))
                    .px(px(palette_ui::LIST_PAD))
                    .pb(px(palette_ui::LIST_PAD_BOTTOM))
                    .gap_0p5()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .h(px(72.))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(design::text_body())
                                .text_color(if has_error {
                                    design::rose(cx)
                                } else {
                                    design::t3(cx)
                                })
                                .child(status.clone()),
                        )
                    })
                    .children(rows),
            )
            .child(
                palette_ui::footer_band()
                    .text_size(design::text_ui())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(design::t4(cx))
                            .child(format!(
                                "{} · {} scanned · {} skipped",
                                self.project_name,
                                self.summary.files_scanned,
                                self.summary.files_skipped
                            )),
                    )
                    .child(div().text_color(design::t3(cx)).child(status))
                    .child(palette_ui::key_hint("↑↓", "navigate", cx))
                    .child(palette_ui::key_hint("↩", "open", cx))
                    .child(palette_ui::key_hint("esc", "close", cx)),
            )
    }
}
