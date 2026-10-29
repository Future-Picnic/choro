use super::*;
use gpui_component::{checkbox::Checkbox, spinner::Spinner, Disableable};
use ide_core::agents::migration::{self, Conversation, PreparedConversation};
use std::time::SystemTime;

use crate::ui::design;

/// Two-line conversation row: title with date, then provider.
const ROW_H: f32 = 52.;
/// Up to five and a half rows, so a partly visible row signals scrolling.
const LIST_MAX_H: f32 = ROW_H * 5.5;
const STATE_H: f32 = 168.;
/// Titles longer than this get a hover tooltip with the full text.
const TITLE_TOOLTIP_CHARS: usize = 56;

pub(super) fn open(
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    project: ProjectId,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(project_info) = workspace
        .read(cx)
        .projects
        .iter()
        .find(|item| item.id == project)
        .cloned()
    else {
        return;
    };
    let search =
        cx.new(|cx| InputState::new(window, cx).placeholder("Search by title or provider"));
    let view = cx.new(|cx| {
        cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        cx.observe(&agents, |_, _, cx| cx.notify()).detach();
        MigrationDialog {
            workspace,
            agents,
            project,
            path: project_info.path,
            project_name: project_info.name.into(),
            search,
            conversations: Vec::new(),
            selected: HashSet::new(),
            pending: Vec::new(),
            loading: true,
            busy: false,
            show_imported: false,
            error: None,
            notice: None,
        }
    });
    view.update(cx, |view, cx| view.scan(cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let state = view.read(cx);
        let busy = state.busy;
        let count = if state.pending.is_empty() {
            state.selected.len()
        } else {
            state.pending.len()
        };
        let retry = !state.pending.is_empty();
        let loading = state.loading;
        let has_notice = state.notice.is_some();
        let footer_view = view.clone();
        dialog
            .title("Migrate conversations")
            .w(px(620.))
            .close_button(!busy && !retry)
            .keyboard(!busy && !retry)
            .overlay_closable(!busy && !retry)
            .child(view.clone())
            .footer(move |_, _, _, cx| {
                let view = footer_view.clone();
                let summary = (!retry && count > 0).then(|| format!("{count} selected"));
                let import = if busy {
                    style::busy_button_compact("migration-import", "Importing…", cx)
                } else {
                    style::primary_button_compact(
                        "migration-import",
                        if retry {
                            "Retry import".to_string()
                        } else if count == 0 {
                            "Import".to_string()
                        } else {
                            format!(
                                "Import {count} conversation{}",
                                if count == 1 { "" } else { "s" }
                            )
                        },
                        cx,
                    )
                    .disabled(loading || count == 0)
                    .on_click(move |_, _, cx| view.update(cx, |view, cx| view.import(cx)))
                };
                vec![
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(design::text_ui())
                        .text_color(design::t2(cx))
                        .children(summary)
                        .into_any_element(),
                    style::dialog_neutral_button(
                        "migration-cancel",
                        if has_notice && count == 0 { "Done" } else { "Cancel" },
                        cx,
                    )
                    .disabled(busy || retry)
                    .on_click(|_, window, cx| window.close_dialog(cx))
                    .into_any_element(),
                    import.into_any_element(),
                ]
            })
    });
}

struct MigrationDialog {
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    project: ProjectId,
    path: PathBuf,
    project_name: SharedString,
    search: Entity<InputState>,
    conversations: Vec<Conversation>,
    selected: HashSet<usize>,
    /// Retain staged identities and message IDs on save failure for a safe retry.
    pending: Vec<PreparedConversation>,
    loading: bool,
    busy: bool,
    /// Conversations already in Choro stay collapsed so eligible chats lead.
    show_imported: bool,
    error: Option<String>,
    notice: Option<String>,
}

#[derive(Clone, Copy)]
enum Row {
    Eligible(usize),
    Imported(usize),
}

impl MigrationDialog {
    fn scan(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { migration::discover(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(conversations) => this.conversations = conversations,
                    Err(error) => {
                        this.error = Some(format!("Could not scan conversations: {error:#}"))
                    }
                }
                cx.notify();
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
    }

    fn available(&self, cx: &App) -> Vec<usize> {
        let records = self.agents.read(cx).all_records_for_project(self.project);
        self.conversations
            .iter()
            .enumerate()
            .filter(|(_, conversation)| !conversation.already_imported(self.project, &records))
            .map(|(index, _)| index)
            .collect()
    }

    fn set_selected(
        &mut self,
        indices: impl IntoIterator<Item = usize>,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        for index in indices {
            if selected {
                self.selected.insert(index);
            } else {
                self.selected.remove(&index);
            }
        }
        self.notice = None;
        cx.notify();
        cx.refresh_windows();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.loading {
            return;
        }
        if !self
            .workspace
            .read(cx)
            .projects
            .iter()
            .any(|project| project.id == self.project && project.path == self.path)
        {
            self.error = Some(
                "This project is no longer open. Reopen it before migrating conversations.".into(),
            );
            cx.notify();
            return;
        }
        if !self.pending.is_empty() {
            self.save(cx);
            return;
        }
        let available = self.available(cx);
        let selected = self
            .conversations
            .iter()
            .enumerate()
            .filter(|(index, _)| self.selected.contains(index) && available.contains(index))
            .map(|(_, conversation)| conversation.clone())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        let project = self.project;
        let path = self.path.clone();
        cx.notify();
        cx.refresh_windows();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                selected.iter().map(|conversation| conversation.prepare(project, &path))
                    .collect::<anyhow::Result<Vec<_>>>()
            }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(prepared) if this.workspace.read(cx).projects.iter().any(|project| project.id == this.project && project.path == this.path) => {
                        this.pending = this.agents.update(cx, |agents, cx| agents.stage_conversation_migration(prepared, cx));
                        this.save(cx);
                    }
                    Ok(_) => {
                        this.busy = false;
                        this.error = Some("The project was removed while reading conversations. Reopen it and retry.".into());
                    }
                    Err(error) => {
                        this.busy = false;
                        this.error = Some(format!("Could not read a selected conversation: {error:#}. Deselect it or restore its local history and retry."));
                    }
                }
                cx.notify();
                cx.refresh_windows();
            }).ok();
        }).detach();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_empty() {
            self.busy = false;
            self.selected.clear();
            self.notice = Some("These conversations are already in Choro.".into());
            return;
        }
        self.busy = true;
        self.error = None;
        let count = self.pending.len();
        let messages = self
            .pending
            .iter()
            .flat_map(|conversation| conversation.messages.iter().cloned())
            .collect();
        let (revision, snapshot) = self
            .agents
            .update(cx, |agents, _| agents.durable_snapshot());
        cx.notify();
        cx.refresh_windows();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                crate::state::agents::persist_conversation_migration(revision, snapshot, messages)
            }).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.pending.clear();
                        this.selected.clear();
                        this.notice = Some(format!("Imported {count} conversation{} into {}. Open one from the sidebar to continue it.", if count == 1 { "" } else { "s" }, this.project_name));
                    }
                    Err(error) => this.error = Some(format!("Could not finish saving the imported history: {error:#}. Retry import to finish saving these conversations.")),
                }
                cx.notify();
                cx.refresh_windows();
            }).ok();
        }).detach();
    }
}

/// Readable title for a discovered conversation. Choro's own delegated runs
/// start with a machine header, so name them by role and a short ID instead.
fn display_title(title: &str) -> String {
    let title = title.trim();
    if let Some(rest) = title.strip_prefix("[Choro delivery") {
        let id = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .take(8)
            .collect::<String>();
        return if id.is_empty() {
            "Bandmate delivery".into()
        } else {
            format!("Bandmate delivery {id}")
        };
    }
    if title.is_empty() {
        return "Untitled conversation".into();
    }
    title.to_string()
}

fn matches_query(conversation: &Conversation, query: &str) -> bool {
    query.is_empty()
        || conversation.chat.title.to_lowercase().contains(query)
        || display_title(&conversation.chat.title)
            .to_lowercase()
            .contains(query)
        || conversation.chat.kind.label().to_lowercase().contains(query)
}

fn date_label(time: SystemTime) -> String {
    let date: chrono::DateTime<chrono::Local> = time.into();
    let now = chrono::Local::now();
    if date.date_naive() == now.date_naive() {
        date.format("%-I:%M %p").to_string()
    } else if date.format("%Y").to_string() == now.format("%Y").to_string() {
        date.format("%b %-d").to_string()
    } else {
        date.format("%b %-d, %Y").to_string()
    }
}

/// Show the home folder as `~` so the meaningful end of the path survives.
fn short_path(path: &std::path::Path) -> String {
    dirs::home_dir()
        .and_then(|home| path.strip_prefix(home).ok().map(|rest| rest.to_path_buf()))
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|| path.display().to_string())
}

fn state_panel(title: &str, detail: Option<String>, cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .h(px(STATE_H))
        .items_center()
        .justify_center()
        .gap_1()
        .px_6()
        .debug_selector(|| "migration-state".into())
        .child(
            div()
                .text_size(design::text_body())
                .font_weight(FontWeight::MEDIUM)
                .text_color(design::t1(cx))
                .child(title.to_string()),
        )
        .children(detail.map(|detail| {
            div()
                .text_size(design::text_ui())
                .text_color(design::t3(cx))
                .text_center()
                .child(detail)
        }))
}

impl MigrationDialog {
    fn render_destination(&self, cx: &App) -> impl IntoElement {
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_1p5()
            .child(
                div()
                    .text_size(design::text_ui())
                    .text_color(design::t2(cx))
                    .child("Bring local Codex and Claude Code chats into this project."),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.))
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(IconName::Folder)
                            .size(design::icon_sm())
                            .text_color(design::t3(cx)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(240.))
                            .truncate()
                            .text_size(design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(design::t1(cx))
                            .child(self.project_name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(design::text_label())
                            .text_color(design::t3(cx))
                            .child(short_path(&self.path)),
                    ),
            )
    }

    fn render_list_header(
        &self,
        eligible: &[usize],
        eligible_total: usize,
        searching: bool,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let all_selected =
            !eligible.is_empty() && eligible.iter().all(|index| self.selected.contains(index));
        let targets = eligible.to_vec();
        let availability = if eligible_total == 0 {
            "Nothing new to import".to_string()
        } else if searching {
            format!("{} of {eligible_total} shown", eligible.len())
        } else {
            format!("{eligible_total} not yet in Choro")
        };
        h_flex()
            .w_full()
            .h(px(36.))
            .flex_none()
            .px_3()
            .gap_3()
            .justify_between()
            .border_b_1()
            .border_color(design::line(cx).opacity(0.5))
            .child(
                Checkbox::new("migration-select-all")
                    .small()
                    .label(if searching { "Select all shown" } else { "Select all" })
                    .debug_selector(|| "migration-select-all".into())
                    .checked(all_selected)
                    .disabled(disabled || eligible.is_empty())
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.set_selected(targets.clone(), *checked, cx)
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(design::text_label())
                    .text_color(design::t3(cx))
                    .debug_selector(|| "migration-availability".into())
                    .child(availability),
            )
    }

    fn render_row(&self, row: Row, disabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (index, imported) = match row {
            Row::Eligible(index) => (index, false),
            Row::Imported(index) => (index, true),
        };
        let conversation = &self.conversations[index];
        let title = display_title(&conversation.chat.title);
        let full_title = SharedString::from(conversation.chat.title.trim().to_string());
        let long = full_title.chars().count() > TITLE_TOOLTIP_CHARS || title != full_title.as_ref();
        let selected = !imported && self.selected.contains(&index);
        let interactive = !imported && !disabled;
        let rest = if selected {
            Some(design::accent_soft(cx))
        } else {
            None
        };
        let hover = if selected {
            design::accent_soft(cx)
        } else {
            design::hover(cx)
        };
        let leading = if imported {
            div()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::CircleCheck)
                        .size(design::icon_sm())
                        .text_color(design::sage(cx)),
                )
                .into_any_element()
        } else {
            Checkbox::new(("migration-select", index))
                .small()
                .checked(selected)
                .disabled(disabled)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    cx.stop_propagation();
                    this.set_selected([index], *checked, cx);
                }))
                .into_any_element()
        };
        let provider = conversation.chat.kind;
        h_flex()
            .id(("migration-row", index))
            .w_full()
            .min_w(px(0.))
            .h(px(ROW_H))
            .flex_none()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(design::line(cx).opacity(0.18))
            .when_some(rest, |row, bg| row.bg(bg))
            .when(interactive, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let selected = !this.selected.contains(&index);
                        this.set_selected([index], selected, cx);
                    }))
            })
            .when(long, |row| {
                row.tooltip(move |window, cx| Tooltip::new(full_title.clone()).build(window, cx))
            })
            .child(leading)
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(3.))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w(px(0.))
                            .gap_3()
                            .items_baseline()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(design::text_body())
                                    .text_color(if imported {
                                        design::t3(cx)
                                    } else {
                                        design::t1(cx)
                                    })
                                    .debug_selector(move || format!("migration-title-{index}").into())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(design::text_label())
                                    .text_color(design::t3(cx))
                                    .debug_selector(move || {
                                        format!("migration-details-{index}").into()
                                    })
                                    .child(date_label(conversation.chat.updated_at)),
                            ),
                    )
                    .child(
                        h_flex()
                            .min_w(px(0.))
                            .gap_1p5()
                            .items_center()
                            .text_size(design::text_label())
                            .text_color(design::t3(cx))
                            .child(
                                crate::ui::center::provider_brand_icon(provider).size(px(12.)),
                            )
                            .child(if imported {
                                format!("{}, already in Choro", provider.label())
                            } else {
                                provider.label().to_string()
                            }),
                    ),
            )
    }

    fn render_notice(&self, cx: &App) -> Option<impl IntoElement> {
        let notice = self.notice.clone()?;
        Some(
            h_flex()
                .w_full()
                .gap_2()
                .items_start()
                .px_3()
                .py_2()
                .rounded(design::r_sm())
                .border_1()
                .border_color(design::sage(cx).opacity(0.32))
                .bg(design::sage(cx).opacity(0.08))
                .debug_selector(|| "migration-notice".into())
                .child(
                    div().pt(px(1.)).child(
                        Icon::new(IconName::CircleCheck)
                            .size(design::icon_sm())
                            .text_color(design::sage(cx)),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(design::text_ui())
                        .text_color(design::t1(cx))
                        .child(notice),
                ),
        )
    }
}

impl Render for MigrationDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let records = self.agents.read(cx).all_records_for_project(self.project);
        let query = self.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let mut eligible = Vec::new();
        let mut imported = Vec::new();
        let mut imported_total = 0;
        for (index, conversation) in self.conversations.iter().enumerate() {
            let is_imported = conversation.already_imported(self.project, &records);
            imported_total += usize::from(is_imported);
            if !matches_query(conversation, &query) {
                continue;
            }
            if is_imported {
                imported.push(index);
            } else {
                eligible.push(index);
            }
        }
        let eligible_total = self.conversations.len() - imported_total;
        let disabled = self.busy || !self.pending.is_empty();
        let scan_failed = self.conversations.is_empty() && self.error.is_some();
        let rows = eligible
            .iter()
            .map(|&index| Row::Eligible(index))
            .chain(
                self.show_imported
                    .then(|| imported.iter().map(|&index| Row::Imported(index)))
                    .into_iter()
                    .flatten(),
            )
            .collect::<Vec<_>>();
        let show_list_header = !self.loading && !self.conversations.is_empty();

        let body = if self.loading {
            h_flex()
                .w_full()
                .h(px(STATE_H))
                .items_center()
                .justify_center()
                .gap_2()
                .text_size(design::text_ui())
                .text_color(design::t2(cx))
                .child(Spinner::new().small())
                .child("Looking for conversations…")
                .into_any_element()
        } else if scan_failed {
            v_flex()
                .w_full()
                .h(px(STATE_H))
                .items_center()
                .justify_center()
                .gap_3()
                .px_6()
                .child(
                    div()
                        .text_size(design::text_ui())
                        .text_color(design::rose(cx))
                        .text_center()
                        .child(self.error.clone().unwrap_or_default()),
                )
                .child(
                    style::dialog_neutral_button("migration-rescan", "Scan again", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                )
                .into_any_element()
        } else if self.conversations.is_empty() {
            state_panel(
                "No conversations found",
                Some("Codex and Claude Code have no local history for this folder.".into()),
                cx,
            )
            .into_any_element()
        } else if rows.is_empty() && !searching {
            state_panel(
                "Everything here is already in Choro",
                Some("Open imported conversations from the sidebar.".into()),
                cx,
            )
            .into_any_element()
        } else if rows.is_empty() {
            state_panel(
                &format!("No conversations match “{}”", query),
                (!imported.is_empty() && !self.show_imported).then(|| {
                    "Matches already in Choro are hidden below.".to_string()
                }),
                cx,
            )
            .into_any_element()
        } else {
            let height = (rows.len() as f32 * ROW_H).min(LIST_MAX_H);
            uniform_list(
                "migration-conversations",
                rows.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| this.render_row(rows[row], disabled, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .w_full()
            .h(px(height))
            .into_any_element()
        };

        let imported_toggle = (show_list_header && imported_total > 0).then(|| {
            let label = if self.show_imported {
                format!("Hide {imported_total} already in Choro")
            } else {
                format!("Show {imported_total} already in Choro")
            };
            style::ghost_button_compact("migration-imported-toggle", label)
                .icon(if self.show_imported {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_imported = !this.show_imported;
                    cx.notify();
                    cx.refresh_windows();
                }))
        });
        let action_error = self.error.clone().filter(|_| !scan_failed);
        let header = show_list_header.then(|| {
            self.render_list_header(&eligible, eligible_total, searching, disabled, cx)
        });
        let notice = self.render_notice(cx);

        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_3()
            .pb_1()
            .debug_selector(|| "migration-body".into())
            .child(self.render_destination(cx))
            .child(
                Input::new(&self.search).prefix(
                    Icon::new(IconName::Search)
                        .size(design::icon_sm())
                        .text_color(design::t3(cx)),
                ),
            )
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .rounded(design::r_md())
                    .border_1()
                    .border_color(design::line(cx))
                    .bg(design::surface(cx).opacity(0.32))
                    .children(header)
                    .child(body),
            )
            .children(imported_toggle.map(|toggle| h_flex().w_full().child(toggle)))
            .children(action_error.map(|error| style::expert_settings_error_notice(error, cx)))
            .children(notice)
    }
}

#[cfg(test)]
mod title_tests {
    use super::display_title;

    #[test]
    fn delegated_runs_get_a_short_readable_title() {
        assert_eq!(
            display_title("[Choro delivery b9942ddd-312b-4988-929e-b9dfb9df775e]"),
            "Bandmate delivery b9942ddd"
        );
        assert_eq!(display_title("[Choro delivery]"), "Bandmate delivery");
        assert_eq!(display_title("  "), "Untitled conversation");
        assert_eq!(display_title(" Fetch and pull main "), "Fetch and pull main");
    }
}

#[cfg(all(test, feature = "ui-layout-tests"))]
mod tests {
    use super::*;
    use ide_core::{AgentAccessMode, AgentKind, AgentModel, AgentRecord, AppConfig, Project};

    struct Fixture(Entity<MigrationDialog>);
    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w_full().max_w(px(560.)).child(self.0.clone())
        }
    }

    #[gpui::test]
    fn selection_excludes_existing_conversations_and_keeps_long_titles_inside_dialog(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let mut dialog = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let project = Project::from_path(PathBuf::from("/in-memory/migration"));
            let mut config = AppConfig::default();
            config.projects.push(project.clone());
            let workspace = cx.new(|_| Workspace::in_memory(config));
            let model = AgentModel::default_for(AgentKind::Codex);
            let mut existing = AgentRecord::new(
                project.id,
                project.path.clone(),
                "Already imported",
                "",
                AgentKind::Codex,
                model,
                model.default_effort(),
                AgentAccessMode::AskForApproval,
            );
            existing.cli_session_id = Some("session-0".into());
            let mut foreign = existing.clone();
            foreign.id = Uuid::new_v4();
            foreign.project_id = ProjectId::new();
            foreign.cli_session_id = Some("session-1".into());
            let agents = cx.new(|_| AgentRecords::in_memory(vec![existing, foreign]));
            let search = cx.new(|cx| InputState::new(window, cx));
            let view = cx.new(|_| MigrationDialog {
                workspace,
                agents,
                project: project.id,
                path: project.path,
                project_name: "Migration fixture".into(),
                search,
                conversations: (0..60)
                    .map(|index| Conversation {
                        chat: ide_core::agents::AgentChat {
                            kind: AgentKind::Codex,
                            session_id: format!("session-{index}"),
                            title: format!(
                                "Conversation {index}: {}",
                                "a long project conversation title ".repeat(8)
                            ),
                            updated_at: std::time::SystemTime::now(),
                        },
                        transcript_path: PathBuf::from("/fixture/unused.jsonl"),
                    })
                    .collect(),
                selected: HashSet::new(),
                pending: Vec::new(),
                loading: false,
                busy: false,
                show_imported: false,
                error: None,
                notice: None,
            });
            dialog = Some(view.clone());
            let fixture = cx.new(|_| Fixture(view));
            gpui_component::Root::new(fixture, window, cx)
        });
        let dialog = dialog.unwrap();
        cx.run_until_parked();
        assert_eq!(
            dialog.read_with(cx, |view, cx| view.available(cx).len()),
            59
        );
        // Already-imported conversations start collapsed and never look selected.
        assert!(cx.debug_bounds("migration-title-0").is_none());
        let select_all = cx
            .debug_bounds("migration-select-all")
            .expect("select all rendered");
        cx.simulate_click(select_all.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(dialog.read_with(cx, |view, _| view.selected.len()), 59);
        assert!(!dialog.read_with(cx, |view, _| view.selected.contains(&0)));
        let title = cx
            .debug_bounds("migration-title-1")
            .expect("second conversation rendered");
        cx.simulate_click(title.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(dialog.read_with(cx, |view, _| view.selected.len()), 58);

        dialog.update(cx, |view, cx| {
            view.show_imported = true;
            cx.notify();
        });
        cx.run_until_parked();
        dialog.update(cx, |view, cx| {
            // Scroll the imported row (listed after every eligible one) into view.
            view.conversations.truncate(4);
            view.selected.retain(|index| *index < 4);
            cx.notify();
        });
        cx.run_until_parked();
        let imported_row = cx
            .debug_bounds("migration-title-0")
            .expect("imported conversation shown when expanded");
        cx.simulate_click(imported_row.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            !dialog.read_with(cx, |view, _| view.selected.contains(&0)),
            "imported conversations cannot be selected"
        );

        for width in [560., 440.] {
            cx.simulate_resize(gpui::size(px(width), px(640.)));
            cx.run_until_parked();
            let body = cx.debug_bounds("migration-body").unwrap();
            let title = cx.debug_bounds("migration-title-1").unwrap();
            let details = cx.debug_bounds("migration-details-1").unwrap();
            assert!(title.size.width > px(60.), "conversation title collapsed");
            assert!(title.right() <= details.left(), "title overlaps the date");
            assert!(details.right() <= body.right(), "metadata leaves the dialog");
        }
    }

    #[gpui::test]
    fn empty_project_explains_what_was_searched(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let project = Project::from_path(PathBuf::from("/in-memory/empty"));
            let mut config = AppConfig::default();
            config.projects.push(project.clone());
            let workspace = cx.new(|_| Workspace::in_memory(config));
            let agents = cx.new(|_| AgentRecords::in_memory(Vec::new()));
            let search = cx.new(|cx| InputState::new(window, cx));
            let view = cx.new(|_| MigrationDialog {
                workspace,
                agents,
                project: project.id,
                path: project.path,
                project_name: "Empty".into(),
                search,
                conversations: Vec::new(),
                selected: HashSet::new(),
                pending: Vec::new(),
                loading: false,
                busy: false,
                show_imported: false,
                error: None,
                notice: None,
            });
            let fixture = cx.new(|_| Fixture(view));
            gpui_component::Root::new(fixture, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("migration-state").is_some());
        assert!(cx.debug_bounds("migration-select-all").is_none());
    }
}
