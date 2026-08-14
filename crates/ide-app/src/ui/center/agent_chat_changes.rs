use super::*;

impl CenterArea {
    pub(super) fn render_changed_files_card(
        &self,
        agent: &AgentRecord,
        _index: usize,
        summary: &crate::state::agent_chat::ChangedFilesSummary,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let artifact_filter = VisualizationArtifactFilter::new(agent.id, agent.runtime_path());
        // Reconcile again while rendering so receipts persisted by older app
        // versions are fixed without requiring a data migration.
        let reconciled = summary.reconciled_final_files(agent.runtime_path());
        let visible_files = reconciled
            .files
            .iter()
            .filter(|file| !artifact_filter.is_artifact(&file.path))
            .collect::<Vec<_>>();
        let visible_observed_files = reconciled
            .observed_files
            .iter()
            .filter(|file| !artifact_filter.is_artifact(&file.path))
            .collect::<Vec<_>>();
        if visible_files.is_empty() && visible_observed_files.is_empty() {
            return div().into_any_element();
        }
        let total_additions = visible_files
            .iter()
            .map(|file| file.additions)
            .sum::<usize>();
        let total_deletions = visible_files
            .iter()
            .map(|file| file.deletions)
            .sum::<usize>();
        let observed_additions = visible_observed_files
            .iter()
            .map(|file| file.additions)
            .sum::<usize>();
        let observed_deletions = visible_observed_files
            .iter()
            .map(|file| file.deletions)
            .sum::<usize>();
        let all_count = visible_files.len() + visible_observed_files.len();
        let card_key = summary
            .snapshot_id
            .map(|id| id.as_u128() as u64)
            .unwrap_or(agent.id.as_u128() as u64);

        crate::ui::style::chat_card(cx)
            .relative()
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ChangedFiles,
                cx,
            ))
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                gpui::svg()
                                    .path("icons/file-diff.svg")
                                    .size(crate::ui::design::icon_md())
                                    .flex_none()
                                    .text_color(crate::ui::design::t3(cx)),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Files changed"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(all_count.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::sage(cx))
                            .child(format!("+{}", total_additions + observed_additions)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(format!("-{}", total_deletions + observed_deletions)),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::ghost_button_compact(
                            ("agent-chat-changed-files-drawer", card_key),
                            "Open diff",
                        )
                        .text_color(crate::ui::design::t3(cx))
                        .on_click(cx.listener({
                            let agent_id = agent.id;
                            let project = agent.project_id;
                            let snapshot_id = summary.snapshot_id;
                            move |this, _, _, cx| {
                                let kind = snapshot_id.map_or(DiffKind::Project, |snapshot_id| {
                                    DiffKind::AgentSnapshot {
                                        snapshot_id,
                                        file: None,
                                    }
                                });
                                this.open_agent_diff_drawer(
                                    agent_id,
                                    project,
                                    kind,
                                    SharedString::from("All changed files"),
                                    cx,
                                );
                            }
                        })),
                    )
                    .child(
                        crate::ui::style::icon_button(
                            ("agent-chat-changed-files-open-diff", card_key),
                            IconName::ExternalLink,
                            cx,
                        )
                        .tooltip("Open in Code / Files")
                        .on_click(cx.listener({
                            let project = agent.project_id;
                            let snapshot_id = summary.snapshot_id;
                            move |this, _, _, cx| {
                                this.open_diff(
                                    project,
                                    snapshot_id.map_or(DiffKind::Project, |snapshot_id| {
                                        DiffKind::AgentSnapshot {
                                            snapshot_id,
                                            file: None,
                                        }
                                    }),
                                    SharedString::from("Changes"),
                                    cx,
                                );
                            }
                        })),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .children(visible_files.iter().enumerate().map(|(row_index, file)| {
                        self.render_changed_file_flat_row(
                            agent,
                            summary.snapshot_id,
                            row_index,
                            file,
                            cx,
                        )
                    }))
                    .when(!visible_observed_files.is_empty(), |list| {
                        list.child(change_receipt_section_label(
                            if summary.attribution_version == 0 {
                                "Earlier activity (unverified)"
                            } else {
                                "Observed from commands"
                            },
                            cx,
                        ))
                        .children(
                            visible_observed_files
                                .iter()
                                .enumerate()
                                .map(|(row_index, file)| {
                                    self.render_changed_file_flat_row(
                                        agent,
                                        summary.snapshot_id,
                                        visible_files.len() + row_index,
                                        file,
                                        cx,
                                    )
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_file_change_activity(
        &self,
        activity: &crate::state::agent_chat::FileChangeActivity,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let file = &activity.file;
        let name = file
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.path.display().to_string());
        let dir_label = changed_file_dir_label(&file.path);

        h_flex()
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_1p5()
            .px_1()
            .py(px(1.))
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(
                gpui_component::Icon::new(IconName::FileText)
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_color(crate::ui::design::t2(cx))
                    .child(name),
            )
            .when(file.additions > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_color(crate::ui::design::sage(cx))
                        .child(format!("+{}", file.additions)),
                )
            })
            .when(file.deletions > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_color(crate::ui::design::rose(cx))
                        .child(format!("-{}", file.deletions)),
                )
            })
            .when_some(dir_label, |row, dir| {
                row.child(
                    div()
                        .min_w(px(0.))
                        .flex_1()
                        .truncate()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .child(dir),
                )
            })
            .into_any_element()
    }

    /// A compact changed-file row. Detailed content is deliberately delegated
    /// to the virtualized DiffPane in the bottom drawer so chat scrolling stays
    /// light even for large historical snapshots.
    pub(super) fn render_changed_file_flat_row(
        &self,
        agent: &AgentRecord,
        snapshot_id: Option<Uuid>,
        row_index: usize,
        file: &crate::state::agent_chat::FileChangeStat,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let name = file
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.path.display().to_string());
        let dir_label = changed_file_dir_label(&file.path);
        let snapshot_key = snapshot_id.map(|id| id.as_u128() as u64).unwrap_or(0);
        let row_key = agent_changed_file_key(agent.id, &file.path) ^ snapshot_key;

        crate::ui::style::chat_card_row(cx)
            .id(("agent-chat-changed-file", row_key))
            .cursor_pointer()
            .when(row_index > 0, |row| {
                row.border_t_1().border_color(crate::ui::design::line(cx))
            })
            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
            .child(
                gpui_component::Icon::new(IconName::FileText)
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_head())
                            .text_color(crate::ui::design::t2(cx))
                            .child(name),
                    )
                    .when_some(dir_label, |row, dir| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx).opacity(0.6))
                                .child(dir),
                        )
                    }),
            )
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(40.))
                            .text_right()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::sage(cx))
                            .child(if file.additions > 0 {
                                format!("+{}", file.additions)
                            } else {
                                String::new()
                            }),
                    )
                    .child(
                        div()
                            .w(px(28.))
                            .text_right()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::rose(cx))
                            .child(if file.deletions > 0 {
                                format!("−{}", file.deletions)
                            } else {
                                String::new()
                            }),
                    )
                    .child(
                        gpui_component::Icon::new(IconName::PanelBottomOpen)
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::t4(cx)),
                    ),
            )
            .on_click(cx.listener({
                let agent_id = agent.id;
                let project = agent.project_id;
                let path = file.path.clone();
                move |this, _, _, cx| {
                    let title: SharedString = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string())
                        .into();
                    let kind = snapshot_id.map_or(
                        DiffKind::File {
                            path: path.clone(),
                            staged: false,
                        },
                        |snapshot_id| DiffKind::AgentSnapshot {
                            snapshot_id,
                            file: Some(path.clone()),
                        },
                    );
                    this.open_agent_diff_drawer(agent_id, project, kind, title, cx);
                }
            }))
            .into_any_element()
    }
}

fn change_receipt_section_label(
    label: &'static str,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    div()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(crate::ui::design::line(cx))
        .bg(crate::ui::design::surface(cx))
        .text_size(crate::ui::design::text_label())
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(crate::ui::design::t3(cx))
        .child(label)
        .into_any_element()
}
