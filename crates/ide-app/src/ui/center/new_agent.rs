use super::agent_chat_attachments::AttachmentRemoval;
use super::*;

impl CenterArea {
    fn render_new_agent_mention_prefix(
        &self,
        mentions: &[ComposerMentionToken],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .min_w(px(0.))
            .gap_1()
            .items_center()
            .overflow_hidden()
            .children(
                mentions.iter().enumerate().map(|(index, mention)| {
                    self.render_new_agent_mention_token(index, mention, cx)
                }),
            )
            .into_any_element()
    }

    fn render_new_agent_mention_token(
        &self,
        index: usize,
        mention: &ComposerMentionToken,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (icon, color) = match mention.kind {
            ComposerMentionKind::Doc => {
                (crate::ui::design::docs_icon(), crate::ui::design::amber(cx))
            }
            ComposerMentionKind::File => (IconName::File, crate::ui::design::sage(cx)),
            ComposerMentionKind::Folder => (IconName::FolderOpen, crate::ui::design::sage(cx)),
            ComposerMentionKind::PenpotDesign => (
                crate::ui::design::design_icon(),
                crate::ui::design::accent(cx),
            ),
            ComposerMentionKind::Project => (IconName::FolderOpen, crate::ui::design::rose(cx)),
        };
        let label = if mention.kind == ComposerMentionKind::Project {
            format!("##{}", mention.chip_label())
        } else {
            mention.chip_label().to_string()
        };
        h_flex()
            .id(("new-agent-selected-mention-token", index))
            .min_w(px(0.))
            .items_center()
            .gap_1()
            .h(crate::ui::design::control_h_xs())
            .px_1p5()
            .py_0p5()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(color.opacity(0.3))
            .bg(color.opacity(0.12))
            .cursor_pointer()
            .hover(move |chip| chip.bg(color.opacity(0.18)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(composer) = this.new_agent_composer.as_mut() {
                    if index < composer.selected_mentions.len() {
                        let removed = composer.selected_mentions.remove(index);
                        if removed.kind == ComposerMentionKind::Doc {
                            composer
                                .linked_docs
                                .retain(|path| path.to_string_lossy() != removed.path_label);
                        }
                        composer.error = None;
                    }
                }
                cx.notify();
            }))
            .child(
                gpui_component::Icon::new(icon)
                    .size(crate::ui::design::icon_md())
                    .text_color(color),
            )
            .child(
                div()
                    .max_w(px(140.))
                    .truncate()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(label),
            )
            .child(
                gpui_component::Icon::new(IconName::Close)
                    .size(crate::ui::design::icon_sm())
                    .text_color(color.opacity(0.72)),
            )
            .into_any_element()
    }

    pub(super) fn render_new_agent_control_rail(
        &self,
        composer_view: Entity<CenterArea>,
        project: ProjectId,
        provider: AgentKind,
        runtime: AgentRuntimeKind,
        interaction_mode: AgentInteractionMode,
        model: AgentModel,
        effort: AgentEffort,
        access_mode: AgentAccessMode,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (attachment_paste_pending, agent_starting) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| (composer.attachment_pastes_pending > 0, composer.starting))
            .unwrap_or_default();
        let send_pending = attachment_paste_pending || agent_starting;
        let send_tooltip = if agent_starting {
            "Starting agent…"
        } else if attachment_paste_pending {
            "Wait for the image to finish attaching"
        } else {
            "Start agent"
        };
        let interaction_mode_label = if interaction_mode == AgentInteractionMode::Plan {
            "Plan"
        } else {
            "Build"
        };
        let (model_short_label, model_label, open_code_efforts) = self
            .new_agent_composer
            .as_ref()
            .filter(|composer| composer.provider == AgentKind::OpenCode)
            .map(|composer| {
                let label = composer
                    .external_model_label
                    .clone()
                    .unwrap_or_else(|| "Choose model".to_string());
                let efforts = AgentEffort::supported_variants(&composer.external_model_variants);
                (label.clone(), label, efforts)
            })
            .unwrap_or_else(|| {
                (
                    model.short_label().to_string(),
                    model.label().to_string(),
                    model.efforts(),
                )
            });
        // Labels always show — the rail no longer collapses to icons.
        let controls = h_flex()
            .flex_1()
            .min_w(px(0.))
            // Small left inset so the first control's icon clears the
            // `overflow_hidden` clip edge instead of being shaved.
            .pl_0p5()
            .gap_1()
            .items_center()
            .overflow_hidden()
            // Provider and model are one decision: keep the richer provider rail
            // and model detail list, but anchor that picker to this chip instead
            // of positioning it against the whole page.
            .child(
                gpui_component::popover::Popover::new("composer-model-popover")
                    .anchor(gpui::Corner::BottomLeft)
                    .appearance(false)
                    .open(self.composer_model_expanded)
                    .on_open_change({
                        let composer_view = composer_view.clone();
                        move |open, _, cx| {
                            composer_view.update(cx, |this, cx| {
                                this.composer_model_expanded = *open;
                                if *open {
                                    this.composer_model_provider = None;
                                    this.refresh_open_code_models(false, cx);
                                }
                                cx.notify();
                            });
                        }
                    })
                    .trigger(
                        crate::ui::style::composer_chip(
                            "composer-model",
                            model_short_label,
                            Some(
                                provider_brand_icon(provider)
                                    .size(crate::ui::design::icon_sm())
                                    .into_any_element(),
                            ),
                            cx,
                        )
                        .tooltip(model_label),
                    )
                    .content({
                        let composer_view = composer_view.clone();
                        move |_, _, cx| {
                            composer_view.update(cx, |this, cx| {
                                this.render_composer_model_picker(model, cx)
                                    .into_any_element()
                            })
                        }
                    }),
            )
            .when(!open_code_efforts.is_empty(), |controls| {
                controls
                    .child(crate::ui::style::composer_control_divider(cx))
                    .child(
                        crate::ui::style::composer_chip(
                            "composer-effort",
                            effort.label(),
                            Some(composer_effort_icon(effort, crate::ui::design::t3(cx))),
                            cx,
                        )
                        .tooltip("Reasoning effort")
                        .dropdown_menu({
                            let composer_view = composer_view.clone();
                            move |mut menu, window, _| {
                                for candidate in open_code_efforts.clone() {
                                    menu = menu.item(
                                        PopupMenuItem::new(candidate.menu_label())
                                            .checked(effort == candidate)
                                            .on_click(window.listener_for(
                                                &composer_view,
                                                move |this: &mut Self, _, _, cx| {
                                                    if let Some(composer) =
                                                        this.new_agent_composer.as_mut()
                                                    {
                                                        composer.effort = candidate;
                                                        composer.error = None;
                                                    }
                                                    cx.notify();
                                                },
                                            )),
                                    );
                                }
                                menu
                            }
                        }),
                    )
            })
            .child(crate::ui::style::composer_control_divider(cx))
            .child(
                crate::ui::style::composer_chip(
                    "composer-access-mode",
                    access_mode.label_for(provider),
                    Some(composer_access_icon(access_mode, crate::ui::design::t3(cx))),
                    cx,
                )
                .tooltip(access_mode.label_for(provider))
                .dropdown_menu({
                    let composer_view = composer_view.clone();
                    move |mut menu, window, _| {
                        for candidate in AgentAccessMode::ALL {
                            let label = candidate.label_for(provider);
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(access_mode == candidate)
                                    .on_click(window.listener_for(
                                        &composer_view,
                                        move |this: &mut Self, _, _, cx| {
                                            if let Some(composer) = this.new_agent_composer.as_mut()
                                            {
                                                composer.access_mode = candidate;
                                                composer.error = None;
                                            }
                                            cx.notify();
                                        },
                                    )),
                            );
                        }
                        menu
                    }
                }),
            )
            // Plan is not shown up front: Shift+Tab turns it on and only then does
            // the chip appear — exactly like the agent-chat composer.
            .when(interaction_mode == AgentInteractionMode::Plan, |controls| {
                controls
                    .child(crate::ui::style::composer_control_divider(cx))
                    .child(
                        div()
                            .relative()
                            .child(
                                crate::ui::style::composer_toggle_chip(
                                    "composer-initial-mode",
                                    interaction_mode_label,
                                    Some(composer_mode_icon(true, crate::ui::design::accent(cx))),
                                    true,
                                    cx,
                                )
                                .tooltip("Plan mode is active. Click to return to build mode")
                                .on_click(cx.listener(
                                    move |this: &mut Self, _, _, cx| {
                                        if crate::ui::onboarding::locks_onboarding_plan_mode(cx) {
                                            return;
                                        }
                                        if let Some(composer) = this.new_agent_composer.as_mut() {
                                            composer.interaction_mode =
                                                AgentInteractionMode::Default;
                                            composer.error = None;
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(crate::ui::onboarding::target_marker(
                                crate::ui::onboarding::SpotlightTarget::ComposerPlan,
                                cx,
                            )),
                    )
            });

        let memory_armed = self
            .new_agent_composer
            .as_ref()
            .is_some_and(|composer| memory_save_intent(&composer.prompt.read(cx).value()));

        h_flex()
            .w_full()
            .min_w(px(0.))
            .relative()
            .gap_1()
            .items_center()
            .text_size(crate::ui::design::text_ui())
            // No row-level text colour: it would cascade over the chips' own
            // foreground and mute every label. The agent-chat composer's rail
            // sets none either — the chips own their colour.
            .child(controls)
            .when(memory_armed, |row| {
                row.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .px_1p5()
                        .py_0p5()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::sage(cx).opacity(0.12))
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::sage(cx))
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::Brain,
                            crate::ui::design::sage(cx),
                            crate::ui::design::icon_sm(),
                        ))
                        .child("Memory"),
                )
            })
            // Runtime lives beside send, as a straight toggle: it decides what
            // kind of agent you're starting, not something you tweak per message.
            .child(
                crate::ui::style::composer_toggle_chip(
                    "composer-runtime-toggle",
                    runtime.label(),
                    Some(
                        gpui_component::Icon::new(if runtime == AgentRuntimeKind::Chat {
                            IconName::Bot
                        } else {
                            IconName::SquareTerminal
                        })
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx))
                        .into_any_element(),
                    ),
                    false,
                    cx,
                )
                .tooltip(if runtime == AgentRuntimeKind::Chat {
                    "Chat agent — switch to Terminal"
                } else {
                    "Terminal agent — switch to Chat"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(composer) = this.new_agent_composer.as_mut() {
                        composer.runtime = if composer.runtime == AgentRuntimeKind::Chat {
                            AgentRuntimeKind::Terminal
                        } else {
                            AgentRuntimeKind::Chat
                        };
                        if composer.runtime == AgentRuntimeKind::Terminal {
                            composer.preview_armed = false;
                        }
                        composer.error = None;
                    }
                    cx.notify();
                })),
            )
            .child(
                // Literally the agent-chat composer's send, so size, radius and
                // accent can never drift between the two composers — except on
                // the tour's send steps, where it becomes a labelled primary
                // button so the one tap the tour is waiting on is unmistakable.
                div()
                    .relative()
                    .flex_none()
                    .child(if crate::ui::onboarding::emphasizes_send(project, cx) {
                        let button = crate::ui::style::primary_button_compact(
                            "start-inline-agent",
                            "Send",
                            cx,
                        )
                        .icon(gpui_component::IconName::ArrowUp)
                        .disabled(send_pending)
                        .tooltip(send_tooltip);
                        if send_pending {
                            button.into_any_element()
                        } else {
                            button
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_agent_composer(window, cx);
                                }))
                                .into_any_element()
                        }
                    } else {
                        // The Solo toggle recolors send to sky from the very
                        // first message — same signal as inside the agent.
                        let solo = self
                            .new_agent_composer
                            .as_ref()
                            .is_some_and(|composer| composer.solo);
                        let send_fill = if solo {
                            crate::ui::design::sky(cx)
                        } else {
                            crate::ui::design::accent(cx)
                        };
                        let send_hover = if solo {
                            crate::ui::design::sky(cx).opacity(0.85)
                        } else {
                            crate::ui::design::accent_2(cx)
                        };
                        let button =
                            crate::ui::style::composer_send_in("start-inline-agent", send_fill, cx)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(send_tooltip).build(window, cx)
                                });
                        if send_pending {
                            button.opacity(0.55).into_any_element()
                        } else {
                            button
                                .cursor_pointer()
                                .hover(move |button| button.bg(send_hover))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_agent_composer(window, cx);
                                }))
                                .into_any_element()
                        }
                    })
                    .child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::ComposerSend,
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_new_agent_project_strip(
        &self,
        composer_view: Entity<CenterArea>,
        project_info: Project,
        project_entries: Vec<(ProjectId, String)>,
        selected_project: ProjectId,
        repository_entries: Vec<(PathBuf, String)>,
        selected_repository: Option<PathBuf>,
        branch_data: Option<(Option<String>, Vec<BranchInfo>, bool)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // A caption under the composer box: no bar, no fill, no divider — just
        // the project and its branch, like the titlebar meta.
        let solo_active = self
            .new_agent_composer
            .as_ref()
            .is_some_and(|composer| composer.solo);
        h_flex()
            .w_full()
            .min_w(px(0.))
            .px_1()
            .gap_2()
            .items_center()
            .child(
                Button::new("composer-project")
                    .small()
                    .compact()
                    .h(crate::ui::design::control_h())
                    .rounded(crate::ui::design::r_sm())
                    .custom(crate::ui::style::header_meta_strong_variant(cx))
                    // Explicit: `.small()` otherwise falls back to gpui's own
                    // larger default instead of the app's control text size.
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_color(crate::ui::design::t1(cx))
                            .child(crate::ui::project_visuals::project_icon_element(
                                &project_info.icon,
                                &project_info.icon_color,
                                project_info.icon_image_path.as_deref(),
                                px(18.),
                                px(13.),
                                cx,
                            ))
                            .child(gpui::SharedString::from(project_info.name.clone()))
                            .child(
                                gpui_component::Icon::new(IconName::ChevronDown)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t3(cx)),
                            ),
                    )
                    .dropdown_menu({
                        let composer_view = composer_view.clone();
                        move |mut menu, window, _| {
                            for (project_id, project_name) in project_entries.clone() {
                                menu = menu.item(
                                    PopupMenuItem::new(project_name)
                                        .icon(IconName::FolderOpen)
                                        .checked(project_id == selected_project)
                                        .on_click(window.listener_for(
                                            &composer_view,
                                            move |this: &mut Self, _, window, cx| {
                                                let repositories = this
                                                    .git_states
                                                    .read(cx)
                                                    .repositories(project_id);
                                                let repository_path =
                                                    (repositories.len() == 1).then(|| {
                                                        repositories[0]
                                                            .read(cx)
                                                            .repo_path
                                                            .clone()
                                                    });
                                                if let Some(composer) =
                                                    this.new_agent_composer.as_mut()
                                                {
                                                    composer.project = project_id;
                                                    composer.repository_path = repository_path;
                                                    composer.solo = false;
                                                    composer.solo_base = None;
                                                    composer.linked_docs.clear();
                                                    composer.attached_files.clear();
                                                    composer.source_doc = None;
                                                    composer.linked_tasks.clear();
                                                    composer.source_task = None;
                                                    composer.selected_mentions.retain(|mention| {
                                                        mention.kind
                                                            != ComposerMentionKind::Project
                                                            || mention.project_id
                                                                != Some(project_id)
                                                    });
                                                    composer.project_mention_selected = 0;
                                                    composer.project_mention_dismissed_query = None;
                                                    composer.error = None;
                                                }
                                                this.workspace.update(cx, |workspace, cx| {
                                                    workspace.set_active(project_id, cx);
                                                });
                                                this.composer_branch_query.update(
                                                    cx,
                                                    |input, cx| {
                                                        input.set_value("", window, cx);
                                                    },
                                                );
                                                this.composer_branch_expanded = false;
                                                cx.notify();
                                            },
                                        )),
                                );
                            }
                            menu
                        }
                    }),
            )
            .child(div().flex_1())
            .when(repository_entries.len() > 1, |row| {
                let label = selected_repository
                    .as_ref()
                    .and_then(|selected| {
                        repository_entries
                            .iter()
                            .find(|(path, _)| path == selected)
                            .map(|(_, label)| label.clone())
                    })
                    .unwrap_or_else(|| "Entire workspace".to_string());
                let entries = repository_entries.clone();
                let selected = selected_repository.clone();
                let entire_workspace = selected_repository.is_none();
                row.child(
                    crate::ui::style::header_meta_button("composer-repository", cx)
                        .text_size(crate::ui::design::text_label())
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .when(entire_workspace, |scope| {
                                    scope.child(
                                        crate::ui::design::indicator::lucide_icon(
                                            lucide_icons::Icon::FolderGit2,
                                            crate::ui::design::sky(cx).opacity(0.78),
                                            crate::ui::design::icon_sm(),
                                        ),
                                    )
                                })
                                .child(div().max_w(px(150.)).truncate().child(label))
                                .child(
                                    gpui_component::Icon::new(IconName::ChevronDown)
                                        .size(crate::ui::design::icon_sm())
                                        .text_color(crate::ui::design::t3(cx)),
                                ),
                        )
                        .tooltip("Choose one repository or let the agent work across the workspace")
                        .dropdown_menu({
                            let composer_view = composer_view.clone();
                            move |mut menu, window, _| {
                                menu = menu.item(
                                    PopupMenuItem::new("Entire workspace")
                                        .checked(selected.is_none())
                                        .on_click(window.listener_for(
                                            &composer_view,
                                            move |this: &mut Self, _, _, cx| {
                                                if let Some(composer) =
                                                    this.new_agent_composer.as_mut()
                                                {
                                                    composer.repository_path = None;
                                                    composer.solo = false;
                                                    composer.solo_base = None;
                                                    composer.error = None;
                                                }
                                                cx.notify();
                                            },
                                        )),
                                );
                                for (path, label) in entries.clone() {
                                    let checked = selected.as_ref() == Some(&path);
                                    menu = menu.item(
                                        PopupMenuItem::new(label)
                                            .checked(checked)
                                            .on_click(window.listener_for(
                                                &composer_view,
                                                move |this: &mut Self, _, _, cx| {
                                                    if let Some(composer) =
                                                        this.new_agent_composer.as_mut()
                                                    {
                                                        composer.repository_path = Some(path.clone());
                                                        composer.solo_base = None;
                                                        composer.error = None;
                                                    }
                                                    cx.notify();
                                                },
                                            )),
                                    );
                                }
                                menu
                            }
                        }),
                )
            })
            // Solo sits beside the branch: it's the same kind of decision —
            // *where* the agent works — but its own control, not a branch.
            .child({
                let (solo, lane_profile, solo_base) = self
                    .new_agent_composer
                    .as_ref()
                    .map(|composer| {
                        (
                            composer.solo,
                            composer.lane_profile,
                            composer.solo_base.clone(),
                        )
                    })
                    .unwrap_or((false, ide_core::LaneProfile::Full, None));
                let project = selected_project;
                // The "from" picker: which branch the Solo forks off. Reuses
                // the strip's branch data; selecting sets the base only — it
                // never switches the project's own branch.
                let from_branches: Vec<String> = branch_data
                    .as_ref()
                    .map(|(_, branches, _)| {
                        branches
                            .iter()
                            .filter(|candidate| !candidate.is_remote)
                            .filter(|candidate| !candidate.name.starts_with("solo/"))
                            .map(|candidate| candidate.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let current_branch = branch_data
                    .as_ref()
                    .and_then(|(current, _, _)| current.clone());
                let from_label = solo_base
                    .clone()
                    .or_else(|| current_branch.clone())
                    .unwrap_or_else(|| "branch".to_string());
                h_flex()
                    .gap_2()
                    .items_center()
                    .when(solo, |group| {
                        group.child(
                            crate::ui::style::header_meta_button("composer-lane-profile", cx)
                                .text_size(crate::ui::design::text_label())
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(lane_profile.label())
                                        .child(
                                            gpui_component::Icon::new(IconName::ChevronDown)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t3(cx)),
                                        ),
                                )
                                .tooltip("How much the Solo lane is prepared with")
                                .dropdown_menu({
                                    let composer_view = composer_view.clone();
                                    move |mut menu, window, _| {
                                        for candidate in ide_core::LaneProfile::ALL {
                                            menu = menu.item(
                                                PopupMenuItem::new(candidate.label())
                                                    .checked(lane_profile == candidate)
                                                    .on_click(window.listener_for(
                                                        &composer_view,
                                                        move |this: &mut Self, _, _, cx| {
                                                            if let Some(composer) =
                                                                this.new_agent_composer.as_mut()
                                                            {
                                                                composer.lane_profile = candidate;
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            );
                                        }
                                        menu
                                    }
                                }),
                        )
                    })
                    .child(
                        crate::ui::style::header_meta_button("composer-solo", cx)
                            .text_size(crate::ui::design::text_label())
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if solo {
                                        crate::ui::design::sky(cx)
                                    } else {
                                        crate::ui::design::t3(cx)
                                    })
                                    .child(composer_solo_icon(if solo {
                                        crate::ui::design::sky(cx)
                                    } else {
                                        crate::ui::design::t3(cx)
                                    }))
                                    .child("Solo"),
                            )
                            .tooltip(if solo {
                                "Solo is on — this agent gets its own branch and folder; your files stay untouched"
                            } else {
                                "Run as a Solo: its own branch and folder, your files untouched"
                            })
                            .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                                let selected_repository = this
                                    .new_agent_composer
                                    .as_ref()
                                    .and_then(|composer| composer.repository_path.clone());
                                let repository = selected_repository.or_else(|| {
                                    this.git_states
                                        .read(cx)
                                        .active_repository_path(project)
                                });
                                let default_profile = this
                                    .project_by_id(project, cx)
                                    .and_then(|_| repository.as_deref().map(ide_core::lanes::default_profile));
                                if let Some(composer) = this.new_agent_composer.as_mut() {
                                    composer.solo = !composer.solo;
                                    if composer.solo {
                                        composer.repository_path = repository;
                                        if let Some(profile) = default_profile {
                                            composer.lane_profile = profile;
                                        }
                                    } else {
                                        composer.solo_base = None;
                                    }
                                    composer.error = None;
                                }
                                cx.notify();
                            })),
                    )
                    .when(solo && !from_branches.is_empty(), |group| {
                        group.child(
                            crate::ui::style::header_meta_button("composer-solo-from", cx)
                                .text_size(crate::ui::design::text_label())
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("from")
                                        .child(
                                            div()
                                                .max_w(px(120.))
                                                .truncate()
                                                .text_color(crate::ui::design::t2(cx))
                                                .child(from_label),
                                        )
                                        .child(
                                            gpui_component::Icon::new(IconName::ChevronDown)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t3(cx)),
                                        ),
                                )
                                .tooltip("Which branch the Solo forks from — your project stays where it is")
                                .dropdown_menu({
                                    let composer_view = composer_view.clone();
                                    let selected = solo_base.or(current_branch);
                                    move |mut menu, window, _| {
                                        for candidate in from_branches.clone() {
                                            let name = candidate.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(candidate.clone())
                                                    .checked(
                                                        selected.as_deref()
                                                            == Some(candidate.as_str()),
                                                    )
                                                    .on_click(window.listener_for(
                                                        &composer_view,
                                                        move |this: &mut Self, _, _, cx| {
                                                            if let Some(composer) =
                                                                this.new_agent_composer.as_mut()
                                                            {
                                                                composer.solo_base =
                                                                    Some(name.clone());
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            );
                                        }
                                        menu
                                    }
                                }),
                        )
                    })
            })
            // A Solo replaces the branch decision entirely — it forks from the
            // "from" pick and the project stays where it is, so showing the
            // project's own branch control too would be two branches for one
            // choice.
            .when_some(
                branch_data.filter(|_| !solo_active),
                |row, (current, branches, busy)| {
                let branch_label = current.unwrap_or_else(|| "branch".into());
                let entries = branches.clone();
                let can_open = !entries.is_empty() && !busy;
                let trigger = crate::ui::style::header_meta_button("composer-branch", cx)
                    // Smaller than the project: the project names the place,
                    // the branch is secondary metadata.
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .disabled(!can_open)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_size(crate::ui::design::text_label())
                            // Muted, like the titlebar: the project name reads,
                            // the branch recedes.
                            .text_color(crate::ui::design::t3(cx))
                            .child(branch_icon(crate::ui::design::t3(cx)))
                            .child(div().max_w(px(180.)).truncate().child(branch_label))
                            .child(
                                gpui_component::Icon::new(IconName::ChevronDown)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t3(cx)),
                            ),
                    );

                if !can_open {
                    return row.child(trigger);
                }

                let composer_for_open = composer_view.clone();
                let composer_for_content = composer_view.clone();
                row.child(
                    gpui_component::popover::Popover::new("composer-branch-popover")
                        .adaptive_anchor(true)
                        .appearance(false)
                        .open(self.composer_branch_expanded)
                        .on_open_change(move |open, window, cx| {
                            composer_for_open.update(cx, |this, cx| {
                                this.composer_branch_expanded = *open;
                                if *open {
                                    this.composer_branch_query.update(cx, |input, cx| {
                                        input.set_value("", window, cx);
                                        input.focus(window, cx);
                                    });
                                }
                                cx.notify();
                            });
                        })
                        .trigger(trigger)
                        .content(move |_, _, cx| {
                            composer_for_content.update(cx, |this, cx| {
                                this.render_composer_branch_picker(
                                    selected_project,
                                    selected_repository.clone().unwrap(),
                                    entries.clone(),
                                    cx,
                                )
                                .into_any_element()
                            })
                        }),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_new_agent_composer(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(composer) = self.new_agent_composer.as_ref() else {
            return div().into_any_element();
        };
        // Copied out up front: the `&self` borrow on `composer` can't survive the
        // `&mut self` picker/mention calls further down.
        let Some(project_info) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
        else {
            return div().into_any_element();
        };

        let prompt = composer.prompt.clone();
        let selected_command = composer.selected_command.clone();
        let selected_mentions = composer.selected_mentions.clone();
        let provider = composer.provider;
        let runtime = composer.runtime;
        let interaction_mode = composer.interaction_mode;
        let model = composer.model;
        let effort = composer.effort;
        let access_mode = composer.access_mode;
        let preview_armed = composer.runtime == AgentRuntimeKind::Chat && composer.preview_armed;
        let prompt_value = prompt.read(cx).value().to_string();
        let preview_suggested = should_suggest_choro_preview(
            &prompt_value,
            preview_armed,
            composer.preview_suggestion_dismissed,
        );
        let linked_docs = composer.linked_docs.clone();
        let visible_linked_docs = linked_docs
            .iter()
            .filter(|path| {
                let path_label = path.to_string_lossy();
                !selected_mentions.iter().any(|mention| {
                    mention.kind == ComposerMentionKind::Doc && mention.path_label == path_label
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let attached_files = composer.attached_files.clone();
        let attachment_pastes_pending = composer.attachment_pastes_pending;
        let error = composer.error.clone();
        let voice_transcribing = self
            .voice
            .read(cx)
            .dictation_transcribing_for(crate::voice::VoiceDictationTarget::NewAgent(project));
        let selected_project = composer.project;
        let selected_repository = composer.repository_path.clone();
        let slash_view = self.active_composer_slash_view(cx);
        let project_mention_view = if slash_view.is_none() {
            self.active_composer_project_mention_view(cx)
        } else {
            None
        };
        let doc_mention_view = if slash_view.is_none() && project_mention_view.is_none() {
            self.active_composer_doc_mention_view(cx)
        } else {
            None
        };
        let file_mention_view =
            if slash_view.is_none() && project_mention_view.is_none() && doc_mention_view.is_none()
            {
                self.active_composer_file_mention_view(cx)
            } else {
                None
            };
        let project_entries = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| (project.id, project.name.clone()))
            .collect::<Vec<_>>();
        let repository_entries = self
            .git_states
            .read(cx)
            .repositories(selected_project)
            .into_iter()
            .map(|git| {
                let path = git.read(cx).repo_path.clone();
                let label = path
                    .strip_prefix(&project_info.path)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project_info.name.clone());
                (path, label)
            })
            .collect::<Vec<_>>();
        let branch_data = selected_repository
            .as_ref()
            .and_then(|repository| {
                self.git_states
                    .read(cx)
                    .get_for_path(selected_project, repository)
            })
            .map(|git| {
                let git = git.read(cx);
                let current = git.branch_label();
                let mut branches = git
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.branches.clone())
                    .unwrap_or_default();
                // Solo branches never appear as checkout targets — they belong to
                // their agents (and live lanes can't be checked out here anyway).
                branches.retain(|branch| !branch.name.starts_with("solo/"));
                (current, branches, git.is_busy)
            });
        let mention_picker = slash_view
            .as_ref()
            .map(|view| self.render_composer_slash_picker(view, cx))
            .or_else(|| {
                project_mention_view
                    .as_ref()
                    .map(|view| self.render_composer_project_mention_picker(view, cx))
            })
            .or_else(|| {
                doc_mention_view
                    .as_ref()
                    .map(|view| self.render_composer_doc_mention_picker(view, &linked_docs, cx))
            })
            .or_else(|| {
                file_mention_view
                    .as_ref()
                    .map(|view| self.render_composer_file_mention_picker(view, cx))
            });

        let composer_view = cx.entity().clone();

        v_flex()
            .id("new-agent-landing-scroll")
            .relative()
            .size_full()
            .min_h(px(0.))
            .items_center()
            // The Brain sections below the composer can be taller than the
            // viewport. Keep the composer at its natural height and let this
            // landing surface scroll instead of flex-shrinking the composer
            // until its control rail is clipped by the rounded frame.
            // Use GPUI's native overflow on this flex owner. The decorated
            // scrollbar helper wraps and resets the element's flex styles,
            // which drops `items_center` and left-aligns the whole landing
            // column after the first layout pass.
            .overflow_y_scroll()
            .px_8()
            .child(div().w_full().flex_1().min_h(px(32.)))
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.))
                    .max_w(crate::ui::design::center_content_max_w())
                    .flex_none()
                    .gap_2()
                    .child(
                        div()
                            .w_full()
                            .text_size(crate::ui::design::text_display())
                            .text_center()
                            .text_color(crate::ui::design::t1(cx))
                            .mb_3()
                            .child(format!("What should we build in {}?", project_info.name)),
                    )
                    .when_some(mention_picker, |view, picker| {
                        view.child(div().w_full().child(picker))
                    })
                    .child(
                        v_flex()
                            .relative()
                            .w_full()
                            .min_w(px(0.))
                            // This frame owns the prompt and its footer. It must
                            // never surrender height to the recent-agent/Brain
                            // content that follows it.
                            .flex_none()
                            // Same frame as the agent-chat composer: r_lg, a
                            // line-2 border, the focus plane, and the shared
                            // shadow — the two composers must read identically.
                            .rounded(crate::ui::design::r_lg())
                            .overflow_hidden()
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::focus(cx))
                            .shadow(crate::ui::design::shadow())
                            // The prompt `Input` binds arrows/enter/escape as
                            // actions and consumes them in the bubble phase, so a
                            // `capture_key_down` here never sees them. Capturing
                            // the *actions* runs ahead of the input and lets the
                            // `@@` doc picker own those keys while it is open.
                            .capture_action(cx.listener(|this, _: &MoveDown, _window, cx| {
                                if let Some(view) = this.active_composer_slash_view(cx) {
                                    if view.matches.is_empty() {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let next = if view.selected + 1 >= view.matches.len() {
                                        0
                                    } else {
                                        view.selected + 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.slash_selection = next;
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_project_mention_view(cx) {
                                    if view.matches.is_empty() {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let next = if view.selected + 1 >= view.matches.len() {
                                        0
                                    } else {
                                        view.selected + 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.project_mention_selected = next;
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_doc_mention_view(cx) {
                                    if view.total() == 0 {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let len = view.total();
                                    let next = if view.selected + 1 >= len {
                                        0
                                    } else {
                                        view.selected + 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.doc_mention_selected = next;
                                    }
                                    cx.notify();
                                    return;
                                }
                                let Some(view) = this.active_composer_file_mention_view(cx) else {
                                    return;
                                };
                                if view.matches.is_empty() {
                                    return;
                                }
                                cx.stop_propagation();
                                let len = view.matches.len();
                                let next = if view.selected + 1 >= len {
                                    0
                                } else {
                                    view.selected + 1
                                };
                                if let Some(composer) = this.new_agent_composer.as_mut() {
                                    composer.file_mention_selected = next;
                                }
                                cx.notify();
                            }))
                            .capture_action(cx.listener(|this, _: &MoveUp, _window, cx| {
                                if let Some(view) = this.active_composer_slash_view(cx) {
                                    if view.matches.is_empty() {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let prev = if view.selected == 0 {
                                        view.matches.len() - 1
                                    } else {
                                        view.selected - 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.slash_selection = prev;
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_project_mention_view(cx) {
                                    if view.matches.is_empty() {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let prev = if view.selected == 0 {
                                        view.matches.len() - 1
                                    } else {
                                        view.selected - 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.project_mention_selected = prev;
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_doc_mention_view(cx) {
                                    if view.total() == 0 {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let len = view.total();
                                    let prev = if view.selected == 0 {
                                        len - 1
                                    } else {
                                        view.selected - 1
                                    };
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.doc_mention_selected = prev;
                                    }
                                    cx.notify();
                                    return;
                                }
                                let Some(view) = this.active_composer_file_mention_view(cx) else {
                                    return;
                                };
                                if view.matches.is_empty() {
                                    return;
                                }
                                cx.stop_propagation();
                                let len = view.matches.len();
                                let prev = if view.selected == 0 {
                                    len - 1
                                } else {
                                    view.selected - 1
                                };
                                if let Some(composer) = this.new_agent_composer.as_mut() {
                                    composer.file_mention_selected = prev;
                                }
                                cx.notify();
                            }))
                            .capture_action(cx.listener(|this, _: &Escape, _window, cx| {
                                if let Some(view) = this.active_composer_slash_view(cx) {
                                    cx.stop_propagation();
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.slash_dismissed_query =
                                            Some(view.query.query.clone());
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_project_mention_view(cx) {
                                    cx.stop_propagation();
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.project_mention_dismissed_query =
                                            Some(view.mention.query.clone());
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(view) = this.active_composer_doc_mention_view(cx) {
                                    cx.stop_propagation();
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.doc_mention_dismissed_query =
                                            Some(view.mention.query.clone());
                                    }
                                    cx.notify();
                                    return;
                                }
                                let Some(view) = this.active_composer_file_mention_view(cx) else {
                                    return;
                                };
                                cx.stop_propagation();
                                if let Some(composer) = this.new_agent_composer.as_mut() {
                                    composer.file_mention_dismissed_query =
                                        Some(view.mention.query.clone());
                                }
                                cx.notify();
                            }))
                            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                                if let Some(view) = this.active_composer_slash_view(cx) {
                                    let Some(command) = view.matches.get(view.selected).cloned()
                                    else {
                                        return;
                                    };
                                    cx.stop_propagation();
                                    this.insert_slash_command_into_composer(
                                        command, view.query, window, cx,
                                    );
                                    return;
                                }
                                if let Some(view) = this.active_composer_project_mention_view(cx) {
                                    let Some(project) = view.matches.get(view.selected).cloned()
                                    else {
                                        return;
                                    };
                                    cx.stop_propagation();
                                    this.insert_project_mention_into_composer(
                                        project,
                                        view.mention,
                                        window,
                                        cx,
                                    );
                                    return;
                                }
                                if let Some(view) = this.active_composer_doc_mention_view(cx) {
                                    cx.stop_propagation();
                                    if view.selected < view.designs.len() {
                                        if let Some(reference) =
                                            view.designs.get(view.selected).cloned()
                                        {
                                            this.insert_design_mention_into_composer(
                                                reference,
                                                view.mention,
                                                window,
                                                cx,
                                            );
                                        }
                                    } else if let Some(doc) =
                                        view.matches.get(view.selected - view.designs.len())
                                    {
                                        this.insert_doc_mention_into_composer(
                                            doc.relative_path.clone(),
                                            view.mention,
                                            window,
                                            cx,
                                        );
                                    }
                                    return;
                                }
                                let Some(view) = this.active_composer_file_mention_view(cx) else {
                                    return;
                                };
                                let Some(file) = view.matches.get(view.selected).cloned() else {
                                    return;
                                };
                                cx.stop_propagation();
                                this.insert_file_mention_into_composer(
                                    file,
                                    view.mention.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                                // Cmd/Ctrl+Enter always submits the composer.
                                if action.secondary {
                                    cx.stop_propagation();
                                    this.start_new_agent_composer(window, cx);
                                    return;
                                }
                                if let Some(view) = this.active_composer_slash_view(cx) {
                                    let Some(command) = view.matches.get(view.selected).cloned()
                                    else {
                                        return;
                                    };
                                    cx.stop_propagation();
                                    this.insert_slash_command_into_composer(
                                        command, view.query, window, cx,
                                    );
                                    return;
                                }
                                if let Some(view) = this.active_composer_project_mention_view(cx) {
                                    let Some(project) = view.matches.get(view.selected).cloned()
                                    else {
                                        return;
                                    };
                                    cx.stop_propagation();
                                    this.insert_project_mention_into_composer(
                                        project,
                                        view.mention,
                                        window,
                                        cx,
                                    );
                                    return;
                                }
                                // Plain Enter accepts the highlighted doc when the
                                // picker is open; otherwise it falls through to the
                                // input (newline).
                                if let Some(view) = this.active_composer_doc_mention_view(cx) {
                                    cx.stop_propagation();
                                    if view.selected < view.designs.len() {
                                        if let Some(reference) =
                                            view.designs.get(view.selected).cloned()
                                        {
                                            this.insert_design_mention_into_composer(
                                                reference,
                                                view.mention,
                                                window,
                                                cx,
                                            );
                                        }
                                    } else if let Some(doc) =
                                        view.matches.get(view.selected - view.designs.len())
                                    {
                                        this.insert_doc_mention_into_composer(
                                            doc.relative_path.clone(),
                                            view.mention,
                                            window,
                                            cx,
                                        );
                                    }
                                    return;
                                }
                                let Some(view) = this.active_composer_file_mention_view(cx) else {
                                    return;
                                };
                                let Some(file) = view.matches.get(view.selected).cloned() else {
                                    return;
                                };
                                cx.stop_propagation();
                                this.insert_file_mention_into_composer(
                                    file,
                                    view.mention.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .capture_action(cx.listener(|this, _: &Paste, _window, cx| {
                                if this.paste_image_into_new_agent_composer(false, cx) {
                                    cx.stop_propagation();
                                }
                            }))
                            .can_drop(|dragged, _, _| dragged.is::<ExternalPaths>())
                            .on_drop::<ExternalPaths>(cx.listener(
                                |this, paths: &ExternalPaths, _, cx| {
                                    this.attach_paths_to_new_agent_composer(paths.paths(), cx);
                                },
                            ))
                            .capture_action(cx.listener(
                                |this, _: &ToggleAgentPlanMode, _window, cx| {
                                    cx.stop_propagation();
                                    if crate::ui::onboarding::locks_onboarding_plan_mode(cx) {
                                        return;
                                    }
                                    if let Some(composer) = this.new_agent_composer.as_mut() {
                                        composer.interaction_mode = if composer.interaction_mode
                                            == AgentInteractionMode::Plan
                                        {
                                            AgentInteractionMode::Default
                                        } else {
                                            AgentInteractionMode::Plan
                                        };
                                        composer.error = None;
                                    }
                                    cx.notify();
                                },
                            ))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .min_w(px(0.))
                                    .px_4()
                                    .pt_4()
                                    .pb_3()
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .min_w(px(0.))
                                            .gap_2()
                                            .when(preview_armed, |col| {
                                                col.child(
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .child(
                                                            crate::ui::style::preview_attachment_chip(
                                                                "new-agent-preview-token",
                                                                cx,
                                                            )
                                                            .tooltip("Remove Choro Preview from this task")
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    if let Some(composer) =
                                                                        this.new_agent_composer.as_mut()
                                                                    {
                                                                        composer.preview_armed = false;
                                                                        composer.preview_suggestion_dismissed = true;
                                                                        composer.error = None;
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        ),
                                                )
                                            })
                                            .when(preview_suggested, |col| {
                                                col.child(
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(
                                                            crate::ui::style::composer_toggle_chip(
                                                                "new-agent-use-preview",
                                                                "Use Choro Preview?",
                                                                Some(
                                                                    crate::ui::design::indicator::lucide_icon(
                                                                        lucide_icons::Icon::MonitorPlay,
                                                                        crate::ui::design::accent(cx),
                                                                        crate::ui::design::icon_sm(),
                                                                    )
                                                                    .into_any_element(),
                                                                ),
                                                                false,
                                                                cx,
                                                            )
                                                            .tooltip("Route this request to Choro's project Preview")
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    if let Some(composer) =
                                                                        this.new_agent_composer.as_mut()
                                                                    {
                                                                        composer.preview_armed = true;
                                                                        composer.preview_suggestion_dismissed = false;
                                                                        composer.error = None;
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            crate::ui::style::composer_icon_action(
                                                                "new-agent-dismiss-preview",
                                                                gpui_component::Icon::new(IconName::Close)
                                                                    .size(crate::ui::design::icon_sm()),
                                                                cx,
                                                            )
                                                            .tooltip("Don't use Preview for this task")
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    if let Some(composer) =
                                                                        this.new_agent_composer.as_mut()
                                                                    {
                                                                        composer.preview_suggestion_dismissed = true;
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        ),
                                                )
                                            })
                                            .when_some(selected_command.clone(), |col, command| {
                                                let command_for_remove = command.clone();
                                                let prompt_for_remove = prompt.clone();
                                                let is_riff = command.is_choro_riff();
                                                let is_orbit = command.is_orbit();
                                                let is_product_capability = is_riff || is_orbit;
                                                let foreground = if is_product_capability {
                                                    crate::ui::design::accent(cx)
                                                } else {
                                                    crate::ui::design::t2(cx)
                                                };
                                                let icon = if is_orbit {
                                                    Icon::new(IconName::Network)
                                                    .size(crate::ui::design::icon_md())
                                                    .text_color(foreground)
                                                    .into_any_element()
                                                } else if is_riff {
                                                    crate::ui::style::choro_riff_icon(
                                                        crate::ui::design::icon_md(),
                                                        foreground,
                                                    )
                                                } else {
                                                    gpui_component::Icon::new(IconName::Asterisk)
                                                        .size(crate::ui::design::icon_md())
                                                        .text_color(foreground)
                                                        .into_any_element()
                                                };
                                                col.child(
                                                    h_flex()
                                                        .w_full()
                                                        .min_w(px(0.))
                                                        .items_center()
                                                        .child(
                                                            h_flex()
                                                                .id("new-agent-selected-command-token")
                                                                .flex_none()
                                                                .min_w(px(0.))
                                                                .items_center()
                                                                .gap_1()
                                                                .h(crate::ui::design::control_h_xs())
                                                                .px_1p5()
                                                                .rounded(crate::ui::design::r_sm())
                                                                .border_1()
                                                                .border_color(if is_product_capability {
                                                                    crate::ui::design::accent(cx)
                                                                        .opacity(0.34)
                                                                } else {
                                                                    crate::ui::design::line_2(cx)
                                                                })
                                                                .bg(if is_product_capability {
                                                                    crate::ui::design::accent_soft(cx)
                                                                } else {
                                                                    crate::ui::design::surface_2(cx)
                                                                })
                                                                .cursor_pointer()
                                                                .hover(|chip| {
                                                                    chip.bg(if is_product_capability {
                                                                        crate::ui::design::accent(cx)
                                                                            .opacity(0.2)
                                                                    } else {
                                                                        crate::ui::design::hover(cx)
                                                                    })
                                                                })
                                                                .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                if let Some(composer) =
                                                                    this.new_agent_composer.as_mut()
                                                                {
                                                                    composer.selected_command = None;
                                                                }
                                                                let current = prompt_for_remove
                                                                    .read(cx)
                                                                    .value()
                                                                    .to_string();
                                                                let (next, cursor) =
                                                                    remove_agent_chat_command_invocation(
                                                                        &current,
                                                                        &command_for_remove,
                                                                    );
                                                                if next != current {
                                                                    prompt_for_remove.update(
                                                                        cx,
                                                                        |input, cx| {
                                                                            input.set_value(
                                                                                next.clone(),
                                                                                window,
                                                                                cx,
                                                                            );
                                                                            input.set_cursor_position(
                                                                                input_position_for_byte_offset(
                                                                                    &next, cursor,
                                                                                ),
                                                                                window,
                                                                                cx,
                                                                            );
                                                                        },
                                                                    );
                                                                }
                                                                cx.notify();
                                                            },
                                                                ))
                                                                .child(icon)
                                                                .child(
                                                            div()
                                                                .max_w(px(170.))
                                                                .truncate()
                                                                .text_size(
                                                                    crate::ui::design::text_head(),
                                                                )
                                                                .font_weight(
                                                                    gpui::FontWeight::SEMIBOLD,
                                                                )
                                                                .text_color(foreground)
                                                                .child(command.title.clone()),
                                                                )
                                                                .child(
                                                            gpui_component::Icon::new(
                                                                IconName::Close,
                                                            )
                                                            .size(
                                                                crate::ui::design::icon_sm(),
                                                            )
                                                                    .text_color(foreground.opacity(0.72)),
                                                                ),
                                                        ),
                                                )
                                            })
                                            .when(!selected_mentions.is_empty(), |col| {
                                                col.child(self.render_new_agent_mention_prefix(
                                                    &selected_mentions,
                                                    cx,
                                                ))
                                            })
                                            .when(voice_transcribing, |col| {
                                                col.child(
                                                    crate::ui::style::composer_voice_transcribing(
                                                        cx,
                                                    ),
                                                )
                                            })
                                            .child({
                                                // On the tour's send steps the
                                                // prompt is the tour's script.
                                                // Lock the input and lay a click
                                                // catcher over it: editing can't
                                                // send the agent off-script, and
                                                // a tap earns a friendly nudge
                                                // instead of a cursor.
                                                let locked = crate::ui::onboarding::locks_composer(
                                                    project, cx,
                                                );
                                                let nudged = self.onboarding_composer_nudged;
                                                div()
                                                    .relative()
                                                    .w_full()
                                                    .child(
                                                        crate::ui::style::composer_text_input(
                                                            &prompt,
                                                        )
                                                        .disabled(locked),
                                                    )
                                                    .when(locked, |wrap| {
                                                        wrap.child(
                                                            div()
                                                                .id("onboarding-composer-lock")
                                                                .absolute()
                                                                .inset_0()
                                                                .cursor_pointer()
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        this.onboarding_composer_nudged = true;
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        )
                                                    })
                                                    .when(locked && nudged, |wrap| {
                                                        wrap.child(
                                                            div()
                                                                .absolute()
                                                                .bottom(px(-2.))
                                                                .left(px(0.))
                                                                .child(
                                                                    crate::ui::style::tag(
                                                                        "✨ the tour typed this one — just send it, you’re up next",
                                                                        cx,
                                                                    ),
                                                                ),
                                                        )
                                                    })
                                            })
                                            .child(self.render_new_agent_control_rail(
                                                composer_view.clone(),
                                                project,
                                                provider,
                                                runtime,
                                                interaction_mode,
                                                model,
                                                effort,
                                                access_mode,
                                                cx,
                                            )),
                                    ),
                            )
                            .child(crate::ui::onboarding::target_marker(
                                crate::ui::onboarding::SpotlightTarget::Composer,
                                cx,
                            ))
                            .when(!visible_linked_docs.is_empty(), |card| {
                                card.child(
                                    h_flex()
                                        .w_full()
                                        .px_4()
                                        .pb_2()
                                        .gap_1()
                                        .flex_wrap()
                                        .children(visible_linked_docs.iter().enumerate().map(
                                            |(index, path)| {
                                                let label = path.to_string_lossy().to_string();
                                                let remove_path = path.clone();
                                                h_flex()
                                                    .id(("composer-doc-chip", index))
                                                    .gap_1()
                                                    .items_center()
                                                    .rounded(crate::ui::design::r_sm())
                                                    .border_1()
                                                    .border_color(crate::ui::style::border(cx))
                                                    .bg(crate::ui::design::surface(cx))
                                                    .px_2()
                                                    .py_1()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t1(cx))
                                                    .child(label)
                                                    .child(
                                                        Button::new(("remove-composer-doc", index))
                                                            .ghost()
                                                            .xsmall()
                                                            .compact()
                                                            .h(px(18.))
                                                            .icon(IconName::Close)
                                                            .tooltip("Remove doc")
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    if let Some(composer) = this
                                                                        .new_agent_composer
                                                                        .as_mut()
                                                                    {
                                                                        composer
                                                                            .linked_docs
                                                                            .retain(|path| {
                                                                                path != &remove_path
                                                                            });
                                                                        composer.error = None;
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .into_any_element()
                                            },
                                        )),
                                )
                            })
                            .when(
                                !attached_files.is_empty() || attachment_pastes_pending > 0,
                                |card| {
                                card.child(
                                    h_flex()
                                        .w_full()
                                        .px_4()
                                        .pb_2()
                                        .gap_2()
                                        .flex_wrap()
                                        .children(attached_files.iter().enumerate().map(
                                            |(index, path)| {
                                                self.render_agent_attachment_preview(
                                                    ("new-agent-composer-attachment", index),
                                                    path.clone(),
                                                    Some(AttachmentRemoval::NewAgent {
                                                        path: path.clone(),
                                                    }),
                                                    58.,
                                                    58.,
                                                    cx,
                                                )
                                            },
                                        ))
                                        .children((0..attachment_pastes_pending).map(|index| {
                                            self.render_agent_attachment_pending(
                                                ("new-agent-composer-attachment-pending", index),
                                                cx,
                                            )
                                        })),
                                )
                            }),
                    )
                    // Sits *under* the composer box rather than inside it, so the
                    // box stays a clean card and these read as its caption.
                    .child(self.render_new_agent_project_strip(
                        composer_view.clone(),
                        project_info.clone(),
                        project_entries.clone(),
                        selected_project,
                        repository_entries.clone(),
                        selected_repository.clone(),
                        branch_data.clone(),
                        cx,
                    ))
                    .when_some(error, |view, error| {
                        view.child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::rose(cx))
                                .child(error),
                        )
                    })
                    // The project's recent conversations, under the composer:
                    // close enough to step back into without going looking.
                    .children(self.render_new_agent_recent_agents(project, cx))
                    .children(self.render_fleet_weekly_digest(project, cx)),
            )
            .child(div().w_full().flex_1().min_h(px(32.)))
            .into_any_element()
    }
}
