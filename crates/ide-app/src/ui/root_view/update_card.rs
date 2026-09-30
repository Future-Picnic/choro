use super::*;

use crate::app_update::{AppUpdatePhase, AvailableUpdate, UpdateCheckOrigin};

impl RootView {
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.app_update
            .update(cx, |updates, cx| updates.check_now(cx));
    }

    fn begin_update_shutdown(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.shutdown_state, ShutdownState::Idle) {
            return;
        }
        if !self
            .app_update
            .update(cx, |updates, cx| updates.restart_and_install(cx))
        {
            return;
        }
        self.shutdown_purpose = ShutdownPurpose::InstallUpdate;
        self.begin_shutdown(cx);
    }

    pub(super) fn render_update_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let onboarding_covers_application = self
            .onboarding
            .as_ref()
            .is_some_and(|onboarding| onboarding.read(cx).covers_application(cx));
        if onboarding_covers_application || !matches!(self.shutdown_state, ShutdownState::Idle) {
            return None;
        }
        let phase = self.app_update.read(cx).phase().clone();
        if !phase.is_visible() {
            return None;
        }

        let (icon, icon_color, title, message, progress) = match &phase {
            AppUpdatePhase::Checking {
                origin: UpdateCheckOrigin::Manual,
            } => (
                IconName::Loader,
                crate::ui::design::accent(cx),
                "Checking for updates…".to_string(),
                "Looking for the latest signed Choro release.".to_string(),
                None,
            ),
            AppUpdatePhase::Available(update) => (
                IconName::ArrowDown,
                crate::ui::design::accent(cx),
                format!("Choro {} is available", update.short_version),
                available_message(update, self.app_update.read(cx).current_version()),
                None,
            ),
            AppUpdatePhase::Downloading {
                update,
                downloaded,
                expected,
            } => {
                let ratio = expected
                    .filter(|expected| *expected > 0)
                    .map(|expected| (*downloaded as f32 / expected as f32).clamp(0.0, 1.0));
                let detail = expected.map_or_else(
                    || format!("{} downloaded", format_bytes(*downloaded)),
                    |expected| {
                        format!(
                            "{} of {} · {}%",
                            format_bytes(*downloaded),
                            format_bytes(expected),
                            (ratio.unwrap_or_default() * 100.0).round() as u32
                        )
                    },
                );
                (
                    IconName::ArrowDown,
                    crate::ui::design::accent(cx),
                    format!("Downloading Choro {}", update.short_version),
                    detail,
                    ratio,
                )
            }
            AppUpdatePhase::Extracting { update, progress } => (
                IconName::Loader,
                crate::ui::design::amber(cx),
                format!("Preparing Choro {}", update.short_version),
                progress.map_or_else(
                    || "Verifying and extracting the signed update…".to_string(),
                    |progress| format!("Preparing update · {}%", (progress * 100.0).round() as u32),
                ),
                *progress,
            ),
            AppUpdatePhase::ReadyToInstall(update) => (
                IconName::CircleCheck,
                crate::ui::design::sage(cx),
                "Ready to install".to_string(),
                format!(
                    "Choro {} is downloaded and verified. Restart to save your work and finish installing.",
                    update.short_version
                ),
                None,
            ),
            AppUpdatePhase::Installing(update) => (
                IconName::Loader,
                crate::ui::design::accent(cx),
                format!("Installing Choro {}…", update.short_version),
                "Choro will reopen when installation is complete.".to_string(),
                None,
            ),
            AppUpdatePhase::UpToDate => (
                IconName::CircleCheck,
                crate::ui::design::sage(cx),
                "Choro is up to date".to_string(),
                format!(
                    "You’re running Choro {}.",
                    self.app_update.read(cx).current_version()
                ),
                None,
            ),
            AppUpdatePhase::Failed { message, .. } => (
                IconName::TriangleAlert,
                crate::ui::design::rose(cx),
                "Update couldn’t finish".to_string(),
                message.to_string(),
                None,
            ),
            AppUpdatePhase::Idle | AppUpdatePhase::Checking { .. } => return None,
        };

        let title_row = h_flex()
            .flex_none()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_none()
                    .size(px(30.))
                    .rounded(crate::ui::design::r_sm())
                    .bg(icon_color.opacity(0.13))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(icon)
                            .size(crate::ui::design::icon_md())
                            .text_color(icon_color),
                    ),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .flex_1()
                    .text_size(crate::ui::design::text_title())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child(title),
            );

        // Keep wrapped copy outside the icon row. GPUI may otherwise size the
        // row from the fixed-height icon and let long text paint into the
        // progress/actions below it.
        let message = div()
            .ml(px(42.))
            .min_w(px(0.))
            .text_size(crate::ui::design::text_ui())
            .line_height(gpui::relative(1.42))
            .text_color(crate::ui::design::t3(cx))
            .child(message);

        let mut card = v_flex()
            .id("app-update-card")
            .absolute()
            .left(px(16.))
            .bottom(px(16.))
            .w(px(336.))
            .max_w(gpui::relative(0.92))
            .occlude()
            .gap_3()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.72))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_4()
            .child(title_row)
            .child(message);

        if let Some(progress) = progress {
            card = card.child(
                div()
                    .w_full()
                    .h(px(3.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(crate::ui::design::line_2(cx).opacity(0.44))
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(progress.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(crate::ui::design::accent(cx)),
                    ),
            );
        }

        if let Some(actions) = self.render_update_actions(&phase, cx) {
            card = card.child(actions);
        }
        Some(card.into_any_element())
    }

    fn render_update_actions(
        &self,
        phase: &AppUpdatePhase,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let row = h_flex().w_full().justify_end().gap_2();
        Some(match phase {
            AppUpdatePhase::Available(_) => row
                .child(
                    style::dialog_neutral_button("update-later", "Later", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.dismiss(cx));
                        }),
                    ),
                )
                .child(
                    style::primary_button_compact("update-download", "Update", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.download(cx));
                        }),
                    ),
                )
                .into_any_element(),
            AppUpdatePhase::Downloading { .. } => row
                .child(
                    style::dialog_neutral_button("update-cancel", "Cancel", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.cancel_download(cx));
                        }),
                    ),
                )
                .into_any_element(),
            AppUpdatePhase::ReadyToInstall(_) => row
                .child(
                    style::dialog_neutral_button("update-not-now", "Not now", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.cancel_ready(cx));
                        }),
                    ),
                )
                .child(
                    style::primary_button_compact(
                        "update-restart-install",
                        "Restart & install",
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.begin_update_shutdown(cx))),
                )
                .into_any_element(),
            AppUpdatePhase::Failed { retryable, .. } => row
                .child(
                    style::dialog_neutral_button("update-dismiss-error", "Dismiss", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.dismiss(cx));
                        }),
                    ),
                )
                .when(*retryable, |row| {
                    row.child(
                        style::primary_button_compact("update-retry", "Retry", cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.app_update.update(cx, |updates, cx| updates.retry(cx));
                            }),
                        ),
                    )
                })
                .into_any_element(),
            AppUpdatePhase::UpToDate => row
                .child(
                    style::dialog_neutral_button("update-dismiss-current", "Dismiss", cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.app_update
                                .update(cx, |updates, cx| updates.dismiss(cx));
                        }),
                    ),
                )
                .into_any_element(),
            _ => return None,
        })
    }
}

fn available_message(update: &AvailableUpdate, current: &SharedString) -> String {
    let version_line = format!("You’re on {current}. Update to {}.", update.short_version);
    let Some(notes) = release_note_excerpt(&update.notes) else {
        return version_line;
    };
    format!("{version_line} {notes}")
}

fn release_note_excerpt(notes: &str) -> Option<String> {
    const MAX_CHARS: usize = 92;
    const INTERNAL_PREFIXES: [&str; 2] = ["chore: publish update feed", "chore: release"];
    const CHANGE_PREFIXES: [&str; 5] = ["feat:", "fix:", "perf:", "refactor:", "docs:"];

    let note = notes.lines().find_map(|line| {
        let line = line
            .trim()
            .trim_start_matches(|character: char| matches!(character, '-' | '*' | '•'))
            .trim();
        if line.is_empty()
            || INTERNAL_PREFIXES
                .iter()
                .any(|prefix| line.to_ascii_lowercase().starts_with(prefix))
        {
            return None;
        }
        let lower = line.to_ascii_lowercase();
        let line = CHANGE_PREFIXES
            .iter()
            .find_map(|prefix| {
                lower
                    .starts_with(prefix)
                    .then(|| line[prefix.len()..].trim())
            })
            .unwrap_or(line);
        (!line.is_empty()).then_some(line)
    })?;

    let mut excerpt = note.chars().take(MAX_CHARS + 1).collect::<String>();
    if excerpt.chars().count() > MAX_CHARS {
        excerpt = excerpt.chars().take(MAX_CHARS).collect();
        if let Some(last_space) = excerpt.rfind(char::is_whitespace) {
            excerpt.truncate(last_space);
        }
        excerpt.push('…');
    }
    Some(excerpt)
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_progress_copy_uses_readable_units() {
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_536), "2 KB");
    }

    #[test]
    fn release_note_excerpt_prefers_user_facing_copy() {
        assert_eq!(
            release_note_excerpt(
                "- chore: publish update feed for 0.89\n- fix: Complete silent Sparkle update checks"
            )
            .as_deref(),
            Some("Complete silent Sparkle update checks")
        );
    }

    #[test]
    fn release_note_excerpt_stays_compact() {
        let excerpt = release_note_excerpt(
            "feat: A deliberately long release note that should remain readable without allowing the update card to take over the workspace around it",
        )
        .expect("release note excerpt");
        assert!(excerpt.chars().count() <= 93);
        assert!(excerpt.ends_with('…'));
    }
}
