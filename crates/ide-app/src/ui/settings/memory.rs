use super::*;

/// Settings → Memory: the facts every agent starts with. Two tabs (Global /
/// Project), inline editor, one-tap enable/pin/delete. Rows live in the
/// shared `memories` table — the same one the `memory_save` MCP tool writes.
impl SettingsView {
    fn reload_memories(&mut self, cx: &mut gpui::Context<Self>) {
        self.memories = ide_core::local_store::LocalStore::open_default()
            .and_then(|store| store.load_all_memories())
            .unwrap_or_default();
        cx.notify();
    }

    fn with_memory_store(
        &mut self,
        cx: &mut gpui::Context<Self>,
        apply: impl FnOnce(&ide_core::local_store::LocalStore) -> anyhow::Result<()>,
    ) {
        match ide_core::local_store::LocalStore::open_default() {
            Ok(store) => {
                if let Err(error) = apply(&store) {
                    self.memory_status = Some(format!("{error:#}"));
                }
            }
            Err(error) => self.memory_status = Some(format!("{error:#}")),
        }
        self.reload_memories(cx);
    }

    fn open_memory_editor(
        &mut self,
        memory: Option<&ide_core::local_store::StoredMemory>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let initial = memory.map(|memory| memory.text.clone()).unwrap_or_default();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("One short, self-contained fact");
            state.set_value(&initial, window, cx);
            state
        });
        self.memory_editor = Some(MemoryEditor {
            id: memory.map(|memory| memory.id),
            input,
            error: None,
        });
        cx.notify();
    }

    fn save_memory_editor(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(editor) = self.memory_editor.as_ref() else {
            return;
        };
        let text = editor.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            if let Some(editor) = self.memory_editor.as_mut() {
                editor.error = Some("Write the fact first.".to_string());
            }
            cx.notify();
            return;
        }
        let editing = editor.id;
        let global = self.memory_scope_global;
        let project = self.memory_project;
        if !global && editing.is_none() && project.is_none() {
            if let Some(editor) = self.memory_editor.as_mut() {
                editor.error = Some("Choose a project for a project memory.".to_string());
            }
            cx.notify();
            return;
        }
        self.memory_editor = None;
        self.with_memory_store(cx, move |store| {
            match editing {
                Some(id) => store.update_memory_text(id, &text)?,
                None => {
                    if global {
                        store.save_memory("global", None, &text, None)?;
                    } else {
                        store.save_memory("project", project, &text, None)?;
                    }
                }
            }
            Ok(())
        });
    }

    pub(super) fn render_memory_section(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let global_tab = self.memory_scope_global;
        let selected_project = self.memory_project;
        let project_label = selected_project
            .and_then(|id| {
                self.projects
                    .iter()
                    .find(|candidate| candidate.id == id)
                    .map(|candidate| candidate.name.clone())
            })
            .unwrap_or_else(|| "Choose project".to_string());
        let rows: Vec<ide_core::local_store::StoredMemory> = self
            .memories
            .iter()
            .filter(|memory| {
                if global_tab {
                    memory.is_global()
                } else {
                    memory.project_id.is_some() && memory.project_id == selected_project
                }
            })
            .cloned()
            .collect();
        let proposals_enabled = self.workspace.read(cx).memory_proposals_enabled;

        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child("Suggest memories from decisions"),
                    )
                    .child(
                        crate::ui::style::ghost_button_compact(
                            "settings-memory-proposals-toggle",
                            if proposals_enabled { "On" } else { "Off" },
                        )
                        .tooltip(
                            "When plan feedback or an answer reveals a preference, Choro proposes a memory — nothing is saved without your OK",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.set_memory_proposals_enabled(!proposals_enabled, cx);
                            });
                            cx.notify();
                        })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        crate::ui::style::segmented_container_quiet(cx)
                            .w_auto()
                            .child(
                                crate::ui::style::segment(
                                    "settings-memory-global",
                                    IconName::CircleUser,
                                    "Global",
                                    global_tab,
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.memory_scope_global = true;
                                    this.memory_editor = None;
                                    cx.notify();
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-memory-project",
                                    IconName::FolderOpen,
                                    "Project",
                                    !global_tab,
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.memory_scope_global = false;
                                    this.memory_editor = None;
                                    cx.notify();
                                })),
                            ),
                    )
                    .when(!global_tab, |row| {
                        let projects = self.projects.clone();
                        let view = cx.entity();
                        row.child(
                            crate::ui::style::dialog_neutral_button(
                                "settings-memory-project-picker",
                                project_label,
                                cx,
                            )
                            .icon(IconName::ChevronDown)
                            .dropdown_menu(move |mut menu, window_ref, _| {
                                for source in projects.clone() {
                                    let id = source.id;
                                    menu = menu.item(
                                        PopupMenuItem::new(source.name.clone())
                                            .checked(selected_project == Some(id))
                                            .on_click(window_ref.listener_for(
                                                &view,
                                                move |this: &mut SettingsView, _, _, cx| {
                                                    this.memory_project = Some(id);
                                                    this.memory_editor = None;
                                                    cx.notify();
                                                },
                                            )),
                                    );
                                }
                                menu
                            }),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::primary_button_compact(
                            "settings-memory-new",
                            "New memory",
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_memory_editor(None, window, cx);
                        })),
                    ),
            )
            .when_some(self.memory_status.clone(), |section, status| {
                section.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(status),
                )
            })
            .when_some(
                self.memory_editor.as_ref().map(|editor| {
                    (editor.input.clone(), editor.error.clone(), editor.id)
                }),
                |section, (input, error, editing)| {
                    section.child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if editing.is_some() {
                                        "Edit memory"
                                    } else {
                                        "New memory"
                                    }),
                            )
                            .child(Input::new(&input))
                            .when_some(error, |editor, error| {
                                editor.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(error),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .justify_end()
                                    .child(
                                        crate::ui::style::secondary_button_compact(
                                            "settings-memory-cancel",
                                            "Cancel",
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.memory_editor = None;
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        crate::ui::style::primary_button_compact(
                                            "settings-memory-save",
                                            "Save",
                                            cx,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_memory_editor(cx);
                                        })),
                                    ),
                            ),
                    )
                },
            )
            .child(if rows.is_empty() {
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child(if global_tab {
                        "Nothing remembered about you yet. Global memories are added explicitly here."
                    } else {
                        "Nothing remembered for this project yet. Say \"remember …\" in any agent chat, or add one here."
                    })
                    .into_any_element()
            } else {
                v_flex()
                    .w_full()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .children(rows.into_iter().enumerate().map(|(index, memory)| {
                        let id = memory.id;
                        let pinned = memory.pinned;
                        let enabled = memory.enabled;
                        h_flex()
                            .w_full()
                            .px_4()
                            .py_2p5()
                            .gap_3()
                            .items_center()
                            .when(index > 0, |row| {
                                row.border_t_1()
                                    .border_color(crate::ui::design::line(cx))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(if enabled {
                                        crate::ui::design::t2(cx)
                                    } else {
                                        crate::ui::design::t4(cx)
                                    })
                                    .child(memory.text.clone()),
                            )
                            .when(pinned, |row| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::amber(cx))
                                        .child("pinned"),
                                )
                            })
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-pin", index),
                                    if pinned { "Unpin" } else { "Pin" },
                                )
                                    .tooltip("Pinned memories are considered first")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.with_memory_store(cx, |store| {
                                            store.set_memory_pinned(id, !pinned)
                                        });
                                    })),
                            )
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-toggle", index),
                                    if enabled { "On" } else { "Off" },
                                )
                                    .tooltip("Disabled memories stay saved but never inject")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.with_memory_store(cx, |store| {
                                            store.set_memory_enabled(id, !enabled)
                                        });
                                    })),
                            )
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-edit", index),
                                    "Edit",
                                )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let memory = this
                                            .memories
                                            .iter()
                                            .find(|memory| memory.id == id)
                                            .cloned();
                                        if let Some(memory) = memory {
                                            this.open_memory_editor(Some(&memory), window, cx);
                                        }
                                    })),
                            )
                            .child(
                                crate::ui::style::danger_button_compact(
                                    ("settings-memory-delete", index),
                                    "Delete",
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.with_memory_store(cx, |store| store.delete_memory(id));
                                })),
                            )
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }
}
