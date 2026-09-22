//! Native system library and review flows. Conversations and specimens reuse Studio.
use super::*;
use gpui_component::Colorize;
use ide_core::studio::*;
use std::collections::BTreeMap;

impl CenterArea {
    pub(crate) fn open_system_library(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        if self.defer_studio_navigation(move |this, cx| this.open_system_library(project, cx), cx) {
            return;
        }
        self.show_penpot_hub(cx);
        self.studio_system_library = Some(project);
        self.set_view_mode(CenterMode::Design, cx);
        self.refresh_studio_catalog(project, cx);
        cx.notify();
    }
    fn system_error(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        if let Some(studio) = self.studio.as_mut() {
            studio.error = Some(format!("{error:#}"));
        } else {
            self.design_hub_error = Some(format!("{error:#}"));
        }
        cx.notify();
    }
    pub(super) fn render_system_library(
        &mut self,
        project: ProjectId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let records = self
            .studio_system_catalog
            .get(&project)
            .cloned()
            .unwrap_or_default();
        let designs = self.studio_designs(project);
        let mut cards = h_flex().flex_wrap().gap_4().items_start();
        for (index, record) in records.iter().enumerate() {
            let id = record.id;
            let used = designs
                .iter()
                .filter(|d| system_id_from_reference(&d.design_system).ok().flatten() == Some(id))
                .count();
            let mut palette = h_flex().w_full().h(px(66.)).gap_1();
            for (_, value) in record
                .draft
                .tokens
                .iter()
                .filter(|(k, _)| k.contains("color"))
                .take(6)
            {
                if let Ok(color) = gpui::Hsla::parse_hex(value) {
                    palette = palette.child(
                        div()
                            .flex_1()
                            .h_full()
                            .rounded(crate::ui::design::r_xs())
                            .bg(color),
                    );
                }
            }
            let status = if record.archived {
                "Archived"
            } else if record.applied.as_ref() == Some(&record.draft) {
                "Applied"
            } else {
                "Draft"
            };
            cards = cards.child(
                style::design_hub_card(("system-card", index), cx)
                    .child(
                        v_flex()
                            .p_4()
                            .gap_3()
                            .w_full()
                            .child(
                                if let Some(path) = self
                                    .studio_catalog_previews
                                    .get(&project)
                                    .and_then(|p| p.get(&id))
                                {
                                    img(path.clone())
                                        .w_full()
                                        .h(px(66.))
                                        .object_fit(ObjectFit::Contain)
                                        .into_any_element()
                                } else {
                                    palette.into_any_element()
                                },
                            )
                            .child(
                                div()
                                    .text_size(px(21.))
                                    .truncate()
                                    .child(record.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(format!(
                                        "{} · {status} · {used} designs",
                                        record.platform
                                    )),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_studio(project, id, cx))),
            );
        }
        v_flex().size_full()
            .child(crate::ui::design::header::workspace_bar(cx)
                .child(style::header_icon_button("system-library-back",IconName::ArrowLeft,cx).tooltip("Back to designs").on_click(cx.listener(|this,_,_,cx| this.show_penpot_hub(cx))))
                .child(crate::ui::design::header::title_col(cx).child(crate::ui::design::header::title("Design systems",cx)))
                .child(crate::ui::design::header::actions().child(style::primary_button_compact("system-create","New system",cx).on_click(cx.listener(move |this,_,window,cx| this.system_name_dialog(project,None,false,window,cx))))))
            .when_some(self.design_hub_error.clone(), |view,error| view.child(div().px_4().py_2().text_color(crate::ui::design::rose(cx)).child(error)))
            .child(div().id("system-library-scroll").flex_1().overflow_y_scroll().p_6()
                .child(div().mb_5().text_color(crate::ui::design::t2(cx)).child("Build a visual language for each part of your product. Choose a system inside each Studio design."))
                .when(records.is_empty(), |view| view.child(v_flex().gap_2().py_6()
                    .child(div().text_size(px(22.)).child("Start with your product"))
                    .child(div().max_w(px(560.)).child("Create a system from existing project styles, describe a new direction, or duplicate a system once you have one. Nothing is applied to your designs until you review it."))))
                .child(cards)).into_any_element()
    }
    pub(super) fn start_system_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(studio) = self
            .studio
            .as_ref()
            .filter(|s| s.design.manifest.system_workspace)
        else {
            return;
        };
        let project = studio.project;
        let path = studio.relative_chat.clone();
        let root = studio.store.project.clone();
        let name = studio.design.manifest.name.clone();
        let record = self.doc_assistants.update(cx, |assistants, cx| {
            assistants.ensure_record(project, path.clone(), cx)
        });
        self.hydrate_doc_assistant_chat_session(&record, &root, cx);
        let agent = Self::doc_assistant_agent_record(&record, root);
        let input = self.agent_chat_input(&agent, "Describe your design system", window, cx);
        if !input.read(cx).value().trim().is_empty() {
            return;
        }
        input.update(cx,|input,cx|input.set_value(format!("Build the {name} design-system draft. Start by reading its platform, sources and requested starting point with studio_context and studio_read. Inspect relevant project UI and styles if this is an existing product; clearly distinguish discovered values from proposed ones. If the project contains multiple apps and the intended source is unclear, ask which app. Build coherent semantic tokens, typography and component recipes, review the generated specimen, and leave changes in the draft for my review."),window,cx));
        self.submit_agent_chat_message_for_surface(
            &agent,
            input,
            false,
            &AgentChatSurface::Document {
                project,
                relative_doc_path: path,
            },
            window,
            cx,
        );
    }
    pub(super) fn studio_recipe_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self
            .studio
            .as_ref()
            .filter(|s| s.design.manifest.system_workspace)
        else {
            return;
        };
        let Some(recipe) = studio.design.system.recipes.get(&name).cloned() else {
            return;
        };
        let expected = studio.design.fingerprint.clone();
        let fields = recipe
            .into_iter()
            .map(|(key, value)| {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
                (key, input)
            })
            .collect::<Vec<_>>();
        let center = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let center = center.clone();
            let fields = fields.clone();
            let name = name.clone();
            let expected = expected.clone();
            let form = v_flex()
                .gap_3()
                .children(fields.iter().map(|(name, input)| {
                    v_flex()
                        .gap_1()
                        .child(div().child(name.clone()))
                        .child(Input::new(input))
                }));
            dialog
                .title(format!("Edit {name} recipe"))
                .child(div().child("Use var(--token-name) to inherit a design token."))
                .child(
                    div()
                        .id("system-recipe-fields")
                        .max_h(px(360.))
                        .overflow_y_scroll()
                        .child(form),
                )
                .footer(move |_, _, _, cx| {
                    let center = center.clone();
                    let fields = fields.clone();
                    let name = name.clone();
                    let expected = expected.clone();
                    vec![
                        style::dialog_neutral_button("recipe-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("recipe-save", "Save draft", cx).on_click(
                            move |_, window, cx| {
                                let properties = fields
                                    .iter()
                                    .map(|(key, input)| {
                                        (key.clone(), input.read(cx).value().to_string())
                                    })
                                    .collect();
                                window.close_dialog(cx);
                                center.update(cx, |this, cx| {
                                    let Some(studio) = this.studio.as_ref() else {
                                        return;
                                    };
                                    if studio.design.fingerprint != expected {
                                        this.system_error(
                                            anyhow::anyhow!(
                                                "System changed; reopen this recipe before saving."
                                            ),
                                            cx,
                                        );
                                        return;
                                    }
                                    let mut system = studio.design.system.clone();
                                    system.recipes.insert(name.clone(), properties);
                                    this.studio_apply_ui(
                                        vec![StudioOperation::SetSystem {
                                            system,
                                            expected_system_revision: studio.design.system.revision,
                                        }],
                                        true,
                                        cx,
                                    );
                                });
                            },
                        ),
                    ]
                })
        });
    }
    fn system_name_dialog(
        &mut self,
        project: ProjectId,
        record: Option<StudioSystemRecord>,
        rename: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    record
                        .as_ref()
                        .map(|r| {
                            if rename {
                                r.name.clone()
                            } else {
                                format!("{} copy", r.name)
                            }
                        })
                        .unwrap_or_default(),
                )
                .placeholder("Mobile, Web app, Admin…")
        });
        let platform = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    record
                        .as_ref()
                        .map(|r| r.platform.clone())
                        .unwrap_or_else(|| "Web".into()),
                )
                .placeholder("Web, iOS, Android, desktop")
        });
        let source = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Optional app folder, or a short design brief")
        });
        let center = cx.entity();
        window.open_dialog(cx,move |dialog,_,_| {
            let center=center.clone(); let name=name.clone(); let platform=platform.clone(); let source=source.clone(); let record=record.clone();
            let name_field=name.clone(); let platform_field=platform.clone(); let source_field=source.clone();
            dialog.title(if rename { "System details" } else if record.is_some() { "Duplicate system" } else { "New design system" })
                .child(v_flex().gap_3().child(div().child("Name")).child(Input::new(&name))
                    .child(div().child("Platform" )).child(Input::new(&platform))
                    .when(!rename && record.is_none(), |form| form.child(div().child("Start from this project or describe a new system")).child(Input::new(&source))))
                .footer(move |_,_,_,cx| {
                    let center=center.clone(); let name=name_field.clone(); let platform=platform_field.clone(); let source=source_field.clone(); let record=record.clone();
                    vec![style::dialog_neutral_button("system-name-cancel","Cancel",cx).on_click(|_,window,cx|window.close_dialog(cx)),
                        style::primary_button_compact("system-name-save",if rename { "Save details" } else { "Create system" },cx).on_click(move |_,window,cx| {
                            let name=name.read(cx).value().to_string(); if name.trim().is_empty(){return;}
                            let platform=platform.read(cx).value().to_string(); let source=source.read(cx).value().to_string(); let record=record.clone();
                            window.close_dialog(cx);
                            center.update(cx,|this,cx| {
                                if rename {
                                    if let Some(record)=record { this.studio_apply_ui(vec![StudioOperation::RenameDesign{name},StudioOperation::SystemDetails{platform,sources:record.sources}],true,cx); }
                                    return;
                                }
                                let Some((_,root))=this.project_by_id(project,cx) else{return};
                                cx.spawn(async move |this,cx| {
                                    let result=cx.background_executor().spawn(async move {
                                        let store=StudioStore::for_project(root)?;
                                        let record=store.create_system(&name,&platform,record.map(|r|r.id))?;
                                        if !source.trim().is_empty() {
                                            let design=store.load(record.id)?;
                                            let scope=scope_for_request(&design,Some(record.id),None);
                                            store.apply(&scope,&StudioTransaction{id:Uuid::new_v4(),scope_id:scope.id,design_id:record.id,expected_revision:design.manifest.revision,expected_fingerprint:design.fingerprint,operations:vec![StudioOperation::SystemDetails{platform,sources:BTreeMap::from([("Requested starting point (not yet verified)".into(),source)])}]})?;
                                        }
                                        Ok::<_,anyhow::Error>(record.id)
                                    }).await;
                                    let _=this.update(cx,|this,cx|match result {Ok(id)=>this.open_studio(project,id,cx),Err(error)=>this.system_error(error,cx)});
                                }).detach();
                            });
                        })]
                })
        });
    }
    pub(super) fn render_system_binding(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let studio = self.studio.as_ref().unwrap();
        let project = studio.project;
        let id = system_id_from_reference(&studio.design.manifest.design_system)
            .ok()
            .flatten();
        let record = self
            .studio_system_catalog
            .get(&project)
            .and_then(|records| records.iter().find(|r| Some(r.id) == id));
        let label = record.map(|r| r.name.clone()).unwrap_or_else(|| {
            if id.is_some() {
                "System unavailable".into()
            } else {
                "No system selected".into()
            }
        });
        let center = cx.entity();
        let host = self.web_host.clone();
        let mut panel = v_flex()
            .p_4()
            .gap_3()
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t2(cx))
                    .child("Design system"),
            )
            .child(
                style::sidebar_selector_button("system-choose", label.clone(), px(284.), cx)
                    .tooltip(label)
                    .dropdown_menu(move |mut menu, window, cx| {
                        web_preview::suspend_for_menu(host.clone(), cx);
                        // Read when opening the menu, so systems created or applied since
                        // this workspace opened are immediately available.
                        let result = center.read(cx).studio.as_ref().map(|s| s.store.systems());
                        let records = match result {
                            Some(Ok(records)) => records,
                            Some(Err(error)) => {
                                return menu.label(format!("Could not load systems: {error}"));
                            }
                            None => return menu,
                        };
                        menu = menu.item(
                            PopupMenuItem::new("No system")
                                .checked(id.is_none())
                                .on_click(window.listener_for(
                                    &center,
                                    |this: &mut Self, _, window, cx| {
                                        this.select_studio_system(None, window, cx);
                                    },
                                )),
                        );
                        for record in records.iter().filter(|r| !r.archived) {
                            let record = record.clone();
                            let name = if record.applied.is_none() {
                                format!("{} (Draft)", record.name)
                            } else {
                                record.name.clone()
                            };
                            menu = menu.item(
                                PopupMenuItem::new(name)
                                    .checked(id == Some(record.id))
                                    .on_click(window.listener_for(
                                        &center,
                                        move |this: &mut Self, _, window, cx| {
                                            this.select_studio_system(
                                                Some(record.clone()),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                            );
                        }
                        menu.separator()
                            .item(PopupMenuItem::new("Manage design systems").on_click(
                                window.listener_for(&center, move |this: &mut Self, _, _, cx| {
                                    this.open_system_library(project, cx);
                                }),
                            ))
                    }),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child(if id.is_some() {
                        "Shared styles, with overrides for this design."
                    } else {
                        "Choose shared colors, type and components for this design."
                    }),
            )
            .when_some(id, |panel, id| {
                panel.child(
                    style::ghost_button_compact("system-open", "Open design system")
                        .icon(IconName::ExternalLink)
                        .justify_start()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.open_studio(project, id, cx)),
                        ),
                )
            });
        if let Some(record) = record {
            let mut swatches = h_flex().gap_1().h(px(32.));
            for value in record
                .applied
                .as_ref()
                .map(|s| &s.tokens)
                .unwrap_or(&record.draft.tokens)
                .iter()
                .filter(|(k, _)| k.contains("color"))
                .take(6)
                .map(|(_, v)| v)
            {
                if let Ok(color) = gpui::Hsla::parse_hex(value) {
                    swatches = swatches.child(
                        div()
                            .h_full()
                            .flex_1()
                            .rounded(crate::ui::design::r_xs())
                            .bg(color),
                    );
                }
            }
            panel = panel.child(swatches);
        }
        panel = panel.child(
            h_flex()
                .mt_2()
                .pt_3()
                .border_t_1()
                .border_color(crate::ui::design::line(cx))
                .justify_between()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child("Local overrides"),
                )
                .child(
                    style::header_icon_button("system-local-add", IconName::Plus, cx)
                        .tooltip("Add local token override")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.studio_override_dialog(None, String::new(), window, cx)
                        })),
                ),
        );
        if studio.design.overrides.tokens.is_empty() {
            panel = panel.child(
                div()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child("Overrides affect only this design."),
            );
        }
        for (index, (name, value)) in studio.design.overrides.tokens.iter().enumerate() {
            let token = name.clone();
            panel = panel.child(
                style::ghost_button_compact(("system-override", index), format!("{name}: {value}"))
                    .justify_start()
                    .tooltip("Edit local token override")
                    .on_click(cx.listener({
                        let value = value.clone();
                        move |this, _, window, cx| {
                            this.studio_override_dialog(
                                Some(token.clone()),
                                value.clone(),
                                window,
                                cx,
                            )
                        }
                    })),
            );
        }
        panel.into_any_element()
    }
    fn select_studio_system(
        &mut self,
        record: Option<StudioSystemRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        if studio.dirty || studio.saving {
            self.system_error(
                anyhow::anyhow!("Finish saving this screen before changing its system."),
                cx,
            );
            return;
        }
        let current = system_id_from_reference(&studio.design.manifest.design_system)
            .ok()
            .flatten();
        if current == record.as_ref().map(|r| r.id) {
            return;
        }
        if let Some(record) = record.as_ref().filter(|r| r.applied.is_none()) {
            let project = studio.project;
            let id = record.id;
            let name = record.name.clone();
            let center = cx.entity();
            window.open_dialog(cx, move |dialog, _, _| {
                let center = center.clone();
                dialog.title(format!("{name} is a draft"))
                    .child(div().child("Open this system, then use Review changes to apply it. It will then be ready to select for this design."))
                    .footer(move |_, _, _, cx| {
                        let center = center.clone();
                        vec![style::dialog_neutral_button("system-draft-cancel", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                            style::primary_button_compact("system-draft-open", "Open draft", cx)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                center.update(cx, |this, cx| this.open_studio(project, id, cx));
                            })]
                    })
            });
            return;
        }
        self.confirm_system_binding(record, window, cx);
    }
    fn confirm_system_binding(
        &mut self,
        record: Option<StudioSystemRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = record.as_ref().map(|r| r.id);
        self.prepare_system_comparison(
            target,
            false,
            window,
            cx,
            move |this, previews, window, cx| {
                this.confirm_system_binding_dialog(record, previews, window, cx)
            },
        );
    }
    fn confirm_system_binding_dialog(
        &mut self,
        record: Option<StudioSystemRecord>,
        previews: Vec<(String, PathBuf, PathBuf)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        let next = record
            .as_ref()
            .and_then(|r| r.applied.clone())
            .unwrap_or_else(empty_system);
        let before = studio.design.system.clone();
        let id = record.as_ref().map(|r| r.id);
        let expected = studio.design.fingerprint.clone();
        let target_fingerprint = id.and_then(|id| studio.store.system_binding_fingerprint(id).ok());
        let name = record.map(|r| r.name).unwrap_or_else(|| "No system".into());
        let changed = system_token_changes(&before, &next);
        let center = cx.entity();
        window.open_dialog(cx,move|dialog,_,_|{
            let center=center.clone();let expected=expected.clone();let target_fingerprint=target_fingerprint.clone();
            dialog.title(format!("Use {name}?"))
                .child(system_comparison_images(&previews))
                .child(div().child("Local overrides and hardcoded screen styles stay in place. Missing token references must be mapped before this change can be applied."))
                .child(div().id("system-switch-changes").max_h(px(300.)).overflow_y_scroll().child(div().child(changed.clone())))
                .footer(move|_,_,_,cx|{let center=center.clone();let expected=expected.clone();let target_fingerprint=target_fingerprint.clone();vec![style::dialog_neutral_button("system-switch-cancel","Cancel",cx).on_click(|_,window,cx|window.close_dialog(cx)),style::primary_button_compact("system-switch-apply","Use system",cx).on_click(move|_,window,cx|{window.close_dialog(cx);center.update(cx,|this,cx|{if this.studio.as_ref().is_some_and(|s|s.design.fingerprint==expected){this.studio_apply_ui(vec![StudioOperation::BindSystem{system_id:id,expected_system_fingerprint:target_fingerprint.clone()}],false,cx)}else{this.system_error(anyhow::anyhow!("Design changed; review the system switch again."),cx)}});})]})
        });
    }
    pub(super) fn system_workspace_actions(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let studio = self.studio.as_ref().unwrap();
        let project = studio.project;
        let id = studio.design.manifest.id;
        let preparing = studio.preparing_comparison;
        let center = cx.entity();
        let host = self.web_host.clone();
        crate::ui::design::header::actions()
            .child(
                style::header_icon_button("system-undo", IconName::Undo, cx)
                    .tooltip("Undo draft edit")
                    .on_click(cx.listener(|this, _, _, cx| this.studio_undo(false, cx))),
            )
            .child(
                style::header_icon_button("system-redo", IconName::Redo, cx)
                    .tooltip("Redo draft edit")
                    .on_click(cx.listener(|this, _, _, cx| this.studio_undo(true, cx))),
            )
            .child(
                style::ghost_button_compact("system-options", "System actions")
                    .dropdown_caret(true)
                    .dropdown_menu(move |menu, window, cx| {
                        web_preview::suspend_for_menu(host.clone(), cx);
                        let record = center
                            .read(cx)
                            .studio
                            .as_ref()
                            .and_then(|s| s.store.system(id).ok());
                        let Some(record) = record else { return menu };
                        let rename = record.clone();
                        let duplicate = record.clone();
                        let archived = record.archived;
                        let revision = record.draft.revision;
                        menu.item(PopupMenuItem::new("System details").on_click(
                            window.listener_for(&center, move |this: &mut Self, _, window, cx| {
                                this.system_name_dialog(
                                    project,
                                    Some(rename.clone()),
                                    true,
                                    window,
                                    cx,
                                )
                            }),
                        ))
                        .item(
                            PopupMenuItem::new("Duplicate").on_click(window.listener_for(
                                &center,
                                move |this: &mut Self, _, window, cx| {
                                    this.system_name_dialog(
                                        project,
                                        Some(duplicate.clone()),
                                        false,
                                        window,
                                        cx,
                                    )
                                },
                            )),
                        )
                        .item(
                            PopupMenuItem::new("Use as project default")
                                .disabled(archived || record.applied.is_none())
                                .on_click(window.listener_for(
                                    &center,
                                    move |this: &mut Self, _, _, cx| {
                                        if let Some(s) = this.studio.as_ref() {
                                            if let Err(error) = s.store.set_default_system(Some(id))
                                            {
                                                this.system_error(error, cx);
                                            }
                                        }
                                    },
                                )),
                        )
                        .item(PopupMenuItem::new("Clear project default").on_click(
                            window.listener_for(&center, move |this: &mut Self, _, _, cx| {
                                if let Some(s) = this.studio.as_ref() {
                                    if let Err(error) = s.store.set_default_system(None) {
                                        this.system_error(error, cx);
                                    }
                                }
                            }),
                        ))
                        .item(
                            PopupMenuItem::new(if archived { "Restore" } else { "Archive" })
                                .on_click(window.listener_for(
                                    &center,
                                    move |this: &mut Self, _, _, cx| {
                                        if let Some(s) = this.studio.as_ref() {
                                            match s.store.archive_system(id, revision, !archived) {
                                                Ok(()) => this.open_system_library(project, cx),
                                                Err(error) => this.system_error(error, cx),
                                            }
                                        }
                                    },
                                )),
                        )
                    }),
            )
            .child(
                style::primary_button_compact("system-review", "Review changes", cx)
                    .disabled(preparing)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.review_system_changes(window, cx)),
                    ),
            )
            .into_any_element()
    }
    fn prepare_system_comparison(
        &mut self,
        target: Option<Uuid>,
        publish: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        ready: impl FnOnce(&mut Self, Vec<(String, PathBuf, PathBuf)>, &mut Window, &mut Context<Self>)
            + 'static,
    ) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        if studio.preparing_comparison {
            return;
        }
        let store = studio.store.clone();
        let id = studio.design.manifest.id;
        let handle = window.window_handle();
        let expected = studio.design.fingerprint.clone();
        let baseline = store.load(id).map(|d| d.fingerprint);
        let target_revision = target
            .map(|id| store.system(id).map(|r| r.draft.revision))
            .transpose();
        let impacts = if publish {
            store.system_impact(id)
        } else {
            Ok(vec![])
        };
        let (baseline, target_revision, impacts) = match (baseline, target_revision, impacts) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            _ => {
                self.system_error(
                    anyhow::anyhow!("Could not read current system; reopen it and try again."),
                    cx,
                );
                return;
            }
        };
        let check_store = store.clone();
        if let Some(s) = self.studio.as_mut() {
            s.notice = Some("Preparing before and after previews…".into());
            s.preparing_comparison = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut paths = vec![];
                    for (name, before, after) in store.system_comparison(id, target, publish)? {
                        let before_path =
                            store.thumbnail_path(&before, before.manifest.screens[0].id);
                        let after_path = store.thumbnail_path(&after, after.manifest.screens[0].id);
                        super::studio_editor::render_thumbnails(store.clone(), before)?;
                        super::studio_editor::render_thumbnails(store.clone(), after)?;
                        paths.push((name, before_path, after_path));
                    }
                    Ok::<_, anyhow::Error>(paths)
                })
                .await;
            let _ = handle.update(cx, |_, window, cx| {
                this.update(cx, |this, cx| {
                    if !this
                        .studio
                        .as_ref()
                        .is_some_and(|s| s.design.manifest.id == id)
                    {
                        return;
                    }
                    if let Some(s) = this.studio.as_mut() {
                        s.notice = None;
                        s.preparing_comparison = false;
                    }
                    let unchanged = check_store
                        .load(id)
                        .is_ok_and(|d| d.fingerprint == baseline)
                        && target
                            .map(|id| check_store.system(id).map(|r| r.draft.revision))
                            .transpose()
                            .ok()
                            == Some(target_revision)
                        && (!publish
                            || check_store.system_impact(id).ok().as_ref() == Some(&impacts))
                        && this
                            .studio
                            .as_ref()
                            .is_some_and(|s| s.design.fingerprint == expected);
                    if !unchanged {
                        this.system_error(
                            anyhow::anyhow!(
                                "System or designs changed while preparing previews; review again."
                            ),
                            cx,
                        );
                        return;
                    }
                    match result {
                        Ok(previews) => {
                            if let Some(s) = this.studio.as_mut() {
                                s.error = None;
                            }
                            ready(this, previews, window, cx);
                        }
                        Err(error) => this.system_error(error, cx),
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    }
    fn publish_system_review(
        &mut self,
        store: StudioStore,
        project: ProjectId,
        id: Uuid,
        revision: u64,
        fingerprint: String,
        impact: Vec<StudioSystemImpact>,
        cx: &mut Context<Self>,
    ) {
        if let Some(s) = self.studio.as_mut() {
            s.notice = Some("Applying system…".into());
            s.preparing_comparison = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result =
                cx.background_executor()
                    .spawn(async move {
                        store.publish_system(id, revision, &fingerprint, &impact)?;
                        let mut failures = vec![];
                        for design in impact {
                            if let Err(error) = store.load(design.id).and_then(|d| {
                                super::studio_editor::render_thumbnails(store.clone(), d)
                            }) {
                                failures.push(format!("{}: {error:#}", design.name));
                            }
                        }
                        Ok::<_, anyhow::Error>(failures)
                    })
                    .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(s) = this.studio.as_mut().filter(|s| s.design.manifest.id == id) {
                    s.preparing_comparison = false;
                    match result {
                        Ok(failures) => {
                            s.error = if failures.is_empty() {
                                None
                            } else {
                                Some(format!(
                                    "System applied; some previews need a refresh: {}",
                                    failures.join("; ")
                                ))
                            };
                            s.notice = Some("System applied to linked designs.".into());
                        }
                        Err(error) => {
                            s.notice = None;
                            s.error = Some(format!("{error:#}"));
                        }
                    }
                }
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn review_system_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.studio.as_ref().map(|s| s.design.manifest.id) else {
            return;
        };
        self.prepare_system_comparison(
            Some(id),
            true,
            window,
            cx,
            move |this, previews, window, cx| {
                this.review_system_changes_dialog(previews, window, cx)
            },
        );
    }
    fn review_system_changes_dialog(
        &mut self,
        previews: Vec<(String, PathBuf, PathBuf)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(studio) = self.studio.as_ref() else {
            return;
        };
        let store = studio.store.clone();
        let id = studio.design.manifest.id;
        let project = studio.project;
        let result = (|| -> anyhow::Result<_> {
            Ok((
                store.system(id)?,
                store.system_impact(id)?,
                store.load(id)?.fingerprint,
            ))
        })();
        let (record, impact, fingerprint) = match result {
            Ok(value) => value,
            Err(error) => {
                self.system_error(error, cx);
                return;
            }
        };
        let changes = system_token_changes(
            &record.applied.clone().unwrap_or_else(empty_system),
            &record.draft,
        );
        let recipes = format!(
            "{} component recipes in this draft",
            record.draft.recipes.len()
        );
        let linked = if impact.is_empty() {
            "No designs use this system yet.".into()
        } else {
            format!(
                "Updates: {}",
                impact
                    .iter()
                    .map(|d| d.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let center = cx.entity();
        window.open_dialog(cx,move|dialog,_,_|{
            let center=center.clone();let store=store.clone();let impact=impact.clone();let record=record.clone();let fingerprint=fingerprint.clone();
            dialog.title("Review system changes").child(system_comparison_images(&previews)).child(div().child(linked.clone()))
                .child(div().child("Only linked designs inherit these changes. Their local overrides remain in place."))
                .child(div().id("system-review-scroll").max_h(px(320.)).overflow_y_scroll().child(v_flex().gap_3().child(div().child(changes.clone())).child(div().child(recipes.clone()))))
                .footer(move|_,_,_,cx|{let center=center.clone();let store=store.clone();let impact=impact.clone();let fingerprint=fingerprint.clone();vec![style::dialog_neutral_button("system-review-cancel","Keep editing",cx).on_click(|_,window,cx|window.close_dialog(cx)),style::primary_button_compact("system-review-apply","Apply system",cx).disabled(record.archived||record.draft.tokens.is_empty()).on_click(move|_,window,cx|{
                    window.close_dialog(cx);center.update(cx,|this,cx|this.publish_system_review(store.clone(),project,id,record.draft.revision,fingerprint.clone(),impact.clone(),cx));
                })]})
        });
    }
}
fn system_token_changes(before: &StudioDesignSystem, after: &StudioDesignSystem) -> String {
    let names = before
        .tokens
        .keys()
        .chain(after.tokens.keys())
        .collect::<std::collections::BTreeSet<_>>();
    let mut lines = names
        .into_iter()
        .filter(|name| before.tokens.get(*name) != after.tokens.get(*name))
        .map(|name| {
            format!(
                "{name}: {} → {}",
                before
                    .tokens
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("Not set"),
                after
                    .tokens
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("Removed")
            )
        })
        .collect::<Vec<_>>();
    if before.font_faces != after.font_faces {
        lines.push("Bundled font faces changed.".into());
    }
    if before.recipes != after.recipes {
        lines.push("Component styles changed.".into());
    }
    if lines.is_empty() {
        "No style changes.".into()
    } else {
        lines.join("\n")
    }
}

fn system_comparison_images(previews: &[(String, PathBuf, PathBuf)]) -> gpui::AnyElement {
    v_flex()
        .gap_2()
        .children(previews.iter().map(|(name, before, after)| {
            v_flex().gap_1().child(div().child(name.clone())).child(
                h_flex()
                    .gap_2()
                    .children([("Before", before), ("After", after)].into_iter().map(
                        |(label, path)| {
                            v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .child(div().child(label))
                                .child(
                                    img(path.clone())
                                        .w_full()
                                        .h(px(150.))
                                        .object_fit(ObjectFit::Contain),
                                )
                        },
                    )),
            )
        }))
        .into_any_element()
}
