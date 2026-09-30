use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::*;

const DIRECTIVE_PREFIX: &str = "::codex-inline-vis{file=\"";
const DIRECTIVE_SUFFIX: &str = "\"}";
const ACTIVE_VISUALIZATION_HEIGHT: f32 = 380.0;
const MAX_VISUALIZATION_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ChatVisualizationKey {
    pub agent_id: Uuid,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct ChatVisualizationContext {
    pub agent_id: Uuid,
    pub project_path: PathBuf,
    pub artifacts_path: Option<PathBuf>,
}

#[derive(Clone)]
pub(super) struct ChatVisualizationRenderContext {
    pub files: ChatVisualizationContext,
    pub active: Option<ChatVisualizationKey>,
    pub web_host: Entity<web_preview::WebPreviewHost>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChatVisualizationSyncStamp {
    message_count: usize,
    last_assistant: Option<(Option<String>, u64, usize)>,
}

impl ChatVisualizationSyncStamp {
    fn from_session(session: &AgentChatSession) -> Self {
        let last_assistant = session
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                AgentChatMessage::Assistant {
                    message_id,
                    text,
                    created_at,
                } => Some((message_id.clone(), *created_at, text.len())),
                _ => None,
            });
        Self {
            message_count: session.messages.len(),
            last_assistant,
        }
    }
}

impl ChatVisualizationContext {
    pub fn for_agent(agent: &AgentRecord) -> Self {
        let artifacts_path = AppConfig::config_path().parent().map(|root| {
            root.join("data")
                .join("agents")
                .join(agent.id.to_string())
                .join("artifacts")
        });
        Self {
            agent_id: agent.id,
            project_path: agent.runtime_path().to_path_buf(),
            artifacts_path,
        }
    }
}

pub(super) fn parse_visualization_directive_line(line: &str) -> Option<String> {
    let line = line.trim();
    let path = line
        .strip_prefix(DIRECTIVE_PREFIX)?
        .strip_suffix(DIRECTIVE_SUFFIX)?;
    if path.is_empty() || path.contains(['\n', '\r', '\0']) || path.contains('"') {
        return None;
    }
    Some(path.to_string())
}

pub(super) fn visualization_directives(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            if let Some(path) = parse_visualization_directive_line(line) {
                paths.push(path);
            }
        }
    }
    paths
}

pub(super) fn resolve_visualization_file(
    context: &ChatVisualizationContext,
    directive_path: &str,
) -> Result<PathBuf, String> {
    let requested = Path::new(directive_path);
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        context.project_path.join(requested)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "The visualization file is no longer available.".to_string())?;
    if !canonical.is_file() {
        return Err("The visualization file is no longer available.".to_string());
    }
    if !canonical
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
    {
        return Err("Only HTML visualization files can be previewed.".to_string());
    }

    let project_root = context.project_path.canonicalize().ok();
    let artifacts_root = context
        .artifacts_path
        .as_ref()
        .and_then(|path| path.canonicalize().ok());
    let allowed = project_root
        .as_ref()
        .is_some_and(|root| canonical.starts_with(root))
        || artifacts_root
            .as_ref()
            .is_some_and(|root| canonical.starts_with(root));
    if !allowed {
        return Err("The visualization is outside this agent's allowed files.".to_string());
    }

    let size = canonical
        .metadata()
        .map_err(|_| "The visualization file is no longer available.".to_string())?
        .len();
    if size > MAX_VISUALIZATION_BYTES {
        return Err("The visualization is larger than the 2 MB preview limit.".to_string());
    }
    Ok(canonical)
}

impl CenterArea {
    pub(super) fn sync_agent_chat_visualizations(
        &mut self,
        agent: &AgentRecord,
        session: &AgentChatSession,
    ) {
        if matches!(
            session.status,
            AgentChatStatus::Running | AgentChatStatus::Cancelling
        ) {
            return;
        }

        let stamp = ChatVisualizationSyncStamp::from_session(session);
        if self.agent_chat_visualization_sync_stamps.get(&agent.id) == Some(&stamp) {
            return;
        }
        self.agent_chat_visualization_sync_stamps
            .insert(agent.id, stamp);

        let context = ChatVisualizationContext::for_agent(agent);
        let keys = session
            .messages
            .iter()
            .filter_map(|message| match message {
                AgentChatMessage::Assistant { text, .. } => Some(text),
                _ => None,
            })
            .flat_map(|text| visualization_directives(text))
            .filter_map(|path| resolve_visualization_file(&context, &path).ok())
            .map(|path| ChatVisualizationKey {
                agent_id: agent.id,
                path,
            })
            .collect::<Vec<_>>();

        let newest_unseen = keys
            .iter()
            .rev()
            .find(|key| !self.auto_loaded_chat_visualizations.contains(*key))
            .cloned();
        self.auto_loaded_chat_visualizations.extend(keys);

        if let Some(key) = newest_unseen {
            let previous_agent = self
                .active_chat_visualization
                .as_ref()
                .map(|active| active.agent_id);
            self.active_chat_visualization = Some(key);
            if let Some(previous_agent) = previous_agent {
                self.remeasure_agent_chat_list(previous_agent);
            }
            self.remeasure_agent_chat_list(agent.id);
        }
    }

    pub(super) fn active_chat_visualization_intent(
        &self,
        selected_agent: &AgentRecord,
        cx: &App,
    ) -> Option<web_preview::WebPreviewIntent> {
        let active = self.active_chat_visualization.as_ref()?;
        if active.agent_id != selected_agent.id {
            return None;
        }
        Some(web_preview::WebPreviewIntent::Visualization {
            key: visualization_key_id(active),
            path: active.path.clone(),
            theme: web_preview::VisualizationTheme::from_app(cx),
        })
    }

    pub(super) fn chat_visualization_render_context(
        &self,
        agent: &AgentRecord,
    ) -> ChatVisualizationRenderContext {
        ChatVisualizationRenderContext {
            files: ChatVisualizationContext::for_agent(agent),
            active: self.active_chat_visualization.clone(),
            web_host: self.web_host.clone(),
        }
    }
}

pub(super) fn render_chat_visualization_card(
    context: &ChatVisualizationRenderContext,
    directive_path: &str,
    element_id: u64,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let resolved = resolve_visualization_file(&context.files, directive_path);
    let title = visualization_title(
        resolved
            .as_deref()
            .unwrap_or_else(|_| Path::new(directive_path)),
    );

    let Ok(path) = resolved else {
        let error = resolved.expect_err("checked as error");
        return visualization_card_shell(cx)
            .child(
                h_flex()
                    .gap_2p5()
                    .items_center()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::rose(cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(error),
                            ),
                    ),
            )
            .into_any_element();
    };

    let key = ChatVisualizationKey {
        agent_id: context.files.agent_id,
        path,
    };
    let is_active = context.active.as_ref() == Some(&key);

    if is_active {
        let host = context.web_host.clone();
        let copy_host = context.web_host.clone();
        visualization_card_shell(cx)
            .p_0()
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(48.))
                    .px_3p5()
                    .gap_2p5()
                    .items_center()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx))
                    .child(
                        div()
                            .size(px(28.))
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::accent_soft(cx))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::Frame)
                                    .size(crate::ui::design::icon())
                                    .text_color(crate::ui::design::accent(cx)),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Interactive visualization"),
                            ),
                    )
                    .child(
                        crate::ui::style::ghost_button_compact(
                            ("copy-chat-visualization", element_id),
                            "Copy image",
                        )
                        .icon(IconName::Copy)
                        .tooltip("Copy the current visualization as an image")
                        .on_click(move |_, _, cx| {
                            copy_host.update(cx, |host, _| {
                                if let Err(error) = host.copy_active_visualization_image() {
                                    eprintln!("failed to copy visualization image: {error}");
                                }
                            });
                        }),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(crate::ui::design::accent_soft(cx))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::accent(cx))
                            .child("Live"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(ACTIVE_VISUALIZATION_HEIGHT))
                    .bg(crate::ui::design::base(cx))
                    .child(
                        v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .text_color(crate::ui::design::t3(cx))
                            .child(logo_spinner(
                                18.,
                                "chat-visualization",
                                element_id as usize,
                                crate::ui::design::t3(cx),
                            ))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .child("Loading visualization…"),
                            ),
                    )
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                host.update(cx, |host, _| host.place(bounds, window));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .into_any_element()
    } else {
        let activate_key = key.clone();
        visualization_card_shell(cx)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(px(34.))
                            .rounded(crate::ui::design::r_md())
                            .bg(crate::ui::design::accent_soft(cx))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::Frame)
                                    .size(crate::ui::design::icon())
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
                                    .truncate()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "Paused — Choro keeps one visualization live at a time.",
                                    ),
                            ),
                    )
                    .child(
                        crate::ui::style::refresh_button(
                            ("reload-chat-visualization", element_id),
                            "Load again",
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let previous_agent = this
                                .active_chat_visualization
                                .as_ref()
                                .map(|active| active.agent_id);
                            this.active_chat_visualization = Some(activate_key.clone());
                            if let Some(previous_agent) = previous_agent {
                                this.remeasure_agent_chat_list(previous_agent);
                            }
                            this.remeasure_agent_chat_list(activate_key.agent_id);
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }
}

fn visualization_card_shell(cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w(px(0.))
        .p_3p5()
        .rounded(crate::ui::design::r_lg())
        .border_1()
        .border_color(crate::ui::design::line(cx))
        .bg(crate::ui::design::surface(cx))
}

fn visualization_title(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Visualization");
    let words = raw.replace(['-', '_'], " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Visualization".to_string(),
    }
}

fn visualization_key_id(key: &ChatVisualizationKey) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("chat-visualization-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        parse_visualization_directive_line, resolve_visualization_file, visualization_directives,
        ChatVisualizationContext,
    };

    #[test]
    fn parses_exact_directive() {
        assert_eq!(
            parse_visualization_directive_line(
                "::codex-inline-vis{file=\"palette-comparison.html\"}"
            ),
            Some("palette-comparison.html".to_string())
        );
    }

    #[test]
    fn ignores_directive_inside_code_fence() {
        let text = "```text\n::codex-inline-vis{file=\"fake.html\"}\n```\n\
                    ::codex-inline-vis{file=\"real.html\"}";
        assert_eq!(visualization_directives(text), vec!["real.html"]);
    }

    #[test]
    fn rejects_extra_text() {
        assert!(parse_visualization_directive_line(
            "::codex-inline-vis{file=\"chart.html\"} trailing"
        )
        .is_none());
    }

    #[test]
    fn resolves_html_inside_project() {
        let project = tempfile::tempdir().expect("project tempdir");
        fs::write(project.path().join("chart.html"), "<div>Chart</div>")
            .expect("write visualization");
        let context = ChatVisualizationContext {
            agent_id: uuid::Uuid::nil(),
            project_path: project.path().to_path_buf(),
            artifacts_path: None,
        };
        assert_eq!(
            resolve_visualization_file(&context, "chart.html").expect("resolve visualization"),
            project.path().join("chart.html").canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_file_outside_allowed_roots() {
        let project = tempfile::tempdir().expect("project tempdir");
        let outside = tempfile::NamedTempFile::with_suffix(".html").expect("outside file");
        let context = ChatVisualizationContext {
            agent_id: uuid::Uuid::nil(),
            project_path: project.path().to_path_buf(),
            artifacts_path: None,
        };
        assert_eq!(
            resolve_visualization_file(&context, outside.path().to_str().unwrap()),
            Err("The visualization is outside this agent's allowed files.".to_string())
        );
    }
}
