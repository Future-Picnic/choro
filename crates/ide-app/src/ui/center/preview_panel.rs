use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SoloPreviewOwner {
    agent_id: Uuid,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreviewScope {
    Project,
    Solo(SoloPreviewOwner),
    Simulator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewChoice {
    label: String,
    url: String,
    revision: u64,
    running: bool,
    scope: PreviewScope,
}

impl PreviewChoice {
    fn is_simulator(&self) -> bool {
        self.scope == PreviewScope::Simulator
    }

    fn solo_owner(&self) -> Option<&SoloPreviewOwner> {
        match &self.scope {
            PreviewScope::Solo(owner) => Some(owner),
            PreviewScope::Project | PreviewScope::Simulator => None,
        }
    }

    fn is_project(&self) -> bool {
        self.scope == PreviewScope::Project
    }
}

fn preview_choice_matches_agent(choice: &PreviewChoice, selected_agent_id: Option<Uuid>) -> bool {
    choice
        .solo_owner()
        .is_none_or(|owner| selected_agent_id == Some(owner.agent_id))
}

fn explicit_preview_choice(
    choices: &[PreviewChoice],
    selected_url: Option<&str>,
) -> Option<PreviewChoice> {
    selected_url
        .and_then(|selected| choices.iter().find(|choice| choice.url == selected))
        .cloned()
}

fn solo_preview_owner(agent: &ide_core::AgentRecord) -> Option<SoloPreviewOwner> {
    agent.is_active_solo().then_some(())?;
    agent.lane_path.as_ref()?;
    let branch = agent.solo_branch.as_deref()?;
    Some(SoloPreviewOwner {
        agent_id: agent.id,
        name: branch.trim_start_matches("solo/").to_string(),
    })
}

fn no_preview_status(agent: Option<&ide_core::AgentRecord>) -> String {
    let label = agent
        .map(|agent| agent.title.as_str())
        .unwrap_or("this agent");
    format!("No preview available for {label}")
}

fn preview_label(url: &str) -> String {
    if let Ok(url) = url::Url::parse(url) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "Static HTML".to_string());
        }
    }
    let without_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    without_scheme
        .split('/')
        .next()
        .filter(|label| !label.is_empty())
        .unwrap_or("Preview")
        .to_string()
}

fn normalize_project_preview_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Enter a URL to open in Preview".to_string());
    }
    let candidate = if value.contains("://") || value.starts_with("file:") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let parsed = url::Url::parse(&candidate)
        .map_err(|_| "Enter a valid http, https, or file URL".to_string())?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(parsed.to_string()),
        "file" if parsed.to_file_path().is_ok() => Ok(parsed.to_string()),
        "http" | "https" => Err("The Preview URL needs a host".to_string()),
        _ => Err("Preview supports http, https, and file URLs".to_string()),
    }
}

type PreviewServerOrigin = (String, String, Option<u16>);

fn preview_server_origin(value: &str) -> Option<PreviewServerOrigin> {
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let raw_host = parsed.host_str()?.trim_matches(['[', ']']);
    let host = if raw_host.eq_ignore_ascii_case("localhost")
        || raw_host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
    {
        "loopback".to_string()
    } else {
        raw_host.to_ascii_lowercase()
    };
    Some((
        parsed.scheme().to_ascii_lowercase(),
        host,
        parsed.port_or_known_default(),
    ))
}

fn stored_preview_is_available(value: &str, active_origins: &HashSet<PreviewServerOrigin>) -> bool {
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    match parsed.scheme() {
        "file" => true,
        "http" | "https" => {
            preview_server_origin(value).is_some_and(|origin| active_origins.contains(&origin))
        }
        _ => false,
    }
}

fn same_preview_server(first: &str, second: &str) -> bool {
    preview_server_origin(first)
        .zip(preview_server_origin(second))
        .is_some_and(|(first, second)| first == second)
}

fn take_latest_preview_scope(
    scope: &PreviewScope,
    project_taken: &mut bool,
    solo_taken: &mut HashSet<Uuid>,
) -> bool {
    match scope {
        PreviewScope::Project => {
            if *project_taken {
                false
            } else {
                *project_taken = true;
                true
            }
        }
        PreviewScope::Solo(owner) => solo_taken.insert(owner.agent_id),
        PreviewScope::Simulator => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewControlSecurityPolicy {
    allow_loopback: bool,
    file_root: Option<String>,
}

fn preview_control_loopback_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
}

fn preview_control_security_policy(
    configured_url: &str,
    project_root: &Path,
) -> Option<PreviewControlSecurityPolicy> {
    let configured = url::Url::parse(configured_url).ok()?;
    if preview_control_loopback_url(&configured) {
        return Some(PreviewControlSecurityPolicy {
            allow_loopback: true,
            file_root: None,
        });
    }
    if configured.scheme() != "file" {
        return None;
    }

    let configured_path = configured.to_file_path().ok()?.canonicalize().ok()?;
    let allowed_root = project_root.canonicalize().ok()?;
    if !configured_path.starts_with(&allowed_root) {
        return None;
    }
    let file_root = url::Url::from_directory_path(&allowed_root)
        .ok()?
        .to_string();
    Some(PreviewControlSecurityPolicy {
        allow_loopback: false,
        file_root: Some(file_root),
    })
}

fn preview_control_policy_allows_url(
    policy: &PreviewControlSecurityPolicy,
    live_url: &str,
) -> bool {
    let Ok(live) = url::Url::parse(live_url) else {
        return false;
    };
    if policy.allow_loopback && preview_control_loopback_url(&live) {
        return true;
    }
    if live.scheme() != "file" {
        return false;
    }
    let Some(root) = policy
        .file_root
        .as_deref()
        .and_then(|root| url::Url::parse(root).ok())
        .and_then(|root| root.to_file_path().ok())
        .and_then(|root| root.canonicalize().ok())
    else {
        return false;
    };
    live.to_file_path()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|path| path.starts_with(root))
}

/// A preview choice's display label, kept short enough for the picker menu:
/// Solo entries drop the branch's trailing id hash in favor of the compact
/// slug, and everything hard-caps with an ellipsis so one long service title
/// can't stretch the dropdown across the window.
fn preview_choice_display_label(label: &str) -> String {
    const SLUG_MAX: usize = 30;
    const TOTAL_MAX: usize = 44;
    let display = match crate::state::terminals::solo_script_slug(label) {
        Some(slug) => {
            let preset = label
                .split_once(crate::state::terminals::TerminalManager::SOLO_SCRIPT_MARKER)
                .map(|(preset, _)| preset)
                .unwrap_or(label);
            // The branch slug carries an id suffix for uniqueness; the picker
            // doesn't need it — the slug words identify the Solo.
            let slug = match slug.rsplit_once('-') {
                Some((body, suffix))
                    if suffix.len() >= 8 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()) =>
                {
                    body
                }
                _ => slug,
            };
            format!(
                "{preset} — solo {}",
                crate::ui::style::solo_slug_short(slug, SLUG_MAX)
            )
        }
        None => label.to_string(),
    };
    if display.chars().count() > TOTAL_MAX {
        let cut: String = display.chars().take(TOTAL_MAX.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        display
    }
}

fn solo_preview_display_name(owner: &SoloPreviewOwner) -> String {
    crate::ui::style::solo_slug_short(&owner.name, 36)
}

fn solo_preview_service_label(label: &str) -> String {
    label
        .split_once(crate::state::terminals::TerminalManager::SOLO_SCRIPT_MARKER)
        .map(|(service, _)| service)
        .unwrap_or(label)
        .trim()
        .to_string()
}

fn solo_preview_menu_row(
    owner: &SoloPreviewOwner,
    service_label: &str,
    running: bool,
    cx: &App,
) -> gpui::AnyElement {
    let solo_name = solo_preview_display_name(owner);
    let service_label = solo_preview_service_label(service_label);
    h_flex()
        .w_full()
        .min_w(px(0.))
        .items_center()
        .gap_1p5()
        .child(crate::ui::design::indicator::solo_icon(
            crate::ui::design::sky(cx),
            crate::ui::design::icon_sm(),
        ))
        .child(
            div()
                .min_w(px(0.))
                .max_w(px(210.))
                .truncate()
                .text_color(crate::ui::design::sky(cx))
                .child(solo_name),
        )
        .when(!service_label.is_empty(), |row| {
            row.child(
                div()
                    .min_w(px(0.))
                    .max_w(px(150.))
                    .truncate()
                    .text_color(crate::ui::design::t3(cx))
                    .child(format!("· {service_label}")),
            )
        })
        .when(running, |row| {
            row.child(div().ml_auto().child(crate::ui::design::indicator::dot(
                crate::ui::design::sage(cx),
            )))
        })
        .into_any_element()
}

fn resized_project_preview_ratio(
    start_x: f32,
    start_ratio: f32,
    current_x: f32,
    available_width: f32,
) -> f32 {
    if available_width <= 1.0 {
        return start_ratio.clamp(0.0, PROJECT_PREVIEW_PANEL_MAX_RATIO);
    }

    let candidate_width = start_ratio * available_width - (current_x - start_x);
    let maximum_width = available_width * PROJECT_PREVIEW_PANEL_MAX_RATIO;
    let minimum_width = PROJECT_PREVIEW_PANEL_MIN.min(maximum_width);
    candidate_width.clamp(minimum_width, maximum_width) / available_width
}

impl CenterArea {
    fn project_preview_host(&self) -> Entity<web_preview::WebPreviewHost> {
        if self.penpot_compare_open && self.view_mode == CenterMode::Design {
            self.compare_web_host.clone()
        } else {
            self.web_host.clone()
        }
    }

    pub(super) fn enqueue_project_preview_control_command(
        &mut self,
        envelope: preview_control_ipc::PreviewControlEnvelope,
        cx: &mut Context<Self>,
    ) {
        self.project_preview_control_queue.push_back(envelope);
        self.start_next_project_preview_control_command(cx);
    }

    fn start_next_project_preview_control_command(&mut self, cx: &mut Context<Self>) {
        if self.project_preview_control_inflight.is_some()
            || self.project_preview_control_navigation_barrier.is_some()
        {
            return;
        }
        let Some(envelope) = self.project_preview_control_queue.pop_front() else {
            return;
        };
        self.project_preview_control_inflight = Some(envelope);
        self.process_project_preview_control_command(cx);
    }

    fn fail_project_preview_control_command(
        &mut self,
        command_id: Uuid,
        project_id: ProjectId,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let matches_inflight = self
            .project_preview_control_inflight
            .as_ref()
            .is_some_and(|inflight| inflight.request.id == command_id);
        if matches_inflight {
            let _ = self.web_host.update(cx, |host, _| {
                host.cancel_project_preview_agent_command(project_id, command_id, &message)
            });
            if let Some(inflight) = self.project_preview_control_inflight.take() {
                let _ = inflight.respond_to.try_send(
                    ide_core::preview_control::PreviewControlResponse::failure(
                        command_id,
                        message.clone(),
                    ),
                );
            }
        }
        if self
            .project_preview_control_navigation_barrier
            .is_some_and(|barrier| {
                barrier.project_id == project_id && barrier.command_id == command_id
            })
        {
            self.project_preview_control_navigation_barrier = None;
        }
        self.set_project_preview_status(project_id, Some(message.clone()));
        self.start_next_project_preview_control_command(cx);
        cx.notify();
    }

    fn complete_project_preview_control_command(
        &mut self,
        command_id: Uuid,
        project_id: ProjectId,
        result_json: String,
        image_base64: Option<String>,
        status: &'static str,
        cx: &mut Context<Self>,
    ) {
        let matches_inflight = self
            .project_preview_control_inflight
            .as_ref()
            .is_some_and(|inflight| inflight.request.id == command_id);
        if !matches_inflight {
            return;
        }
        if let Some(inflight) = self.project_preview_control_inflight.take() {
            let _ = inflight.respond_to.try_send(
                ide_core::preview_control::PreviewControlResponse::success(
                    command_id,
                    result_json,
                    image_base64,
                ),
            );
        }
        self.set_project_preview_status(project_id, Some(status.to_string()));
        self.start_next_project_preview_control_command(cx);
        cx.notify();
    }

    fn complete_project_preview_navigation_command(
        &mut self,
        command_id: Uuid,
        project_id: ProjectId,
        result_json: String,
        cx: &mut Context<Self>,
    ) {
        let matches_inflight =
            self.project_preview_control_inflight
                .as_ref()
                .is_some_and(|inflight| {
                    inflight.request.id == command_id && inflight.request.project_id == project_id
                });
        if !matches_inflight {
            return;
        }
        if let Err(message) = self.web_host.update(cx, |host, _| {
            host.activate_project_preview_navigation(project_id, command_id)
        }) {
            self.fail_project_preview_control_command(command_id, project_id, message, cx);
            return;
        }
        if let Some(inflight) = self.project_preview_control_inflight.take() {
            let _ = inflight.respond_to.try_send(
                ide_core::preview_control::PreviewControlResponse::success(
                    command_id,
                    result_json,
                    None,
                ),
            );
        }
        self.project_preview_control_navigation_barrier = Some(ProjectPreviewNavigationBarrier {
            project_id,
            command_id,
            started: false,
        });
        self.set_project_preview_status(project_id, Some("Agent navigation settling".to_string()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let Some(center) = this.upgrade() else {
                return;
            };
            center
                .update(cx, |this: &mut Self, cx| {
                    if this
                        .project_preview_control_navigation_barrier
                        .is_some_and(|barrier| {
                            barrier.project_id == project_id && barrier.command_id == command_id
                        })
                    {
                        this.project_preview_control_navigation_barrier = None;
                        this.set_project_preview_status(
                            project_id,
                            Some("Agent navigation finished".to_string()),
                        );
                        this.start_next_project_preview_control_command(cx);
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn project_preview_page_load_changed(
        &mut self,
        project_id: ProjectId,
        finished: bool,
        live_url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_project_preview_url_input(project_id, &live_url, window, cx);
        let Some(barrier) = self.project_preview_control_navigation_barrier.as_mut() else {
            return;
        };
        if barrier.project_id != project_id {
            return;
        }
        if !finished {
            barrier.started = true;
            self.set_project_preview_status(
                project_id,
                Some("Agent navigation loading".to_string()),
            );
            cx.notify();
            return;
        }
        if !barrier.started {
            return;
        }
        self.project_preview_control_navigation_barrier = None;
        self.set_project_preview_status(
            project_id,
            Some(format!(
                "Agent navigation finished · {}",
                preview_label(&live_url)
            )),
        );
        self.start_next_project_preview_control_command(cx);
        cx.notify();
    }

    fn project_preview_same_document_navigation_settled(
        &mut self,
        project_id: ProjectId,
        command_id: Uuid,
        live_url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_project_preview_url_input(project_id, &live_url, window, cx);
        let matches_barrier =
            self.project_preview_control_navigation_barrier
                .is_some_and(|barrier| {
                    barrier.project_id == project_id && barrier.command_id == command_id
                });
        if !matches_barrier {
            return;
        }
        self.project_preview_control_navigation_barrier = None;
        self.set_project_preview_status(
            project_id,
            Some(format!(
                "Agent navigation finished · {}",
                preview_label(&live_url)
            )),
        );
        self.start_next_project_preview_control_command(cx);
        cx.notify();
    }

    fn process_project_preview_control_command(&mut self, cx: &mut Context<Self>) {
        let Some(command) = self.project_preview_control_inflight.as_ref() else {
            return;
        };
        let command_id = command.request.id;
        let project_id = command.request.project_id;
        let agent_id = command.request.agent_id;
        let action = command.request.action.clone();
        let payload_json = command.request.payload_json.clone();
        if !self.is_project_preview_open(project_id) {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "Open this project's Preview before asking the agent to control it.".to_string(),
                cx,
            );
            return;
        }
        let selected_agent = self.agents.read(cx).selected_agent(project_id);
        if selected_agent.as_ref().map(|agent| agent.id) != Some(agent_id) {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "The requesting agent's chat must be selected to control Preview.".to_string(),
                cx,
            );
            return;
        }
        let Some(choice) = self.active_project_preview_choice(project_id, cx) else {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "Choose a web Preview before asking the agent to control it.".to_string(),
                cx,
            );
            return;
        };
        if choice.is_simulator() {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "Agent control currently supports web Previews, not iOS Simulator Preview."
                    .to_string(),
                cx,
            );
            return;
        }
        let Some(project_root) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "The Preview project is no longer available.".to_string(),
                cx,
            );
            return;
        };
        let control_root = if choice.solo_owner().is_some() {
            let Some(lane_path) = selected_agent
                .as_ref()
                .and_then(|agent| agent.lane_path.clone())
            else {
                self.fail_project_preview_control_command(
                    command_id,
                    project_id,
                    "The Solo Preview worktree is no longer available.".to_string(),
                    cx,
                );
                return;
            };
            lane_path
        } else {
            project_root
        };
        let Some(policy) = preview_control_security_policy(&choice.url, &control_root) else {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "Agent control is limited to localhost and project-local HTML Previews."
                    .to_string(),
                cx,
            );
            return;
        };
        let live_url = match self
            .web_host
            .update(cx, |host, _| host.project_preview_live_url(project_id))
        {
            Ok(url) => url,
            Err(message) => {
                self.fail_project_preview_control_command(command_id, project_id, message, cx);
                return;
            }
        };
        if !preview_control_policy_allows_url(&policy, &live_url) {
            self.fail_project_preview_control_command(
                command_id,
                project_id,
                "Preview control stopped because the live page left its approved local source."
                    .to_string(),
                cx,
            );
            return;
        }
        let policy_json = match serde_json::to_string(&policy) {
            Ok(policy) => policy,
            Err(error) => {
                self.fail_project_preview_control_command(
                    command_id,
                    project_id,
                    format!("Could not secure Preview control: {error}"),
                    cx,
                );
                return;
            }
        };
        match self.web_host.update(cx, |host, _| {
            host.execute_project_preview_agent_command(
                project_id,
                command_id,
                &action,
                &payload_json,
                &policy_json,
            )
        }) {
            Ok(()) => {
                self.set_project_preview_status(
                    project_id,
                    Some(format!("Agent controlling Preview · {action}")),
                );
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs(25))
                        .await;
                    let Some(center) = this.upgrade() else {
                        return;
                    };
                    center
                        .update(cx, |this: &mut Self, cx| {
                            if this
                                .project_preview_control_inflight
                                .as_ref()
                                .is_some_and(|inflight| inflight.request.id == command_id)
                            {
                                this.fail_project_preview_control_command(
                                    command_id,
                                    project_id,
                                    "Preview action timed out inside the WebView".to_string(),
                                    cx,
                                );
                            }
                        })
                        .ok();
                })
                .detach();
                cx.notify();
            }
            Err(message) => {
                self.fail_project_preview_control_command(command_id, project_id, message, cx);
            }
        }
    }

    pub(super) fn set_project_preview_overlay_suspended(
        &mut self,
        suspended: bool,
        cx: &mut Context<Self>,
    ) {
        self.project_preview_host()
            .update(cx, |host, _| host.set_overlay_suspended(suspended));
    }

    pub fn supports_project_preview(&self) -> bool {
        matches!(
            self.view_mode,
            CenterMode::Agents | CenterMode::Split | CenterMode::Files | CenterMode::Terminal
        ) || (self.view_mode == CenterMode::Design && self.penpot_compare_open)
    }

    pub fn is_project_preview_open(&self, project: ProjectId) -> bool {
        self.project_preview_ui
            .get(&project)
            .is_some_and(|ui| ui.open)
            && self.supports_project_preview()
    }

    pub(super) fn set_project_preview_status(
        &mut self,
        project: ProjectId,
        status: Option<String>,
    ) {
        self.project_preview_ui.entry(project).or_default().status = status;
    }

    pub fn toggle_project_preview(&mut self, cx: &mut Context<Self>) {
        if !self.supports_project_preview() {
            return;
        }
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let open = {
            let ui = self.project_preview_ui.entry(project).or_default();
            ui.open = !ui.open;
            ui.status = ui.open.then(|| {
                "Preview follows the selected agent · Solos keep their own preview".to_string()
            });
            ui.open
        };
        if self.project_preview_inspecting == Some(project) {
            self.project_preview_inspecting = None;
        }
        if open {
            self.reconcile_project_preview_for_selected_agent(project, cx);
        } else {
            self.project_preview_host().update(cx, |host, _| {
                let _ = host.set_project_preview_inspecting(false);
            });
        }
        cx.notify();
    }

    fn project_preview_choices(&self, project: ProjectId, cx: &App) -> Vec<PreviewChoice> {
        let agents = self.agents.read(cx).all_records();
        let terminals = self.terminals.read(cx);
        let services = terminals.project_preview_services(project);
        let active_service_origins = services
            .iter()
            .filter_map(|service| preview_server_origin(&service.url))
            .collect::<HashSet<_>>();
        let mut choices = Vec::new();
        let mut project_record_taken = false;
        let mut solo_records_taken = HashSet::new();
        for preview in self
            .project_preview_records
            .get(&project)
            .into_iter()
            .flatten()
        {
            let source_agent = preview
                .source_agent_id
                .and_then(|agent_id| agents.iter().find(|agent| agent.id == agent_id));
            if source_agent.is_some_and(|agent| {
                agent.is_active_solo()
                    && (agent.lane_path.is_none() || self.lane_exit_pending.contains(&agent.id))
            }) {
                continue;
            }
            let scope = source_agent
                .and_then(solo_preview_owner)
                .map(PreviewScope::Solo)
                .unwrap_or(PreviewScope::Project);
            if !stored_preview_is_available(&preview.url, &active_service_origins)
                || !take_latest_preview_scope(
                    &scope,
                    &mut project_record_taken,
                    &mut solo_records_taken,
                )
            {
                continue;
            }
            choices.push(PreviewChoice {
                label: if preview.title.trim().is_empty() {
                    preview_label(&preview.url)
                } else {
                    preview.title.clone()
                },
                url: preview.url.clone(),
                revision: preview.updated_at,
                running: false,
                scope,
            });
        }
        for service in services {
            let service_session = terminals
                .sessions
                .iter()
                .find(|session| session.id == service.session_id);
            let solo_owner = service_session
                .and_then(|session| {
                    agents.iter().find(|agent| {
                        agent.project_id == project
                            && agent
                                .lane_path
                                .as_ref()
                                .is_some_and(|lane| lane == &session.cwd)
                    })
                })
                .and_then(solo_preview_owner)
                // Compatibility for lane servers created before session/agent
                // ownership was available.
                .or_else(|| {
                    let slug = crate::state::terminals::solo_script_slug(&service.title)?;
                    agents
                        .iter()
                        .find(|agent| {
                            agent.project_id == project
                                && agent.solo_branch.as_deref().is_some_and(|branch| {
                                    branch.trim_start_matches("solo/") == slug
                                })
                        })
                        .and_then(solo_preview_owner)
                });
            if solo_owner
                .as_ref()
                .is_some_and(|owner| self.lane_exit_pending.contains(&owner.agent_id))
            {
                continue;
            }
            let scope = solo_owner
                .map(PreviewScope::Solo)
                .unwrap_or(PreviewScope::Project);
            if let Some(existing) = choices.iter_mut().find(|choice| {
                choice.url == service.url || same_preview_server(&choice.url, &service.url)
            }) {
                existing.running = true;
                if existing.label == preview_label(&existing.url) {
                    existing.label = service.title;
                }
                if matches!(scope, PreviewScope::Solo(_)) {
                    existing.scope = scope;
                }
                continue;
            }
            choices.push(PreviewChoice {
                label: service.title,
                url: service.url,
                revision: 0,
                running: true,
                scope,
            });
        }
        for simulator in &self.ios_simulators {
            let display_label = simulator.display_label();
            let duplicate_label = self
                .ios_simulators
                .iter()
                .filter(|candidate| candidate.display_label() == display_label)
                .count()
                > 1;
            let label = if duplicate_label {
                let short_udid = simulator.udid.chars().rev().take(4).collect::<String>();
                format!(
                    "{display_label} · {}",
                    short_udid.chars().rev().collect::<String>()
                )
            } else {
                display_label
            };
            choices.push(PreviewChoice {
                label,
                url: simulator.source_key(),
                revision: 0,
                running: true,
                scope: PreviewScope::Simulator,
            });
        }
        choices
    }

    fn remember_project_preview_choice(&mut self, project: ProjectId, choice: &PreviewChoice) {
        match &choice.scope {
            PreviewScope::Project => {
                self.project_preview_project_urls
                    .insert(project, choice.url.clone());
            }
            PreviewScope::Solo(owner) => {
                self.project_preview_solo_urls
                    .insert(owner.agent_id, choice.url.clone());
            }
            PreviewScope::Simulator => {}
        }
    }

    fn project_preview_project_fallback<'a>(
        &self,
        project: ProjectId,
        choices: &'a [PreviewChoice],
    ) -> Option<&'a PreviewChoice> {
        self.project_preview_project_urls
            .get(&project)
            .and_then(|url| {
                choices
                    .iter()
                    .find(|choice| choice.is_project() && choice.url == *url)
            })
            .or_else(|| {
                choices
                    .iter()
                    .find(|choice| choice.is_project() && choice.running)
            })
            .or_else(|| choices.iter().find(|choice| choice.is_project()))
    }

    fn project_preview_solo_choice<'a>(
        &self,
        agent_id: Uuid,
        choices: &'a [PreviewChoice],
    ) -> Option<&'a PreviewChoice> {
        self.project_preview_solo_urls
            .get(&agent_id)
            .and_then(|url| {
                choices.iter().find(|choice| {
                    choice
                        .solo_owner()
                        .is_some_and(|owner| owner.agent_id == agent_id)
                        && choice.url == *url
                })
            })
            .or_else(|| {
                choices.iter().find(|choice| {
                    choice.running
                        && choice
                            .solo_owner()
                            .is_some_and(|owner| owner.agent_id == agent_id)
                })
            })
            .or_else(|| {
                choices.iter().find(|choice| {
                    choice
                        .solo_owner()
                        .is_some_and(|owner| owner.agent_id == agent_id)
                })
            })
    }

    /// Re-scope Preview when agent navigation changes. Simulator selection is
    /// explicit and project-wide, so it survives chat navigation. Web previews
    /// follow the selected agent: its Solo source first, then project Main.
    pub(super) fn reconcile_project_preview_for_selected_agent(
        &mut self,
        project: ProjectId,
        cx: &App,
    ) {
        let choices = self.project_preview_choices(project, cx);
        let selected_url = self.project_preview_selected_urls.get(&project);
        let current = selected_url.and_then(|url| choices.iter().find(|choice| choice.url == *url));
        if current.is_some_and(PreviewChoice::is_simulator) {
            return;
        }

        let selected_agent = self.agents.read(cx).selected_agent(project);
        let next = selected_agent
            .as_ref()
            .and_then(solo_preview_owner)
            .and_then(|owner| self.project_preview_solo_choice(owner.agent_id, &choices))
            .or_else(|| self.project_preview_project_fallback(project, &choices))
            .cloned();

        match next {
            Some(choice) => {
                let changed = selected_url != Some(&choice.url);
                self.project_preview_selected_urls
                    .insert(project, choice.url.clone());
                self.remember_project_preview_choice(project, &choice);
                if changed
                    && self
                        .project_preview_ui
                        .get(&project)
                        .is_some_and(|ui| ui.open)
                {
                    self.set_project_preview_status(
                        project,
                        Some(match choice.solo_owner() {
                            Some(owner) => format!("Showing Solo {}", owner.name),
                            None => {
                                format!("Showing {}", preview_choice_display_label(&choice.label))
                            }
                        }),
                    );
                }
            }
            None => {
                self.project_preview_selected_urls.remove(&project);
                if self
                    .project_preview_ui
                    .get(&project)
                    .is_some_and(|ui| ui.open)
                {
                    self.set_project_preview_status(
                        project,
                        Some(no_preview_status(selected_agent.as_ref())),
                    );
                }
            }
        }
    }

    fn active_project_preview_choice(
        &mut self,
        project: ProjectId,
        cx: &App,
    ) -> Option<PreviewChoice> {
        let choices = self.project_preview_choices(project, cx);
        let selected = self
            .project_preview_selected_urls
            .get(&project)
            .map(String::as_str);
        let selected_agent = self.agents.read(cx).selected_agent(project);
        let selected_agent_id = selected_agent.as_ref().map(|agent| agent.id);
        let choice = explicit_preview_choice(&choices, selected)
            .or_else(|| {
                selected_agent_id
                    .and_then(|agent_id| self.project_preview_solo_choice(agent_id, &choices))
                    .cloned()
            })
            .or_else(|| {
                self.project_preview_project_fallback(project, &choices)
                    .cloned()
            });
        let Some(mut choice) = choice else {
            self.project_preview_selected_urls.remove(&project);
            if self
                .project_preview_ui
                .get(&project)
                .is_some_and(|ui| ui.open)
            {
                self.set_project_preview_status(
                    project,
                    Some(no_preview_status(selected_agent.as_ref())),
                );
            }
            return None;
        };

        // Never leave a Solo's world visible while another agent is open.
        if !preview_choice_matches_agent(&choice, selected_agent_id) {
            let Some(fallback) = self.project_preview_project_fallback(project, &choices) else {
                self.project_preview_selected_urls.remove(&project);
                if self
                    .project_preview_ui
                    .get(&project)
                    .is_some_and(|ui| ui.open)
                {
                    self.set_project_preview_status(
                        project,
                        Some(no_preview_status(selected_agent.as_ref())),
                    );
                }
                return None;
            };
            choice = fallback.clone();
        }
        self.project_preview_selected_urls
            .insert(project, choice.url.clone());
        self.remember_project_preview_choice(project, &choice);
        Some(choice)
    }

    pub(super) fn project_preview_intent(
        &mut self,
        project: ProjectId,
        cx: &App,
    ) -> Option<web_preview::WebPreviewIntent> {
        if self
            .project_preview_inspecting
            .is_some_and(|inspecting| inspecting != project)
        {
            self.project_preview_inspecting = None;
        }
        if !self.is_project_preview_open(project) {
            return None;
        }
        let choice = self.active_project_preview_choice(project, cx)?;
        if choice.is_simulator() {
            let udid = ios_simulator_preview::source_udid(&choice.url)?;
            let endpoint = self
                .simulator_bridge_endpoint
                .as_ref()
                .filter(|endpoint| endpoint.udid == udid)?;
            return Some(web_preview::WebPreviewIntent::ProjectPreview {
                project_id: project,
                url: endpoint.url.clone(),
                revision: 0,
            });
        }
        // Fresh agent work bumps the refresh counter; folding it into the
        // revision makes the webview re-render without a manual reload.
        let refresh = self
            .project_preview_refresh
            .get(&project)
            .copied()
            .unwrap_or(0);
        Some(web_preview::WebPreviewIntent::ProjectPreview {
            project_id: project,
            url: choice.url,
            revision: choice.revision.wrapping_add(refresh),
        })
    }

    pub(super) fn handle_project_preview_messages(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let messages = self
            .project_preview_host()
            .update(cx, |host, _| host.take_project_preview_messages());
        for message in messages {
            match message {
                web_preview::ProjectPreviewMessage::ToggleFocusMode { project_id } => {
                    if self
                        .active_project(cx)
                        .is_some_and(|(active_project, _)| active_project == project_id)
                    {
                        cx.on_next_frame(window, |_, window, cx| {
                            window.dispatch_action(
                                Box::new(crate::actions::ToggleFocusMode::default()),
                                cx,
                            );
                        });
                    }
                }
                web_preview::ProjectPreviewMessage::Cancelled { project_id } => {
                    if self.project_preview_inspecting == Some(project_id) {
                        self.project_preview_inspecting = None;
                    }
                    self.set_project_preview_status(
                        project_id,
                        Some("Review cancelled".to_string()),
                    );
                }
                web_preview::ProjectPreviewMessage::SelectionReady {
                    project_id,
                    target_kind,
                } => {
                    if self.project_preview_inspecting == Some(project_id) {
                        self.project_preview_inspecting = None;
                    }
                    self.set_project_preview_status(
                        project_id,
                        Some(if target_kind == "area" {
                            "Image area selected · add your comment in the Preview".to_string()
                        } else {
                            "Element selected · add your comment in the Preview".to_string()
                        }),
                    );
                }
                web_preview::ProjectPreviewMessage::SubmitReview {
                    project_id,
                    comment,
                    mut target_kind,
                    mut element,
                    mut area,
                } => {
                    if self.project_preview_inspecting == Some(project_id) {
                        self.project_preview_inspecting = None;
                    }
                    let agent = if self.penpot_compare_open && self.view_mode == CenterMode::Design
                    {
                        self.penpot_design_assistant_agent(project_id, cx)
                    } else {
                        self.agents.read(cx).selected_agent(project_id)
                    };
                    let Some(agent) = agent else {
                        self.set_project_preview_status(
                            project_id,
                            Some(
                                "Implement the design or select an agent before sending a review"
                                    .to_string(),
                            ),
                        );
                        self.project_preview_host().update(cx, |host, _| {
                            host.finish_project_preview_submission(project_id, false)
                        });
                        continue;
                    };
                    let comment = comment.trim().to_string();
                    if comment.is_empty() || comment.len() > 20_000 {
                        self.set_project_preview_status(
                            project_id,
                            Some("Add a shorter comment before sending".to_string()),
                        );
                        self.project_preview_host().update(cx, |host, _| {
                            host.finish_project_preview_submission(project_id, false)
                        });
                        continue;
                    }
                    if let Some(source) = self
                        .project_preview_selected_urls
                        .get(&project_id)
                        .filter(|source| ios_simulator_preview::source_udid(source).is_some())
                        .cloned()
                    {
                        let page_title = ios_simulator_preview::source_udid(&source)
                            .and_then(|udid| {
                                self.ios_simulators
                                    .iter()
                                    .find(|device| device.udid == udid)
                            })
                            .map(|device| device.display_label())
                            .unwrap_or_else(|| "iOS Simulator".to_string());
                        if let Some(selection) = element.take() {
                            area = Some(ide_core::visual_review::VisualAreaSelection {
                                page_url: source.clone(),
                                page_title: page_title.clone(),
                                rect: selection.rect,
                            });
                            target_kind = "area".to_string();
                        } else if let Some(selection) = area.as_mut() {
                            selection.page_url = source;
                            selection.page_title = page_title;
                        }
                    }
                    let url = element
                        .as_ref()
                        .map(|selection| selection.page_url.clone())
                        .or_else(|| area.as_ref().map(|selection| selection.page_url.clone()))
                        .unwrap_or_default();
                    self.set_project_preview_status(
                        project_id,
                        Some(format!("Capturing review for {}…", agent.title)),
                    );
                    self.project_preview_host().update(cx, |host, _| {
                        host.capture_project_preview_review(
                            project_id,
                            agent.id,
                            url,
                            comment,
                            target_kind,
                            element,
                            area,
                        )
                    });
                }
                web_preview::ProjectPreviewMessage::CaptureReady {
                    project_id,
                    agent_id,
                    url,
                    comment,
                    target_kind,
                    element,
                    area,
                    image_base64,
                } => {
                    let agent_context = self
                        .penpot_design_assistant_agent(project_id, cx)
                        .filter(|agent| agent.id == agent_id)
                        .or_else(|| {
                            self.agents
                                .read(cx)
                                .agent(agent_id)
                                .filter(|agent| agent.project_id == project_id)
                                .cloned()
                        });
                    let preview_id = self
                        .project_preview_records
                        .get(&project_id)
                        .and_then(|previews| previews.iter().find(|preview| preview.url == url))
                        .map(|preview| preview.id);
                    let created_at = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.project_preview_host().update(cx, |host, _| {
                        host.finish_project_preview_submission(project_id, true)
                    });
                    if let Some(agent) = agent_context.as_ref() {
                        self.set_project_preview_status(
                            project_id,
                            Some(format!("Review sent to {}", agent.title)),
                        );
                    } else {
                        self.set_project_preview_status(
                            project_id,
                            Some("The selected agent is no longer available".to_string()),
                        );
                        continue;
                    }
                    self.enqueue_inline_preview_review(
                        ide_core::visual_review::VisualReviewSubmission {
                            id: Uuid::new_v4(),
                            project_id,
                            agent_id,
                            preview_id,
                            url,
                            comment,
                            target_kind,
                            element,
                            area,
                            image_base64,
                            created_at,
                        },
                        agent_context,
                        cx,
                    );
                }
                web_preview::ProjectPreviewMessage::CaptureFailed {
                    project_id,
                    message,
                } => {
                    self.set_project_preview_status(project_id, Some(message));
                    self.project_preview_host().update(cx, |host, _| {
                        host.finish_project_preview_submission(project_id, false)
                    });
                }
                web_preview::ProjectPreviewMessage::AgentNativeInputRequest {
                    project_id,
                    command_id,
                    action,
                    x,
                    y,
                    key,
                    code,
                    meta,
                    control,
                    alt,
                    shift,
                } => {
                    let matches_inflight = self
                        .project_preview_control_inflight
                        .as_ref()
                        .is_some_and(|inflight| {
                            inflight.request.id == command_id
                                && inflight.request.project_id == project_id
                        });
                    if !matches_inflight {
                        continue;
                    }
                    let result = self.web_host.update(cx, |host, _| {
                        host.perform_project_preview_native_input(
                            project_id,
                            command_id,
                            &action,
                            x,
                            y,
                            key.as_deref(),
                            code.as_deref(),
                            meta,
                            control,
                            alt,
                            shift,
                        )
                    });
                    if let Err(message) = result {
                        self.fail_project_preview_control_command(
                            command_id, project_id, message, cx,
                        );
                    }
                }
                web_preview::ProjectPreviewMessage::AgentActionResult {
                    project_id,
                    command_id,
                    success,
                    capture,
                    result_json,
                    error,
                } => {
                    if !success {
                        self.fail_project_preview_control_command(
                            command_id,
                            project_id,
                            error.unwrap_or_else(|| "Preview action failed.".to_string()),
                            cx,
                        );
                        continue;
                    }
                    if capture {
                        self.web_host.update(cx, |host, _| {
                            host.capture_project_preview_agent_snapshot(
                                project_id,
                                command_id,
                                result_json,
                            )
                        });
                        continue;
                    }
                    self.complete_project_preview_control_command(
                        command_id,
                        project_id,
                        result_json,
                        None,
                        "Agent action completed",
                        cx,
                    );
                }
                web_preview::ProjectPreviewMessage::AgentNavigationReady {
                    project_id,
                    command_id,
                    result_json,
                } => {
                    self.complete_project_preview_navigation_command(
                        command_id,
                        project_id,
                        result_json,
                        cx,
                    );
                }
                web_preview::ProjectPreviewMessage::AgentNavigationSettled {
                    project_id,
                    command_id,
                    url,
                } => {
                    self.project_preview_same_document_navigation_settled(
                        project_id, command_id, url, window, cx,
                    );
                }
                web_preview::ProjectPreviewMessage::AgentPageLoad {
                    project_id,
                    finished,
                    url,
                } => {
                    self.project_preview_page_load_changed(project_id, finished, url, window, cx);
                }
                web_preview::ProjectPreviewMessage::AgentSnapshotReady {
                    project_id,
                    command_id,
                    result_json,
                    image_base64,
                } => {
                    self.complete_project_preview_control_command(
                        command_id,
                        project_id,
                        result_json,
                        Some(image_base64),
                        "Agent observed Preview",
                        cx,
                    );
                }
                web_preview::ProjectPreviewMessage::AgentSnapshotFailed {
                    project_id,
                    command_id,
                    message,
                } => {
                    self.fail_project_preview_control_command(command_id, project_id, message, cx);
                }
            }
        }
    }

    fn project_preview_url_input(
        &mut self,
        project: ProjectId,
        initial_url: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.project_preview_url_inputs.get(&project) {
            return input.clone();
        }
        let initial_url = initial_url.unwrap_or_default().to_string();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Enter a URL")
                .default_value(initial_url)
        });
        cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.navigate_project_preview_from_url_input(project, window, cx)
                }
                InputEvent::Focus => {
                    this.project_preview_ui
                        .entry(project)
                        .or_default()
                        .url_editing = true;
                }
                InputEvent::Blur => {
                    this.project_preview_ui
                        .entry(project)
                        .or_default()
                        .url_editing = false;
                    this.sync_project_preview_live_url(project, window, cx);
                }
                InputEvent::Change | InputEvent::SelectionChange => {}
            },
        )
        .detach();
        self.project_preview_url_inputs
            .insert(project, input.clone());
        input
    }

    fn set_project_preview_url_input(
        &mut self,
        project: ProjectId,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if url == "about:blank"
            || self
                .project_preview_selected_urls
                .get(&project)
                .is_some_and(|source| ios_simulator_preview::source_udid(source).is_some())
        {
            return;
        }
        let Some(input) = self.project_preview_url_inputs.get(&project).cloned() else {
            return;
        };
        if input.read(cx).value().as_ref() != url {
            let url = url.to_string();
            input.update(cx, move |input, cx| input.set_value(url, window, cx));
        }
    }

    fn sync_project_preview_live_url(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(url) = self
            .project_preview_host()
            .update(cx, |host, _| host.project_preview_live_url(project))
        else {
            return;
        };
        self.set_project_preview_url_input(project, &url, window, cx);
    }

    fn navigate_project_preview_from_url_input(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.project_preview_url_inputs.get(&project).cloned() else {
            return;
        };
        let value = input.read(cx).value().to_string();
        let url = match normalize_project_preview_url(&value) {
            Ok(url) => url,
            Err(message) => {
                self.set_project_preview_status(project, Some(message));
                cx.notify();
                return;
            }
        };
        match self.project_preview_host().update(cx, |host, _| {
            host.navigate_project_preview_url(project, &url)
        }) {
            Ok(()) => {
                input.update(cx, |input, cx| input.set_value(url.clone(), window, cx));
                self.project_preview_inspecting = None;
                let _ = self
                    .project_preview_host()
                    .update(cx, |host, _| host.set_project_preview_inspecting(false));
                self.set_project_preview_status(
                    project,
                    Some(format!("Opening {}…", preview_label(&url))),
                );
            }
            Err(message) => self.set_project_preview_status(project, Some(message)),
        }
        cx.notify();
    }

    pub(super) fn render_project_preview_panel(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let choices = self.project_preview_choices(project, cx);
        let active = self.active_project_preview_choice(project, cx);
        let active_url = active.as_ref().map(|choice| choice.url.clone());
        let active_label = active
            .as_ref()
            .map(|choice| preview_choice_display_label(&choice.label))
            .unwrap_or_else(|| "Choose preview".to_string());
        let active_solo = active.as_ref().and_then(PreviewChoice::solo_owner).cloned();
        let selected_agent = if self.penpot_compare_open && self.view_mode == CenterMode::Design {
            self.penpot_design_assistant_agent(project, cx)
        } else {
            self.agents.read(cx).selected_agent(project)
        };
        let active_is_simulator = active.as_ref().is_some_and(PreviewChoice::is_simulator);
        let url_input = self.project_preview_url_input(
            project,
            (!active_is_simulator)
                .then_some(active_url.as_deref())
                .flatten(),
            window,
            cx,
        );
        let url_editing = self
            .project_preview_ui
            .get(&project)
            .is_some_and(|ui| ui.url_editing);
        if !active_is_simulator && !url_editing {
            self.sync_project_preview_live_url(project, window, cx);
        }
        let active_simulator_udid = active
            .as_ref()
            .filter(|choice| choice.is_simulator())
            .and_then(|choice| ios_simulator_preview::source_udid(&choice.url))
            .map(ToOwned::to_owned);
        let simulator_bridge_ready = active_simulator_udid.as_deref().is_some_and(|udid| {
            self.simulator_bridge_endpoint
                .as_ref()
                .is_some_and(|endpoint| endpoint.udid == udid)
        });
        let can_review = active.is_some()
            && selected_agent.is_some()
            && (!active_is_simulator || simulator_bridge_ready);

        let service_center = cx.entity().clone();
        let preview_host = self.project_preview_host().clone();
        let selected_for_menu = active_url.clone();
        let has_choices = !choices.is_empty();
        let has_project_choices = choices.iter().any(PreviewChoice::is_project);
        let has_solo_choices = choices.iter().any(|choice| choice.solo_owner().is_some());
        let has_simulator_choices = choices.iter().any(PreviewChoice::is_simulator);
        let service_selector = style::header_dropdown_button("project-preview-service", cx);
        let service_selector = if let Some(owner) = active_solo.as_ref() {
            service_selector.child(
                gpui_component::h_flex()
                    .gap_1()
                    .items_center()
                    .child(crate::ui::design::indicator::solo_icon(
                        crate::ui::design::sky(cx),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(
                        gpui::div()
                            .text_color(crate::ui::design::sky(cx))
                            .max_w(px(220.))
                            .truncate()
                            .child(solo_preview_display_name(owner)),
                    ),
            )
        } else {
            service_selector.label(active_label)
        };
        let selector_tooltip = active_solo
            .as_ref()
            .map(|owner| format!("Solo preview: {}", owner.name))
            .unwrap_or_else(|| "Choose a project preview".to_string());
        let service_selector = service_selector
            .disabled(choices.is_empty())
            .tooltip(selector_tooltip)
            .dropdown_menu(move |mut menu, window, menu_cx| {
                preview_host.update(menu_cx, |host, _| host.set_overlay_suspended(true));
                let host_after_menu = preview_host.clone();
                menu_cx
                    .on_release(move |_, cx| {
                        host_after_menu.update(cx, |host, _| host.set_overlay_suspended(false));
                    })
                    .detach();
                let mut section_started = false;
                if has_project_choices {
                    menu = menu.item(PopupMenuItem::label("Web"));
                    section_started = true;
                }
                for scope in [0_u8, 1, 2] {
                    if scope == 1 && has_solo_choices {
                        if section_started {
                            menu = menu.item(PopupMenuItem::separator());
                        }
                        menu = menu.item(PopupMenuItem::label("Solos"));
                        section_started = true;
                    } else if scope == 2 && has_simulator_choices {
                        if section_started {
                            menu = menu.item(PopupMenuItem::separator());
                        }
                        menu = menu.item(PopupMenuItem::label("Active iOS Simulators"));
                        section_started = true;
                    }

                    for choice in choices.iter().filter(|choice| match scope {
                        0 => choice.is_project(),
                        1 => choice.solo_owner().is_some(),
                        _ => choice.is_simulator(),
                    }) {
                        let center = service_center.clone();
                        let choice = choice.clone();
                        let selected = selected_for_menu.as_deref() == Some(choice.url.as_str());
                        let choice_label = preview_choice_display_label(&choice.label);
                        let item = if let Some(owner) = choice.solo_owner().cloned() {
                            let service_label = choice.label.clone();
                            let running = choice.running;
                            PopupMenuItem::element(move |_, cx| {
                                solo_preview_menu_row(&owner, &service_label, running, cx)
                            })
                        } else {
                            let label: SharedString = if choice.is_simulator() {
                                format!("◉  {choice_label}").into()
                            } else if choice.running {
                                format!("●  {choice_label}").into()
                            } else {
                                choice_label.clone().into()
                            };
                            PopupMenuItem::new(label)
                        };
                        menu = menu.item(item.checked(selected).on_click(window.listener_for(
                            &center,
                            move |_, _, window, cx| {
                                let choice = choice.clone();
                                let choice_label = choice_label.clone();
                                cx.on_next_frame(window, move |this: &mut Self, window, cx| {
                                    if let Some(owner) = choice.solo_owner() {
                                        if !(this.penpot_compare_open
                                            && this.view_mode == CenterMode::Design)
                                        {
                                            this.open_agent(owner.agent_id, window, cx);
                                        }
                                    }
                                    this.project_preview_selected_urls
                                        .insert(project, choice.url.clone());
                                    this.remember_project_preview_choice(project, &choice);
                                    this.project_preview_inspecting = None;
                                    let _ = this.project_preview_host().update(cx, |host, _| {
                                        host.set_project_preview_inspecting(false)
                                    });
                                    this.set_project_preview_status(
                                        project,
                                        Some(if choice.is_simulator() {
                                            format!("Connecting to {choice_label}…")
                                        } else if let Some(owner) = choice.solo_owner() {
                                            format!("Showing Solo {}", owner.name)
                                        } else {
                                            format!("Showing {choice_label}")
                                        }),
                                    );
                                    cx.notify();
                                });
                            },
                        )));
                    }
                }
                menu
            });

        let status = self
            .project_preview_ui
            .get(&project)
            .and_then(|ui| ui.status.clone())
            .unwrap_or_else(|| {
                selected_agent
                    .as_ref()
                    .map(|agent| format!("Reviews go to {}", agent.title))
                    .unwrap_or_else(|| "Select an agent before sending a review".to_string())
            });
        let viewport = self
            .project_preview_ui
            .get(&project)
            .map(|ui| ui.viewport)
            .unwrap_or_default();
        let mobile_viewport = viewport == ProjectPreviewViewport::Mobile;
        let host_for_canvas = self.project_preview_host().clone();
        let inspecting = self.project_preview_inspecting == Some(project);

        v_flex()
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(
                        h_flex()
                            .min_w(px(0.))
                            .flex_1()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(IconName::Eye)
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t3(cx)),
                            )
                            .child(service_selector)
                            .when(!active_is_simulator, |row| {
                                row.child(
                                    div()
                                        .min_w(px(96.))
                                        .flex_1()
                                        .child(
                                            Input::new(&url_input)
                                                .small()
                                                .h(crate::ui::design::control_h_sm())
                                                .w_full()
                                                .disabled(active.is_none())
                                                .prefix(
                                                    Icon::new(IconName::Globe)
                                                        .size(crate::ui::design::icon_sm())
                                                        .text_color(crate::ui::design::t3(cx)),
                                                ),
                                        ),
                                )
                            }),
                    )
                    .child(
                        style::header_icon_button(
                            "project-preview-back",
                            IconName::ChevronLeft,
                            cx,
                        )
                        .disabled(active.is_none() || active_is_simulator)
                        .tooltip("Back")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(message) = this.project_preview_host().update(cx, |host, _| {
                                host.navigate_project_preview_history(false)
                            }) {
                                this.set_project_preview_status(project, Some(message));
                            }
                        })),
                    )
                    .child(
                        style::header_icon_button(
                            "project-preview-forward",
                            IconName::ChevronRight,
                            cx,
                        )
                        .disabled(active.is_none() || active_is_simulator)
                        .tooltip("Forward")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(message) = this.project_preview_host().update(cx, |host, _| {
                                host.navigate_project_preview_history(true)
                            }) {
                                this.set_project_preview_status(project, Some(message));
                            }
                        })),
                    )
                    .child(
                        style::header_icon_button(
                            "project-preview-reload",
                            IconName::Redo2,
                            cx,
                        )
                        .disabled(active.is_none() || active_is_simulator)
                        .tooltip("Reload Preview")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match this
                                .project_preview_host()
                                .update(cx, |host, _| host.reload_project_preview())
                            {
                                Ok(()) => this.set_project_preview_status(
                                    project,
                                    Some("Reloading Preview…".to_string()),
                                ),
                                Err(message) => {
                                    this.set_project_preview_status(project, Some(message))
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .when(!active_is_simulator, |header| header.child(
                        style::context_panel_action_button(
                            "project-preview-viewport",
                            if mobile_viewport {
                                IconName::Frame
                            } else {
                                IconName::WindowMaximize
                            },
                            if mobile_viewport {
                                "Mobile"
                            } else {
                                "Desktop"
                            },
                            cx,
                        )
                        .tooltip(if mobile_viewport {
                            "Switch to desktop viewport"
                        } else {
                            "Switch to 390 px mobile viewport"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let ui = this.project_preview_ui.entry(project).or_default();
                            ui.viewport = ui.viewport.toggled();
                            cx.notify();
                        })),
                    ))
                    .child(
                        style::context_panel_action_button(
                            "project-preview-review",
                            IconName::Inspector,
                            if inspecting { "Selecting…" } else { "Review" },
                            cx,
                        )
                        .disabled(!can_review)
                        .tooltip(if selected_agent.is_none() {
                            "Select an agent chat first"
                        } else if active_is_simulator {
                            "Drag an area on the Simulator screen"
                        } else {
                            "Click an element or drag an image area"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let inspecting = this.project_preview_inspecting != Some(project);
                            match this.project_preview_host().update(cx, |host, _| {
                                host.set_project_preview_inspecting(inspecting)
                            }) {
                                Ok(()) => {
                                    this.project_preview_inspecting = inspecting.then_some(project);
                                    this.set_project_preview_status(
                                        project,
                                        Some(if inspecting {
                                            if active_is_simulator {
                                                "Review · drag any area on the Simulator screen"
                                                    .to_string()
                                            } else {
                                                "Review · click an element or drag any image area"
                                                    .to_string()
                                            }
                                        } else {
                                            "Review cancelled".to_string()
                                        }),
                                    );
                                }
                                Err(message) => {
                                    this.set_project_preview_status(project, Some(message))
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        style::header_icon_button(
                            "close-project-preview",
                            IconName::Close,
                            cx,
                        )
                        .tooltip("Close Preview")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.penpot_compare_open
                                && this.view_mode == CenterMode::Design
                            {
                                this.close_penpot_compare(cx);
                                return;
                            }
                            let ui = this.project_preview_ui.entry(project).or_default();
                            ui.open = false;
                            ui.status = None;
                            if this.project_preview_inspecting == Some(project) {
                                this.project_preview_inspecting = None;
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .bg(crate::ui::design::base(cx))
                    .when(active.is_none(), |body| {
                        body.child(
                            v_flex()
                                .size_full()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    Icon::new(IconName::Globe)
                                        .size_8()
                                        .text_color(crate::ui::design::t3(cx)),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .child(if has_choices {
                                            "Choose what to preview"
                                        } else {
                                            "Nothing available to preview"
                                        }),
                                )
                                .child(
                                    div()
                                        .max_w(px(300.))
                                        .text_center()
                                        .text_size(crate::ui::design::text_ui())
                                        .child(if has_choices {
                                            "Select a web preview or active Simulator from the menu above."
                                        } else {
                                            "Open index.html, run a web script, or boot an iOS Simulator."
                                        }),
                                ),
                        )
                    })
                    .when(
                        active.is_some()
                            && (!active_is_simulator || simulator_bridge_ready),
                        |body| {
                        body.child(
                            div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(mobile_viewport && !active_is_simulator, |stage| {
                                stage.p_2().bg(crate::ui::design::base(cx))
                            })
                            .child(
                                div()
                                .relative()
                                .h_full()
                                .w_full()
                                .when(mobile_viewport && !active_is_simulator, |frame| {
                                    frame
                                        .w(px(
                                            PROJECT_PREVIEW_MOBILE_WIDTH
                                                + PROJECT_PREVIEW_MOBILE_FRAME_INSET * 2.0,
                                        ))
                                        .max_w_full()
                                        .h(px(
                                            PROJECT_PREVIEW_MOBILE_HEIGHT
                                                + PROJECT_PREVIEW_MOBILE_FRAME_INSET * 2.0,
                                        ))
                                        .max_h_full()
                                        .p(px(PROJECT_PREVIEW_MOBILE_FRAME_INSET))
                                        .rounded(px(16.))
                                        .border_1()
                                        .border_color(
                                            crate::ui::design::line(cx).opacity(0.72),
                                        )
                                        .bg(crate::ui::design::base(cx))
                                })
                                .child(
                                    canvas(
                                        move |bounds, window, cx| {
                                            host_for_canvas.update(cx, |host, _| {
                                                host.place(bounds, window)
                                            });
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .size_full(),
                                ),
                            ),
                        )
                    })
                    .when(active_is_simulator && !simulator_bridge_ready, |body| {
                        body.child(
                            v_flex()
                                .size_full()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(crate::ui::design::t3(cx))
                                .child(logo_spinner(
                                    24.,
                                    "simulator-preview",
                                    0,
                                    crate::ui::design::t3(cx),
                                ))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .child("Connecting to Simulator…"),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .h(px(28.))
                    .flex_none()
                    .w_full()
                    .px_2p5()
                    .gap_1p5()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.35))
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(if active.as_ref().is_some_and(|choice| choice.running) {
                                crate::ui::design::sage(cx)
                            } else {
                                crate::ui::design::t4(cx)
                            }),
                    )
                    .child(div().truncate().child(status)),
            )
            .into_any_element()
    }

    pub(super) fn project_preview_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("project-preview-resize")
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(PROJECT_PREVIEW_RESIZE_GUTTER))
            .h_full()
            .bg(crate::ui::design::base(cx))
            .cursor_ew_resize()
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .bg(crate::ui::design::line(cx).opacity(0.32)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.project_preview_resize = Some(ProjectPreviewResizeState {
                        start_x: event.position.x.as_f32(),
                        start_ratio: this.project_preview_panel_ratio,
                    });
                }),
            )
            .on_drag(ProjectPreviewResizeHandle, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ProjectPreviewResizeHandle>, _, cx| {
                    let Some(resize) = &this.project_preview_resize else {
                        return;
                    };
                    this.project_preview_panel_ratio = resized_project_preview_ratio(
                        resize.start_x,
                        resize.start_ratio,
                        event.event.position.x.as_f32(),
                        this.project_preview_available_width,
                    );
                    cx.notify();
                },
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        fs,
    };

    use ide_core::project::ProjectId;
    use uuid::Uuid;

    use super::{
        explicit_preview_choice, normalize_project_preview_url, preview_choice_matches_agent,
        preview_control_policy_allows_url, preview_control_security_policy, preview_server_origin,
        resized_project_preview_ratio, stored_preview_is_available, take_latest_preview_scope,
        PreviewChoice, PreviewScope, ProjectPreviewUiState, ProjectPreviewViewport,
        SoloPreviewOwner, PROJECT_PREVIEW_PANEL_MAX_RATIO, PROJECT_PREVIEW_PANEL_MIN,
    };

    #[test]
    fn preview_address_bar_normalizes_common_local_urls() {
        assert_eq!(
            normalize_project_preview_url(" localhost:5173/app ").unwrap(),
            "http://localhost:5173/app"
        );
        assert_eq!(
            normalize_project_preview_url("https://example.com/preview").unwrap(),
            "https://example.com/preview"
        );
    }

    #[test]
    fn preview_address_bar_rejects_empty_and_unsupported_urls() {
        assert!(normalize_project_preview_url(" ").is_err());
        assert!(normalize_project_preview_url("ftp://localhost/file").is_err());
    }

    #[test]
    fn mobile_viewport_is_scoped_to_one_project() {
        let first = ProjectId(Uuid::from_u128(1));
        let second = ProjectId(Uuid::from_u128(2));
        let mut states = HashMap::<ProjectId, ProjectPreviewUiState>::new();

        let first_ui = states.entry(first).or_default();
        first_ui.viewport = first_ui.viewport.toggled();

        assert_eq!(
            states.get(&first).map(|ui| ui.viewport),
            Some(ProjectPreviewViewport::Mobile)
        );
        assert_eq!(
            states.entry(second).or_default().viewport,
            ProjectPreviewViewport::Desktop
        );
    }

    #[test]
    fn preview_resize_is_capped_at_seventy_percent_of_the_center() {
        let ratio = resized_project_preview_ratio(900.0, 0.55, -1_000.0, 1_200.0);
        assert_eq!(ratio, PROJECT_PREVIEW_PANEL_MAX_RATIO);
    }

    #[test]
    fn preview_resize_keeps_a_minimum_width_without_hiding_center_content() {
        assert_eq!(
            resized_project_preview_ratio(900.0, 0.55, 2_000.0, 1_200.0),
            PROJECT_PREVIEW_PANEL_MIN / 1_200.0
        );
        assert_eq!(
            resized_project_preview_ratio(300.0, 0.55, 2_000.0, 400.0),
            PROJECT_PREVIEW_PANEL_MAX_RATIO
        );
    }

    #[test]
    fn agent_control_rechecks_the_live_loopback_origin() {
        let project = tempfile::tempdir().unwrap();
        let policy =
            preview_control_security_policy("http://localhost:5173/app", project.path()).unwrap();
        assert!(preview_control_policy_allows_url(
            &policy,
            "http://127.0.0.1:3000/next"
        ));
        assert!(preview_control_policy_allows_url(
            &policy,
            "https://[::1]:4173"
        ));
        assert!(!preview_control_policy_allows_url(
            &policy,
            "https://example.com"
        ));
        assert!(!preview_control_policy_allows_url(
            &policy,
            "file:///tmp/index.html"
        ));
    }

    #[test]
    fn agent_control_keeps_file_navigation_inside_the_project() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let entry = project.path().join("index.html");
        let sibling = project.path().join("about.html");
        let escaped = outside.path().join("private.html");
        fs::write(&entry, "index").unwrap();
        fs::write(&sibling, "about").unwrap();
        fs::write(&escaped, "private").unwrap();
        let entry_url = url::Url::from_file_path(&entry).unwrap().to_string();
        let sibling_url = url::Url::from_file_path(&sibling).unwrap().to_string();
        let escaped_url = url::Url::from_file_path(&escaped).unwrap().to_string();
        let policy = preview_control_security_policy(&entry_url, project.path()).unwrap();
        assert!(preview_control_policy_allows_url(&policy, &sibling_url));
        assert!(!preview_control_policy_allows_url(&policy, &escaped_url));
        assert!(preview_control_security_policy(&escaped_url, project.path()).is_none());
        assert!(!preview_control_policy_allows_url(
            &policy,
            "https://example.com"
        ));
    }

    #[test]
    fn stored_script_preview_requires_a_matching_active_service() {
        let active = HashSet::from([preview_server_origin("http://127.0.0.1:5173/").unwrap()]);

        assert!(stored_preview_is_available(
            "http://localhost:5173/dashboard",
            &active
        ));
        assert!(!stored_preview_is_available(
            "http://localhost:4173/",
            &active
        ));
        assert!(stored_preview_is_available(
            "file:///tmp/project/index.html",
            &active
        ));
    }

    #[test]
    fn ad_hoc_preview_keeps_only_the_latest_record_per_scope() {
        let mut project_taken = false;
        let mut solo_taken = HashSet::new();
        let project = PreviewScope::Project;
        let first_solo = PreviewScope::Solo(SoloPreviewOwner {
            agent_id: Uuid::from_u128(1),
            name: "first".to_string(),
        });
        let second_solo = PreviewScope::Solo(SoloPreviewOwner {
            agent_id: Uuid::from_u128(2),
            name: "second".to_string(),
        });

        assert!(take_latest_preview_scope(
            &project,
            &mut project_taken,
            &mut solo_taken
        ));
        assert!(!take_latest_preview_scope(
            &project,
            &mut project_taken,
            &mut solo_taken
        ));
        assert!(take_latest_preview_scope(
            &first_solo,
            &mut project_taken,
            &mut solo_taken
        ));
        assert!(!take_latest_preview_scope(
            &first_solo,
            &mut project_taken,
            &mut solo_taken
        ));
        assert!(take_latest_preview_scope(
            &second_solo,
            &mut project_taken,
            &mut solo_taken
        ));
    }

    #[test]
    fn preview_ratio_is_independent_of_window_and_sidebar_width() {
        let ratio = resized_project_preview_ratio(700.0, 0.63, 700.0, 900.0);
        assert_eq!(ratio, 0.63);
        assert_eq!(ratio * 900.0, 567.0);
        assert_eq!(ratio * 1_600.0, 1_008.0);
    }

    #[test]
    fn simulator_is_never_selected_implicitly() {
        let simulator = PreviewChoice {
            label: "iPhone".to_string(),
            url: "simulator://PHONE".to_string(),
            revision: 0,
            running: true,
            scope: PreviewScope::Simulator,
        };

        assert_eq!(explicit_preview_choice(&[simulator], None), None);
    }

    #[test]
    fn simulator_is_selected_after_an_explicit_choice() {
        let simulator = PreviewChoice {
            label: "iPhone".to_string(),
            url: "simulator://PHONE".to_string(),
            revision: 0,
            running: true,
            scope: PreviewScope::Simulator,
        };

        assert_eq!(
            explicit_preview_choice(&[simulator.clone()], Some("simulator://PHONE")),
            Some(simulator)
        );
    }

    #[test]
    fn solo_preview_only_matches_its_own_agent() {
        let owner_id = Uuid::from_u128(11);
        let other_id = Uuid::from_u128(22);
        let solo = PreviewChoice {
            label: "App".to_string(),
            url: "http://127.0.0.1:52741".to_string(),
            revision: 0,
            running: true,
            scope: PreviewScope::Solo(SoloPreviewOwner {
                agent_id: owner_id,
                name: "solo-one".to_string(),
            }),
        };
        let project = PreviewChoice {
            label: "App".to_string(),
            url: "http://127.0.0.1:5173".to_string(),
            revision: 0,
            running: true,
            scope: PreviewScope::Project,
        };

        assert!(preview_choice_matches_agent(&solo, Some(owner_id)));
        assert!(!preview_choice_matches_agent(&solo, Some(other_id)));
        assert!(!preview_choice_matches_agent(&solo, None));
        assert!(preview_choice_matches_agent(&project, Some(other_id)));
        assert!(preview_choice_matches_agent(&project, None));
    }
}
