use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingProviderChoice {
    Claude,
    Codex,
    OpenCode,
    Gemini,
}
impl OnboardingTour {
    pub(super) fn render_provider_card(
        &self,
        id: &'static str,
        provider: OnboardingProviderChoice,
        agent_kind: ide_core::AgentKind,
        name: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.provider_choices.contains(&provider);
        let is_default = default_provider_choice(&self.provider_choices) == Some(provider);
        let (status_label, status_color) = match self
            .provider_connection_statuses
            .for_provider(provider)
        {
            ProviderConnectionStatus::Checking => ("Checking…", crate::ui::design::sky(cx)),
            ProviderConnectionStatus::Connected => ("Connected", crate::ui::design::sage(cx)),
            ProviderConnectionStatus::Ready => ("Ready", crate::ui::design::sage(cx)),
            ProviderConnectionStatus::NeedsSignIn => {
                ("Sign in needed", crate::ui::design::amber(cx))
            }
            ProviderConnectionStatus::NotInstalled => ("Not installed", crate::ui::design::t3(cx)),
            ProviderConnectionStatus::Unavailable => {
                ("Status unavailable", crate::ui::design::t3(cx))
            }
        };
        let tint = provider_tint(agent_kind, cx);

        style::dialog_choice_card_button(id, selected, cx)
            .h(px(PROVIDER_CARD_H))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h_full()
                    .px_3()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2p5()
                    .child(
                        // The brand mark on its own plate, seated the way the
                        // stack step seats its tool glyphs.
                        div()
                            .flex_none()
                            .size(px(38.))
                            .rounded(crate::ui::design::r_md())
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(tint.opacity(if selected { 0.16 } else { 0.10 }))
                            .text_color(crate::ui::design::t1(cx))
                            .child(
                                crate::ui::center::provider_brand_icon(agent_kind).size(px(20.)),
                            ),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(name),
                            )
                            .child(crate::ui::design::indicator::status(
                                status_label,
                                status_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .h(px(PROVIDER_BADGE_H))
                            .flex()
                            .items_center()
                            .children(is_default.then(|| {
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(crate::ui::design::accent_soft(cx))
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::accent(cx))
                                    .child("Default")
                            })),
                    )
                    // The tick sits in the corner rather than in the stack: it
                    // is the card's state, not another thing to read. Only the
                    // picked cards draw one — an empty ring on every card is
                    // three more circles competing with three brand glyphs.
                    .children(selected.then(|| {
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .size(px(16.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(crate::ui::design::accent(cx))
                            .child(
                                Icon::new(IconName::Check)
                                    .size(px(10.))
                                    .text_color(crate::ui::design::on_accent(cx)),
                            )
                    })),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.handle_event(OnboardingEvent::ProviderSelected(provider), cx)
            }))
    }
}

impl OnboardingProviderChoice {
    pub(super) const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::OpenCode, Self::Gemini];
    const DEFAULT_PRIORITY: [Self; 4] = [Self::Codex, Self::Claude, Self::OpenCode, Self::Gemini];

    pub(super) fn key(self) -> String {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
        }
        .to_string()
    }

    pub(super) fn from_key(key: &str) -> Option<Self> {
        match key {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

pub(super) fn default_provider_choice(
    choices: &[OnboardingProviderChoice],
) -> Option<OnboardingProviderChoice> {
    OnboardingProviderChoice::DEFAULT_PRIORITY
        .into_iter()
        .find(|provider| choices.contains(provider))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderConnectionStatus {
    Checking,
    Connected,
    Ready,
    NeedsSignIn,
    NotInstalled,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProviderConnectionStatuses {
    claude: ProviderConnectionStatus,
    codex: ProviderConnectionStatus,
    opencode: ProviderConnectionStatus,
    gemini: ProviderConnectionStatus,
}

impl Default for ProviderConnectionStatuses {
    fn default() -> Self {
        Self {
            claude: ProviderConnectionStatus::Checking,
            codex: ProviderConnectionStatus::Checking,
            opencode: ProviderConnectionStatus::Checking,
            gemini: ProviderConnectionStatus::Checking,
        }
    }
}

impl ProviderConnectionStatuses {
    pub(super) fn for_provider(
        self,
        provider: OnboardingProviderChoice,
    ) -> ProviderConnectionStatus {
        match provider {
            OnboardingProviderChoice::Claude => self.claude,
            OnboardingProviderChoice::Codex => self.codex,
            OnboardingProviderChoice::OpenCode => self.opencode,
            OnboardingProviderChoice::Gemini => self.gemini,
        }
    }
}

const PROVIDER_STATUS_TIMEOUT: Duration = Duration::from_secs(4);

fn provider_status_output(executable: &Path, arguments: &[&str]) -> Option<Output> {
    let mut child = Command::new(executable)
        .args(arguments)
        .env(
            "PATH",
            crate::state::agent_chat::protocol::agent_command_path_env(),
        )
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + PROVIDER_STATUS_TIMEOUT;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub(super) fn claude_connection_status(output: &Output) -> ProviderConnectionStatus {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("\"loggedIn\": true") {
        ProviderConnectionStatus::Connected
    } else if stdout.contains("\"loggedIn\": false") || !output.status.success() {
        ProviderConnectionStatus::NeedsSignIn
    } else {
        ProviderConnectionStatus::Unavailable
    }
}

pub(super) fn opencode_connection_status(output: &Output) -> ProviderConnectionStatus {
    if !output.status.success() {
        return ProviderConnectionStatus::NeedsSignIn;
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let fields = text.split_whitespace().collect::<Vec<_>>();
    let credential_count = fields.windows(2).find_map(|pair| {
        pair[1]
            .trim_matches(|character: char| !character.is_ascii_alphabetic())
            .starts_with("credential")
            .then(|| {
                pair[0]
                    .trim_matches(|character: char| !character.is_ascii_digit())
                    .parse::<usize>()
                    .ok()
            })
            .flatten()
    });
    match credential_count {
        Some(count) if count > 0 => ProviderConnectionStatus::Connected,
        Some(_) => ProviderConnectionStatus::NeedsSignIn,
        None => ProviderConnectionStatus::Unavailable,
    }
}

pub(super) fn detect_provider_connection_statuses() -> ProviderConnectionStatuses {
    let claude = crate::state::agent_chat::protocol::find_agent_cli_executable("claude")
        .map(|path| {
            provider_status_output(&path, &["auth", "status"])
                .as_ref()
                .map(claude_connection_status)
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);
    let codex = crate::state::agent_chat::protocol::find_agent_cli_executable("codex")
        .map(|path| {
            provider_status_output(&path, &["login", "status"])
                .map(|output| {
                    if output.status.success() {
                        ProviderConnectionStatus::Connected
                    } else {
                        ProviderConnectionStatus::NeedsSignIn
                    }
                })
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);
    let opencode = crate::state::agent_chat::protocol::find_opencode_executable()
        .map(|path| {
            provider_status_output(&path, &["auth", "list"])
                .as_ref()
                .map(opencode_connection_status)
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);

    let gemini = if crate::state::agent_chat::protocol::gemini::provider_available() {
        ProviderConnectionStatus::Ready
    } else {
        // Choro prepares the official Google provider on first use, then
        // displays a sign-in approval inside the conversation.
        ProviderConnectionStatus::NeedsSignIn
    };
    ProviderConnectionStatuses {
        gemini,
        claude,
        codex,
        opencode,
    }
}
