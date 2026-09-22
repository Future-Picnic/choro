use super::*;

const AGENT_NOTES_COLLAPSED_HEIGHT: f32 = 320.0;

fn agent_notes_drawer_height(viewport_height: f32, expanded: bool) -> f32 {
    if !expanded {
        return AGENT_NOTES_COLLAPSED_HEIGHT;
    }

    let available_height = (viewport_height - 160.0).max(AGENT_NOTES_COLLAPSED_HEIGHT);
    (viewport_height * 0.68)
        .clamp(420.0, 720.0)
        .min(available_height)
}

impl CenterArea {
    /// Shared chrome for the agent bottom drawers (Terminal / Files / Notes /
    /// Plan): one bottom-anchored sheet with a rounded top, a single header
    /// height, design-system tokens, and elevation — plus `.occlude()` so
    /// scrolling or clicking inside the drawer never falls through to the chat
    /// behind it. Each drawer supplies its icon, title, optional subtitle, the
    /// right-aligned actions (incl. its own close button), a height, and a body.
    pub(in crate::ui::center) fn render_agent_detail_drawer(
        &self,
        icon: IconName,
        title: impl Into<SharedString>,
        subtitle: Option<gpui::AnyElement>,
        actions: gpui::AnyElement,
        height: gpui::Pixels,
        body: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(crate::ui::design::header_h())
            .h(height)
            // Capture scroll/clicks so they don't reach the chat underneath.
            .occlude()
            .rounded_t(px(crate::ui::style::RADIUS_LG))
            .border_t_1()
            .border_color(crate::ui::style::border(cx))
            .bg(crate::ui::design::base(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(44.))
                    .px_4()
                    .items_center()
                    .gap_2()
                    // Round the header's own top corners (gpui's overflow clip
                    // mask is rectangular, so the sheet's rounding can't do it).
                    .rounded_t(px(crate::ui::style::RADIUS_LG - 1.))
                    .border_b_1()
                    .border_color(crate::ui::style::border(cx))
                    .bg(crate::ui::design::surface(cx))
                    .child(
                        gpui_component::Icon::new(icon)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title.into()),
                    )
                    .when_some(subtitle, |row, subtitle| row.child(subtitle))
                    .child(div().flex_1().min_w(px(0.)))
                    .child(actions),
            )
            .child(div().flex_1().min_h(px(0.)).overflow_hidden().child(body))
            .into_any_element()
    }

    /// The consistent close button used by every detail drawer header.
    pub(in crate::ui::center) fn agent_drawer_close_button(
        &self,
        id: (&'static str, u64),
        tooltip: &'static str,
        on_close: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .xsmall()
            .compact()
            .h(crate::ui::design::control_h())
            .icon(IconName::Close)
            .tooltip(tooltip)
            .on_click(cx.listener(move |this, _, window, cx| on_close(this, window, cx)))
    }

    /// Open a file or snapshot diff in the agent's bottom drawer, using the
    /// same virtualized DiffPane as the main Code / Files diff view.
    pub(in crate::ui::center) fn open_agent_diff_drawer(
        &mut self,
        agent_id: Uuid,
        project: ProjectId,
        kind: DiffKind,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let key = kind.key();
        let already_open = self
            .agent_diff_drawers
            .get(&agent_id)
            .is_some_and(|drawer| drawer.project == project && drawer.key == key);

        if !already_open {
            let Some(git) = self.git_states.read(cx).get(project) else {
                return;
            };
            let repo = git.read(cx).repo_path.clone();
            let center = cx.weak_entity();
            let drawer_kind = kind.clone();
            let view = cx
                .new(|cx| DiffPane::new(project, repo, drawer_kind, Some(git.clone()), center, cx));
            self.agent_diff_drawers.insert(
                agent_id,
                AgentDiffDrawer {
                    project,
                    key,
                    title,
                    kind,
                    view,
                },
            );
        }

        self.agent_detail_tabs
            .insert(agent_id, AgentDetailTab::Diff);
        cx.notify();
    }

    pub(in crate::ui::center) fn render_agent_diff_drawer(
        &self,
        agent: &AgentRecord,
        drawer: &AgentDiffDrawer,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let project = drawer.project;
        let kind = drawer.kind.clone();
        let title = drawer.title.clone();
        let open_title = title.clone();
        let actions = h_flex()
            .gap_1()
            .child(
                crate::ui::style::icon_button(
                    ("agent-diff-drawer-open-full", agent_id.as_u128() as u64),
                    IconName::ExternalLink,
                    cx,
                )
                .tooltip("Open in Code / Files")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_diff(project, kind.clone(), open_title.clone(), cx);
                })),
            )
            .child(self.agent_drawer_close_button(
                ("close-agent-diff", agent_id.as_u128() as u64),
                "Close diff",
                move |this, _, cx| {
                    this.agent_detail_tabs
                        .insert(agent_id, AgentDetailTab::Terminal);
                    cx.notify();
                },
                cx,
            ))
            .into_any_element();
        let subtitle = div()
            .max_w(px(520.))
            .min_w(px(0.))
            .truncate()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(title)
            .into_any_element();
        let body = div()
            .size_full()
            .child(drawer.view.clone())
            .into_any_element();

        self.render_agent_detail_drawer(
            IconName::FileText,
            "Diff",
            Some(subtitle),
            actions,
            px(460.),
            body,
            cx,
        )
    }

    pub(in crate::ui::center) fn render_agent_notes_drawer(
        &self,
        agent: &AgentRecord,
        notes_input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let summary = self.agent_summaries.get(&agent_id);
        let has_summary = summary.is_some();
        let notes_expanded = self.agent_notes_expanded.contains(&agent_id);
        let drawer_height = px(agent_notes_drawer_height(
            f32::from(window.viewport_size().height),
            notes_expanded,
        ));
        let prompt_lines = if agent.doc.is_empty() {
            vec![String::from("No prompt saved.")]
        } else {
            agent.doc.lines().map(ToString::to_string).collect()
        };

        let actions = h_flex()
            .gap_1()
            .child(
                crate::ui::style::header_icon_button(
                    ("toggle-agent-notes-height", agent_id.as_u128() as u64),
                    if notes_expanded {
                        IconName::Minimize
                    } else {
                        IconName::Maximize
                    },
                    cx,
                )
                .tooltip(if notes_expanded {
                    "Restore Notes height"
                } else {
                    "Make Notes taller"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if notes_expanded {
                        this.agent_notes_expanded.remove(&agent_id);
                    } else {
                        this.agent_notes_expanded.insert(agent_id);
                    }
                    cx.notify();
                })),
            )
            .child(self.agent_drawer_close_button(
                ("close-agent-notes", agent_id.as_u128() as u64),
                "Close notes",
                move |this, _, cx| {
                    this.agent_detail_tabs
                        .insert(agent_id, AgentDetailTab::Terminal);
                    cx.notify();
                },
                cx,
            ))
            .into_any_element();

        let body = v_flex()
            .size_full()
            .overflow_y_scrollbar()
            .p_3()
            .gap_3()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("General prompt"),
                    )
                    .child(
                        v_flex()
                            .max_h(px(128.))
                            .overflow_y_scrollbar()
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::style::border(cx))
                            .bg(crate::ui::style::surface(cx))
                            .p_2()
                            .gap_0p5()
                            .text_size(crate::ui::design::text_ui())
                            .line_height(gpui::relative(1.35))
                            .text_color(crate::ui::design::t1(cx))
                            .children(prompt_lines.into_iter().map(|line| {
                                div().child(if line.is_empty() {
                                    SharedString::from(" ")
                                } else {
                                    SharedString::from(line)
                                })
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("Notes"),
                    )
                    .child(
                        div()
                            .min_h(px(96.))
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::style::border(cx))
                            .bg(crate::ui::style::surface(cx))
                            .p_2()
                            .child(
                                Input::new(&notes_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .h_full(),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Agent summary"),
                            )
                            .child(div().flex_1())
                            .child(
                                crate::ui::style::secondary_button_compact(
                                    ("refresh-agent-summary", agent_id.as_u128() as u64),
                                    if has_summary {
                                        "Update summary"
                                    } else {
                                        "Add summary"
                                    },
                                )
                                .disabled(
                                    self.agent_summary_requests_pending.contains_key(&agent_id),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.request_agent_summary(agent_id, cx);
                                    },
                                )),
                            ),
                    )
                    .when_some(summary, |section, summary| {
                        section.child(
                            div()
                                .min_h(px(120.))
                                .rounded(crate::ui::design::r_sm())
                                .border_1()
                                .border_color(crate::ui::style::border(cx))
                                .bg(crate::ui::style::surface(cx))
                                .p_2()
                                .text_size(crate::ui::design::text_ui())
                                .line_height(gpui::relative(1.45))
                                .text_color(crate::ui::design::t2(cx))
                                .child(
                                    TextView::markdown(
                                        ("agent-notes-summary", agent_id.as_u128() as u64),
                                        summary.summary_text.clone(),
                                        window,
                                        cx,
                                    )
                                    .selectable(true)
                                    .style(chat_message_text_style()),
                                ),
                        )
                    })
                    .when(!has_summary, |section| {
                        section.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child("No summary has been saved yet."),
                        )
                    }),
            )
            .into_any_element();

        self.render_agent_detail_drawer(
            IconName::File,
            "Notes",
            None,
            actions,
            drawer_height,
            body,
            cx,
        )
    }

    pub(in crate::ui::center) fn render_agent_files_drawer(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let mut exact_files = std::collections::BTreeMap::<PathBuf, (usize, usize)>::new();
        if let Some(session) = self.agent_chats.read(cx).session(agent.id) {
            let reconciled = session
                .changed_files
                .reconciled_final_files(agent.runtime_path());
            let filter = VisualizationArtifactFilter::new(agent.id, agent.runtime_path());
            for file in reconciled
                .conversation_files()
                .filter(|file| !filter.is_artifact(&file.path))
            {
                exact_files.insert(file.path.clone(), (file.additions, file.deletions));
            }
        }
        let exact_rows = exact_files.into_iter().collect::<Vec<_>>();
        let row_count = exact_rows.len();

        let close = self
            .agent_drawer_close_button(
                ("close-agent-files", agent_id.as_u128() as u64),
                "Close files",
                move |this, _, cx| {
                    this.agent_detail_tabs
                        .insert(agent_id, AgentDetailTab::Terminal);
                    cx.notify();
                },
                cx,
            )
            .into_any_element();
        let count = div()
            .flex_none()
            .rounded_full()
            .bg(crate::ui::design::surface(cx))
            .px_2()
            .py_0p5()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(row_count.to_string())
            .into_any_element();

        let body = v_flex()
            .size_full()
            .p_3()
            .gap_1()
            .overflow_y_scrollbar()
            .when(row_count == 0, |list| {
                list.child(
                    v_flex()
                        .h_full()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(IconName::FolderOpen)
                                .size(crate::ui::design::icon_xl()),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child("No files connected yet"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child("Files appear here after the agent changes them."),
                        ),
                )
            })
            .when(!exact_rows.is_empty(), |list| {
                list.child(agent_files_section_label("Edited in this chat", cx))
                    .children(exact_rows.into_iter().enumerate().map(
                        |(index, (path, (add, del)))| {
                            self.render_agent_file_ledger_row(
                                project, agent, index, path, add, del, cx,
                            )
                        },
                    ))
            })
            .into_any_element();

        self.render_agent_detail_drawer(
            IconName::FolderOpen,
            "Files",
            Some(count),
            close,
            px(320.),
            body,
            cx,
        )
    }

    fn render_agent_file_ledger_row(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        index: usize,
        path: PathBuf,
        add: usize,
        del: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let absolute = agent.runtime_path().join(&path);
        let label = SharedString::from(path.display().to_string());
        h_flex()
            .id(("agent-connected-file", index))
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_2()
            .rounded(crate::ui::design::r_sm())
            .px_2()
            .py_1p5()
            .cursor_pointer()
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.24)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_file(project, absolute.clone(), window, cx);
            }))
            .child(
                gpui_component::Icon::new(IconName::File)
                    .size(crate::ui::design::icon_md())
                    .text_color(crate::ui::design::t3(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t1(cx))
                    .child(label),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1p5()
                    .text_size(crate::ui::design::text_ui())
                    .font_family(crate::ui::design::FONT_MONO)
                    .child(
                        div()
                            .text_color(crate::ui::design::sage(cx))
                            .child(format!("+{add}")),
                    )
                    .child(
                        div()
                            .text_color(crate::ui::design::rose(cx))
                            .child(format!("-{del}")),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_agent_plan_drawer(
        &self,
        agent: &AgentRecord,
        plan: &crate::state::agent_chat::ProposedPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let close = self
            .agent_drawer_close_button(
                ("close-agent-plan", agent_id.as_u128() as u64),
                "Close plan",
                move |this, _, cx| {
                    this.agent_detail_tabs
                        .insert(agent_id, AgentDetailTab::Terminal);
                    cx.notify();
                },
                cx,
            )
            .into_any_element();
        let subtitle = div()
            .min_w(px(0.))
            .max_w(px(360.))
            .truncate()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(clean_plan_heading(&plan.title))
            .into_any_element();

        let body = v_flex().size_full().p_4().overflow_y_scrollbar().child(
            v_flex()
                .w_full()
                .max_w(px(900.))
                .mx_auto()
                .rounded(crate::ui::design::r_lg())
                .border_1()
                .border_color(crate::ui::style::border(cx))
                .bg(crate::ui::style::surface(cx))
                .p_4()
                .gap_3()
                .child(render_plan_markdown(&plan.display_markdown(), window, cx)),
        );

        self.render_agent_detail_drawer(
            IconName::CircleCheck,
            "Plan",
            Some(subtitle),
            close,
            px(420.),
            body.into_any_element(),
            cx,
        )
    }
}

fn agent_files_section_label(
    label: &'static str,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    div()
        .mt_2()
        .px_2()
        .text_size(crate::ui::design::text_label())
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(crate::ui::design::t3(cx))
        .child(label)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_notes_drawer_stays_compact_until_expanded() {
        assert_eq!(agent_notes_drawer_height(900.0, false), 320.0);
        assert!(agent_notes_drawer_height(900.0, true) > 320.0);
    }

    #[test]
    fn expanded_agent_notes_drawer_respects_window_bounds() {
        assert_eq!(agent_notes_drawer_height(400.0, true), 320.0);
        assert!(agent_notes_drawer_height(500.0, true) <= 340.0);
        assert_eq!(agent_notes_drawer_height(2_000.0, true), 720.0);
    }
}
