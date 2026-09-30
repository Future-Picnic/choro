use gpui::{
    actions, div, img, prelude::FluentBuilder, px, App, AppContext, Context, Entity, ExternalPaths,
    Focusable, FontWeight, ImageFormat, InteractiveElement, IntoElement, KeyBinding, ObjectFit,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window,
};
use gpui_component::{
    h_flex,
    input::{Enter, InputEvent, InputState, Paste},
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex, Disableable, Icon, IconName, Sizable, WindowExt,
};
use ide_core::config::GenerationAgent;
use ide_core::local_store::StoredQuickAskExchange;
use ide_core::{AgentKind, AgentModel};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::state::{QuickAskEvent, QuickAskPhase, QuickAskScope, QuickAskState, Workspace};
use crate::ui::center::attachment_helpers::{
    clipboard_image_from_item, image_format_for_path, materialize_quick_ask_clipboard_image,
};
use crate::ui::center::{provider_brand_icon, CenterArea};

actions!(quick_ask, [QuickAskSubmit]);

const CONTEXT: &str = "QuickAsk";

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("enter", QuickAskSubmit, Some("QuickAsk > Input")),
        KeyBinding::new(
            "shift-enter",
            Enter { secondary: false },
            Some("QuickAsk > Input"),
        ),
    ]
}

pub struct QuickAskModal {
    workspace: Entity<Workspace>,
    quick_ask: Entity<QuickAskState>,
    center: Entity<CenterArea>,
    question: Entity<InputState>,
    attached_images: Vec<PathBuf>,
    attachment_pastes_pending: usize,
    composer_error: Option<String>,
    pending_started_at: Option<u64>,
}

impl QuickAskModal {
    pub fn open(
        workspace: Entity<Workspace>,
        quick_ask: Entity<QuickAskState>,
        center: Entity<CenterArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        quick_ask.update(cx, |state, cx| state.begin_session(cx));
        Self::open_prepared(workspace, quick_ask, center, window, cx);
    }

    pub fn open_continuation(
        session_id: Uuid,
        workspace: Entity<Workspace>,
        quick_ask: Entity<QuickAskState>,
        center: Entity<CenterArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let restored =
            quick_ask.update(cx, |state, cx| state.continue_conversation(session_id, cx));
        if !restored {
            return;
        }
        Self::open_prepared(workspace, quick_ask, center, window, cx);
    }

    fn open_prepared(
        workspace: Entity<Workspace>,
        quick_ask: Entity<QuickAskState>,
        center: Entity<CenterArea>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let question = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 5)
                .placeholder("Ask anything about this project…")
        });
        let observed_center = center.clone();
        let modal = cx.new(|cx| {
            let modal = Self {
                workspace,
                quick_ask: quick_ask.clone(),
                center,
                question: question.clone(),
                attached_images: Vec::new(),
                attachment_pastes_pending: 0,
                composer_error: None,
                pending_started_at: None,
            };
            cx.observe(&quick_ask, |_, _, cx| cx.notify()).detach();
            // Message hover/copy state belongs to the canonical agent renderer
            // hosted by CenterArea. Mirror its notifications into the dialog.
            cx.observe(&observed_center, |_, _, cx| cx.notify())
                .detach();
            // This is the behavior the working agent composers rely on: the
            // owner must rerender as the native input changes so glyphs and the
            // auto-growing frame repaint together.
            cx.subscribe_in(&question, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.subscribe_in(
                &quick_ask,
                window,
                |this: &mut Self, _, event: &QuickAskEvent, window, cx| match event {
                    QuickAskEvent::Completed => {
                        this.pending_started_at = None;
                        this.question.focus_handle(cx).focus(window);
                    }
                    QuickAskEvent::Failed {
                        question,
                        attachments,
                    } => {
                        this.pending_started_at = None;
                        this.question.update(cx, |input, cx| {
                            // Clear-on-send matches agent chat. Restore the
                            // failed question only when the user has not
                            // already begun drafting the next one.
                            if input.value().trim().is_empty() {
                                input.set_value(question.clone(), window, cx);
                            }
                        });
                        if this.attached_images.is_empty() {
                            this.attached_images = attachments.clone();
                        }
                        this.question.focus_handle(cx).focus(window);
                        cx.notify();
                    }
                },
            )
            .detach();
            modal
        });
        let dialog_modal = modal.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(700.))
                .margin_top(px(64.))
                .overlay(true)
                .close_button(false)
                .keyboard(true)
                .p_0()
                .child(dialog_modal.clone())
        });
        // Focus after the dialog and its platform input handler have mounted.
        // This is especially important when opening from a mouse click in the
        // history workspace: the originating button must not reclaim focus at
        // the end of the same event cycle.
        let question_focus = question.focus_handle(cx);
        window.on_next_frame(move |window, _| {
            question_focus.focus(window);
        });
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let question = self.question.read(cx).value().trim().to_string();
        if (question.is_empty() && self.attached_images.is_empty())
            || self.attachment_pastes_pending > 0
            || self.quick_ask.read(cx).phase() == QuickAskPhase::Thinking
        {
            return;
        }
        let attachments = std::mem::take(&mut self.attached_images);
        self.composer_error = None;
        self.pending_started_at = Some(unix_now_secs());
        self.question
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.quick_ask
            .update(cx, |state, cx| state.submit(question, attachments, cx));
    }

    fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(image) = cx.read_from_clipboard().and_then(clipboard_image_from_item) else {
            return false;
        };
        if self.attached_images.len() + self.attachment_pastes_pending >= 5 {
            self.composer_error = Some("Attach at most 5 images.".into());
            cx.notify();
            return true;
        }
        if !matches!(
            image.format,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp
        ) {
            self.composer_error =
                Some("Quick Ask supports PNG, JPEG, GIF, and WebP images.".into());
            cx.notify();
            return true;
        }
        if image.bytes.len() > 10 * 1024 * 1024 {
            self.composer_error = Some("Each image must be 10 MB or smaller.".into());
            cx.notify();
            return true;
        }

        let session_id = self.quick_ask.read(cx).session_id();
        self.attachment_pastes_pending += 1;
        self.composer_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { materialize_quick_ask_clipboard_image(session_id, &image) })
                .await;
            this.update(cx, |this, cx| {
                this.attachment_pastes_pending = this.attachment_pastes_pending.saturating_sub(1);
                match result {
                    Ok(path) => {
                        this.attached_images.push(path);
                        this.composer_error = None;
                    }
                    Err(error) => {
                        this.composer_error = Some(format!("Could not attach image: {error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }

    fn attach_dropped_images(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut unsupported = false;
        for path in paths.iter().filter(|path| path.is_file()) {
            if !matches!(
                image_format_for_path(path),
                Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp)
            ) {
                unsupported = true;
                continue;
            }
            if path
                .metadata()
                .map(|metadata| metadata.len() > 10 * 1024 * 1024)
                .unwrap_or(true)
            {
                self.composer_error = Some("Each image must be 10 MB or smaller.".into());
                cx.notify();
                return;
            }
            if self.attached_images.len() >= 5 {
                self.composer_error = Some("Attach at most 5 images.".into());
                cx.notify();
                return;
            }
            if !self.attached_images.iter().any(|existing| existing == path) {
                self.attached_images.push(path.clone());
            }
        }
        self.composer_error =
            unsupported.then(|| "Quick Ask accepts image attachments only.".to_string());
        cx.notify();
    }

    fn start_agent_for_project(
        &mut self,
        exchange: StoredQuickAskExchange,
        project: ide_core::ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = format!(
            "Continue from this Quick Ask discussion and help with the project.\n\nQuestion:\n{}\n\nQuick Ask answer:\n{}\n\nTreat the answer as context, verify its claims against the repository, and ask before making a materially different change.",
            exchange.question, exchange.answer
        );
        window.close_dialog(cx);
        self.center.update(cx, |center, cx| {
            center.open_new_agent_with_prompt(project, prompt, window, cx)
        });
    }

    fn render_start_agent_action(
        &self,
        id: impl Into<gpui::ElementId>,
        exchange: StoredQuickAskExchange,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let view = cx.entity().clone();
        if let Some(project) = self.agent_project_for(&exchange, cx) {
            return crate::ui::style::dialog_neutral_button(id, "Start agent", cx)
                .icon(IconName::Bot)
                .tooltip("Open an unsent agent draft with this conversation")
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.start_agent_for_project(exchange.clone(), project, window, cx)
                    });
                })
                .into_any_element();
        }

        let projects = self.workspace.read(cx).projects.clone();
        if projects.is_empty() {
            return crate::ui::style::dialog_neutral_button(id, "Start agent", cx)
                .icon(IconName::Bot)
                .disabled(true)
                .tooltip("Add a project before starting an agent")
                .into_any_element();
        }

        crate::ui::style::dialog_neutral_button(id, "Start agent", cx)
            .icon(IconName::Bot)
            .dropdown_caret(true)
            .tooltip("Choose which project should receive this Quick Ask context")
            .dropdown_menu(move |mut menu, window, _| {
                for project in &projects {
                    let project_id = project.id;
                    let exchange = exchange.clone();
                    menu = menu.item(PopupMenuItem::new(project.name.clone()).on_click(
                        window.listener_for(&view, move |this: &mut Self, _, window, cx| {
                            this.start_agent_for_project(exchange.clone(), project_id, window, cx);
                        }),
                    ));
                }
                menu
            })
            .into_any_element()
    }

    fn agent_project_for(
        &self,
        exchange: &StoredQuickAskExchange,
        cx: &App,
    ) -> Option<ide_core::ProjectId> {
        let workspace = self.workspace.read(cx);
        exchange
            .project_id
            .filter(|project_id| {
                workspace
                    .projects
                    .iter()
                    .any(|project| project.id == *project_id)
            })
            .or(workspace.active)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let start_agent = self
            .quick_ask
            .read(cx)
            .session()
            .last()
            .cloned()
            .map(|exchange| self.render_start_agent_action("quick-ask-start-agent", exchange, cx));
        h_flex()
            .h(px(60.))
            .w_full()
            .px_4()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.6))
            .child(
                div()
                    .size(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(crate::ui::design::r_md())
                    .bg(crate::ui::design::accent_soft(cx))
                    .child(
                        Icon::new(IconName::Asterisk)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::accent(cx)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_head())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Quick Ask"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Fast answers without starting an agent"),
                    ),
            )
            .children(start_agent)
            .child(
                crate::ui::style::header_icon_button(
                    "quick-ask-open-history",
                    IconName::BookOpen,
                    cx,
                )
                .tooltip("Open Ask History")
                .on_click(cx.listener(|this, _, window, cx| {
                    window.close_dialog(cx);
                    this.center
                        .update(cx, |center, cx| center.show_quick_ask_history(cx));
                })),
            )
            .child(
                crate::ui::style::header_icon_button("quick-ask-close", IconName::Close, cx)
                    .tooltip("Close Quick Ask")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    }

    fn render_scope_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.quick_ask.read(cx);
        let selected = state.scope();
        let projects = self.workspace.read(cx).projects.clone();
        let label = match selected {
            QuickAskScope::General => "General".to_string(),
            QuickAskScope::Project(project_id) => projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "General".to_string()),
        };
        let view = cx.entity().clone();
        crate::ui::style::composer_chip(
            "quick-ask-scope",
            SharedString::from(label),
            Some(
                Icon::new(if selected == QuickAskScope::General {
                    IconName::Globe
                } else {
                    IconName::FolderOpen
                })
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t3(cx))
                .into_any_element(),
            ),
            cx,
        )
        .tooltip("Question scope")
        .dropdown_menu(move |mut menu, window, _| {
            menu = menu.item(
                PopupMenuItem::new("General")
                    .checked(selected == QuickAskScope::General)
                    .on_click(window.listener_for(&view, |this: &mut Self, _, _, cx| {
                        this.quick_ask
                            .update(cx, |state, cx| state.set_scope(QuickAskScope::General, cx));
                    })),
            );
            if !projects.is_empty() {
                menu = menu.item(PopupMenuItem::separator());
            }
            for project in &projects {
                let project_id = project.id;
                menu = menu.item(
                    PopupMenuItem::new(project.name.clone())
                        .checked(selected == QuickAskScope::Project(project_id))
                        .on_click(
                            window.listener_for(&view, move |this: &mut Self, _, _, cx| {
                                this.quick_ask.update(cx, |state, cx| {
                                    state.set_scope(QuickAskScope::Project(project_id), cx)
                                });
                            }),
                        ),
                );
            }
            menu
        })
    }

    fn render_model_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.quick_ask.read(cx).agent().clone();
        let label = selected.model_label().to_string();
        let provider = selected.provider;
        let view = cx.entity().clone();
        crate::ui::style::composer_chip(
            "quick-ask-model",
            SharedString::from(label.clone()),
            Some(
                provider_brand_icon(provider)
                    .size(crate::ui::design::icon_sm())
                    .into_any_element(),
            ),
            cx,
        )
        .tooltip(format!("{} · {}", provider.label(), label))
        .dropdown_menu(move |mut menu, window, _| {
            for (provider_index, candidate_provider) in
                [AgentKind::Codex, AgentKind::Claude, AgentKind::OpenCode]
                    .into_iter()
                    .enumerate()
            {
                if provider_index > 0 {
                    menu = menu.item(PopupMenuItem::separator());
                }
                for model in AgentModel::models_for(candidate_provider) {
                    let candidate = if candidate_provider == AgentKind::OpenCode {
                        GenerationAgent::for_provider(AgentKind::OpenCode)
                    } else {
                        GenerationAgent {
                            provider: candidate_provider,
                            model: *model,
                            external_model_id: None,
                            external_model_label: None,
                        }
                    };
                    let checked = selected == candidate;
                    let item_label = format!(
                        "{} · {}",
                        candidate_provider.label(),
                        candidate.model_label()
                    );
                    menu = menu.item(PopupMenuItem::new(item_label).checked(checked).on_click(
                        window.listener_for(&view, move |this: &mut Self, _, _, cx| {
                            this.quick_ask
                                .update(cx, |state, cx| state.set_agent(candidate.clone(), cx));
                        }),
                    ));
                }
            }
            menu
        })
    }

    fn render_session_exchange(
        &self,
        exchange: StoredQuickAskExchange,
        turn_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.center.update(cx, |center, cx| {
            center.render_quick_ask_exchange_as_agent(&exchange, turn_index, window, cx)
        })
    }

    fn render_pending_question(
        &self,
        question: String,
        message_index: usize,
        created_at: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (session_id, project_id, provider, model_label) = {
            let state = self.quick_ask.read(cx);
            let project_id = match state.scope() {
                QuickAskScope::General => None,
                QuickAskScope::Project(project_id) => Some(project_id),
            };
            (
                state.session_id(),
                project_id,
                state.agent().provider,
                state.agent().model_label().to_string(),
            )
        };

        self.center.update(cx, |center, cx| {
            center.render_quick_ask_pending_as_agent(
                session_id,
                project_id,
                provider,
                &model_label,
                question,
                message_index,
                created_at,
                window,
                cx,
            )
        })
    }

    fn render_thinking(&self, started_at: u64, cx: &mut Context<Self>) -> gpui::AnyElement {
        let session_id = self.quick_ask.read(cx).session_id();
        self.center.update(cx, |center, cx| {
            center.render_quick_ask_thinking_as_agent(session_id, started_at, cx)
        })
    }

    fn render_error(&self, error: String, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w_full()
            .px(crate::ui::design::agent_chat_gutter_x())
            .pb_2()
            .child(
                div()
                    .w_full()
                    .max_w(crate::ui::design::agent_chat_content_max_w())
                    .mx_auto()
                    .child(crate::ui::style::agent_attention_strip(
                        "copy-quick-ask-error",
                        error,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn render_attachment_preview(
        &self,
        index: usize,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Attached image")
            .to_string();
        let remove_path = path.clone();
        let view = cx.entity().clone();
        div()
            .id(("quick-ask-attachment", index))
            .relative()
            .size(px(58.))
            .flex_none()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::surface(cx))
            .overflow_hidden()
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .child(
                img(path)
                    .size_full()
                    .rounded(crate::ui::design::r_lg())
                    .object_fit(ObjectFit::Cover),
            )
            .child(
                div().absolute().top_1().right_1().child(
                    crate::ui::style::attachment_remove_button(
                        ("remove-quick-ask-attachment", index),
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.attached_images
                                .retain(|candidate| candidate != &remove_path);
                            this.composer_error = None;
                            cx.notify();
                        });
                    }),
                ),
            )
            .into_any_element()
    }

    fn render_attachment_pending(&self, index: usize, cx: &App) -> gpui::AnyElement {
        h_flex()
            .id(("quick-ask-attachment-pending", index))
            .h(px(34.))
            .flex_none()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::sage(cx).opacity(0.28))
            .bg(crate::ui::design::sage(cx).opacity(0.10))
            .px_2()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t2(cx))
            .child(Spinner::new().xsmall())
            .child("Attaching image…")
            .into_any_element()
    }

    fn render_ask(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.quick_ask.read(cx);
        let session = state.session().to_vec();
        let pending_question = state.pending_question().map(str::to_string);
        let phase = state.phase();
        let error = state.error().map(str::to_string);
        let view = cx.entity().clone();
        let attached_images = self.attached_images.clone();
        let attachment_pastes_pending = self.attachment_pastes_pending;
        let composer_error = self.composer_error.clone();
        let has_draft = !self.question.read(cx).value().trim().is_empty();
        let can_send = phase != QuickAskPhase::Thinking
            && attachment_pastes_pending == 0
            && (has_draft || !attached_images.is_empty());
        let session_len = session.len();
        let session_empty = session.is_empty();
        let transcript = session
            .into_iter()
            .enumerate()
            .map(|(index, exchange)| self.render_session_exchange(exchange, index, window, cx))
            .collect::<Vec<_>>();
        let pending_created_at = self.pending_started_at.unwrap_or_else(unix_now_secs);
        let pending_message = pending_question.map(|question| {
            self.render_pending_question(
                question,
                session_len.saturating_mul(2),
                pending_created_at,
                window,
                cx,
            )
        });
        let thinking = (phase == QuickAskPhase::Thinking)
            .then(|| self.render_thinking(pending_created_at, cx));
        let error_notice = error.map(|error| self.render_error(error, cx));
        v_flex()
            .w_full()
            .child(
                v_flex()
                    .id("quick-ask-session-scroll")
                    .w_full()
                    .h(px(360.))
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .py_5()
                    .when(session_empty && phase == QuickAskPhase::Idle, |body| {
                        body.child(
                            v_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .px_5()
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    div()
                                        .size(px(58.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(crate::ui::design::r_lg())
                                        .bg(crate::ui::design::accent_soft(cx))
                                        .child(
                                            Icon::new(IconName::Asterisk)
                                                .size(crate::ui::design::icon_xl())
                                                .text_color(crate::ui::design::accent(cx)),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_title())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(crate::ui::design::t1(cx))
                                                .child("Ask without starting an agent"),
                                        )
                                        .child(
                                            div()
                                                .max_w(px(430.))
                                                .text_center()
                                                .line_height(gpui::relative(1.45))
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("Use project scope for repository-grounded answers, or switch to General for anything else."),
                                        ),
                                ),
                        )
                    })
                    .children(transcript)
                    .children(pending_message)
                    .children(thinking)
                    .children(error_notice),
            )
            .child(
                v_flex()
                    .w_full()
                    .px_4()
                    .pb_4()
                    .child(
                        crate::ui::style::composer_frame(cx)
                            .capture_action(cx.listener(|this, _: &Paste, _, cx| {
                                if this.paste_image(cx) {
                                    cx.stop_propagation();
                                }
                            }))
                            .can_drop(|dragged, _, _| dragged.is::<ExternalPaths>())
                            .on_drop::<ExternalPaths>(cx.listener(
                                |this, paths: &ExternalPaths, _, cx| {
                                    this.attach_dropped_images(paths.paths(), cx);
                                },
                            ))
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .min_h(crate::ui::design::composer_input_min_h())
                                    .gap_2()
                                    .when(
                                        !attached_images.is_empty()
                                            || attachment_pastes_pending > 0,
                                        |column| {
                                            column.child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .flex_wrap()
                                                    .children(attached_images.iter().enumerate().map(
                                                        |(index, path)| {
                                                            self.render_attachment_preview(
                                                                index,
                                                                path.clone(),
                                                                cx,
                                                            )
                                                        },
                                                    ))
                                                    .children(
                                                        (0..attachment_pastes_pending).map(|index| {
                                                            self.render_attachment_pending(index, cx)
                                                        }),
                                                    ),
                                            )
                                        },
                                    )
                                    .child(
                                        div().flex_1().min_h(px(0.)).child(
                                            crate::ui::style::composer_text_input(&self.question)
                                                .h_full(),
                                        ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .items_center()
                                    .child(self.render_scope_control(cx))
                                    .child(crate::ui::style::composer_control_divider(cx))
                                    .child(self.render_model_control(cx))
                                    .child(div().flex_1())
                                    .child({
                                        let button = crate::ui::style::composer_send(
                                            "quick-ask-submit-button",
                                            cx,
                                        )
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(if phase == QuickAskPhase::Thinking {
                                                "Quick Ask is thinking…"
                                            } else if attachment_pastes_pending > 0 {
                                                "Wait for the image to finish attaching"
                                            } else {
                                                "Ask"
                                            })
                                            .build(window, cx)
                                        });
                                        if can_send {
                                            button
                                                .cursor_pointer()
                                                .hover(|button| {
                                                    button.bg(crate::ui::design::accent_2(cx))
                                                })
                                                .on_click(move |_, window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        this.submit(window, cx)
                                                    });
                                                })
                                        } else {
                                            button.opacity(0.55)
                                        }
                                    }),
                            )
                            .when_some(composer_error, |composer, error| {
                                composer.child(
                                    div()
                                        .pt_1()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(error),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for QuickAskModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &QuickAskSubmit, window, cx| this.submit(window, cx)))
            .w_full()
            .overflow_hidden()
            .rounded(crate::ui::design::r_lg())
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .child(self.render_header(cx))
            .child(self.render_ask(window, cx))
    }
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
