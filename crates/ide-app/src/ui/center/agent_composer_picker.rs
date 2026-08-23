use super::*;

/// Every provider the composer can start an agent with. Ordered as the rail
/// lists them.
const COMPOSER_PROVIDERS: [AgentKind; 3] =
    [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode];

/// Keeps provider filtering from resizing the popover. The viewport is tuned
/// to reveal three complete model rows plus half of the next one, making the
/// scroll affordance visible without adding extra chrome.
const COMPOSER_MODEL_PICKER_H: f32 = 320.0;

#[derive(Clone)]
enum ComposerModelRow {
    BuiltIn(AgentKind, AgentModel),
    OpenCode(OpenCodeModel),
}

#[derive(Clone)]
enum ComposerModelPickerTarget {
    NewAgent,
    AgentChat {
        surface: AgentChatSurface,
        agent_id: Uuid,
        provider_switch_locked: bool,
    },
}

impl ComposerModelPickerTarget {
    fn provider_switch_locked(&self) -> bool {
        matches!(
            self,
            Self::AgentChat {
                provider_switch_locked: true,
                ..
            }
        )
    }

    fn shows_provider(&self, provider: AgentKind, current_provider: AgentKind) -> bool {
        match self {
            Self::NewAgent => true,
            Self::AgentChat {
                surface,
                provider_switch_locked,
                ..
            } => {
                // Once an assistant turn has started, its provider cannot be
                // switched without resetting the conversation. Match the
                // regular Agent picker by showing only the active provider
                // instead of listing every other provider as disabled.
                if *provider_switch_locked {
                    provider == current_provider
                } else {
                    surface.is_document() || provider == current_provider
                }
            }
        }
    }
}

impl ComposerModelRow {
    fn label(&self) -> &str {
        match self {
            Self::BuiltIn(_, model) => model.label(),
            Self::OpenCode(model) => &model.name,
        }
    }

    fn provider_label(&self) -> String {
        match self {
            Self::BuiltIn(provider, _) => provider.label().to_string(),
            Self::OpenCode(model) => model.provider_label(),
        }
    }
}

fn open_code_effort(variants: &[String], current: AgentEffort) -> AgentEffort {
    let supported = AgentEffort::supported_variants(variants);
    if supported.is_empty() || supported.contains(&current) {
        return current;
    }
    [
        AgentEffort::High,
        AgentEffort::Medium,
        AgentEffort::Low,
        AgentEffort::XHigh,
        AgentEffort::Max,
    ]
    .into_iter()
    .find(|effort| supported.contains(effort))
    .unwrap_or(current)
}

impl CenterArea {
    pub fn refresh_open_code_models(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some((_, project_path)) = self.active_project(cx) else {
            return;
        };
        if self.open_code_catalog.state == OpenCodeCatalogState::Loading
            || (!force && !self.open_code_catalog.is_stale_for(&project_path))
        {
            return;
        }

        self.open_code_catalog.state = OpenCodeCatalogState::Loading;
        self.open_code_catalog.project_path = Some(project_path.clone());
        self.open_code_catalog.error = None;
        cx.notify();

        let requested_path = project_path.clone();
        cx.spawn(async move |this, cx| {
            let discovery =
                cx.background_executor()
                    .spawn(async move {
                        crate::state::open_code::discover_open_code_models(&project_path)
                    })
                    .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .active_project(cx)
                    .is_none_or(|(_, active_path)| active_path != requested_path)
                {
                    this.open_code_catalog.state = OpenCodeCatalogState::NotLoaded;
                    this.open_code_catalog.refreshed_at = None;
                    this.refresh_open_code_models(false, cx);
                    return;
                }
                this.open_code_catalog.refreshed_at = Some(Instant::now());
                match discovery {
                    Ok(Some(discovery)) => {
                        this.open_code_catalog.state = OpenCodeCatalogState::Ready;
                        this.open_code_catalog.executable = Some(discovery.executable);
                        this.open_code_catalog.models = discovery.models;
                        this.open_code_catalog.error = None;
                    }
                    Ok(None) => {
                        this.open_code_catalog.state = OpenCodeCatalogState::NotInstalled;
                        this.open_code_catalog.executable = None;
                        this.open_code_catalog.models.clear();
                        this.open_code_catalog.error = None;
                    }
                    Err(error) => {
                        this.open_code_catalog.state = OpenCodeCatalogState::Failed;
                        this.open_code_catalog.models.clear();
                        this.open_code_catalog.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The composer's model picker: a provider rail, a search field, and the
    /// matching models. This single control replaces the old provider + model
    /// chip pair — picking a model implies its provider — and is where new
    /// models land as the catalog grows.
    pub(super) fn render_composer_model_picker(
        &self,
        current: AgentModel,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (current_provider, current_external_id, current_effort) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| {
                (
                    composer.provider,
                    composer.external_model_id.clone(),
                    composer.effort,
                )
            })
            .unwrap_or((AgentKind::Codex, None, current.default_effort()));
        self.render_model_picker(
            ComposerModelPickerTarget::NewAgent,
            current_provider,
            current,
            current_external_id,
            current_effort,
            cx,
        )
    }

    pub(super) fn render_agent_chat_model_picker(
        &self,
        agent: &AgentRecord,
        surface: AgentChatSurface,
        provider_switch_locked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.render_model_picker(
            ComposerModelPickerTarget::AgentChat {
                surface,
                agent_id: agent.id,
                provider_switch_locked,
            },
            agent.provider,
            agent.model,
            agent.external_model_id.clone(),
            agent.effort,
            cx,
        )
    }

    fn render_model_picker(
        &self,
        target: ComposerModelPickerTarget,
        current_provider: AgentKind,
        current: AgentModel,
        current_external_id: Option<String>,
        current_effort: AgentEffort,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filter = self.composer_model_provider;
        let query = self
            .composer_model_query
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let provider_switch_locked = target.provider_switch_locked();

        let mut rows: Vec<ComposerModelRow> = Vec::new();
        for kind in COMPOSER_PROVIDERS {
            if !target.shows_provider(kind, current_provider) {
                continue;
            }
            if filter.is_some_and(|only| only != kind) {
                continue;
            }
            if kind == AgentKind::OpenCode {
                rows.extend(
                    self.open_code_catalog
                        .models
                        .iter()
                        .cloned()
                        .map(ComposerModelRow::OpenCode),
                );
            } else {
                rows.extend(
                    AgentModel::models_for(kind)
                        .iter()
                        .copied()
                        .map(|model| ComposerModelRow::BuiltIn(kind, model)),
                );
            }
        }
        if !query.is_empty() {
            rows.retain(|row| {
                row.label().to_lowercase().contains(&query)
                    || row.provider_label().to_lowercase().contains(&query)
                    || matches!(row, ComposerModelRow::OpenCode(model) if model.id.to_lowercase().contains(&query))
            });
        }

        let open_code_empty_message = match self.open_code_catalog.state {
            OpenCodeCatalogState::Loading => "Reading models from OpenCode…".to_string(),
            OpenCodeCatalogState::NotInstalled => {
                "OpenCode is not installed on this computer.".to_string()
            }
            OpenCodeCatalogState::Failed => self
                .open_code_catalog
                .error
                .as_deref()
                .map(|error| format!("Could not read OpenCode models: {error}"))
                .unwrap_or_else(|| "Could not read OpenCode models.".to_string()),
            OpenCodeCatalogState::Ready => {
                "No models are available. Configure providers in OpenCode, then refresh."
                    .to_string()
            }
            OpenCodeCatalogState::NotLoaded => {
                "Open OpenCode or refresh to load its models.".to_string()
            }
        };

        h_flex()
            .w(px(540.))
            .h(px(COMPOSER_MODEL_PICKER_H))
            .items_start()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .flex_none()
                    .h_full()
                    .gap_1()
                    .p_1p5()
                    .bg(crate::ui::design::base(cx).opacity(0.32))
                    .child(
                        div()
                            .id("composer-model-rail-all")
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .when(filter.is_none(), |rail| {
                                rail.bg(crate::ui::design::surface_2(cx))
                            })
                            .when(filter.is_some(), |rail| {
                                rail.hover(|rail| rail.bg(crate::ui::design::hover(cx)))
                            })
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::LayoutGrid,
                                if filter.is_none() {
                                    crate::ui::design::t1(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                },
                                crate::ui::design::icon_sm(),
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // Filtering must not reach the close-on-outside
                                // backdrop underneath — the picker stays open.
                                cx.stop_propagation();
                                this.composer_model_provider = None;
                                cx.notify();
                            })),
                    )
                    .children(
                        COMPOSER_PROVIDERS
                            .into_iter()
                            .filter(|kind| target.shows_provider(*kind, current_provider))
                            .enumerate()
                            .map(|(ix, kind)| {
                                let selected = filter == Some(kind);
                                div()
                                    .id(("composer-model-rail", ix))
                                    .size(px(30.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(crate::ui::design::r_sm())
                                    .cursor_pointer()
                                    .when(selected, |rail| {
                                        rail.bg(crate::ui::design::surface_2(cx))
                                    })
                                    .when(!selected, |rail| {
                                        rail.hover(|rail| rail.bg(crate::ui::design::hover(cx)))
                                    })
                                    .child(
                                        provider_brand_icon(kind)
                                            .size(crate::ui::design::icon_sm()),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.composer_model_provider = Some(kind);
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w(px(0.))
                    .p_1p5()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(Input::new(&self.composer_model_query)),
                            )
                            .child(
                                crate::ui::style::refresh_icon_button(
                                    "composer-opencode-model-refresh",
                                    cx,
                                )
                                .tooltip("Refresh models from OpenCode")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.refresh_open_code_models(true, cx);
                                })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("composer-model-picker-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scrollbar()
                            .gap_0p5()
                            .when(rows.is_empty(), |list| {
                                let message = if filter == Some(AgentKind::OpenCode) {
                                    open_code_empty_message.clone()
                                } else if query.is_empty() {
                                    "No models are available".to_string()
                                } else {
                                    "No matching models".to_string()
                                };
                                list.child(
                                    div()
                                        .px_2()
                                        .py_1p5()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(message),
                                )
                            })
                            .children(rows.into_iter().enumerate().scan(
                                None::<String>,
                                |previous_provider, (ix, row)| {
                                    let provider_label = row.provider_label();
                                    let show_provider = previous_provider.as_deref()
                                        != Some(provider_label.as_str());
                                    *previous_provider = Some(provider_label.clone());
                                    let selected = match &row {
                                        ComposerModelRow::BuiltIn(kind, model) => {
                                            current_provider == *kind && *model == current
                                        }
                                        ComposerModelRow::OpenCode(model) => {
                                            current_provider == AgentKind::OpenCode
                                                && current_external_id.as_deref()
                                                    == Some(model.id.as_str())
                                        }
                                    };
                                    let row_provider = match &row {
                                        ComposerModelRow::BuiltIn(kind, _) => *kind,
                                        ComposerModelRow::OpenCode(_) => AgentKind::OpenCode,
                                    };
                                    let disabled =
                                        provider_switch_locked && row_provider != current_provider;
                                    let click_row = row.clone();
                                    let click_target = target.clone();
                                    let is_free = matches!(
                                        &row,
                                        ComposerModelRow::OpenCode(model) if model.free
                                    );
                                    Some(
                                        v_flex()
                                            .w_full()
                                            .when(show_provider, |item| {
                                                item.child(
                                                    div()
                                                        .px_2()
                                                        .pt_1p5()
                                                        .pb_0p5()
                                                        .text_size(crate::ui::design::text_label())
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(provider_label.clone()),
                                                )
                                            })
                                            .child(
                                                h_flex()
                                                    .id(("composer-model-row", ix))
                                                    .w_full()
                                                    .min_w(px(0.))
                                                    .gap_2()
                                                    .items_center()
                                                    .px_2()
                                                    .py_1p5()
                                                    .rounded(crate::ui::design::r_sm())
                                                    .when(!disabled, |row| {
                                                        row.cursor_pointer().hover(|row| {
                                                            row.bg(
                                                                crate::ui::design::surface_2(cx)
                                                                    .opacity(0.5),
                                                            )
                                                        })
                                                    })
                                                    .when(disabled, |row| row.opacity(0.42))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w(px(0.))
                                                            .truncate()
                                                            .text_size(
                                                                crate::ui::design::text_ui(),
                                                            )
                                                            .text_color(
                                                                crate::ui::design::t1(cx),
                                                            )
                                                            .child(row.label().to_string()),
                                                    )
                                                    .when(is_free, |row| {
                                                        row.child(
                                                            div()
                                                                .px_1()
                                                                .rounded(
                                                                    crate::ui::design::r_sm(),
                                                                )
                                                                .bg(crate::ui::design::sage_soft(cx))
                                                                .text_size(
                                                                    crate::ui::design::text_label(),
                                                                )
                                                                .text_color(
                                                                    crate::ui::design::sage(cx),
                                                                )
                                                                .child("Free"),
                                                        )
                                                    })
                                                    .when(selected, |row| {
                                                        row.child(
                                                            gpui_component::Icon::new(
                                                                IconName::Check,
                                                            )
                                                            .size(
                                                                crate::ui::design::icon_sm(),
                                                            )
                                                            .text_color(
                                                                crate::ui::design::accent(cx),
                                                            ),
                                                        )
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if disabled {
                                                                return;
                                                            }
                                                            match &click_target {
                                                                ComposerModelPickerTarget::NewAgent => {
                                                                    if let Some(composer) =
                                                                        this.new_agent_composer.as_mut()
                                                                    {
                                                                        match &click_row {
                                                                            ComposerModelRow::BuiltIn(
                                                                                kind,
                                                                                model,
                                                                            ) => {
                                                                                composer.provider =
                                                                                    *kind;
                                                                                composer.model =
                                                                                    *model;
                                                                                composer.external_model_id =
                                                                                    None;
                                                                                composer.external_model_label =
                                                                                    None;
                                                                                composer
                                                                                    .external_model_variants
                                                                                    .clear();
                                                                                composer.effort = model
                                                                                    .normalize_effort(
                                                                                        composer.effort,
                                                                                    );
                                                                            }
                                                                            ComposerModelRow::OpenCode(
                                                                                model,
                                                                            ) => {
                                                                                composer.provider =
                                                                                    AgentKind::OpenCode;
                                                                                composer.model =
                                                                                    AgentModel::OpenCode;
                                                                                composer.external_model_id =
                                                                                    Some(
                                                                                        model.id.clone(),
                                                                                    );
                                                                                composer
                                                                                    .external_model_label =
                                                                                    Some(
                                                                                        model
                                                                                            .name
                                                                                            .clone(),
                                                                                    );
                                                                                composer
                                                                                    .external_model_variants =
                                                                                    model
                                                                                        .variants
                                                                                        .clone();
                                                                                composer.effort =
                                                                                    open_code_effort(
                                                                                        &model
                                                                                            .variants,
                                                                                        composer
                                                                                            .effort,
                                                                                    );
                                                                            }
                                                                        }
                                                                        composer.error = None;
                                                                    }
                                                                }
                                                                ComposerModelPickerTarget::AgentChat {
                                                                    surface,
                                                                    agent_id,
                                                                    ..
                                                                } => match &click_row {
                                                                    ComposerModelRow::BuiltIn(
                                                                        provider,
                                                                        model,
                                                                    ) => {
                                                                        let effort = if *provider
                                                                            == current_provider
                                                                        {
                                                                            model.normalize_effort(
                                                                                current_effort,
                                                                            )
                                                                        } else {
                                                                            model.default_effort()
                                                                        };
                                                                        this.update_agent_chat_surface_model_effort(
                                                                            surface,
                                                                            *agent_id,
                                                                            *provider,
                                                                            *model,
                                                                            effort,
                                                                            cx,
                                                                        );
                                                                    }
                                                                    ComposerModelRow::OpenCode(
                                                                        model,
                                                                    ) => {
                                                                        let effort =
                                                                            open_code_effort(
                                                                                &model.variants,
                                                                                current_effort,
                                                                            );
                                                                        this.update_agent_chat_surface_external_model(
                                                                            surface,
                                                                            *agent_id,
                                                                            model.clone(),
                                                                            effort,
                                                                            cx,
                                                                        );
                                                                    }
                                                                },
                                                            }
                                                            this.composer_model_expanded = false;
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                },
                            )),
                    ),
            )
    }

    /// Backdrop + centred frame for [`Self::render_composer_model_picker`],
    /// mirroring the branch overlay.
    pub(super) fn render_composer_model_overlay(
        &self,
        current: AgentModel,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.composer_model_expanded {
            return None;
        }

        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("composer-model-picker-backdrop")
                        .absolute()
                        .size_full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer_model_expanded = false;
                            cx.notify();
                        })),
                )
                .child(
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .pt(px(210.))
                        .child(
                            div()
                                .w(px(760.))
                                .pl(px(124.))
                                .child(self.render_composer_model_picker(current, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_composer_branch_picker(
        &self,
        project: ProjectId,
        repository_path: PathBuf,
        mut branches: Vec<BranchInfo>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self
            .composer_branch_query
            .read(cx)
            .value()
            .trim()
            .to_string();
        if !query.is_empty() {
            let needle = query.to_lowercase();
            branches.retain(|branch| branch.name.to_lowercase().contains(&needle));
        }
        branches.sort_by(|a, b| {
            (!a.is_head, a.is_remote)
                .cmp(&(!b.is_head, b.is_remote))
                .then_with(|| b.tip_time.cmp(&a.tip_time))
                .then_with(|| a.name.cmp(&b.name))
        });
        branches.truncate(if query.is_empty() {
            COMPOSER_BRANCH_PICKER_LIMIT
        } else {
            20
        });

        v_flex()
            .w(px(360.))
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("composer-branch-picker-scroll")
                    .max_h(px(260.))
                    .overflow_y_scrollbar()
                    .p_1()
                    .gap_0p5()
                    .when(branches.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No matching branches"),
                        )
                    })
                    .children(branches.into_iter().enumerate().map(|(ix, branch)| {
                        let name: SharedString = branch.name.clone().into();
                        let checkout_name = branch.name.clone();
                        let checkout_repository = repository_path.clone();
                        let git_states = self.git_states.clone();
                        let ahead_behind: Option<SharedString> =
                            if branch.ahead > 0 || branch.behind > 0 {
                                Some(format!("↑{} ↓{}", branch.ahead, branch.behind).into())
                            } else {
                                None
                            };
                        let detail: SharedString = {
                            let mut parts: Vec<String> = Vec::new();
                            if !branch.tip_author.is_empty() {
                                parts.push(branch.tip_author.clone());
                            }
                            let when = branch_relative_time(branch.tip_time);
                            if !when.is_empty() {
                                parts.push(when);
                            }
                            if !branch.tip_summary.is_empty() {
                                parts.push(branch.tip_summary.clone());
                            }
                            parts.join(" · ").into()
                        };

                        h_flex()
                            .id(("composer-branch-row", ix))
                            .w_full()
                            .px_2()
                            .py_0p5()
                            .gap_2()
                            .items_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .when(branch.is_head, |row| {
                                row.bg(crate::ui::design::surface_2(cx))
                            })
                            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !branch.is_head {
                                    if let Some(git) = git_states
                                        .read(cx)
                                        .get_for_path(project, &checkout_repository)
                                    {
                                        git.update(cx, |git, cx| {
                                            git.checkout(checkout_name.clone(), cx);
                                        });
                                    }
                                }
                                this.composer_branch_query
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.composer_branch_expanded = false;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if branch.is_head {
                                        gpui_component::Icon::new(IconName::Check)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::accent(cx))
                                    } else if branch.is_remote {
                                        gpui_component::Icon::new(IconName::Globe)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t3(cx))
                                    } else {
                                        gpui_component::Icon::new(IconName::Replace)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t3(cx))
                                    }),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .truncate()
                                            .child(name),
                                    )
                                    .when(!detail.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .text_size(crate::ui::design::text_label())
                                                .text_color(crate::ui::design::t3(cx))
                                                .truncate()
                                                .child(detail),
                                        )
                                    }),
                            )
                            .when_some(ahead_behind, |row, label| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(label),
                                )
                            })
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.28))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.composer_branch_query)),
                    )
                    .child(
                        gpui_component::Icon::new(IconName::SortDescending)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }

    pub(super) fn render_composer_branch_overlay(
        &self,
        project: ProjectId,
        branches: Vec<BranchInfo>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.composer_branch_expanded || branches.is_empty() {
            return None;
        }

        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("composer-branch-picker-backdrop")
                        .absolute()
                        .size_full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer_branch_expanded = false;
                            cx.notify();
                        })),
                )
                .child(
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .pt(px(210.))
                        .child(
                            div().w(px(760.)).pl(px(124.)).child(
                                self.render_composer_branch_picker(
                                    project,
                                    self.git_states
                                        .read(cx)
                                        .active_repository_path(project)
                                        .unwrap_or_default(),
                                    branches,
                                    cx,
                                ),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn workspace_file_entries(
        &mut self,
        project: ProjectId,
        root: &Path,
        cx: &mut Context<Self>,
    ) -> Vec<ComposerFileEntry> {
        if let Some(entries) = self.composer_file_cache.get(&project) {
            return entries.clone();
        }
        if self.composer_file_cache_loading.insert(project) {
            let root = root.to_path_buf();
            cx.spawn(async move |this, cx| {
                let entries = cx
                    .background_executor()
                    .spawn(async move { collect_composer_file_entries(&root) })
                    .await;
                this.update(cx, |this, cx| {
                    this.composer_file_cache_loading.remove(&project);
                    this.composer_file_cache.insert(project, entries);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        Vec::new()
    }

    pub(super) fn active_composer_slash_view(&self, cx: &App) -> Option<AgentChatSlashView> {
        let composer = self.new_agent_composer.as_ref()?;
        let query = agent_chat_slash_query(&composer.prompt.read(cx).value())?;
        if composer.slash_dismissed_query.as_deref() == Some(query.query.as_str()) {
            return None;
        }
        let commands =
            self.cached_agent_chat_slash_capabilities(composer.provider, composer.project, cx);
        let matches = agent_chat_slash_matches(&commands, &query.query);
        let selected = composer
            .slash_selection
            .min(matches.len().saturating_sub(1));
        Some(AgentChatSlashView {
            query,
            matches,
            selected,
        })
    }

    pub(super) fn render_composer_slash_picker(
        &self,
        view: &AgentChatSlashView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = view.selected;
        v_flex()
            .id("composer-slash-picker")
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(view.matches.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .px_2()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No matching skills or commands"),
                )
            })
            .children(view.matches.iter().enumerate().map(|(index, command)| {
                let command = command.clone();
                let query = view.query.clone();
                let is_active = index == selected;
                let is_riff = command.is_choro_riff();
                let is_orbit = command.is_orbit();
                let is_preview = command.is_choro_preview();
                let detail = command
                    .description
                    .as_ref()
                    .filter(|description| !description.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| {
                        if is_orbit {
                            "Added to this project".to_string()
                        } else if is_riff {
                            "Available in every project".to_string()
                        } else {
                            command.invocation.trim().to_string()
                        }
                    });
                let title = command.title.clone();
                let source = command.source.label();
                let icon = if is_preview {
                    crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::MonitorPlay,
                        crate::ui::design::accent(cx),
                        crate::ui::design::icon_md(),
                    )
                    .into_any_element()
                } else if is_orbit {
                    Icon::new(IconName::Network)
                        .size(crate::ui::design::icon_md())
                        .text_color(crate::ui::design::accent(cx))
                        .into_any_element()
                } else if is_riff {
                    crate::ui::style::choro_riff_icon(
                        crate::ui::design::icon_md(),
                        crate::ui::design::accent(cx),
                    )
                } else {
                    gpui_component::Icon::new(IconName::Asterisk)
                        .size(crate::ui::design::icon_md())
                        .text_color(if is_active {
                            crate::ui::design::accent(cx)
                        } else {
                            crate::ui::design::t3(cx)
                        })
                        .into_any_element()
                };
                h_flex()
                    .id(("composer-slash-command", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx).opacity(0.72)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .child(icon)
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(detail),
                    )
                    .child(
                        div()
                            .rounded(crate::ui::design::r_sm())
                            .bg(if is_preview {
                                crate::ui::design::accent_soft(cx)
                            } else {
                                crate::ui::design::base(cx).opacity(0.42)
                            })
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(if is_preview {
                                crate::ui::design::accent(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .child(source),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_slash_command_into_composer(
                            command.clone(),
                            query.clone(),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            }))
            .into_any_element()
    }

    pub(super) fn active_composer_project_mention_view(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<ComposerProjectMentionView> {
        let composer = self.new_agent_composer.as_ref()?;
        let mention = active_composer_project_mention(&composer.prompt.read(cx))?;
        if composer.project_mention_dismissed_query.as_deref() == Some(mention.query.as_str()) {
            return None;
        }
        let mut matches = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .filter(|project| project.id != composer.project)
            .map(|project| ComposerProjectEntry {
                id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
                is_favorite: project.is_favorite,
            })
            .filter(|project| composer_project_matches(project, &mention.query))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right.is_favorite.cmp(&left.is_favorite).then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
        });
        matches.truncate(COMPOSER_PICKER_VISIBLE_LIMIT);
        let selected = composer
            .project_mention_selected
            .min(matches.len().saturating_sub(1));
        Some(ComposerProjectMentionView {
            mention,
            matches,
            selected,
        })
    }

    pub(super) fn render_composer_project_mention_picker(
        &self,
        view: &ComposerProjectMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = view.selected;
        v_flex()
            .id("composer-project-mention-picker")
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(view.matches.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .px_2()
                        .gap_1p5()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(IconName::FolderOpen)
                                .size(crate::ui::design::icon_md()),
                        )
                        .child(if view.mention.query.is_empty() {
                            "No other projects in Choro".to_string()
                        } else {
                            format!("No projects matching {}", view.mention.query)
                        }),
                )
            })
            .children(view.matches.iter().enumerate().map(|(index, project)| {
                let is_active = index == selected;
                let project = project.clone();
                let mention = view.mention.clone();
                let name = project.name.clone();
                let path = project.path.to_string_lossy().to_string();
                h_flex()
                    .id(("composer-project-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx).opacity(0.72)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .child(
                        gpui_component::Icon::new(IconName::FolderOpen)
                            .size(crate::ui::design::icon_md())
                            .text_color(if is_active {
                                crate::ui::design::rose(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            }),
                    )
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(path),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::rose(cx).opacity(0.12))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::rose(cx))
                            .child("Project"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_project_mention_into_composer(
                            project.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Resolves the currently active `@@` doc-mention picker, if any: the
    /// mention under the cursor, the docs matching its query, and the clamped
    /// highlight index. Returns `None` when there is no mention or the picker
    /// was dismissed for this exact query.
    pub(super) fn active_composer_doc_mention_view(
        &self,
        cx: &App,
    ) -> Option<ComposerDocMentionView> {
        let composer = self.new_agent_composer.as_ref()?;
        let mention = active_composer_doc_mention(&composer.prompt.read(cx))?;
        if composer.doc_mention_dismissed_query.as_deref() == Some(mention.query.as_str()) {
            return None;
        }
        let docs = self.docs.read(cx).docs_for_project(composer.project);
        let mut matches = composer_doc_mention_matches(&mention, &docs);
        let query = mention.query.to_ascii_lowercase();
        let mut designs: Vec<ProjectReference> = self
            .designs
            .read(cx)
            .references_for_project(composer.project)
            .into_iter()
            .filter(|reference| {
                query.is_empty()
                    || reference.title.to_ascii_lowercase().contains(&query)
                    || reference.source.to_ascii_lowercase().contains(&query)
            })
            .take(COMPOSER_PICKER_VISIBLE_LIMIT / 2)
            .collect();
        {
            let penpot = self.penpot.read(cx);
            designs.extend(
                penpot
                    .designs_for_project(composer.project)
                    .into_iter()
                    .filter_map(|design| {
                        let source = penpot.design_url(&design)?;
                        (query.is_empty()
                            || design.name.to_ascii_lowercase().contains(&query)
                            || source.to_ascii_lowercase().contains(&query))
                        .then(|| penpot_design_reference(&design, source))
                    }),
            );
        }
        designs.truncate(COMPOSER_PICKER_VISIBLE_LIMIT / 2);
        matches.truncate(COMPOSER_PICKER_VISIBLE_LIMIT.saturating_sub(designs.len()));
        let total = designs.len() + matches.len();
        let selected = composer.doc_mention_selected.min(total.saturating_sub(1));
        Some(ComposerDocMentionView {
            mention,
            matches,
            designs,
            selected,
        })
    }

    pub(super) fn active_composer_file_mention_view(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<ComposerFileMentionView> {
        let (project, prompt, dismissed, selected_index) = {
            let composer = self.new_agent_composer.as_ref()?;
            (
                composer.project,
                composer.prompt.clone(),
                composer.file_mention_dismissed_query.clone(),
                composer.file_mention_selected,
            )
        };
        let mention = active_composer_file_mention(&prompt.read(cx))?;
        if dismissed.as_deref() == Some(mention.query.as_str()) {
            return None;
        }
        let (_, root) = self.project_by_id(project, cx)?;
        let files = self.workspace_file_entries(project, &root, cx);
        let loading = self.composer_file_cache_loading.contains(&project);
        let mut matches = composer_file_mention_matches(&mention, &files);
        matches.truncate(COMPOSER_FILE_MENTION_LIMIT.min(COMPOSER_PICKER_VISIBLE_LIMIT));
        let selected = selected_index.min(matches.len().saturating_sub(1));
        Some(ComposerFileMentionView {
            mention,
            matches,
            selected,
            loading,
        })
    }

    pub(super) fn render_composer_doc_mention_picker(
        &self,
        view: &ComposerDocMentionView,
        linked_docs: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let ComposerDocMentionView {
            mention,
            matches,
            designs,
            selected,
        } = view;
        let selected = *selected;
        let design_count = designs.len();

        v_flex()
            .id("composer-doc-mention-picker")
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(matches.is_empty() && designs.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .px_2()
                        .gap_1p5()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(crate::ui::design::docs_icon())
                                .size(crate::ui::design::icon_md()),
                        )
                        .child(if mention.query.is_empty() {
                            "No docs or assets in this project".to_string()
                        } else {
                            format!("No docs or assets matching {}", mention.query)
                        }),
                )
            })
            .children(designs.iter().enumerate().map(|(index, reference)| {
                let is_active = index == selected;
                let kind = reference.kind;
                let is_penpot = project_reference_is_penpot(reference);
                let accent = if is_penpot {
                    crate::ui::design::accent(cx)
                } else {
                    crate::ui::designs_panel::design_kind_color(kind, cx)
                };
                let title = SharedString::from(reference.title.clone());
                let source = SharedString::from(reference.source.clone());
                let mention = mention.clone();
                let reference = reference.clone();
                h_flex()
                    .id(("composer-design-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx).opacity(0.72)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .child(if is_penpot {
                        gpui_component::Icon::new(crate::ui::design::design_icon())
                            .size(crate::ui::design::icon_md())
                            .text_color(accent)
                            .into_any_element()
                    } else {
                        crate::ui::designs_panel::design_kind_glyph(
                            kind,
                            crate::ui::design::icon_md(),
                            accent,
                        )
                    })
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(source),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(accent.opacity(0.14))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(accent)
                            .child(if is_penpot {
                                "Design"
                            } else {
                                crate::ui::designs_panel::design_kind_label(kind)
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_design_mention_into_composer(
                            reference.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            }))
            .children(matches.iter().enumerate().map(|(index, doc)| {
                let title = doc.title.clone();
                let relative = doc.relative_path.clone();
                let relative_label = relative.to_string_lossy().to_string();
                let is_linked = linked_docs.contains(&relative);
                let is_active = design_count + index == selected;
                let mention = mention.clone();
                h_flex()
                    .id(("composer-doc-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx).opacity(0.72)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .child(
                        gpui_component::Icon::new(if is_linked {
                            IconName::Check
                        } else {
                            crate::ui::design::docs_icon()
                        })
                        .size(crate::ui::design::icon_md())
                        .text_color(if is_linked {
                            crate::ui::design::accent(cx)
                        } else if is_active {
                            crate::ui::design::amber(cx)
                        } else {
                            crate::ui::design::t3(cx)
                        }),
                    )
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(relative_label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::base(cx).opacity(0.42))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Doc"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_doc_mention_into_composer(
                            relative.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            }))
            .into_any_element()
    }

    pub(super) fn render_composer_file_mention_picker(
        &self,
        view: &ComposerFileMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let ComposerFileMentionView {
            mention,
            matches,
            selected,
            loading,
        } = view;
        let selected = *selected;

        v_flex()
            .id("composer-file-mention-picker")
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(matches.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .px_2()
                        .gap_1p5()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(if *loading {
                            gpui_component::spinner::Spinner::new()
                                .xsmall()
                                .into_any_element()
                        } else {
                            gpui_component::Icon::new(IconName::File)
                                .size(crate::ui::design::icon_md())
                                .into_any_element()
                        })
                        .child(if *loading {
                            "Indexing project files…".to_string()
                        } else if mention.query.is_empty() {
                            "No files in this project".to_string()
                        } else {
                            format!("No files matching {}", mention.query)
                        }),
                )
            })
            .children(matches.iter().enumerate().map(|(index, file)| {
                let file_for_click = file.clone();
                let mention = mention.clone();
                let is_active = index == selected;
                h_flex()
                    .id(("composer-file-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx).opacity(0.72)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .child(
                        gpui_component::Icon::new(IconName::File)
                            .size(crate::ui::design::icon_md())
                            .text_color(if is_active {
                                crate::ui::design::sage(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            }),
                    )
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(file.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(file.relative_label.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::base(cx).opacity(0.42))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("File"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_file_mention_into_composer(
                            file_for_click.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            }))
            .into_any_element()
    }
}

fn collect_composer_file_entries(root: &Path) -> Vec<ComposerFileEntry> {
    let ignored_dirs = [
        ".git",
        "target",
        "node_modules",
        ".next",
        "dist",
        "build",
        ".choro_agent_attachments",
        ".my_ide_agent_attachments",
        DOCS_DIR_NAME,
    ];
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for item in read_dir.flatten() {
            if entries.len() >= COMPOSER_FILE_CACHE_LIMIT {
                break;
            }
            let path = item.path();
            let Ok(file_type) = item.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let should_skip = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| ignored_dirs.contains(&name));
                if !should_skip {
                    stack.push(path);
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative_path = relative.to_path_buf();
            let relative_label = relative_path.to_string_lossy().to_string();
            let name = relative_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(relative_label.as_str())
                .to_string();
            entries.push(ComposerFileEntry {
                relative_path,
                absolute_path: path,
                relative_label,
                name,
            });
        }
        if entries.len() >= COMPOSER_FILE_CACHE_LIMIT {
            break;
        }
    }
    entries.sort_by(|left, right| {
        left.relative_label
            .to_ascii_lowercase()
            .cmp(&right.relative_label.to_ascii_lowercase())
    });
    entries
}
