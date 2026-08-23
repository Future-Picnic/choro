use super::*;

use std::collections::BTreeMap;

use ide_core::{
    local_store::{OrbitFieldKind, OrbitModuleId, OrbitRecord, OrbitRecordInput},
    DetectedService, ServiceCategory, SubAppServices,
};

impl CenterArea {
    /// Orbit's center surface. Built-in detection runs only for the selected
    /// Environment or Integrations item; custom views read local Orbit records.
    pub(super) fn render_services_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.orbit.read(cx).loading() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .child(logo_spinner(
                    crate::ui::design::SERVICES_SPINNER_SIZE,
                    "orbit-load",
                    project.0.as_u128() as usize,
                    crate::ui::design::surface_2(cx),
                ))
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Loading this project's Orbit…"),
                )
                .into_any_element();
        }
        let Some(orbit_selection) = self.orbit.read(cx).selected(project) else {
            return style::empty_state(
                IconName::Network,
                "Nothing is visible in this Orbit",
                "Use Add to Orbit in the sidebar to restore Environment, Integrations, or one of your Orbit views.",
                cx,
            )
            .into_any_element();
        };
        let kind = match orbit_selection {
            OrbitModuleId::Builtin(builtin) => ServicesScanKind::from_builtin(builtin),
            OrbitModuleId::Custom(module_id) => {
                return self.render_orbit_custom_module(project, module_id, cx);
            }
        };
        let root = self
            .workspace
            .read(cx)
            .active_project()
            .map(|p| p.path.clone());

        if let Some(root) = root.clone() {
            self.services.update(cx, |state, cx| {
                state.ensure_scanned(project, root, kind, cx)
            });
        }

        let sub_apps = self
            .services
            .read(cx)
            .services_for(project, kind)
            .cloned()
            .unwrap_or_default();
        let scanning = self.services.read(cx).is_scanning(project, kind);
        let tabs = services_center_tabs(&sub_apps, kind);

        if tabs.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(crate::ui::design::services_empty_gap())
                .when(scanning, |col| {
                    col.child(logo_spinner(
                        crate::ui::design::SERVICES_SPINNER_SIZE,
                        "services-scan",
                        project.0.as_u128() as usize,
                        crate::ui::design::surface_2(cx),
                    ))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::design::t3(cx))
                            .child(kind.scanning_message()),
                    )
                })
                .when(!scanning, |col| {
                    col.child(
                        v_flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(160.))
                                    .h(px(110.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        crate::ui::illustrations::illustration(
                                            crate::ui::illustrations::Illustration::Services,
                                            cx,
                                        )
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_title())
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(kind.empty_title()),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(kind.empty_body()),
                                    ),
                            ),
                    )
                })
                .into_any_element();
        }

        let root = root.unwrap_or_default();
        let tab_keys = tabs.iter().map(|tab| tab.key.clone()).collect::<Vec<_>>();
        let selected_key = self
            .services
            .read(cx)
            .selected_tab(project, kind, &tab_keys)
            .unwrap_or_else(|| tabs[0].key.clone());
        let selected_tab = tabs
            .iter()
            .find(|tab| tab.key == selected_key)
            .unwrap_or(&tabs[0]);
        let selected_sub = &sub_apps[selected_tab.sub_index];
        let content = match kind {
            ServicesScanKind::Integrations => self.render_services_grid(selected_sub, cx),
            ServicesScanKind::Environment => self.render_services_env_file(
                project,
                selected_sub,
                selected_tab.env_file.as_deref().unwrap_or_default(),
                &root,
                cx,
            ),
        };

        v_flex()
            .size_full()
            .child(self.render_services_header(project, &sub_apps, kind, cx))
            .child(self.render_services_tabs(project, kind, &tabs, &selected_key, cx))
            .child(
                v_flex()
                    .id("services-center-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .items_center()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            // Match the chat column: use the available center-pane
                            // width, then stay centered once the readable cap is hit.
                            .max_w(crate::ui::design::agent_chat_content_max_w())
                            .mx_auto()
                            .px(crate::ui::design::agent_chat_gutter_x())
                            .py(crate::ui::design::center_column_pad_y())
                            .child(content),
                    ),
            )
            .into_any_element()
    }

    fn render_orbit_custom_module(
        &mut self,
        project: ProjectId,
        module_id: uuid::Uuid,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let module_data = {
            let orbit = self.orbit.read(cx);
            orbit.module(module_id).map(|module| {
                (
                    module.name.clone(),
                    module.section_label.clone(),
                    module
                        .fields
                        .iter()
                        .filter(|field| !field.archived)
                        .cloned()
                        .collect::<Vec<_>>(),
                    orbit.records_shared(project, module_id),
                )
            })
        };
        let Some((module_name, section_label, fields, records)) = module_data else {
            return style::empty_state(
                IconName::Network,
                "Orbit view unavailable",
                "This Orbit view may have been archived in Settings.",
                cx,
            )
            .into_any_element();
        };
        let query = self.orbit_search.read(cx).value().trim().to_lowercase();
        let filtered = records
            .iter()
            .filter(|record| orbit_record_matches(record, &query))
            .collect::<Vec<_>>();
        let record_count = records.len();
        let grouping = section_label
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .map(|label| format!("Grouped by {label}"))
            .unwrap_or_else(|| "Ungrouped table".to_string());
        let empty_action = if module_name.eq_ignore_ascii_case("Analytics") {
            "/Analytics Find the existing analytics events and build this lexicon.".to_string()
        } else {
            format!("/{module_name} Inspect the project and build this Orbit.")
        };

        let header = crate::ui::design::header::bar(cx)
            .child(
                crate::ui::design::header::title_col(cx)
                    .child(crate::ui::design::header::title(
                        SharedString::from(module_name),
                        cx,
                    ))
                    .child(crate::ui::design::header::subtitle(
                        SharedString::from(format!(
                            "{record_count} record{} · {grouping}",
                            if record_count == 1 { "" } else { "s" },
                        )),
                        cx,
                    )),
            )
            .child(
                crate::ui::design::header::actions()
                    .child(div().w(px(260.)).child(Input::new(&self.orbit_search)))
                    .child(
                        style::primary_button_compact("orbit-add-record", "Add record", cx)
                            .icon(IconName::Plus)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.begin_orbit_record_edit(project, module_id, None, window, cx);
                            })),
                    ),
            );

        let content = if filtered.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(style::empty_state(
                    IconName::Network,
                    if query.is_empty() {
                        "No records yet"
                    } else {
                        "No matching records"
                    },
                    if query.is_empty() {
                        empty_action
                    } else {
                        "Try a different search.".to_string()
                    },
                    cx,
                ))
                .into_any_element()
        } else {
            self.render_orbit_grouped_table(module_id, &fields, &filtered, project, cx)
        };

        v_flex()
            .size_full()
            .child(header)
            .when_some(
                self.orbit.read(cx).error().map(str::to_string),
                |page, error| {
                    page.child(
                        div()
                            .px_4()
                            .py_2()
                            .bg(crate::ui::design::rose(cx).opacity(0.12))
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::rose(cx))
                            .child(error),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(content),
            )
            .into_any_element()
    }

    fn render_orbit_grouped_table(
        &self,
        module_id: uuid::Uuid,
        fields: &[ide_core::local_store::OrbitFieldDefinition],
        records: &[&OrbitRecord],
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut groups: BTreeMap<&str, Vec<&OrbitRecord>> = BTreeMap::new();
        for record in records.iter().copied() {
            groups
                .entry(record.section.as_deref().unwrap_or("Records"))
                .or_default()
                .push(record);
        }
        let table_min_width = orbit_table_min_width(fields.len());
        div()
            .size_full()
            .relative()
            .child(
                v_flex()
                    .size_full()
                    .id("orbit-records-scroll")
                    .track_scroll(&self.orbit_table_scroll)
                    .overflow_scroll()
                    .child(
                        v_flex()
                            .min_w(px(table_min_width))
                            .w_full()
                            .px_5()
                            .py(crate::ui::design::center_column_pad_y())
                            .gap_6()
                            .children(groups.into_iter().map(|(section, records)| {
                                let collapse_key = (project, module_id, section.to_string());
                                let collapsed =
                                    self.orbit_collapsed_sections.contains(&collapse_key);
                                let toggle_key = collapse_key.clone();
                                v_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(
                                        style::orbit_section_toggle_button(SharedString::from(
                                            format!("orbit-section-{module_id}-{section}"),
                                        ))
                                        .tooltip(if collapsed {
                                            "Expand section"
                                        } else {
                                            "Collapse section"
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !this.orbit_collapsed_sections.remove(&toggle_key) {
                                                this.orbit_collapsed_sections
                                                    .insert(toggle_key.clone());
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .items_center()
                                                .gap_2()
                                                .px_1()
                                                .py_1()
                                                .child(
                                                    Icon::new(if collapsed {
                                                        IconName::ChevronRight
                                                    } else {
                                                        IconName::ChevronDown
                                                    })
                                                    .size(crate::ui::design::icon_sm())
                                                    .text_color(crate::ui::design::t4(cx)),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_head())
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(crate::ui::design::t1(cx))
                                                        .child(SharedString::from(section.to_string())),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_label())
                                                        .text_color(crate::ui::design::t4(cx))
                                                        .child(format!(
                                                            "{} record{}",
                                                            records.len(),
                                                            if records.len() == 1 { "" } else { "s" }
                                                        )),
                                                ),
                                        ),
                                    )
                                    .when(!collapsed, |group| {
                                        group.child(
                                            style::chat_card(cx)
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .min_h(px(36.))
                                                        .items_center()
                                                        .bg(crate::ui::design::surface_2(cx).opacity(0.48))
                                                        .border_b_1()
                                                        .border_color(crate::ui::design::line(cx))
                                                        .children(fields.iter().map(|field| {
                                                            orbit_table_header_cell(
                                                                SharedString::from(field.label.clone()),
                                                                cx,
                                                            )
                                                        })),
                                                )
                                                .children(
                                                    records.into_iter().enumerate().map(|(index, record)| {
                                                        let record_id = record.id;
                                                        style::orbit_table_data_row(
                                                            SharedString::from(format!(
                                                                "orbit-record-{record_id}"
                                                            )),
                                                            cx,
                                                        )
                                                        .when(index > 0, |row| {
                                                            row.border_t_1()
                                                                .border_color(crate::ui::design::line(cx))
                                                        })
                                                        .children(fields.iter().enumerate().map(
                                                            |(field_index, field)| {
                                                                orbit_table_value_cell(
                                                                    record.values.get(&field.key),
                                                                    field.primary,
                                                                    SharedString::from(format!(
                                                                        "orbit-cell-{record_id}-{field_index}"
                                                                    )),
                                                                    cx,
                                                                )
                                                            },
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.begin_orbit_record_edit(
                                                                    project,
                                                                    module_id,
                                                                    Some(record_id),
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        ))
                                                    }),
                                                ),
                                        )
                                    })
                            })),
                    ),
            )
            .child(Scrollbar::new(&self.orbit_table_scroll).axis(ScrollbarAxis::Both))
            .into_any_element()
    }

    fn begin_orbit_record_edit(
        &mut self,
        project: ProjectId,
        module_id: uuid::Uuid,
        record_id: Option<uuid::Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(module) = self.orbit.read(cx).module(module_id).cloned() else {
            return;
        };
        let record = record_id.and_then(|record_id| {
            self.orbit
                .read(cx)
                .records(project, module_id)
                .iter()
                .find(|record| record.id == record_id)
                .cloned()
        });
        let section = module.section_key.as_ref().map(|_| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(
                        record
                            .as_ref()
                            .and_then(|record| record.section.clone())
                            .unwrap_or_default(),
                    )
                    .placeholder(
                        module
                            .section_label
                            .clone()
                            .unwrap_or_else(|| "Section".into()),
                    )
            })
        });
        let fields = module
            .fields
            .iter()
            .filter(|field| !field.archived)
            .map(|field| {
                let value = record
                    .as_ref()
                    .and_then(|record| record.values.get(&field.key))
                    .map(orbit_value_editor_text)
                    .unwrap_or_default();
                let kind = field.kind;
                let input = cx.new(|cx| {
                    let input = InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(field.label.clone())
                        .multi_line(kind != OrbitFieldKind::ShortText);
                    match kind {
                        OrbitFieldKind::ShortText => input,
                        OrbitFieldKind::LongText => input.rows(4),
                        OrbitFieldKind::List => input.rows(5),
                    }
                });
                OrbitRecordFieldEditor {
                    key: field.key.clone(),
                    label: field.label.clone(),
                    kind,
                    required: field.primary,
                    input,
                }
            })
            .collect();
        let request_id = uuid::Uuid::new_v4();
        let window_handle = window.window_handle();
        self.orbit_record_editor = Some(OrbitRecordEditor {
            request_id,
            project,
            module_id,
            record_id,
            section,
            fields,
            confirming_delete: false,
            window_handle,
        });
        let center = cx.entity().clone();
        let dialog_body = OrbitRecordDialogView::view(center.clone(), cx);
        let title = if record_id.is_some() {
            "Edit record"
        } else {
            "New record"
        };
        let close_center = center.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let footer_center = center.clone();
            dialog
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(crate::ui::confirm::icon_badge(
                            IconName::Network,
                            crate::ui::design::accent(cx),
                            cx,
                        ))
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title)),
                )
                .w(px(680.))
                .max_w(px(680.))
                .margin_top(px(56.))
                .overlay_closable(false)
                .on_close({
                    let close_center = close_center.clone();
                    move |_, _, cx| {
                        close_center.update(cx, |center, cx| {
                            center.orbit_record_editor = None;
                            cx.notify();
                        });
                    }
                })
                .child(dialog_body.clone())
                .footer(move |_, _, _, cx| {
                    let (editing, confirming_delete, saving) = {
                        let center = footer_center.read(cx);
                        let editing = center
                            .orbit_record_editor
                            .as_ref()
                            .is_some_and(|editor| editor.record_id.is_some());
                        let confirming_delete = center
                            .orbit_record_editor
                            .as_ref()
                            .is_some_and(|editor| editor.confirming_delete);
                        (editing, confirming_delete, center.orbit.read(cx).saving())
                    };

                    if confirming_delete {
                        let keep_center = footer_center.clone();
                        let delete_center = footer_center.clone();
                        return vec![
                            style::dialog_neutral_button("orbit-keep-record", "Keep record", cx)
                                .disabled(saving)
                                .on_click(move |_, _, cx| {
                                    keep_center.update(cx, |center, cx| {
                                        if let Some(editor) = center.orbit_record_editor.as_mut() {
                                            editor.confirming_delete = false;
                                        }
                                        cx.notify();
                                    });
                                }),
                            style::danger_button_compact(
                                "orbit-delete-record-confirm",
                                "Delete record",
                            )
                            .disabled(saving)
                            .on_click(move |_, window, cx| {
                                delete_center.update(cx, |center, cx| {
                                    center.delete_orbit_record_editor(cx);
                                });
                                window.close_dialog(cx);
                            }),
                        ];
                    }

                    let mut actions = Vec::new();
                    if editing {
                        let delete_center = footer_center.clone();
                        actions.push(
                            style::danger_button_compact("orbit-delete-record", "Delete…")
                                .disabled(saving)
                                .on_click(move |_, _, cx| {
                                    delete_center.update(cx, |center, cx| {
                                        if let Some(editor) = center.orbit_record_editor.as_mut() {
                                            editor.confirming_delete = true;
                                        }
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    let cancel_center = footer_center.clone();
                    actions.push(
                        style::dialog_neutral_button("orbit-cancel-record", "Cancel", cx)
                            .disabled(saving)
                            .on_click(move |_, window, cx| {
                                cancel_center.update(cx, |center, cx| {
                                    center.orbit_record_editor = None;
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    );
                    let save_center = footer_center.clone();
                    actions.push(
                        style::primary_button_compact(
                            "orbit-save-record",
                            if saving { "Saving…" } else { "Save record" },
                            cx,
                        )
                        .disabled(saving)
                        .on_click(move |_, _, cx| {
                            save_center.update(cx, |center, cx| {
                                center.save_orbit_record_editor(cx);
                            });
                        }),
                    );
                    actions
                })
        });

        let focus_target = self.orbit_record_editor.as_ref().and_then(|editor| {
            editor
                .section
                .clone()
                .or_else(|| editor.fields.first().map(|field| field.input.clone()))
        });
        if let Some(input) = focus_target {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    fn render_orbit_record_editor_dialog(&self, cx: &App) -> gpui::AnyElement {
        let Some(editor) = self.orbit_record_editor.as_ref() else {
            return div().into_any_element();
        };
        let module = self.orbit.read(cx).module(editor.module_id);
        let module_name = module
            .map(|module| module.name.as_str())
            .unwrap_or("Orbit view");
        let module_description = module
            .map(|module| module.description.trim())
            .filter(|description| !description.is_empty());
        let section_label = module
            .and_then(|module| module.section_label.as_deref())
            .filter(|label| !label.trim().is_empty())
            .unwrap_or("Section");

        v_flex()
            .w_full()
            .max_h(px(560.))
            .gap_4()
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_head())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(SharedString::from(module_name.to_string())),
                    )
                    .when_some(
                        module_description.map(str::to_string),
                        |header, description| {
                            header.child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.4))
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(SharedString::from(description)),
                            )
                        },
                    ),
            )
            .when(editor.confirming_delete, |body| {
                body.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .py_2p5()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.35))
                        .bg(crate::ui::design::rose(cx).opacity(0.1))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::rose(cx)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::rose(cx))
                                        .child("Delete this record?"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("This removes it from this project's Orbit."),
                                ),
                        ),
                )
            })
            .when_some(
                self.orbit.read(cx).error().map(str::to_string),
                |body, error| {
                    body.child(
                        h_flex()
                            .w_full()
                            .items_start()
                            .gap_2()
                            .px_3()
                            .py_2p5()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::rose(cx).opacity(0.1))
                            .child(
                                Icon::new(IconName::CircleX)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::rose(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_label())
                                    .line_height(gpui::relative(1.35))
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(SharedString::from(error)),
                            ),
                    )
                },
            )
            .child(
                v_flex()
                    .id("orbit-record-dialog-fields")
                    .w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .gap_4()
                    .pr_2()
                    .when_some(editor.section.as_ref(), |form, input| {
                        form.child(orbit_editor_field(
                            section_label,
                            Input::new(input),
                            OrbitFieldKind::ShortText,
                            true,
                            cx,
                        ))
                    })
                    .children(editor.fields.iter().map(|field| {
                        orbit_editor_field(
                            &field.label,
                            Input::new(&field.input),
                            field.kind,
                            field.required,
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }

    fn save_orbit_record_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.orbit_record_editor.as_ref() else {
            return;
        };
        let section = editor
            .section
            .as_ref()
            .map(|input| input.read(cx).value().to_string());
        let mut values = BTreeMap::new();
        for field in &editor.fields {
            let text = field.input.read(cx).value().to_string();
            let value = match field.kind {
                OrbitFieldKind::ShortText | OrbitFieldKind::LongText => {
                    serde_json::Value::String(text)
                }
                OrbitFieldKind::List => serde_json::Value::Array(
                    text.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(|line| serde_json::Value::String(line.to_string()))
                        .collect(),
                ),
            };
            values.insert(field.key.clone(), value);
        }
        let request_id = editor.request_id;
        self.orbit.update(cx, |orbit, cx| {
            orbit.save_record(
                editor.project,
                editor.module_id,
                OrbitRecordInput {
                    id: editor.record_id,
                    section,
                    values,
                },
                request_id,
                cx,
            )
        });
        cx.notify();
    }

    fn delete_orbit_record_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.orbit_record_editor.as_ref() else {
            return;
        };
        let Some(record_id) = editor.record_id else {
            return;
        };
        let project = editor.project;
        let module_id = editor.module_id;
        self.orbit.update(cx, |orbit, cx| {
            orbit.delete_record(project, module_id, record_id, cx)
        });
        self.orbit_record_editor = None;
        cx.notify();
    }

    fn render_services_header(
        &self,
        project: ProjectId,
        sub_apps: &[SubAppServices],
        kind: ServicesScanKind,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let item_count = sub_apps
            .iter()
            .map(|sub| match kind {
                ServicesScanKind::Integrations => sub.services.len(),
                ServicesScanKind::Environment => sub.env_files.len(),
            })
            .sum::<usize>();
        let source_count = sub_apps.len();
        let title = kind.builtin().label();
        let item_label = kind.item_label(item_count);
        let subtitle = if source_count <= 1 {
            format!("{item_count} {item_label}")
        } else {
            format!("{item_count} {item_label} across {source_count} project areas")
        };
        let reveal = self.services_reveal;
        use crate::ui::design::header;
        header::bar(cx)
            .child(
                header::title_col(cx)
                    .child(header::title(title, cx))
                    .child(header::subtitle(SharedString::from(subtitle), cx)),
            )
            .child(
                header::actions()
                    .when(
                        kind == ServicesScanKind::Environment
                            && sub_apps.iter().any(|sub| !sub.env_files.is_empty()),
                        |row| {
                            row.child(
                                style::ghost_button_compact(
                                    "services-env-reveal",
                                    if reveal {
                                        "Hide values"
                                    } else {
                                        "Reveal values"
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.services_reveal = !this.services_reveal;
                                        cx.notify();
                                    },
                                )),
                            )
                        },
                    )
                    .child(
                        style::ghost_button_compact("services-rescan", "Rescan").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.services
                                    .update(cx, |state, cx| state.invalidate(project, kind, cx));
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_services_tabs(
        &self,
        project: ProjectId,
        kind: ServicesScanKind,
        tabs: &[ServicesCenterTab],
        selected_key: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .h(px(38.))
            .flex_none()
            .items_center()
            .px_5()
            .border_b_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::base(cx))
            .child(
                div()
                    .id("services-source-tabs-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_x_scroll()
                    .child(h_flex().h_full().items_center().gap_1().children(
                        tabs.iter().enumerate().map(|(index, tab)| {
                            let key = tab.key.clone();
                            style::nav_tab(
                                ("services-source-tab", index),
                                SharedString::from(tab.label.clone()),
                                tab.key == selected_key,
                                cx,
                            )
                            .flex_none()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.services.update(cx, |services, cx| {
                                        services.select_tab(project, kind, key.clone(), cx)
                                    });
                                },
                            ))
                        }),
                    )),
            )
            .into_any_element()
    }

    fn render_services_grid(
        &self,
        sub: &SubAppServices,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Services arrive pre-sorted by category, so group by walking runs of the
        // same category into their own labelled section.
        let mut groups: Vec<(ServiceCategory, Vec<&DetectedService>)> = Vec::new();
        for svc in &sub.services {
            match groups.last_mut() {
                Some((cat, items)) if *cat == svc.category => items.push(svc),
                _ => groups.push((svc.category, vec![svc])),
            }
        }

        v_flex()
            .w_full()
            .gap(crate::ui::design::services_group_gap())
            .children(groups.into_iter().map(|(category, items)| {
                style::chat_card(cx)
                    .child(
                        style::chat_card_head(cx)
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Server,
                                crate::ui::design::t3(cx),
                                crate::ui::design::icon_md(),
                            ))
                            .child(SharedString::from(category.label().to_string()))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(format!("{}", items.len())),
                            ),
                    )
                    .children(
                        items
                            .into_iter()
                            .enumerate()
                            .map(|(index, service)| render_service_row(service, index, cx)),
                    )
            }))
            .into_any_element()
    }

    fn render_services_env_file(
        &mut self,
        project: ProjectId,
        sub: &SubAppServices,
        selected_file: &str,
        root: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if sub.env_files.is_empty() {
            return style::empty_state(
                IconName::File,
                "No env files here",
                "This source has no .env files to show.",
                cx,
            )
            .into_any_element();
        }

        let dir = if sub.rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(&sub.rel_path)
        };
        let names = sub.env_files.clone();
        let rel = sub.rel_path.clone();
        let files = self.services.update(cx, |state, _cx| {
            state.env_files(project, &rel, dir.clone(), &names)
        });
        let Some(file) = files.into_iter().find(|file| file.name == selected_file) else {
            return style::empty_state(
                IconName::File,
                "Env file unavailable",
                "Rescan Environment to refresh the available files.",
                cx,
            )
            .into_any_element();
        };
        let reveal = self.services_reveal;
        let path = dir.join(&file.name);
        let file_name = file.name.clone();
        let (badge, badge_color) = env_badge(&file_name, cx);
        let rows = file
            .entries
            .iter()
            .enumerate()
            .map(|(ix, entry)| self.render_env_row(&path, entry, reveal, ix, cx))
            .collect::<Vec<_>>();
        style::chat_card(cx)
            .child(
                style::chat_card_head(cx)
                    .child(
                        gpui_component::Icon::new(IconName::File)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(
                        div()
                            .font_family(crate::ui::design::FONT_MONO)
                            .child(SharedString::from(file_name)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(SharedString::from(if sub.rel_path == "." {
                                "Project".to_string()
                            } else {
                                sub.rel_path.clone()
                            })),
                    )
                    .when_some(badge, |header, badge| {
                        header.child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(badge_color)
                                .child(SharedString::from(badge)),
                        )
                    }),
            )
            .when(file.entries.is_empty(), |card| {
                card.child(
                    style::chat_card_row(cx)
                        .text_color(crate::ui::design::t3(cx))
                        .child("No variables in this file."),
                )
            })
            .children(rows)
            .into_any_element()
    }

    fn render_env_row(
        &self,
        path: &std::path::Path,
        entry: &ide_core::EnvEntry,
        reveal: bool,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let editing = self
            .services_env_edit
            .as_ref()
            .filter(|e| e.path == path && e.key == entry.key);

        let row = style::chat_card_row(cx)
            .when(ix > 0, |r| {
                r.border_t_1().border_color(crate::ui::design::line(cx))
            })
            .child(
                div()
                    .w(crate::ui::design::services_env_key_w())
                    .flex_none()
                    .font_family(crate::ui::design::FONT_MONO)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t2(cx))
                    .truncate()
                    .child(SharedString::from(entry.key.clone())),
            );

        if let Some(edit) = editing {
            row.child(div().flex_1().min_w(px(0.)).child(Input::new(&edit.input)))
                .child(
                    style::primary_button_compact("env-save", "Save", cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.save_env_edit(cx);
                        },
                    )),
                )
                .child(
                    style::ghost_button_compact("env-cancel", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.cancel_env_edit(cx);
                        },
                    )),
                )
                .into_any_element()
        } else {
            let shown = if reveal {
                entry.value.clone()
            } else {
                "•".repeat(entry.value.chars().count().clamp(6, 16))
            };
            let path = path.to_path_buf();
            let key = entry.key.clone();
            let value = entry.value.clone();
            row.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(crate::ui::design::FONT_MONO)
                    .truncate()
                    .text_color(if reveal {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(SharedString::from(shown)),
            )
            .child(
                style::ghost_button_compact(
                    SharedString::from(format!("env-edit-{}-{}", path.display(), entry.key)),
                    "Edit",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_env_edit(path.clone(), key.clone(), value.clone(), window, cx);
                })),
            )
            .into_any_element()
        }
    }

    pub(super) fn begin_env_edit(
        &mut self,
        path: std::path::PathBuf,
        key: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        self.services_env_edit = Some(ServicesEnvEdit { path, key, input });
        cx.notify();
    }

    pub(super) fn save_env_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.services_env_edit.take() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        if let Err(error) = ide_core::set_env_value(&edit.path, &edit.key, &value) {
            eprintln!("services: failed to write env value: {error}");
        }
        self.services
            .update(cx, |state, cx| state.clear_env_cache(cx));
        cx.notify();
    }

    pub(super) fn cancel_env_edit(&mut self, cx: &mut Context<Self>) {
        if self.services_env_edit.is_some() {
            self.services_env_edit = None;
            cx.notify();
        }
    }
}

/// The env value currently being edited inline.
pub(super) struct ServicesEnvEdit {
    pub path: std::path::PathBuf,
    pub key: String,
    pub input: gpui::Entity<InputState>,
}

#[derive(Clone)]
struct ServicesCenterTab {
    key: String,
    label: String,
    sub_index: usize,
    env_file: Option<String>,
}

fn services_center_tabs(
    sub_apps: &[SubAppServices],
    kind: ServicesScanKind,
) -> Vec<ServicesCenterTab> {
    match kind {
        ServicesScanKind::Integrations => sub_apps
            .iter()
            .enumerate()
            .filter(|(_, sub)| !sub.services.is_empty())
            .map(|(sub_index, sub)| ServicesCenterTab {
                key: format!("integrations:{}", sub.rel_path),
                label: sub.name.clone(),
                sub_index,
                env_file: None,
            })
            .collect(),
        ServicesScanKind::Environment => {
            let source_count = sub_apps
                .iter()
                .filter(|sub| !sub.env_files.is_empty())
                .count();
            sub_apps
                .iter()
                .enumerate()
                .flat_map(|(sub_index, sub)| {
                    sub.env_files.iter().map(move |file| ServicesCenterTab {
                        key: format!("environment:{}:{file}", sub.rel_path),
                        label: if source_count > 1 {
                            format!("{} · {file}", sub.name)
                        } else {
                            file.clone()
                        },
                        sub_index,
                        env_file: Some(file.clone()),
                    })
                })
                .collect()
        }
    }
}

#[derive(Clone)]
pub(super) struct OrbitRecordEditor {
    pub(super) request_id: uuid::Uuid,
    pub(super) project: ProjectId,
    pub(super) module_id: uuid::Uuid,
    record_id: Option<uuid::Uuid>,
    section: Option<gpui::Entity<InputState>>,
    fields: Vec<OrbitRecordFieldEditor>,
    confirming_delete: bool,
    pub(super) window_handle: gpui::AnyWindowHandle,
}

#[derive(Clone)]
struct OrbitRecordFieldEditor {
    key: String,
    label: String,
    kind: OrbitFieldKind,
    required: bool,
    input: gpui::Entity<InputState>,
}

struct OrbitRecordDialogView {
    center: Entity<CenterArea>,
    _center_subscription: gpui::Subscription,
}

impl OrbitRecordDialogView {
    fn view(center: Entity<CenterArea>, cx: &mut Context<CenterArea>) -> Entity<Self> {
        cx.new(|cx| {
            let center_subscription = cx.observe(&center, |_, _, cx| cx.notify());
            Self {
                center,
                _center_subscription: center_subscription,
            }
        })
    }
}

impl Render for OrbitRecordDialogView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.center.read(cx).render_orbit_record_editor_dialog(cx)
    }
}

fn orbit_record_matches(record: &OrbitRecord, query: &str) -> bool {
    query.is_empty()
        || record
            .section
            .as_deref()
            .is_some_and(|section| section.to_lowercase().contains(query))
        || record
            .values
            .values()
            .any(|value| orbit_value_matches(value, query))
}

fn orbit_value_matches(value: &serde_json::Value, query: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value.to_lowercase().contains(query),
        serde_json::Value::Array(values) => values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|value| value.to_lowercase().contains(query))
        }),
        serde_json::Value::Null => false,
        other => other.to_string().to_lowercase().contains(query),
    }
}

fn orbit_value_display(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn orbit_value_editor_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => orbit_value_display(value),
    }
}

const ORBIT_TABLE_COLUMN_MIN_WIDTH: f32 = 320.0;
const ORBIT_TABLE_MIN_WIDTH: f32 = 720.0;

fn orbit_table_min_width(field_count: usize) -> f32 {
    (field_count as f32 * ORBIT_TABLE_COLUMN_MIN_WIDTH).max(ORBIT_TABLE_MIN_WIDTH)
}

fn orbit_table_header_cell(text: SharedString, cx: &App) -> gpui::AnyElement {
    orbit_table_cell_frame()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t2(cx))
        .truncate()
        .child(text)
        .into_any_element()
}

fn orbit_table_value_cell(
    value: Option<&serde_json::Value>,
    primary: bool,
    cell_id: SharedString,
    cx: &App,
) -> gpui::AnyElement {
    let (preview, full_lines, hidden_lines) = orbit_value_table_preview(value);
    let preview_row = h_flex()
        .id(cell_id)
        .w_full()
        .min_w(px(0.))
        .items_center()
        .gap_1p5()
        .when_some(full_lines, |row, full_lines| {
            row.tooltip(move |window, cx| {
                let lines = full_lines.clone();
                Tooltip::element(move |_, cx| {
                    v_flex()
                        .w(px(420.))
                        .gap_1()
                        .py_1()
                        .children(lines.clone().into_iter().map(|line| {
                            div()
                                .w_full()
                                .whitespace_normal()
                                .text_size(crate::ui::design::text_label())
                                .line_height(gpui::relative(1.4))
                                .text_color(crate::ui::design::t2(cx))
                                .child(SharedString::from(line))
                        }))
                })
                .build(window, cx)
            })
        })
        .child(
            div()
                .flex_shrink()
                .min_w(px(0.))
                .truncate()
                .child(SharedString::from(preview)),
        )
        .when(hidden_lines > 0, |row| {
            row.child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    .text_color(crate::ui::design::t4(cx))
                    .child(
                        Icon::new(IconName::Ellipsis)
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::t4(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .child(format!("+{hidden_lines}")),
                    ),
            )
        });

    orbit_table_cell_frame()
        .text_size(crate::ui::design::text_ui())
        .font_weight(if primary {
            gpui::FontWeight::MEDIUM
        } else {
            gpui::FontWeight::NORMAL
        })
        .text_color(if primary {
            crate::ui::design::t1(cx)
        } else {
            crate::ui::design::t3(cx)
        })
        .child(preview_row)
        .into_any_element()
}

fn orbit_table_cell_frame() -> gpui::Div {
    div()
        .flex_1()
        .min_w(px(ORBIT_TABLE_COLUMN_MIN_WIDTH))
        .overflow_hidden()
        .px_3()
        .py_2p5()
}

fn orbit_value_table_preview(
    value: Option<&serde_json::Value>,
) -> (String, Option<Vec<String>>, usize) {
    let Some(value) = value else {
        return (String::new(), None, 0);
    };
    let lines = match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        serde_json::Value::String(value) => value
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        serde_json::Value::Null => Vec::new(),
        other => vec![other.to_string()],
    };
    let Some(first) = lines.first().cloned() else {
        return (String::new(), None, 0);
    };
    let hidden_lines = lines.len().saturating_sub(1);
    // Every populated value gets hover details. This matters for long one-line
    // values (especially Notes), which can be visually truncated even though
    // they do not have additional logical lines to count.
    let full_lines = Some(lines);
    (first, full_lines, hidden_lines)
}

fn orbit_editor_field(
    label: &str,
    input: gpui_component::input::Input,
    kind: OrbitFieldKind,
    required: bool,
    cx: &App,
) -> gpui::AnyElement {
    let input = match kind {
        OrbitFieldKind::ShortText => input.w_full(),
        OrbitFieldKind::LongText => input.w_full().h(px(112.)).flex_none(),
        OrbitFieldKind::List => input.w_full().h(px(136.)).flex_none(),
    };
    v_flex()
        .w_full()
        .gap_1p5()
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t2(cx))
                        .child(SharedString::from(label.to_string())),
                )
                .when(required, |row| {
                    row.child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::rose(cx))
                            .child("Required"),
                    )
                }),
        )
        .child(input)
        .when(kind == OrbitFieldKind::List, |field| {
            field.child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t4(cx))
                    .child("One item per line"),
            )
        })
        .into_any_element()
}

fn env_badge(name: &str, cx: &App) -> (Option<String>, gpui::Hsla) {
    if name.ends_with(".production") {
        (Some("prod".into()), crate::ui::design::rose(cx))
    } else if name.ends_with(".development") {
        (Some("dev".into()), crate::ui::design::sky(cx))
    } else if name.ends_with(".local") {
        (Some("local".into()), crate::ui::design::t3(cx))
    } else if name.ends_with(".example") {
        (Some("template".into()), crate::ui::design::t3(cx))
    } else {
        (None, crate::ui::design::t3(cx))
    }
}

fn render_service_row(
    svc: &DetectedService,
    index: usize,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let dash_url = svc.dashboard_url.clone().or_else(|| svc.docs_url.clone());
    let evidence = svc.evidence.first().cloned();
    v_flex()
        .w_full()
        .when(index > 0, |row| {
            row.border_t_1().border_color(crate::ui::design::line(cx))
        })
        .child(
            style::chat_card_row(cx)
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::Server,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_md(),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(crate::ui::design::services_fact_gap())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_head())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t2(cx))
                                .truncate()
                                .child(SharedString::from(svc.name.clone())),
                        )
                        .when_some(evidence, |column, evidence| {
                            column.child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_color(crate::ui::design::t4(cx))
                                    .truncate()
                                    .child(SharedString::from(evidence)),
                            )
                        }),
                )
                .when_some(dash_url, |row, url| {
                    row.child(
                        div()
                            .id(SharedString::from(format!("svc-dash-{}", svc.id)))
                            .flex_none()
                            .h(crate::ui::design::control_h_xs())
                            .w(crate::ui::design::control_h_xs())
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|button| button.bg(crate::ui::design::surface_2(cx)))
                            .child(
                                gpui_component::Icon::new(IconName::ExternalLink)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t4(cx)),
                            )
                            .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url)),
                    )
                }),
        )
        .when(!svc.facts.is_empty(), |card| {
            card.child(
                v_flex().w_full().children(
                    svc.facts
                        .iter()
                        .enumerate()
                        .map(|(ix, fact)| render_service_fact(&svc.id, ix, fact, cx)),
                ),
            )
        })
        .into_any_element()
}

fn render_service_fact(
    service_id: &str,
    ix: usize,
    fact: &ide_core::ServiceFact,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let row = h_flex()
        .w_full()
        .items_center()
        .gap(crate::ui::design::services_fact_gap())
        .px(crate::ui::design::chat_card_row_pad_x())
        .pb(crate::ui::design::chat_card_row_pad_y())
        .when(!fact.label.is_empty(), |r| {
            r.child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t4(cx))
                    .child(SharedString::from(fact.label.clone())),
            )
        });
    match &fact.url {
        Some(url) => {
            let url = url.clone();
            let link = crate::ui::design::sky(cx);
            row.child(
                h_flex()
                    .id(SharedString::from(format!("svc-fact-{service_id}-{ix}")))
                    .flex_1()
                    .min_w(px(0.))
                    .items_center()
                    .gap(crate::ui::design::services_fact_gap())
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .hover(|r| r.bg(link.opacity(0.12)))
                    .child(
                        gpui_component::Icon::new(IconName::ExternalLink)
                            .size(crate::ui::design::icon_sm())
                            .text_color(link),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_color(link)
                            .truncate()
                            .child(SharedString::from(fact.value.clone())),
                    )
                    .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url)),
            )
            .into_any_element()
        }
        None => row
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_ui())
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_color(crate::ui::design::t3(cx))
                    .truncate()
                    .child(SharedString::from(fact.value.clone())),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_table_lists_render_as_a_single_preview_with_hover_details() {
        let value =
            serde_json::json!(["workspace_id — string", "route — string", "source — string"]);
        let (preview, full_lines, hidden_lines) = orbit_value_table_preview(Some(&value));

        assert_eq!(preview, "workspace_id — string");
        assert_eq!(hidden_lines, 2);
        assert_eq!(full_lines.unwrap().len(), 3);
    }

    #[test]
    fn orbit_table_single_line_values_still_have_hover_details() {
        let value = serde_json::json!(
            "Fires client-side only after feedback data exists and never fires for missing data."
        );
        let (preview, full_lines, hidden_lines) = orbit_value_table_preview(Some(&value));

        assert_eq!(preview, value.as_str().unwrap());
        assert_eq!(hidden_lines, 0);
        assert_eq!(full_lines.unwrap(), vec![preview]);
    }

    #[test]
    fn orbit_table_columns_share_one_content_independent_width() {
        assert_eq!(orbit_table_min_width(1), ORBIT_TABLE_MIN_WIDTH);
        assert_eq!(orbit_table_min_width(4), 1280.0);
        assert_eq!(orbit_table_min_width(8), 2560.0);
    }

    #[test]
    fn environment_tabs_select_one_file_at_a_time() {
        let sources = vec![
            SubAppServices {
                name: "Project".into(),
                rel_path: ".".into(),
                services: Vec::new(),
                env_files: vec![".env".into(), ".env.local".into()],
            },
            SubAppServices {
                name: "Web".into(),
                rel_path: "web".into(),
                services: Vec::new(),
                env_files: vec![".env".into()],
            },
        ];

        let tabs = services_center_tabs(&sources, ServicesScanKind::Environment);
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].label, "Project · .env");
        assert_eq!(tabs[2].label, "Web · .env");
        assert!(tabs.iter().all(|tab| tab.env_file.is_some()));
    }
}
