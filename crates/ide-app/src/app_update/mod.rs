#[cfg(target_os = "macos")]
mod native;

use std::time::Duration;

use anyhow::{anyhow, Result};
use gpui::{Context, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCheckOrigin {
    Automatic,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub short_version: SharedString,
    pub build_number: SharedString,
    pub notes: SharedString,
    pub archive_size: Option<u64>,
    pub current_bundle_path: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppUpdatePhase {
    Idle,
    Checking {
        origin: UpdateCheckOrigin,
    },
    Available(AvailableUpdate),
    Downloading {
        update: AvailableUpdate,
        downloaded: u64,
        expected: Option<u64>,
    },
    Extracting {
        update: AvailableUpdate,
        progress: Option<f32>,
    },
    ReadyToInstall(AvailableUpdate),
    Installing(AvailableUpdate),
    UpToDate,
    Failed {
        message: SharedString,
        retryable: bool,
    },
}

impl AppUpdatePhase {
    pub fn is_visible(&self) -> bool {
        !matches!(
            self,
            Self::Idle
                | Self::Checking {
                    origin: UpdateCheckOrigin::Automatic
                }
        )
    }

    fn kind(&self) -> PhaseKind {
        match self {
            Self::Idle => PhaseKind::Idle,
            Self::Checking { .. } => PhaseKind::Checking,
            Self::Available(_) => PhaseKind::Available,
            Self::Downloading { .. } => PhaseKind::Downloading,
            Self::Extracting { .. } => PhaseKind::Extracting,
            Self::ReadyToInstall(_) => PhaseKind::ReadyToInstall,
            Self::Installing(_) => PhaseKind::Installing,
            Self::UpToDate => PhaseKind::UpToDate,
            Self::Failed { .. } => PhaseKind::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhaseKind {
    Idle,
    Checking,
    Available,
    Downloading,
    Extracting,
    ReadyToInstall,
    Installing,
    UpToDate,
    Failed,
}

#[derive(Debug)]
enum BridgeEvent {
    ManualCheckStarted,
    UpdateFound {
        short_version: String,
        build_number: String,
        notes: String,
        archive_size: Option<u64>,
        ready_to_install: bool,
    },
    NoUpdate {
        manual: bool,
    },
    Error {
        message: String,
        manual: bool,
    },
    DownloadStarted,
    ExpectedLength(u64),
    ReceivedData(u64),
    Extracting,
    ExtractionProgress(f32),
    Ready,
    Installing,
    Dismissed,
}

impl BridgeEvent {
    fn from_native(event: i32, primary: String, secondary: String, value: u64) -> Option<Self> {
        Some(match event {
            1 => Self::ManualCheckStarted,
            2 | 13 => {
                let (build_number, notes) = secondary.split_once('\n').unwrap_or((&secondary, ""));
                Self::UpdateFound {
                    short_version: primary,
                    build_number: build_number.to_string(),
                    notes: notes.trim().to_string(),
                    archive_size: (value > 0).then_some(value),
                    ready_to_install: event == 13,
                }
            }
            3 => Self::NoUpdate { manual: value != 0 },
            4 => Self::Error {
                message: if secondary.trim().is_empty() {
                    primary
                } else {
                    format!("{primary} {secondary}")
                },
                manual: value != 0,
            },
            5 => Self::DownloadStarted,
            6 => Self::ExpectedLength(value),
            7 => Self::ReceivedData(value),
            8 => Self::Extracting,
            9 => Self::ExtractionProgress((value as f32 / 10_000.0).clamp(0.0, 1.0)),
            10 => Self::Ready,
            11 => Self::Installing,
            12 => Self::Dismissed,
            _ => return None,
        })
    }

    fn kind(&self) -> BridgeEventKind {
        match self {
            Self::ManualCheckStarted => BridgeEventKind::ManualCheckStarted,
            Self::UpdateFound { .. } => BridgeEventKind::UpdateFound,
            Self::NoUpdate { .. } => BridgeEventKind::NoUpdate,
            Self::Error { .. } => BridgeEventKind::Error,
            Self::DownloadStarted => BridgeEventKind::DownloadStarted,
            Self::ExpectedLength(_) => BridgeEventKind::ExpectedLength,
            Self::ReceivedData(_) => BridgeEventKind::ReceivedData,
            Self::Extracting => BridgeEventKind::Extracting,
            Self::ExtractionProgress(_) => BridgeEventKind::ExtractionProgress,
            Self::Ready => BridgeEventKind::Ready,
            Self::Installing => BridgeEventKind::Installing,
            Self::Dismissed => BridgeEventKind::Dismissed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BridgeEventKind {
    ManualCheckStarted,
    UpdateFound,
    NoUpdate,
    Error,
    DownloadStarted,
    ExpectedLength,
    ReceivedData,
    Extracting,
    ExtractionProgress,
    Ready,
    Installing,
    Dismissed,
}

fn bridge_transition_allowed(phase: PhaseKind, event: BridgeEventKind) -> bool {
    use BridgeEventKind as Event;
    use PhaseKind as Phase;

    match event {
        Event::Error | Event::Dismissed => true,
        Event::ManualCheckStarted => matches!(
            phase,
            Phase::Idle | Phase::Checking | Phase::UpToDate | Phase::Failed
        ),
        Event::UpdateFound | Event::NoUpdate => matches!(phase, Phase::Idle | Phase::Checking),
        Event::DownloadStarted => matches!(phase, Phase::Available | Phase::Downloading),
        Event::ExpectedLength | Event::ReceivedData => phase == Phase::Downloading,
        Event::Extracting => matches!(
            phase,
            Phase::Available | Phase::Downloading | Phase::Extracting
        ),
        Event::ExtractionProgress => phase == Phase::Extracting,
        Event::Ready => matches!(
            phase,
            Phase::Available | Phase::Downloading | Phase::Extracting | Phase::ReadyToInstall
        ),
        Event::Installing => matches!(
            phase,
            Phase::Available
                | Phase::Downloading
                | Phase::Extracting
                | Phase::ReadyToInstall
                | Phase::Installing
        ),
    }
}

fn version_is_suppressed(dismissed: Option<&str>, offered: &str, manual_check: bool) -> bool {
    !manual_check && dismissed.is_some_and(|dismissed| dismissed == offered)
}

fn update_session_is_active(phase: &AppUpdatePhase) -> bool {
    matches!(
        phase,
        AppUpdatePhase::Checking { .. }
            | AppUpdatePhase::Available(_)
            | AppUpdatePhase::Downloading { .. }
            | AppUpdatePhase::Extracting { .. }
            | AppUpdatePhase::ReadyToInstall(_)
            | AppUpdatePhase::Installing(_)
    )
}

fn error_is_visible(phase: &AppUpdatePhase, native_reported_manual: bool) -> bool {
    native_reported_manual
        || matches!(
            phase,
            AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Manual
            } | AppUpdatePhase::Available(_)
                | AppUpdatePhase::Downloading { .. }
                | AppUpdatePhase::Extracting { .. }
                | AppUpdatePhase::ReadyToInstall(_)
                | AppUpdatePhase::Installing(_)
        )
}

pub struct AppUpdateController {
    phase: AppUpdatePhase,
    current_version: SharedString,
    current_bundle_path: SharedString,
    available: Option<AvailableUpdate>,
    dismissed_version: Option<SharedString>,
    native: Option<native::NativeUpdater>,
    credential_check_in_progress: bool,
    credential_retry_started: bool,
    pending_origin: UpdateCheckOrigin,
    notice_generation: u64,
    normal_quit_deferred: bool,
    normal_quit_ready: bool,
}

impl AppUpdateController {
    pub fn new() -> Self {
        Self {
            phase: AppUpdatePhase::Idle,
            current_version: native::bundle_short_version().into(),
            current_bundle_path: native::bundle_path().into(),
            available: None,
            dismissed_version: None,
            native: None,
            credential_check_in_progress: false,
            credential_retry_started: false,
            pending_origin: UpdateCheckOrigin::Automatic,
            notice_generation: 0,
            normal_quit_deferred: false,
            normal_quit_ready: false,
        }
    }

    pub fn phase(&self) -> &AppUpdatePhase {
        &self.phase
    }

    pub fn current_version(&self) -> &SharedString {
        &self.current_version
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if cfg!(debug_assertions) && std::env::var_os("CHORO_UPDATE_FEED_URL").is_none() {
            return;
        }
        self.ensure_started(UpdateCheckOrigin::Automatic, cx);
    }

    pub fn check_now(&mut self, cx: &mut Context<Self>) {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        if matches!(self.phase, AppUpdatePhase::Checking { .. }) {
            // Upgrade an in-flight automatic check to user-visible behavior;
            // its result will satisfy this manual request without starting a
            // second Sparkle session.
            self.phase = AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Manual,
            };
            cx.notify();
            return;
        }
        if update_session_is_active(&self.phase) {
            cx.notify();
            return;
        }
        if !self.validate_installation_location(UpdateCheckOrigin::Manual, cx) {
            return;
        }
        self.phase = AppUpdatePhase::Checking {
            origin: UpdateCheckOrigin::Manual,
        };
        cx.notify();
        if let Some(native) = &self.native {
            native.check();
        } else {
            self.ensure_started(UpdateCheckOrigin::Manual, cx);
        }
    }

    pub fn download(&mut self, cx: &mut Context<Self>) {
        let Some(update) = self.available.clone() else {
            return;
        };
        if self
            .native
            .as_ref()
            .is_some_and(native::NativeUpdater::download)
        {
            self.phase = AppUpdatePhase::Downloading {
                expected: update.archive_size,
                update,
                downloaded: 0,
            };
        } else {
            self.phase = AppUpdatePhase::Failed {
                message: "The update session expired. Check again to retry.".into(),
                retryable: true,
            };
        }
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        if matches!(self.phase, AppUpdatePhase::Available(_)) {
            if let Some(update) = self.available.as_ref() {
                self.dismissed_version = Some(update.short_version.clone());
            }
            if let Some(native) = &self.native {
                let _ = native.dismiss_offer();
            }
        }
        self.phase = AppUpdatePhase::Idle;
        cx.notify();
    }

    pub fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if self
            .native
            .as_ref()
            .is_some_and(native::NativeUpdater::cancel_download)
        {
            self.phase = AppUpdatePhase::Idle;
        } else {
            self.phase = AppUpdatePhase::Failed {
                message: "The download can no longer be cancelled safely.".into(),
                retryable: false,
            };
        }
        cx.notify();
    }

    pub fn cancel_ready(&mut self, cx: &mut Context<Self>) {
        if let Some(update) = self.available.as_ref() {
            self.dismissed_version = Some(update.short_version.clone());
        }
        if self
            .native
            .as_ref()
            .is_some_and(native::NativeUpdater::cancel_ready)
        {
            self.phase = AppUpdatePhase::Idle;
        } else {
            self.phase = AppUpdatePhase::Failed {
                message: "The prepared update could not be cancelled.".into(),
                retryable: false,
            };
        }
        cx.notify();
    }

    pub fn retry(&mut self, cx: &mut Context<Self>) {
        self.check_now(cx);
    }

    /// Confirms that a prepared update can enter Choro's safe shutdown flow.
    /// This deliberately does not reply to Sparkle's ready callback; the reply
    /// is deferred until `finish_restart_and_install` after saving and process
    /// shutdown complete.
    pub fn restart_and_install(&mut self, cx: &mut Context<Self>) -> bool {
        if !matches!(self.phase, AppUpdatePhase::ReadyToInstall(_)) {
            self.phase = AppUpdatePhase::Failed {
                message: "The prepared update is no longer ready to install.".into(),
                retryable: true,
            };
            cx.notify();
            return false;
        }
        self.validate_installation_location(UpdateCheckOrigin::Manual, cx)
    }

    pub(crate) fn finish_restart_and_install(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let Some(update) = self.available.clone() else {
            let message = "The prepared update is no longer available";
            self.phase = AppUpdatePhase::Failed {
                message: message.into(),
                retryable: true,
            };
            cx.notify();
            return Err(anyhow!(message));
        };
        if !self
            .native
            .as_ref()
            .is_some_and(native::NativeUpdater::install_ready)
        {
            let message = "Sparkle no longer has a prepared update to install";
            self.phase = AppUpdatePhase::Failed {
                message: message.into(),
                retryable: true,
            };
            cx.notify();
            return Err(anyhow!(message));
        }
        self.phase = AppUpdatePhase::Installing(update);
        cx.notify();
        Ok(())
    }

    /// Prevents an ordinary quit from turning a staged Sparkle session into an
    /// implicit installation. Extraction cannot be cancelled safely, so Choro
    /// briefly defers quitting until Sparkle reaches its explicit ready choice.
    pub fn prepare_for_normal_quit(&mut self, cx: &mut Context<Self>) -> bool {
        self.normal_quit_ready = false;
        let may_quit = match self.phase {
            AppUpdatePhase::Available(_) => self
                .native
                .as_ref()
                .is_none_or(native::NativeUpdater::dismiss_offer),
            AppUpdatePhase::Downloading { .. } => self
                .native
                .as_ref()
                .is_none_or(native::NativeUpdater::cancel_download),
            AppUpdatePhase::ReadyToInstall(_) => self
                .native
                .as_ref()
                .is_none_or(native::NativeUpdater::cancel_ready),
            AppUpdatePhase::Extracting { .. } | AppUpdatePhase::Installing(_) => {
                self.normal_quit_deferred = true;
                false
            }
            _ => true,
        };
        if may_quit {
            self.normal_quit_deferred = false;
            self.phase = AppUpdatePhase::Idle;
            cx.notify();
        } else if matches!(self.phase, AppUpdatePhase::Downloading { .. }) {
            // Sparkle may advance from downloading to extraction between the
            // GPUI state snapshot and the cancellation request. In that race,
            // wait for Ready and answer Skip before continuing the quit.
            self.normal_quit_deferred = true;
        }
        may_quit
    }

    pub(crate) fn normal_quit_is_deferred(&self) -> bool {
        self.normal_quit_deferred
    }

    pub(crate) fn normal_quit_can_resume(&self) -> bool {
        self.normal_quit_ready
    }

    fn ensure_started(&mut self, origin: UpdateCheckOrigin, cx: &mut Context<Self>) {
        if !self.validate_installation_location(origin, cx) {
            return;
        }
        if self.native.is_some() {
            return;
        }
        if origin == UpdateCheckOrigin::Manual {
            self.pending_origin = UpdateCheckOrigin::Manual;
        }
        if self.credential_check_in_progress {
            return;
        }
        self.pending_origin = origin;
        self.credential_check_in_progress = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let credential = executor
                .spawn(async { ide_core::git::choro_release_credential() })
                .await;
            this.update(cx, |this, cx| {
                this.credential_check_in_progress = false;
                let origin = this.pending_origin;
                this.pending_origin = UpdateCheckOrigin::Automatic;
                match credential {
                    Ok(credential) => {
                        this.phase = AppUpdatePhase::Checking { origin };
                        cx.notify();
                        let start = native::NativeUpdater::start(
                            credential,
                            origin == UpdateCheckOrigin::Manual,
                        );
                        this.finish_native_start(start, origin, cx);
                    }
                    Err(error) => {
                        if origin == UpdateCheckOrigin::Manual {
                            this.phase = AppUpdatePhase::Failed {
                                message: error.to_string().into(),
                                retryable: true,
                            };
                            cx.notify();
                        }
                        this.schedule_credential_retry(cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_native_start(
        &mut self,
        start: Result<(native::NativeUpdater, async_channel::Receiver<BridgeEvent>)>,
        origin: UpdateCheckOrigin,
        cx: &mut Context<Self>,
    ) {
        match start {
            Ok((native, receiver)) => {
                self.native = Some(native);
                cx.spawn(async move |this, cx| {
                    while let Ok(event) = receiver.recv().await {
                        if this
                            .update(cx, |this, cx| this.handle_event(event, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => {
                if origin == UpdateCheckOrigin::Manual {
                    self.phase = AppUpdatePhase::Failed {
                        message: error.to_string().into(),
                        retryable: true,
                    };
                    cx.notify();
                } else {
                    self.phase = AppUpdatePhase::Idle;
                    cx.notify();
                }
                self.schedule_credential_retry(cx);
            }
        }
    }

    fn validate_installation_location(
        &mut self,
        origin: UpdateCheckOrigin,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(message) = native::installation_error() else {
            return true;
        };
        self.phase = if origin == UpdateCheckOrigin::Manual {
            AppUpdatePhase::Failed {
                message: message.into(),
                retryable: true,
            }
        } else {
            AppUpdatePhase::Idle
        };
        cx.notify();
        false
    }

    fn schedule_credential_retry(&mut self, cx: &mut Context<Self>) {
        if self.credential_retry_started {
            return;
        }
        self.credential_retry_started = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(3600))
                .await;
            let continue_retrying = this
                .update(cx, |this, cx| {
                    if this.native.is_some() {
                        this.credential_retry_started = false;
                        false
                    } else {
                        this.ensure_started(UpdateCheckOrigin::Automatic, cx);
                        true
                    }
                })
                .unwrap_or(false);
            if !continue_retrying {
                break;
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        if !bridge_transition_allowed(self.phase.kind(), event.kind()) {
            return;
        }
        match event {
            BridgeEvent::ManualCheckStarted => {
                self.phase = AppUpdatePhase::Checking {
                    origin: UpdateCheckOrigin::Manual,
                };
            }
            BridgeEvent::UpdateFound {
                short_version,
                build_number,
                notes,
                archive_size,
                ready_to_install,
            } => {
                let manual_check = matches!(
                    self.phase,
                    AppUpdatePhase::Checking {
                        origin: UpdateCheckOrigin::Manual
                    }
                );
                if version_is_suppressed(
                    self.dismissed_version
                        .as_ref()
                        .map(|version| version.as_ref()),
                    &short_version,
                    manual_check || ready_to_install,
                ) {
                    if let Some(native) = &self.native {
                        let _ = native.dismiss_offer();
                    }
                    self.phase = AppUpdatePhase::Idle;
                    cx.notify();
                    return;
                }
                let update = AvailableUpdate {
                    short_version: short_version.into(),
                    build_number: build_number.into(),
                    notes: notes.into(),
                    archive_size,
                    current_bundle_path: self.current_bundle_path.clone(),
                };
                self.available = Some(update.clone());
                self.phase = if ready_to_install {
                    AppUpdatePhase::ReadyToInstall(update)
                } else {
                    AppUpdatePhase::Available(update)
                };
            }
            BridgeEvent::NoUpdate { manual } => {
                if manual
                    || matches!(
                        self.phase,
                        AppUpdatePhase::Checking {
                            origin: UpdateCheckOrigin::Manual
                        }
                    )
                {
                    self.phase = AppUpdatePhase::UpToDate;
                    self.schedule_notice_dismiss(cx);
                } else {
                    self.phase = AppUpdatePhase::Idle;
                }
            }
            BridgeEvent::Error { message, manual } => {
                if self.normal_quit_deferred {
                    self.normal_quit_deferred = false;
                    self.normal_quit_ready = true;
                }
                let user_was_aware = error_is_visible(&self.phase, manual);
                self.phase = if user_was_aware {
                    AppUpdatePhase::Failed {
                        message: concise_error(&message).into(),
                        retryable: true,
                    }
                } else {
                    AppUpdatePhase::Idle
                };
            }
            BridgeEvent::DownloadStarted => {
                if let Some(update) = self.available.clone() {
                    self.phase = AppUpdatePhase::Downloading {
                        expected: update.archive_size,
                        update,
                        downloaded: 0,
                    };
                }
            }
            BridgeEvent::ExpectedLength(length) => {
                if let AppUpdatePhase::Downloading { expected, .. } = &mut self.phase {
                    *expected = (length > 0).then_some(length);
                }
            }
            BridgeEvent::ReceivedData(length) => {
                if let AppUpdatePhase::Downloading { downloaded, .. } = &mut self.phase {
                    *downloaded = downloaded.saturating_add(length);
                }
            }
            BridgeEvent::Extracting => {
                if let Some(update) = self.available.clone() {
                    self.phase = AppUpdatePhase::Extracting {
                        update,
                        progress: None,
                    };
                }
            }
            BridgeEvent::ExtractionProgress(value) => {
                if let AppUpdatePhase::Extracting { progress, .. } = &mut self.phase {
                    *progress = Some(value);
                }
            }
            BridgeEvent::Ready => {
                if self.normal_quit_deferred {
                    if self
                        .native
                        .as_ref()
                        .is_some_and(native::NativeUpdater::cancel_ready)
                    {
                        self.normal_quit_deferred = false;
                        self.normal_quit_ready = true;
                        self.phase = AppUpdatePhase::Idle;
                    } else {
                        self.normal_quit_deferred = false;
                        self.phase = AppUpdatePhase::Failed {
                            message: "The prepared update could not be cancelled before quitting. Try closing Choro again."
                                .into(),
                            retryable: false,
                        };
                    }
                } else if let Some(update) = self.available.clone() {
                    self.phase = AppUpdatePhase::ReadyToInstall(update);
                }
            }
            BridgeEvent::Installing => {
                if let Some(update) = self.available.clone() {
                    self.phase = AppUpdatePhase::Installing(update);
                }
            }
            BridgeEvent::Dismissed => {
                if self.normal_quit_deferred {
                    self.normal_quit_deferred = false;
                    self.normal_quit_ready = true;
                }
                if matches!(
                    self.phase,
                    AppUpdatePhase::Checking { .. }
                        | AppUpdatePhase::Available(_)
                        | AppUpdatePhase::Downloading { .. }
                        | AppUpdatePhase::Extracting { .. }
                        | AppUpdatePhase::ReadyToInstall(_)
                ) {
                    self.phase = AppUpdatePhase::Idle;
                }
            }
        }
        cx.notify();
    }

    fn schedule_notice_dismiss(&mut self, cx: &mut Context<Self>) {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        let generation = self.notice_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            this.update(cx, |this, cx| {
                if this.notice_generation == generation
                    && matches!(this.phase, AppUpdatePhase::UpToDate)
                {
                    this.phase = AppUpdatePhase::Idle;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

fn concise_error(message: &str) -> String {
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        "The update could not be completed. Check your connection and try again.".to_string()
    } else {
        message.chars().take(240).collect()
    }
}

#[cfg(test)]
fn parse_product_version(version: &str) -> Result<u64> {
    let Some(number) = version.strip_prefix("0.") else {
        return Err(anyhow!("Choro versions must use the 0.N format"));
    };
    if number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || number.starts_with('0')
    {
        return Err(anyhow!("Choro versions must use the 0.N format"));
    }
    number
        .parse()
        .map_err(|_| anyhow!("Choro version is too large"))
}

#[cfg(test)]
fn next_product_version(version: &str) -> Result<String> {
    let next = parse_product_version(version)?
        .checked_add(1)
        .ok_or_else(|| anyhow!("Choro version is too large"))?;
    Ok(format!("0.{next}"))
}

#[cfg(test)]
fn validate_next_product_version(current: &str, candidate: &str) -> Result<u64> {
    let current = parse_product_version(current)?;
    let candidate = parse_product_version(candidate)?;
    let expected = current
        .checked_add(1)
        .ok_or_else(|| anyhow!("Choro version is too large"))?;
    if candidate != expected {
        return Err(anyhow!(
            "The next sequential Choro build is {expected}, not {candidate}"
        ));
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_versions_are_sequential_numeric_builds() {
        assert_eq!(parse_product_version("0.89").unwrap(), 89);
        assert_eq!(next_product_version("0.99").unwrap(), "0.100");
    }

    #[test]
    fn malformed_product_versions_are_rejected() {
        for version in ["", "1.0", "0.0", "0.089", "0.8.9", "0.next"] {
            assert!(parse_product_version(version).is_err(), "{version}");
        }
    }

    #[test]
    fn duplicate_lower_and_skipped_product_versions_are_rejected() {
        assert_eq!(validate_next_product_version("0.89", "0.90").unwrap(), 90);
        for candidate in ["0.89", "0.88", "0.87", "0.91", "0.100"] {
            assert!(
                validate_next_product_version("0.89", candidate).is_err(),
                "unexpectedly accepted {candidate}"
            );
        }
    }

    #[test]
    fn native_events_decode_update_metadata_and_progress() {
        let event =
            BridgeEvent::from_native(2, "0.90".into(), "90\nA safer updater".into(), 42).unwrap();
        assert!(
            matches!(event, BridgeEvent::UpdateFound { short_version, build_number, notes, archive_size: Some(42), ready_to_install: false } if short_version == "0.90" && build_number == "90" && notes == "A safer updater")
        );
        assert!(matches!(
            BridgeEvent::from_native(13, "0.90".into(), "90\nAlready prepared".into(), 42),
            Some(BridgeEvent::UpdateFound {
                ready_to_install: true,
                ..
            })
        ));
        assert!(
            matches!(BridgeEvent::from_native(9, String::new(), String::new(), 2_500), Some(BridgeEvent::ExtractionProgress(value)) if value == 0.25)
        );
    }

    #[test]
    fn updater_transition_policy_is_exhaustive() {
        use BridgeEventKind as Event;
        use PhaseKind as Phase;

        let phases = [
            Phase::Idle,
            Phase::Checking,
            Phase::Available,
            Phase::Downloading,
            Phase::Extracting,
            Phase::ReadyToInstall,
            Phase::Installing,
            Phase::UpToDate,
            Phase::Failed,
        ];
        let events = [
            Event::ManualCheckStarted,
            Event::UpdateFound,
            Event::NoUpdate,
            Event::Error,
            Event::DownloadStarted,
            Event::ExpectedLength,
            Event::ReceivedData,
            Event::Extracting,
            Event::ExtractionProgress,
            Event::Ready,
            Event::Installing,
            Event::Dismissed,
        ];

        for phase in phases {
            for event in events {
                let expected = match event {
                    Event::Error | Event::Dismissed => true,
                    Event::ManualCheckStarted => matches!(
                        phase,
                        Phase::Idle | Phase::Checking | Phase::UpToDate | Phase::Failed
                    ),
                    Event::UpdateFound | Event::NoUpdate => {
                        matches!(phase, Phase::Idle | Phase::Checking)
                    }
                    Event::DownloadStarted => {
                        matches!(phase, Phase::Available | Phase::Downloading)
                    }
                    Event::ExpectedLength | Event::ReceivedData => phase == Phase::Downloading,
                    Event::Extracting => matches!(
                        phase,
                        Phase::Available | Phase::Downloading | Phase::Extracting
                    ),
                    Event::ExtractionProgress => phase == Phase::Extracting,
                    Event::Ready => matches!(
                        phase,
                        Phase::Available
                            | Phase::Downloading
                            | Phase::Extracting
                            | Phase::ReadyToInstall
                    ),
                    Event::Installing => matches!(
                        phase,
                        Phase::Available
                            | Phase::Downloading
                            | Phase::Extracting
                            | Phase::ReadyToInstall
                            | Phase::Installing
                    ),
                };
                assert_eq!(
                    bridge_transition_allowed(phase, event),
                    expected,
                    "unexpected transition policy for {phase:?} + {event:?}"
                );
            }
        }
    }

    #[test]
    fn dismissal_is_session_only_and_does_not_hide_newer_or_manual_offers() {
        assert!(version_is_suppressed(Some("0.90"), "0.90", false));
        assert!(!version_is_suppressed(Some("0.90"), "0.91", false));
        assert!(!version_is_suppressed(Some("0.90"), "0.90", true));
        assert!(!version_is_suppressed(None, "0.90", false));
    }

    #[test]
    fn only_idle_notices_and_failures_can_start_a_new_update_session() {
        let update = AvailableUpdate {
            short_version: "0.90".into(),
            build_number: "90".into(),
            notes: "Ready".into(),
            archive_size: Some(42),
            current_bundle_path: "/Applications/Choro.app".into(),
        };
        for phase in [
            AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Automatic,
            },
            AppUpdatePhase::Available(update.clone()),
            AppUpdatePhase::Downloading {
                update: update.clone(),
                downloaded: 1,
                expected: Some(42),
            },
            AppUpdatePhase::Extracting {
                update: update.clone(),
                progress: Some(0.5),
            },
            AppUpdatePhase::ReadyToInstall(update.clone()),
            AppUpdatePhase::Installing(update),
        ] {
            assert!(update_session_is_active(&phase));
        }
        assert!(!update_session_is_active(&AppUpdatePhase::Idle));
        assert!(!update_session_is_active(&AppUpdatePhase::UpToDate));
        assert!(!update_session_is_active(&AppUpdatePhase::Failed {
            message: "Try again".into(),
            retryable: true,
        }));
    }

    #[test]
    fn automatic_errors_stay_silent_while_manual_errors_are_visible() {
        assert!(!error_is_visible(&AppUpdatePhase::Idle, false));
        assert!(!error_is_visible(
            &AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Automatic,
            },
            false,
        ));
        assert!(error_is_visible(
            &AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Manual,
            },
            false,
        ));
        assert!(error_is_visible(&AppUpdatePhase::Idle, true));
        assert!(error_is_visible(
            &AppUpdatePhase::Installing(AvailableUpdate {
                short_version: "0.90".into(),
                build_number: "90".into(),
                notes: "Ready".into(),
                archive_size: Some(42),
                current_bundle_path: "/Applications/Choro.app".into(),
            }),
            false,
        ));
    }
}
