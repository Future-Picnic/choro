use super::*;

use ide_core::local_store::OrbitBuiltin;

impl SettingsView {
    pub(super) fn render_orbit_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.orbit_editor.is_some() {
            return self.render_orbit_editor(window, cx);
        }

        let orbit = self.orbit.read(cx);
        let active = orbit
            .modules()
            .iter()
            .filter(|module| !module.archived)
            .collect::<Vec<_>>();
        let archived = orbit
            .modules()
            .iter()
            .filter(|module| module.archived)
            .collect::<Vec<_>>();
        let settings = cx.entity().clone();
        let loading = orbit.loading();
        let error = orbit.error().map(str::to_string);

        v_flex()
            .w_full()
            .gap_6()
            .when(loading, |page| {
                page.child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Spinner::new().xsmall())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Loading Orbit settings…"),
                        ),
                )
            })
            .when_some(error, |page, error| {
                page.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(crate::ui::design::r_md())
                        .bg(crate::ui::design::rose(cx).opacity(0.12))
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(orbit_section_heading(
                        "Built-ins",
                        "Every project can show or hide these, but their behavior and structure are fixed.",
                        cx,
                    ))
                    .child(orbit_settings_row(
                        IconName::File,
                        OrbitBuiltin::Environment.label(),
                        OrbitBuiltin::Environment.description(),
                        Some("Built-in"),
                        None,
                        cx,
                    ))
                    .child(orbit_settings_row(
                        IconName::Network,
                        OrbitBuiltin::Integrations.label(),
                        OrbitBuiltin::Integrations.description(),
                        Some("Built-in"),
                        None,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_4()
                            .child(
                                div()
                                    .flex_1()
                                    .child(orbit_section_heading(
                                        "Your modules",
                                        "Reusable structured views that become available to projects and agents only after you add them to Orbit.",
                                        cx,
                                    )),
                            )
                            .child(
                                crate::ui::style::primary_button_compact(
                                    "orbit-new-module",
                                    "Add new module",
                                    cx,
                                )
                                .dropdown_menu(move |menu, _window, _| {
                                    let analytics_settings = settings.clone();
                                    let blank_settings = settings.clone();
                                    menu.item(PopupMenuItem::new("Analytics template").on_click(
                                        move |_, window, cx| {
                                            analytics_settings.update(cx, |settings, cx| {
                                                settings.open_orbit_editor(
                                                    analytics_orbit_template(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        },
                                    ))
                                    .item(PopupMenuItem::new("Blank module").on_click(
                                        move |_, window, cx| {
                                            blank_settings.update(cx, |settings, cx| {
                                                settings.open_orbit_editor(
                                                    blank_orbit_module(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        },
                                    ))
                                }),
                            ),
                    )
                    .when(active.is_empty() && !loading, |section| {
                        section.child(
                            div()
                                .w_full()
                                .p_5()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No custom modules yet. Start from Analytics or create a blank one."),
                        )
                    })
                    .children(active.into_iter().map(|module| {
                        let edit_settings = cx.entity().clone();
                        let archive_settings = cx.entity().clone();
                        let archive_name = module.name.clone();
                        let archive_id = module.id;
                        let edit_module = module.clone();
                        orbit_settings_row(
                            IconName::Network,
                            &module.name,
                            &module.description,
                            Some("Grouped table"),
                            Some(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        crate::ui::style::dialog_neutral_button(
                                            ("orbit-edit-module", module.id.as_u128() as u64),
                                            "Edit",
                                            cx,
                                        )
                                        .on_click(move |_, window, cx| {
                                            edit_settings.update(cx, |settings, cx| {
                                                settings.open_orbit_editor(
                                                    edit_module.clone(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }),
                                    )
                                    .child(
                                        crate::ui::style::dialog_neutral_button(
                                            ("orbit-archive-module", module.id.as_u128() as u64),
                                            "Archive",
                                            cx,
                                        )
                                        .on_click(move |_, window, cx| {
                                            let archive_settings = archive_settings.clone();
                                            let archive_name = archive_name.clone();
                                            crate::ui::confirm::ConfirmDialog::new(
                                                "Archive Orbit module?",
                                                format!(
                                                    "“{archive_name}” will disappear from every project, but its records will be preserved."
                                                ),
                                            )
                                            .confirm_label("Archive")
                                            .confirm_id("orbit-archive-module-confirm")
                                            .on_confirm(move |_, cx| {
                                                archive_settings.update(cx, |settings, cx| {
                                                    settings.archive_orbit_module(
                                                        archive_id,
                                                        true,
                                                        cx,
                                                    );
                                                });
                                            })
                                            .open(window, cx);
                                        }),
                                    )
                                    .into_any_element(),
                            ),
                            cx,
                        )
                    })),
            )
            .when(!archived.is_empty(), |page| {
                page.child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(orbit_section_heading(
                            "Archived",
                            "Archived definitions and every project's records remain available for restoration.",
                            cx,
                        ))
                        .children(archived.into_iter().map(|module| {
                            let settings = cx.entity().clone();
                            let module_id = module.id;
                            orbit_settings_row(
                                IconName::Network,
                                &module.name,
                                &module.description,
                                Some("Archived"),
                                Some(
                                    crate::ui::style::dialog_neutral_button(
                                        ("orbit-restore-module", module.id.as_u128() as u64),
                                        "Restore",
                                        cx,
                                    )
                                    .on_click(move |_, _, cx| {
                                        settings.update(cx, |settings, cx| {
                                            settings.archive_orbit_module(module_id, false, cx);
                                        });
                                    })
                                    .into_any_element(),
                                ),
                                cx,
                            )
                        })),
                )
            })
            .into_any_element()
    }

    fn open_orbit_editor(
        &mut self,
        module: OrbitModuleDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut input =
            |value: String, placeholder: &'static str, multiline: bool, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(placeholder)
                        .multi_line(multiline)
                })
            };
        let fields = module
            .fields
            .iter()
            .filter(|field| !field.archived)
            .map(|field| OrbitFieldEditor {
                id: field.id,
                key: field.key.clone(),
                label: input(field.label.clone(), "Field name", false, cx),
                kind: field.kind,
                primary: field.primary,
            })
            .collect();
        self.orbit_editor = Some(OrbitModuleEditor {
            request_id: Uuid::new_v4(),
            name: input(module.name.clone(), "Module name", false, cx),
            description: input(
                module.description.clone(),
                "What this module contains",
                true,
                cx,
            ),
            section: input(
                module.section_label.clone().unwrap_or_default(),
                "Optional, for example Area",
                false,
                cx,
            ),
            agent_job: input(
                module.agent_job.clone(),
                "Tell the agent how to work with this module",
                true,
                cx,
            ),
            module,
            fields,
            error: None,
        });
        cx.notify();
    }

    fn render_orbit_editor(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let editor = self.orbit_editor.as_ref().unwrap();
        let is_new = editor.module.revision == 0;
        let name = editor.name.clone();
        let description = editor.description.clone();
        let section = editor.section.clone();
        let agent_job = editor.agent_job.clone();
        let field_rows = editor
            .fields
            .iter()
            .map(|field| (field.id, field.label.clone(), field.kind, field.primary))
            .collect::<Vec<_>>();
        let field_count = field_rows.len();
        let error = editor.error.clone();
        let async_error = self.orbit.read(cx).error().map(str::to_string);

        v_flex()
            .w_full()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .child(
                        crate::ui::style::header_icon_button(
                            "orbit-editor-back",
                            IconName::ArrowLeft,
                            cx,
                        )
                        .tooltip("Back to Orbit settings")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.orbit_editor = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(if is_new { "New Orbit module" } else { "Edit Orbit module" }),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::primary_button_compact(
                            "orbit-save-module",
                            "Save module",
                            cx,
                        )
                        .disabled(self.orbit.read(cx).saving())
                        .on_click(cx.listener(|this, _, _, cx| this.save_orbit_editor(cx))),
                    ),
            )
            .when_some(error, |form, error| {
                form.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(crate::ui::design::r_md())
                        .bg(crate::ui::design::rose(cx).opacity(0.12))
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when_some(async_error, |form, error| {
                form.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(crate::ui::design::r_md())
                        .bg(crate::ui::design::rose(cx).opacity(0.12))
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(orbit_form_field("Name", Input::new(&name), None, cx))
            .child(orbit_form_field(
                "Description",
                Input::new(&description),
                Some(96.),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_5()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("View"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("A compact table, optionally grouped into sections."),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded(crate::ui::design::r_md())
                            .bg(crate::ui::design::surface_2(cx))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child("Grouped table"),
                    ),
            )
            .child(orbit_form_field(
                "Section field",
                Input::new(&section),
                None,
                cx,
            ))
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .child(orbit_section_heading(
                                        "Fields",
                                        "The first field is the required record identity. List fields use one item per line when edited manually.",
                                        cx,
                                    )),
                            )
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    "orbit-add-field",
                                    "Add field",
                                    cx,
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.add_orbit_field(window, cx);
                                })),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .overflow_hidden()
                            .children(field_rows.into_iter().enumerate().map(
                                |(index, (field_id, label, kind, primary))| {
                                    let type_settings = cx.entity().clone();
                                    let remove_settings = cx.entity().clone();
                                    let move_up_settings = cx.entity().clone();
                                    let move_down_settings = cx.entity().clone();
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .gap_3()
                                        .p_3()
                                        .when(index > 0, |row| {
                                            row.border_t_1()
                                                .border_color(crate::ui::design::line(cx))
                                        })
                                        .child(div().flex_1().child(Input::new(&label)))
                                        .when(primary, |row| {
                                            row.child(
                                                div()
                                                    .text_size(crate::ui::design::text_label())
                                                    .text_color(crate::ui::design::t4(cx))
                                                    .child("Primary"),
                                            )
                                        })
                                        .child(
                                            crate::ui::style::dialog_neutral_button(
                                                ("orbit-field-type", field_id.as_u128() as u64),
                                                kind.label(),
                                                cx,
                                            )
                                            .disabled(primary)
                                            .dropdown_menu(move |mut menu, _, _| {
                                                for option in [
                                                    OrbitFieldKind::ShortText,
                                                    OrbitFieldKind::LongText,
                                                    OrbitFieldKind::List,
                                                ] {
                                                    let settings = type_settings.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(option.label())
                                                            .checked(option == kind)
                                                            .on_click(move |_, _, cx| {
                                                                settings.update(cx, |settings, cx| {
                                                                    settings.set_orbit_field_kind(
                                                                        field_id,
                                                                        option,
                                                                        cx,
                                                                    );
                                                                });
                                                            }),
                                                    );
                                                }
                                                menu
                                            }),
                                        )
                                        .when(!primary, |row| {
                                            row.child(
                                                crate::ui::style::settings_inline_icon_button(
                                                    ("orbit-move-field-up", field_id.as_u128() as u64),
                                                    IconName::ArrowUp,
                                                )
                                                .tooltip("Move field up")
                                                .disabled(index <= 1)
                                                .on_click(move |_, _, cx| {
                                                    move_up_settings.update(cx, |settings, cx| {
                                                        settings.move_orbit_field(field_id, -1, cx);
                                                    });
                                                }),
                                            )
                                            .child(
                                                crate::ui::style::settings_inline_icon_button(
                                                    ("orbit-move-field-down", field_id.as_u128() as u64),
                                                    IconName::ArrowDown,
                                                )
                                                .tooltip("Move field down")
                                                .disabled(index + 1 >= field_count)
                                                .on_click(move |_, _, cx| {
                                                    move_down_settings.update(cx, |settings, cx| {
                                                        settings.move_orbit_field(field_id, 1, cx);
                                                    });
                                                }),
                                            )
                                        })
                                        .when(!primary, |row| {
                                            row.child(
                                                crate::ui::style::settings_inline_icon_button(
                                                    ("orbit-remove-field", field_id.as_u128() as u64),
                                                    IconName::Close,
                                                )
                                                .tooltip("Remove field")
                                                .on_click(move |_, _, cx| {
                                                    remove_settings.update(cx, |settings, cx| {
                                                        settings.remove_orbit_field(field_id, cx);
                                                    });
                                                }),
                                            )
                                        })
                                },
                            )),
                    ),
            )
            .child(orbit_form_field(
                "Agent job",
                Input::new(&agent_job),
                Some(320.),
                cx,
            ))
            .into_any_element()
    }

    fn add_orbit_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.orbit_editor.as_mut() else {
            return;
        };
        editor.fields.push(OrbitFieldEditor {
            id: Uuid::new_v4(),
            key: String::new(),
            label: cx.new(|cx| InputState::new(window, cx).placeholder("Field name")),
            kind: OrbitFieldKind::ShortText,
            primary: false,
        });
        cx.notify();
    }

    fn set_orbit_field_kind(
        &mut self,
        field_id: Uuid,
        kind: OrbitFieldKind,
        cx: &mut Context<Self>,
    ) {
        if let Some(field) = self
            .orbit_editor
            .as_mut()
            .and_then(|editor| editor.fields.iter_mut().find(|field| field.id == field_id))
        {
            field.kind = kind;
            cx.notify();
        }
    }

    fn remove_orbit_field(&mut self, field_id: Uuid, cx: &mut Context<Self>) {
        if let Some(editor) = self.orbit_editor.as_mut() {
            editor
                .fields
                .retain(|field| field.id != field_id || field.primary);
            cx.notify();
        }
    }

    fn move_orbit_field(&mut self, field_id: Uuid, delta: isize, cx: &mut Context<Self>) {
        let Some(editor) = self.orbit_editor.as_mut() else {
            return;
        };
        let Some(index) = editor.fields.iter().position(|field| field.id == field_id) else {
            return;
        };
        let Some(target) = orbit_field_move_target(
            index,
            editor.fields.len(),
            editor.fields[index].primary,
            delta,
        ) else {
            return;
        };
        editor.fields.swap(index, target);
        cx.notify();
    }

    fn save_orbit_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.orbit_editor.as_mut() else {
            return;
        };
        let mut module = editor.module.clone();
        module.name = editor.name.read(cx).value().to_string();
        module.description = editor.description.read(cx).value().to_string();
        module.agent_job = editor.agent_job.read(cx).value().to_string();
        let section_label = editor.section.read(cx).value().trim().to_string();
        if section_label.is_empty() {
            module.section_key = None;
            module.section_label = None;
        } else {
            module.section_key = Some(
                module
                    .section_key
                    .clone()
                    .filter(|key| !key.is_empty())
                    .unwrap_or_else(|| normalize_orbit_field_key(&section_label)),
            );
            module.section_label = Some(section_label);
        }
        module.fields = editor
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let label = field.label.read(cx).value().trim().to_string();
                OrbitFieldDefinition {
                    id: field.id,
                    key: if field.key.is_empty() {
                        normalize_orbit_field_key(&label)
                    } else {
                        field.key.clone()
                    },
                    label,
                    kind: if field.primary {
                        OrbitFieldKind::ShortText
                    } else {
                        field.kind
                    },
                    primary: field.primary,
                    sort_order: index as i64,
                    archived: false,
                }
            })
            .collect();
        if let Err(error) = validate_orbit_module(&module) {
            editor.error = Some(error.to_string());
            cx.notify();
            return;
        }
        let request_id = editor.request_id;
        self.orbit
            .update(cx, |orbit, cx| orbit.save_module(module, request_id, cx));
        cx.notify();
    }

    fn archive_orbit_module(&mut self, module_id: Uuid, archived: bool, cx: &mut Context<Self>) {
        self.orbit
            .update(cx, |orbit, cx| orbit.set_archived(module_id, archived, cx));
    }
}

fn orbit_section_heading(title: &str, description: &str, cx: &App) -> gpui::AnyElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_size(crate::ui::design::text_body())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t1(cx))
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .max_w(px(620.))
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child(SharedString::from(description.to_string())),
        )
        .into_any_element()
}

fn orbit_settings_row(
    icon: IconName,
    title: &str,
    description: &str,
    badge: Option<&str>,
    actions: Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .min_h(px(72.))
        .items_center()
        .gap_4()
        .px_4()
        .py_3()
        .rounded(crate::ui::design::r_lg())
        .border_1()
        .border_color(crate::ui::design::line_2(cx))
        .bg(crate::ui::design::surface(cx).opacity(0.55))
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(crate::ui::design::r_md())
                .bg(crate::ui::design::surface_2(cx))
                .child(
                    Icon::new(icon)
                        .size(crate::ui::design::icon_md())
                        .text_color(crate::ui::design::t3(cx)),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child(SharedString::from(title.to_string())),
                        )
                        .when_some(badge, |row, badge| {
                            row.child(
                                div()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded(crate::ui::design::r_sm())
                                    .bg(crate::ui::design::surface_2(cx))
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(SharedString::from(badge.to_string())),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(SharedString::from(description.to_string())),
                ),
        )
        .when_some(actions, |row, actions| row.child(actions))
        .into_any_element()
}

fn orbit_form_field(
    label: &str,
    input: gpui_component::input::Input,
    height: Option<f32>,
    cx: &App,
) -> gpui::AnyElement {
    let input = match height {
        Some(height) => input.w_full().h(px(height)).flex_none(),
        None => input.w_full(),
    };
    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_size(crate::ui::design::text_body())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t1(cx))
                .child(SharedString::from(label.to_string())),
        )
        .child(div().w_full().child(input))
        .into_any_element()
}

fn orbit_field_move_target(
    index: usize,
    field_count: usize,
    primary: bool,
    delta: isize,
) -> Option<usize> {
    if primary {
        return None;
    }
    let target = index.saturating_add_signed(delta);
    (target > 0 && target < field_count).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_reordering_keeps_the_primary_field_first() {
        assert_eq!(orbit_field_move_target(0, 4, true, 1), None);
        assert_eq!(orbit_field_move_target(1, 4, false, -1), None);
        assert_eq!(orbit_field_move_target(2, 4, false, -1), Some(1));
        assert_eq!(orbit_field_move_target(2, 4, false, 1), Some(3));
        assert_eq!(orbit_field_move_target(3, 4, false, 1), None);
    }
}
