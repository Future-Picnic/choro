use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    div, img, point, prelude::FluentBuilder, px, size, App, AppContext, Bounds, Context, Entity,
    InteractiveElement, IntoElement, MouseButton, ObjectFit, ParentElement, PlatformDisplay,
    Render, Resource, RetainAllImageCache, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions,
};
use gpui_component::{
    h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    v_flex, Disableable, IconName, Root,
};
use ide_core::{AgentRuntimeKind, ProjectId};
use uuid::Uuid;

use crate::companion_music::CompanionMusicState;
use crate::state::agent_chat::{AgentChatMessage, AgentChatStatus, AgentChatTimelineItem};
use crate::state::{AgentChatState, AgentRecords, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::project_list::ProjectList;
use crate::ui::settings::{ProcessInfo, ProjectSource, SettingsView};
use crate::voice::{VoiceEvent, VoicePhase, VoiceState};

const WINDOW_WIDTH: f32 = 330.0;
const AVATAR_CANVAS_SIZE: f32 = 165.0;
const AVATAR_BASE_HEIGHT: f32 = AVATAR_CANVAS_SIZE + 45.0;
const COMPANION_CONTROLS_HEIGHT: f32 = 35.0;
const COMPANION_CONTROLS_WIDTH: f32 = 156.0;
const ASSISTANT_STATUS_WIDTH: f32 = 220.0;
#[cfg(test)]
const ANIMATION_CANVAS_WIDTH: u64 = 320;
#[cfg(test)]
const ANIMATION_CANVAS_HEIGHT: u64 = 320;
const ATTENTION_ROW_HEIGHT: f32 = 48.0;
const ATTENTION_ROW_STEP: f32 = 54.0;
const ATTENTION_PREVIEW_LIMIT: usize = 3;
const ATTENTION_OVERFLOW_STEP: f32 = 36.0;
const MUSIC_MENU_HEIGHT: f32 = 180.0;
const PROCESS_MONITOR_HEIGHT: f32 = 246.0;
const PROCESS_MONITOR_LIMIT: usize = 5;
const PROCESS_MONITOR_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const PERSISTENT_MOTION_FRAME_COUNT: usize = 8;
const IDLE_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-idle-medium-0.webp",
    "avatar/choro-companion-idle-medium-1.webp",
    "avatar/choro-companion-idle-medium-2.webp",
    "avatar/choro-companion-idle-medium-3.webp",
    "avatar/choro-companion-idle-medium-4.webp",
    "avatar/choro-companion-idle-medium-5.webp",
    "avatar/choro-companion-idle-medium-6.webp",
    "avatar/choro-companion-idle-medium-7.webp",
];
const WORKING_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-working-medium-0.webp",
    "avatar/choro-companion-working-medium-1.webp",
    "avatar/choro-companion-working-medium-2.webp",
    "avatar/choro-companion-working-medium-3.webp",
    "avatar/choro-companion-working-medium-4.webp",
    "avatar/choro-companion-working-medium-5.webp",
    "avatar/choro-companion-working-medium-6.webp",
    "avatar/choro-companion-working-medium-7.webp",
];
const NEEDS_ATTENTION_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-needs-attention-medium-0.webp",
    "avatar/choro-companion-needs-attention-medium-1.webp",
    "avatar/choro-companion-needs-attention-medium-2.webp",
    "avatar/choro-companion-needs-attention-medium-3.webp",
    "avatar/choro-companion-needs-attention-medium-4.webp",
    "avatar/choro-companion-needs-attention-medium-5.webp",
    "avatar/choro-companion-needs-attention-medium-6.webp",
    "avatar/choro-companion-needs-attention-medium-7.webp",
];
const DONE_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-done-medium-0.webp",
    "avatar/choro-companion-done-medium-1.webp",
    "avatar/choro-companion-done-medium-2.webp",
    "avatar/choro-companion-done-medium-3.webp",
    "avatar/choro-companion-done-medium-4.webp",
    "avatar/choro-companion-done-medium-5.webp",
    "avatar/choro-companion-done-medium-6.webp",
    "avatar/choro-companion-done-medium-7.webp",
];
const DEEP_FOCUS_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-music-deep-focus-medium-0.webp",
    "avatar/choro-companion-music-deep-focus-medium-1.webp",
    "avatar/choro-companion-music-deep-focus-medium-2.webp",
    "avatar/choro-companion-music-deep-focus-medium-3.webp",
    "avatar/choro-companion-music-deep-focus-medium-4.webp",
    "avatar/choro-companion-music-deep-focus-medium-5.webp",
    "avatar/choro-companion-music-deep-focus-medium-6.webp",
    "avatar/choro-companion-music-deep-focus-medium-7.webp",
];
const LOFI_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-music-lofi-flow-medium-0.webp",
    "avatar/choro-companion-music-lofi-flow-medium-1.webp",
    "avatar/choro-companion-music-lofi-flow-medium-2.webp",
    "avatar/choro-companion-music-lofi-flow-medium-3.webp",
    "avatar/choro-companion-music-lofi-flow-medium-4.webp",
    "avatar/choro-companion-music-lofi-flow-medium-5.webp",
    "avatar/choro-companion-music-lofi-flow-medium-6.webp",
    "avatar/choro-companion-music-lofi-flow-medium-7.webp",
];
const CALM_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-music-calm-medium-0.webp",
    "avatar/choro-companion-music-calm-medium-1.webp",
    "avatar/choro-companion-music-calm-medium-2.webp",
    "avatar/choro-companion-music-calm-medium-3.webp",
    "avatar/choro-companion-music-calm-medium-4.webp",
    "avatar/choro-companion-music-calm-medium-5.webp",
    "avatar/choro-companion-music-calm-medium-6.webp",
    "avatar/choro-companion-music-calm-medium-7.webp",
];
const HIGH_ENERGY_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-music-high-energy-medium-0.webp",
    "avatar/choro-companion-music-high-energy-medium-1.webp",
    "avatar/choro-companion-music-high-energy-medium-2.webp",
    "avatar/choro-companion-music-high-energy-medium-3.webp",
    "avatar/choro-companion-music-high-energy-medium-4.webp",
    "avatar/choro-companion-music-high-energy-medium-5.webp",
    "avatar/choro-companion-music-high-energy-medium-6.webp",
    "avatar/choro-companion-music-high-energy-medium-7.webp",
];
const AGENT_ASSISTANT_PERSISTENT_MOTION_ASSETS: [&str; PERSISTENT_MOTION_FRAME_COUNT] = [
    "avatar/choro-companion-agent-assistant-medium-0.webp",
    "avatar/choro-companion-agent-assistant-medium-1.webp",
    "avatar/choro-companion-agent-assistant-medium-2.webp",
    "avatar/choro-companion-agent-assistant-medium-3.webp",
    "avatar/choro-companion-agent-assistant-medium-4.webp",
    "avatar/choro-companion-agent-assistant-medium-5.webp",
    "avatar/choro-companion-agent-assistant-medium-6.webp",
    "avatar/choro-companion-agent-assistant-medium-7.webp",
];
const SCREEN_MARGIN: f32 = 20.0;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum CompanionAnimation {
    Idle,
    Working,
    NeedsAttention,
    Done,
    DeepFocus,
    LofiFlow,
    Calm,
    HighEnergy,
    AgentAssistant,
}

impl CompanionAnimation {
    fn asset(self) -> &'static str {
        match self {
            Self::Idle => "avatar/choro-companion-idle.webp",
            Self::Working => "avatar/choro-companion-working.webp",
            Self::NeedsAttention => "avatar/choro-companion-needs-attention.webp",
            Self::Done => "avatar/choro-companion-done.webp",
            Self::DeepFocus => "avatar/choro-companion-music-deep-focus.webp",
            Self::LofiFlow => "avatar/choro-companion-music-lofi-flow.webp",
            Self::Calm => "avatar/choro-companion-music-calm.webp",
            Self::HighEnergy => "avatar/choro-companion-music-high-energy.webp",
            Self::AgentAssistant => "avatar/choro-companion-agent-assistant.webp",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::NeedsAttention => 0,
            Self::Done => 1,
            Self::Working => 2,
            Self::AgentAssistant => 3,
            Self::DeepFocus | Self::LofiFlow | Self::Calm | Self::HighEnergy => 4,
            Self::Idle => 5,
        }
    }

    #[cfg(test)]
    fn frame_count(self) -> usize {
        PERSISTENT_MOTION_FRAME_COUNT
    }

    fn persistent_motion_assets(
        self,
    ) -> Option<&'static [&'static str; PERSISTENT_MOTION_FRAME_COUNT]> {
        match self {
            Self::Idle => Some(&IDLE_PERSISTENT_MOTION_ASSETS),
            Self::Working => Some(&WORKING_PERSISTENT_MOTION_ASSETS),
            Self::NeedsAttention => Some(&NEEDS_ATTENTION_PERSISTENT_MOTION_ASSETS),
            Self::Done => Some(&DONE_PERSISTENT_MOTION_ASSETS),
            Self::DeepFocus => Some(&DEEP_FOCUS_PERSISTENT_MOTION_ASSETS),
            Self::LofiFlow => Some(&LOFI_PERSISTENT_MOTION_ASSETS),
            Self::Calm => Some(&CALM_PERSISTENT_MOTION_ASSETS),
            Self::HighEnergy => Some(&HIGH_ENERGY_PERSISTENT_MOTION_ASSETS),
            Self::AgentAssistant => Some(&AGENT_ASSISTANT_PERSISTENT_MOTION_ASSETS),
        }
    }

    fn persistent_motion_interval(self) -> Option<Duration> {
        match self {
            Self::Idle => Some(Duration::from_millis(300)),
            Self::Working => Some(Duration::from_millis(160)),
            Self::NeedsAttention | Self::Done => Some(Duration::from_millis(140)),
            Self::DeepFocus => Some(Duration::from_millis(260)),
            Self::LofiFlow => Some(Duration::from_millis(180)),
            Self::Calm => Some(Duration::from_millis(280)),
            Self::HighEnergy => Some(Duration::from_millis(140)),
            Self::AgentAssistant => Some(Duration::from_millis(160)),
        }
    }

    fn render_asset(self, persistent_motion_frame_index: usize) -> &'static str {
        self.persistent_motion_assets()
            .map(|assets| assets[persistent_motion_frame_index % PERSISTENT_MOTION_FRAME_COUNT])
            .unwrap_or_else(|| self.asset())
    }

    fn for_each_render_asset(self, mut visit: impl FnMut(&'static str)) {
        if let Some(assets) = self.persistent_motion_assets() {
            for asset in assets {
                visit(asset);
            }
        } else {
            visit(self.asset());
        }
    }

    #[cfg(test)]
    fn render_asset_count(self) -> usize {
        self.persistent_motion_assets()
            .map(|assets| assets.len())
            .unwrap_or(1)
    }

    #[cfg(test)]
    fn decoded_frame_ceiling_bytes(self) -> u64 {
        // Every embedded WebP uses the same tightly cropped canvas. This is the
        // uncompressed RGBA ceiling when the decoder materializes full frames.
        self.frame_count() as u64 * ANIMATION_CANVAS_WIDTH * ANIMATION_CANVAS_HEIGHT * 4
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AnimationSwitch {
    previous: Option<CompanionAnimation>,
    current: CompanionAnimation,
}

/// Keeps the current animation on screen until every asset for its replacement is decoded.
/// This prevents persistent poses from appearing one-by-one during their first cycle.
#[derive(Clone, Copy, Debug)]
struct AnimationHandoff {
    current: CompanionAnimation,
    current_ready: bool,
    pending: Option<CompanionAnimation>,
}

impl AnimationHandoff {
    fn new() -> Self {
        Self {
            current: CompanionAnimation::Idle,
            current_ready: false,
            pending: None,
        }
    }

    /// Returns a superseded pending animation whose partially loaded assets may be released.
    fn request(&mut self, desired: CompanionAnimation) -> Option<CompanionAnimation> {
        if self.current_ready && desired == self.current {
            return self
                .pending
                .take()
                .filter(|pending| *pending != self.current);
        }

        if self.pending == Some(desired) {
            return None;
        }

        let superseded = self
            .pending
            .replace(desired)
            .filter(|pending| *pending != self.current && *pending != desired);
        if !self.current_ready {
            self.current = desired;
        }
        superseded
    }

    fn commit_pending(&mut self) -> Option<AnimationSwitch> {
        let next = self.pending.take()?;
        let previous = self
            .current_ready
            .then_some(self.current)
            .filter(|current| *current != next);
        self.current = next;
        self.current_ready = true;
        Some(AnimationSwitch {
            previous,
            current: next,
        })
    }

    fn clear(&mut self) {
        self.current_ready = false;
        self.pending = None;
    }
}

#[derive(Clone, Debug)]
struct CompanionItem {
    project_id: ProjectId,
    agent_id: Uuid,
    title: String,
    project_name: String,
    status: String,
    animation: CompanionAnimation,
    created_at: u64,
    acknowledge_on_open: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProcessMetric {
    #[default]
    Cpu,
    Memory,
}

pub(crate) struct CompanionView {
    _project_list: Entity<ProjectList>,
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    agent_chats: Entity<AgentChatState>,
    voice: Entity<VoiceState>,
    music: Entity<CompanionMusicState>,
    center: Entity<CenterArea>,
    main_window: WindowHandle<Root>,
    window_height: f32,
    companion_hovered: bool,
    music_menu_open: bool,
    process_monitor_open: bool,
    process_metric: ProcessMetric,
    processes: Vec<ProcessInfo>,
    process_total_memory_bytes: u64,
    process_monitor_error: Option<String>,
    process_monitor_loading: bool,
    process_monitor_seq: u64,
    agents_expanded: bool,
    companion_visible: bool,
    persistent_motion_frame_index: usize,
    animation_handoff: AnimationHandoff,
    animation_cache: Entity<RetainAllImageCache>,
}

impl CompanionView {
    fn open_item(
        &mut self,
        project_id: ProjectId,
        agent_id: Uuid,
        acknowledge: bool,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let center = self.center.clone();
        let main_window = self.main_window.clone();

        if acknowledge {
            crate::notifications::acknowledge_agent(project_id, agent_id);
        }
        cx.activate(true);
        let _ = main_window.update(cx, move |_root, window, cx| {
            workspace.update(cx, |workspace, cx| workspace.set_active(project_id, cx));
            center.update(cx, |center, cx| center.open_agent(agent_id, window, cx));
            window.activate_window();
        });
        cx.notify();
    }

    fn play_music(&mut self, index: usize, playlist_uri: String, cx: &mut Context<Self>) {
        self.music
            .update(cx, |music, cx| music.play(index, playlist_uri, cx));
        self.music_menu_open = false;
        cx.notify();
    }

    fn toggle_music(&mut self, playing: bool, paused: bool, cx: &mut Context<Self>) {
        if playing {
            self.music.update(cx, |music, cx| music.pause(cx));
            self.music_menu_open = false;
        } else if paused {
            self.music.update(cx, |music, cx| music.resume(cx));
            self.music_menu_open = false;
        } else {
            self.music_menu_open = !self.music_menu_open;
            if self.music_menu_open {
                self.process_monitor_open = false;
            }
        }
        cx.notify();
    }

    fn toggle_music_menu(&mut self, cx: &mut Context<Self>) {
        self.music_menu_open = !self.music_menu_open;
        if self.music_menu_open {
            self.process_monitor_open = false;
        }
        cx.notify();
    }

    fn toggle_process_monitor(&mut self, cx: &mut Context<Self>) {
        self.process_monitor_open = !self.process_monitor_open;
        if self.process_monitor_open {
            self.music_menu_open = false;
            self.refresh_process_monitor(cx);
        }
        cx.notify();
    }

    fn set_process_metric(&mut self, metric: ProcessMetric, cx: &mut Context<Self>) {
        if self.process_metric != metric {
            self.process_metric = metric;
            cx.notify();
        }
    }

    fn refresh_process_monitor(&mut self, cx: &mut Context<Self>) {
        if !self.process_monitor_open || !self.companion_visible || self.process_monitor_loading {
            return;
        }

        self.process_monitor_loading = true;
        self.process_monitor_seq = self.process_monitor_seq.wrapping_add(1);
        let seq = self.process_monitor_seq;
        let projects = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| ProjectSource {
                id: project.id,
                name: project.name.clone(),
                path: project.path.display().to_string(),
            })
            .collect::<Vec<_>>();
        let agents = self.agents.read(cx).all_records();

        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { SettingsView::load_live_process_snapshot(&projects, &agents) })
                .await;
            this.update(cx, |this, cx| {
                if this.process_monitor_seq == seq {
                    this.processes = snapshot.processes;
                    this.process_total_memory_bytes = snapshot.total_bytes;
                    this.process_monitor_error = snapshot.error;
                    this.process_monitor_loading = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_agent_assistant(&mut self, cx: &mut Context<Self>) {
        self.music_menu_open = false;
        self.voice
            .update(cx, |voice, cx| voice.toggle_agent_assistant(cx));
        cx.notify();
    }

    fn toggle_agents_expanded(&mut self, cx: &mut Context<Self>) {
        self.agents_expanded = !self.agents_expanded;
        cx.notify();
    }

    fn remove_animation_assets(
        &self,
        animation: CompanionAnimation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.animation_cache.update(cx, |cache, cx| {
            animation.for_each_render_asset(|asset| {
                cache.remove(&Resource::Embedded(asset.into()), window, cx);
            });
        });
    }

    fn animation_assets_ready(
        &self,
        animation: CompanionAnimation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.animation_cache.update(cx, |cache, cx| {
            let mut all_ready = true;
            animation.for_each_render_asset(|asset| {
                let resource = Resource::Embedded(asset.into());
                match cache.load(&resource, window, cx) {
                    Some(Ok(_)) => {}
                    // GPUI's image asset loader already reports the detailed failure.
                    // Treat a failed load as settled so the handoff cannot deadlock.
                    Some(Err(_)) => {}
                    None => all_ready = false,
                }
            });
            all_ready
        })
    }

    fn clear_animation_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.animation_cache
            .update(cx, |cache, cx| cache.clear(window, cx));
        self.animation_handoff.clear();
        self.persistent_motion_frame_index = 0;
    }

    fn prepare_animation(
        &mut self,
        desired: Option<CompanionAnimation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<CompanionAnimation> {
        let Some(desired) = desired else {
            if self.animation_handoff.current_ready || self.animation_handoff.pending.is_some() {
                self.clear_animation_cache(window, cx);
            }
            return None;
        };

        if let Some(superseded) = self.animation_handoff.request(desired) {
            self.remove_animation_assets(superseded, window, cx);
        }

        if let Some(pending) = self.animation_handoff.pending {
            if self.animation_assets_ready(pending, window, cx) {
                if let Some(animation_switch) = self.animation_handoff.commit_pending() {
                    if let Some(previous) = animation_switch.previous {
                        self.remove_animation_assets(previous, window, cx);
                    }
                    self.persistent_motion_frame_index = 0;
                }
            }
        }

        Some(self.animation_handoff.current)
    }

    fn render_music_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let settings = self.workspace.read(cx).companion_music.clone();
        let music = self.music.read(cx);
        let active_playlist = music.active_playlist();
        let error = music
            .error()
            .map(|error| SharedString::from(error.to_string()));

        v_flex()
            .id("companion-music-menu")
            .absolute()
            .right(px(24.))
            .bottom(px(AVATAR_BASE_HEIGHT))
            .w(px(244.))
            .gap_1()
            .p_2()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx).opacity(0.98))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child("CHOOSE A MUSIC MODE"),
            )
            .children(
                settings
                    .playlists
                    .into_iter()
                    .enumerate()
                    .map(|(index, playlist)| {
                        let uri = playlist.spotify_uri();
                        let configured = uri.is_some();
                        let label = if configured {
                            playlist.display_label(index)
                        } else {
                            format!("{} · Set in Settings", playlist.display_label(index))
                        };
                        crate::ui::style::popover_selection_button(
                            ("companion-music-playlist", index),
                            label,
                            active_playlist == Some(index),
                            cx,
                        )
                        .disabled(!configured)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(uri) = uri.clone() {
                                this.play_music(index, uri, cx);
                            }
                        }))
                    }),
            )
            .when_some(error, |menu, error| {
                menu.child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .into_any_element()
    }

    fn render_process_monitor(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let metric = self.process_metric;
        let processes = top_processes(&self.processes, metric);
        let loading = self.process_monitor_loading && self.processes.is_empty();
        let error = self.process_monitor_error.clone();
        let cpu_selected = metric == ProcessMetric::Cpu;
        let memory_selected = metric == ProcessMetric::Memory;
        let cpu_color = if cpu_selected {
            crate::ui::design::t1(cx)
        } else {
            crate::ui::design::t3(cx)
        };
        let memory_color = if memory_selected {
            crate::ui::design::t1(cx)
        } else {
            crate::ui::design::t3(cx)
        };

        v_flex()
            .id("companion-process-monitor")
            .w_full()
            .h(px(PROCESS_MONITOR_HEIGHT))
            .flex_none()
            .gap_2()
            .p_2()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx).opacity(0.98))
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                h_flex()
                    .h(px(22.0))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Live processes"),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .bg(crate::ui::design::sage(cx)),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Live · 2s"),
                            ),
                    ),
            )
            .child(
                crate::ui::style::segmented_container_quiet(cx)
                    .h(px(30.0))
                    .child(
                        crate::ui::style::segment_with_leading(
                            "companion-process-cpu",
                            crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Cpu,
                                cpu_color,
                                crate::ui::design::icon_sm(),
                            )
                            .into_any_element(),
                            "CPU",
                            cpu_selected,
                            cx,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_process_metric(ProcessMetric::Cpu, cx);
                        })),
                    )
                    .child(
                        crate::ui::style::segment_with_leading(
                            "companion-process-memory",
                            crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::MemoryStick,
                                memory_color,
                                crate::ui::design::icon_sm(),
                            )
                            .into_any_element(),
                            "RAM",
                            memory_selected,
                            cx,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_process_metric(ProcessMetric::Memory, cx);
                        })),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when_some(error.clone(), |list, _error| {
                        list.child(
                            h_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::rose(cx))
                                .child("Process data unavailable · retrying"),
                        )
                    })
                    .when(loading && error.is_none(), |list| {
                        list.child(
                            h_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Sampling processes…"),
                        )
                    })
                    .when(
                        !loading && error.is_none() && processes.is_empty(),
                        |list| {
                            list.child(
                                h_flex()
                                    .flex_1()
                                    .items_center()
                                    .justify_center()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("No related processes found"),
                            )
                        },
                    )
                    .when(error.is_none(), |list| {
                        list.children(processes.into_iter().enumerate().map(|(index, process)| {
                            let project = process.project.clone().unwrap_or_else(|| {
                                if process.name == "Choro app" {
                                    "Choro".to_string()
                                } else {
                                    "Background".to_string()
                                }
                            });
                            let value = match metric {
                                ProcessMetric::Cpu => format!("{:.1}%", process.cpu),
                                ProcessMetric::Memory => format!(
                                    "{:.0}% · {}",
                                    process_memory_share(
                                        process.memory_bytes,
                                        self.process_total_memory_bytes,
                                    ),
                                    format_process_memory(process.memory_bytes),
                                ),
                            };

                            h_flex()
                                .id(("companion-process-row", process.pid as u64))
                                .h(px(30.0))
                                .flex_none()
                                .gap_2()
                                .items_center()
                                .when(index > 0, |row| {
                                    row.border_t_1().border_color(crate::ui::design::line(cx))
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(crate::ui::design::text_label())
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(crate::ui::design::t1(cx))
                                                .child(process.name),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(crate::ui::design::text_label())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(project),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(crate::ui::design::text_label())
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(value),
                                )
                        }))
                    }),
            )
            .into_any_element()
    }

    fn items(&self, cx: &App) -> Vec<CompanionItem> {
        let records = self.agents.read(cx).all_records();
        let records_by_id: HashMap<_, _> = records.iter().map(|agent| (agent.id, agent)).collect();
        let project_names: HashMap<_, _> = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| (project.id, project.name.clone()))
            .collect();
        let chats = self.agent_chats.read(cx);
        let mut items = Vec::new();

        // Running chat sessions are live state, not unread attention. Sending a
        // turn sets this status synchronously, so the companion responds before
        // the backend has produced its first event.
        for agent in &records {
            if agent.runtime != AgentRuntimeKind::Chat {
                continue;
            }
            let Some(session) = chats.session(agent.id) else {
                continue;
            };
            if session.hidden_from_notifications
                || !matches!(
                    session.status,
                    AgentChatStatus::Running | AgentChatStatus::Cancelling
                )
            {
                continue;
            }
            let Some(project_name) = project_names.get(&agent.project_id) else {
                continue;
            };
            let animation = CompanionAnimation::Working;
            items.push(CompanionItem {
                project_id: agent.project_id,
                agent_id: agent.id,
                title: agent.title.clone(),
                project_name: project_name.clone(),
                status: match session.status {
                    AgentChatStatus::Cancelling => "Wrapping up".to_string(),
                    _ => "Working".to_string(),
                },
                animation,
                created_at: session
                    .started_running_at
                    .unwrap_or(session.last_activity_at),
                acknowledge_on_open: false,
            });
        }

        // Attention is coordinator-owned and remains here until the exact
        // conversation is opened or the underlying event becomes stale.
        for attention in crate::notifications::companion_attention() {
            let changed_file_count = chats
                .session(attention.agent_id)
                .map(|session| current_turn_changed_file_count(&session.timeline))
                .unwrap_or_else(|| {
                    records_by_id
                        .get(&attention.agent_id)
                        .map(|agent| agent.changed_files.len())
                        .unwrap_or_default()
                });
            let animation = attention_animation(attention.category, changed_file_count);
            let status = match animation {
                CompanionAnimation::Done => match changed_file_count {
                    1 => "Done · 1 file changed".to_string(),
                    count => format!("Done · {count} files changed"),
                },
                _ => "Needs attention".to_string(),
            };
            items.push(CompanionItem {
                project_id: attention.project_id,
                agent_id: attention.agent_id,
                title: attention.agent_title,
                project_name: attention.project_name,
                status,
                animation,
                created_at: attention.created_at,
                acknowledge_on_open: true,
            });
        }

        items.sort_by(|a, b| {
            a.animation
                .priority()
                .cmp(&b.animation.priority())
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| a.title.cmp(&b.title))
        });
        items
    }

    fn render_item_row(
        &self,
        ix: usize,
        item: CompanionItem,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let project_id = item.project_id;
        let agent_id = item.agent_id;
        let acknowledge = item.acknowledge_on_open;
        let title = SharedString::from(item.title);
        let status = SharedString::from(item.status);
        let project_name = SharedString::from(item.project_name);
        let (accent, badge_fill, icon) = match item.animation {
            CompanionAnimation::Working => (
                crate::ui::design::accent(cx),
                crate::ui::design::accent_soft(cx),
                lucide_icons::Icon::Activity,
            ),
            CompanionAnimation::NeedsAttention => (
                crate::ui::design::amber(cx),
                crate::ui::design::amber_soft(cx),
                lucide_icons::Icon::MessageCircleMore,
            ),
            CompanionAnimation::Done => (
                crate::ui::design::sage(cx),
                crate::ui::design::sage_soft(cx),
                lucide_icons::Icon::CheckCheck,
            ),
            CompanionAnimation::DeepFocus
            | CompanionAnimation::LofiFlow
            | CompanionAnimation::Calm
            | CompanionAnimation::HighEnergy
            | CompanionAnimation::AgentAssistant => (
                crate::ui::design::accent(cx),
                crate::ui::design::accent_soft(cx),
                lucide_icons::Icon::Bot,
            ),
            CompanionAnimation::Idle => (
                crate::ui::design::t3(cx),
                crate::ui::design::surface(cx),
                lucide_icons::Icon::Bot,
            ),
        };
        let card_fill = crate::ui::design::focus(cx);

        h_flex()
            .id(("companion-attention-row", ix))
            .w_full()
            .h(px(ATTENTION_ROW_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(accent.opacity(0.24))
            .bg(card_fill.opacity(0.96))
            .shadow_sm()
            .cursor_pointer()
            .hover(|row| row.bg(crate::ui::design::control_on(card_fill, cx)))
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_item(project_id, agent_id, acknowledge, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .relative()
                    .size(px(34.0))
                    .items_center()
                    .justify_center()
                    .rounded(crate::ui::design::r_md())
                    .bg(badge_fill)
                    .child(crate::ui::design::indicator::lucide_icon(
                        icon,
                        accent,
                        crate::ui::design::icon_sm(),
                    ))
                    .when(item.animation == CompanionAnimation::Done, |badge| {
                        badge.child(
                            div()
                                .absolute()
                                .top(px(4.0))
                                .right(px(4.0))
                                .size(px(3.0))
                                .rounded_full()
                                .bg(accent.opacity(0.75)),
                        )
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(0.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap_1()
                            .text_size(crate::ui::design::text_label())
                            .line_height(gpui::relative(1.0))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(accent)
                                    .child(status),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(3.0))
                                    .rounded_full()
                                    .bg(crate::ui::design::t4(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(project_name),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CompanionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        configure_native_companion_window(window);
        let items = self.items(cx);
        let (music_playing, music_paused, active_music_animation) = {
            let music = self.music.read(cx);
            let playing = music.is_playing();
            (
                playing,
                music.is_paused(),
                if playing {
                    music.active_playlist().map(music_animation)
                } else {
                    None
                },
            )
        };
        let (agent_assistant_active, assistant_phase, assistant_notice, assistant_level) = {
            let voice = self.voice.read(cx);
            (
                voice.agent_assistant_active(),
                voice.phase().clone(),
                voice.session_notice().map(str::to_string),
                voice.level(),
            )
        };
        let ambient_animation = if agent_assistant_active {
            Some(CompanionAnimation::AgentAssistant)
        } else {
            active_music_animation
        };
        let desired_animation = if !self.companion_visible {
            None
        } else {
            Some(primary_animation(&items, ambient_animation))
        };
        let animation = self.prepare_animation(desired_animation, window, cx);
        let item_count = items.len();
        if item_count <= ATTENTION_PREVIEW_LIMIT {
            self.agents_expanded = false;
        }
        let visible_item_count = visible_item_count(item_count, self.agents_expanded);
        let music_menu_height = if self.music_menu_open {
            MUSIC_MENU_HEIGHT
        } else {
            0.0
        };
        let process_monitor_height = if self.process_monitor_open {
            PROCESS_MONITOR_HEIGHT + 8.0
        } else {
            0.0
        };
        let uncapped_height = window_height(item_count, self.agents_expanded)
            + music_menu_height
            + process_monitor_height;
        let display_height = window
            .display(cx)
            .map(|display| f32::from(display.bounds().size.height));
        let desired_height = capped_window_height(uncapped_height, display_height);
        let attention_list_height =
            (desired_height - AVATAR_BASE_HEIGHT - music_menu_height).max(0.0);
        if (desired_height - self.window_height).abs() > f32::EPSILON {
            self.window_height = desired_height;
            window.resize(size(px(WINDOW_WIDTH), px(desired_height)));
        }
        let music_menu_open = self.music_menu_open;
        let process_monitor_open = self.process_monitor_open;
        let show_companion_controls = self.companion_hovered
            || music_menu_open
            || process_monitor_open
            || agent_assistant_active;
        let overflow_count = item_count.saturating_sub(ATTENTION_PREVIEW_LIMIT);
        let agents_expanded = self.agents_expanded;
        let workspace_for_menu = self.workspace.clone();

        div()
            .id("choro-companion-drag-surface")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(gpui::transparent_black())
            .cursor_grab()
            .on_mouse_down(MouseButton::Left, |_event, window, cx| {
                start_companion_window_drag(window);
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id("companion-attention-list")
                    .absolute()
                    .top(px(8.0))
                    .left(px(10.0))
                    .right(px(10.0))
                    .max_h(px(attention_list_height))
                    .overflow_y_scroll()
                    .gap(px(8.0))
                    .children(
                        items
                            .into_iter()
                            .take(visible_item_count)
                            .enumerate()
                            .map(|(ix, item)| self.render_item_row(ix, item, cx)),
                    )
                    .when(overflow_count > 0, |list| {
                        let label = if agents_expanded {
                            "Show fewer agents".to_string()
                        } else if overflow_count == 1 {
                            "Show 1 more agent".to_string()
                        } else {
                            format!("Show {overflow_count} more agents")
                        };
                        list.child(
                            crate::ui::style::companion_overflow_button(
                                "companion-agent-overflow",
                                label,
                                agents_expanded,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_agents_expanded(cx);
                            })),
                        )
                    })
                    .when(process_monitor_open, |list| {
                        list.child(self.render_process_monitor(cx))
                    }),
            )
            .child(
                div()
                    .id("choro-companion-hover-region")
                    .absolute()
                    .bottom(px(45.0 - COMPANION_CONTROLS_HEIGHT))
                    .left(px((WINDOW_WIDTH - AVATAR_CANVAS_SIZE) / 2.0))
                    .w(px(AVATAR_CANVAS_SIZE))
                    .h(px(AVATAR_CANVAS_SIZE + COMPANION_CONTROLS_HEIGHT))
                    .on_hover(cx.listener(|this, hovered, _, cx| {
                        this.companion_hovered = *hovered;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("choro-companion-avatar-drag-surface")
                            .relative()
                            .size(px(AVATAR_CANVAS_SIZE))
                            .cursor_grab()
                            .on_mouse_down(MouseButton::Left, |_event, window, cx| {
                                start_companion_window_drag(window);
                                cx.stop_propagation();
                            })
                            .when_some(animation, |avatar, animation| {
                                let asset =
                                    animation.render_asset(self.persistent_motion_frame_index);
                                avatar.child(
                                    img(asset)
                                        .image_cache(&self.animation_cache)
                                        .id(asset)
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                )
                            })
                            .child(
                                div()
                                    .id("companion-avatar-drag-hit-area")
                                    .absolute()
                                    .inset_0()
                                    .bg(gpui::transparent_black())
                                    .cursor_grab()
                                    .on_mouse_down(MouseButton::Left, |_event, window, cx| {
                                        start_companion_window_drag(window);
                                        cx.stop_propagation();
                                    }),
                            )
                            .when(agent_assistant_active, |avatar| {
                                let label = agent_assistant_status(
                                    &assistant_phase,
                                    assistant_notice.as_deref(),
                                );
                                avatar.child(
                                    h_flex()
                                        .id("companion-agent-assistant-status")
                                        .absolute()
                                        .left(px(
                                            (AVATAR_CANVAS_SIZE - ASSISTANT_STATUS_WIDTH) / 2.0
                                        ))
                                        .bottom(px(27.0))
                                        .w(px(ASSISTANT_STATUS_WIDTH))
                                        .h(px(34.0))
                                        .px_3()
                                        .gap_2()
                                        .items_center()
                                        .rounded(crate::ui::design::r_pill())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx).opacity(0.55))
                                        .bg(crate::ui::design::focus(cx).opacity(0.98))
                                        .shadow_lg()
                                        .on_mouse_down(MouseButton::Left, |_event, window, cx| {
                                            start_companion_window_drag(window);
                                            cx.stop_propagation();
                                        })
                                        .child(
                                            gpui::svg()
                                                .path("icons/microphone.svg")
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::accent(cx)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.0))
                                                .truncate()
                                                .text_size(crate::ui::design::text_label())
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(crate::ui::design::t1(cx))
                                                .child(label),
                                        )
                                        .when(
                                            matches!(assistant_phase, VoicePhase::Listening),
                                            |status| {
                                                let level = assistant_level.clamp(0.0, 1.0);
                                                status.child(
                                                    div()
                                                        .flex_none()
                                                        .size(px(7.0 + level * 5.0))
                                                        .rounded_full()
                                                        .bg(crate::ui::design::accent(cx)
                                                            .opacity(0.55 + level * 0.45)),
                                                )
                                            },
                                        ),
                                )
                            }),
                    )
                    .when(show_companion_controls, |region| {
                        region.child(
                            h_flex()
                                .id("companion-controls")
                                .absolute()
                                .left(px((AVATAR_CANVAS_SIZE - COMPANION_CONTROLS_WIDTH) / 2.0))
                                .bottom(px(0.0))
                                .gap_1()
                                .p_1()
                                .rounded(crate::ui::design::r_pill())
                                .bg(crate::ui::design::focus(cx).opacity(0.98))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    crate::ui::style::companion_music_toggle_button(
                                        "companion-music-button",
                                        music_playing,
                                        cx,
                                    )
                                    .tooltip(if music_playing {
                                        "Pause music"
                                    } else if music_paused {
                                        "Resume music"
                                    } else {
                                        "Play music"
                                    })
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.toggle_music(music_playing, music_paused, cx);
                                        },
                                    )),
                                )
                                .child(
                                    crate::ui::style::companion_music_playlist_button(
                                        "companion-playlist-button",
                                        music_menu_open,
                                        cx,
                                    )
                                    .tooltip("Change music mode")
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.toggle_music_menu(cx);
                                        },
                                    )),
                                )
                                .child(
                                    crate::ui::style::companion_process_monitor_button(
                                        "companion-process-monitor-button",
                                        process_monitor_open,
                                        cx,
                                    )
                                    .tooltip(if process_monitor_open {
                                        "Hide live processes"
                                    } else {
                                        "Show live processes"
                                    })
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.toggle_process_monitor(cx);
                                        },
                                    )),
                                )
                                .child(
                                    crate::ui::style::companion_agent_assistant_button(
                                        "companion-agent-assistant-button",
                                        agent_assistant_active,
                                        cx,
                                    )
                                    .tooltip(if agent_assistant_active {
                                        "Pause Agent Assistant"
                                    } else {
                                        "Start Agent Assistant"
                                    })
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.toggle_agent_assistant(cx);
                                        },
                                    )),
                                ),
                        )
                    }),
            )
            .when(music_menu_open, |surface| {
                surface.child(self.render_music_menu(cx))
            })
            .context_menu(move |menu, _, _| {
                let workspace = workspace_for_menu.clone();
                menu.item(
                    PopupMenuItem::new("Hide Companion")
                        .icon(IconName::EyeOff)
                        .on_click(move |_, _, cx| {
                            workspace.update(cx, |workspace, cx| {
                                workspace.set_companion_enabled(false, cx);
                                workspace.save_now();
                            });
                        }),
                )
            })
    }
}

#[cfg(target_os = "macos")]
fn start_companion_window_drag(window: &mut Window) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(window_handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = window_handle.as_raw() else {
        return;
    };

    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![application, currentEvent];
        if !event.is_null() {
            let _: () = msg_send![ns_window, performWindowDragWithEvent:event];
            // macOS moves a cached transparent-window surface during the
            // native drag. Redraw once the tracking loop ends so text is
            // rasterized at the final screen position instead of occasionally
            // retaining the scaled drag snapshot.
            window.refresh();
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn start_companion_window_drag(window: &mut Window) {
    window.start_window_move();
}

#[cfg(target_os = "macos")]
fn configure_native_companion_window(window: &Window) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(window_handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = window_handle.as_raw() else {
        return;
    };

    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let clear_color: *mut Object = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![ns_window, setOpaque:false];
        let _: () = msg_send![ns_window, setBackgroundColor:clear_color];
        let _: () = msg_send![ns_window, setHasShadow:false];
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_native_companion_window(_window: &Window) {}

#[cfg(target_os = "macos")]
fn set_native_companion_window_visible(window: &Window, visible: bool) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(window_handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = window_handle.as_raw() else {
        return;
    };

    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let sender: *mut Object = std::ptr::null_mut();
        if visible {
            let _: () = msg_send![ns_window, orderFront:sender];
        } else {
            let _: () = msg_send![ns_window, orderOut:sender];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_native_companion_window_visible(_window: &Window, _visible: bool) {}

pub(crate) fn view(
    project_list: Entity<ProjectList>,
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    agent_chats: Entity<AgentChatState>,
    voice: Entity<VoiceState>,
    center: Entity<CenterArea>,
    main_window: WindowHandle<Root>,
    initial_attention_count: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<CompanionView> {
    configure_native_companion_window(window);
    let music = CompanionMusicState::view(cx);
    let animation_cache = RetainAllImageCache::new(cx);
    let companion_visible = workspace.read(cx).companion_enabled;
    cx.new(|cx: &mut Context<CompanionView>| {
        cx.observe(&project_list, |_, _, cx| cx.notify()).detach();
        cx.observe_in(&workspace, window, |this, workspace, window, cx| {
            let visible = workspace.read(cx).companion_enabled;
            if visible != this.companion_visible {
                this.companion_visible = visible;
                set_native_companion_window_visible(window, visible);
                if !visible {
                    this.clear_animation_cache(window, cx);
                    if this.voice.read(cx).agent_assistant_active() {
                        this.voice.update(cx, |voice, cx| voice.stop(cx));
                    }
                }
            }
            cx.notify();
        })
        .detach();
        cx.observe(&agents, |_, _, cx| cx.notify()).detach();
        cx.observe(&agent_chats, |_, _, cx| cx.notify()).detach();
        cx.observe(&voice, |_, _, cx| cx.notify()).detach();
        cx.subscribe(
            &voice,
            |this: &mut CompanionView, _, event: &VoiceEvent, cx| {
                if matches!(event, VoiceEvent::CreateAgent { .. }) {
                    cx.activate(true);
                    let _ = this.main_window.update(cx, |_root, window, _| {
                        window.activate_window();
                    });
                }
            },
        )
        .detach();
        cx.observe(&music, |_, _, cx| cx.notify()).detach();
        cx.observe(&center, |_, _, cx| cx.notify()).detach();
        cx.spawn(async move |this, cx| loop {
            let interval = match this.update(cx, |companion, _| {
                let animation = companion.animation_handoff.current;
                let is_rendered = companion.companion_visible;
                is_rendered
                    .then(|| animation.persistent_motion_interval())
                    .flatten()
                    .unwrap_or(Duration::from_secs(1))
            }) {
                Ok(interval) => interval,
                Err(_) => break,
            };
            cx.background_executor().timer(interval).await;
            if this
                .update(cx, |companion, cx| {
                    let animation = companion.animation_handoff.current;
                    let is_rendered =
                        companion.companion_visible && companion.animation_handoff.current_ready;
                    if is_rendered && animation.persistent_motion_assets().is_some() {
                        companion.persistent_motion_frame_index =
                            (companion.persistent_motion_frame_index + 1)
                                % PERSISTENT_MOTION_FRAME_COUNT;
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(PROCESS_MONITOR_REFRESH_INTERVAL)
                .await;
            if this
                .update(cx, |companion, cx| {
                    companion.refresh_process_monitor(cx);
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        CompanionView {
            _project_list: project_list,
            workspace,
            agents,
            agent_chats,
            voice,
            music,
            center,
            main_window,
            window_height: window_height(initial_attention_count, false),
            companion_hovered: false,
            music_menu_open: false,
            process_monitor_open: false,
            process_metric: ProcessMetric::Cpu,
            processes: Vec::new(),
            process_total_memory_bytes: 0,
            process_monitor_error: None,
            process_monitor_loading: false,
            process_monitor_seq: 0,
            agents_expanded: false,
            companion_visible,
            persistent_motion_frame_index: 0,
            animation_handoff: AnimationHandoff::new(),
            animation_cache,
        }
    })
}

pub(crate) fn window_options(
    cx: &App,
    attention_count: usize,
    companion_enabled: bool,
) -> WindowOptions {
    let display = cx.primary_display();
    let initial_height = capped_window_height(
        window_height(attention_count, false),
        display
            .as_deref()
            .map(|display| f32::from(display.bounds().size.height)),
    );
    let window_size = size(px(WINDOW_WIDTH), px(initial_height));
    let window_bounds = display
        .as_deref()
        .map(|display| companion_bounds(display, window_size))
        .unwrap_or_else(|| Bounds::centered(None, window_size, cx));

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(window_bounds)),
        titlebar: None,
        focus: false,
        show: companion_enabled,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        display_id: display.as_ref().map(|display| display.id()),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

fn visible_item_count(item_count: usize, expanded: bool) -> usize {
    if expanded {
        item_count
    } else {
        item_count.min(ATTENTION_PREVIEW_LIMIT)
    }
}

fn window_height(item_count: usize, expanded: bool) -> f32 {
    AVATAR_BASE_HEIGHT
        + ATTENTION_ROW_STEP * visible_item_count(item_count, expanded) as f32
        + if item_count > ATTENTION_PREVIEW_LIMIT {
            ATTENTION_OVERFLOW_STEP
        } else {
            0.0
        }
}

fn capped_window_height(content_height: f32, display_height: Option<f32>) -> f32 {
    display_height
        .map(|height| content_height.min((height - SCREEN_MARGIN * 2.0).max(AVATAR_BASE_HEIGHT)))
        .unwrap_or(content_height)
}

fn top_processes(processes: &[ProcessInfo], metric: ProcessMetric) -> Vec<ProcessInfo> {
    let mut processes = processes.to_vec();
    processes.sort_by(|a, b| match metric {
        ProcessMetric::Cpu => b.cpu.total_cmp(&a.cpu),
        ProcessMetric::Memory => b.memory_bytes.cmp(&a.memory_bytes),
    });
    processes.truncate(PROCESS_MONITOR_LIMIT);
    processes
}

fn format_process_memory(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * MIB;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GB", bytes / GIB)
    } else {
        format!("{:.0} MB", bytes / MIB)
    }
}

fn process_memory_share(bytes: u64, total_bytes: u64) -> f64 {
    if total_bytes == 0 {
        0.0
    } else {
        bytes as f64 / total_bytes as f64 * 100.0
    }
}

fn current_turn_changed_file_count(timeline: &[AgentChatTimelineItem]) -> usize {
    for item in timeline.iter().rev() {
        match item {
            AgentChatTimelineItem::Message(AgentChatMessage::User { .. }) => return 0,
            AgentChatTimelineItem::ChangedFiles(summary) => return summary.files.len(),
            _ => {}
        }
    }
    0
}

fn attention_animation(
    category: crate::notifications::AttentionCategory,
    changed_file_count: usize,
) -> CompanionAnimation {
    if category == crate::notifications::AttentionCategory::Completed && changed_file_count > 0 {
        CompanionAnimation::Done
    } else {
        CompanionAnimation::NeedsAttention
    }
}

fn music_animation(index: usize) -> CompanionAnimation {
    match index {
        0 => CompanionAnimation::DeepFocus,
        1 => CompanionAnimation::LofiFlow,
        2 => CompanionAnimation::Calm,
        3 => CompanionAnimation::HighEnergy,
        _ => CompanionAnimation::LofiFlow,
    }
}

fn agent_assistant_status(phase: &VoicePhase, notice: Option<&str>) -> String {
    match phase {
        VoicePhase::Idle => notice.unwrap_or("Talk to me").to_string(),
        VoicePhase::Downloading { downloaded, total } => {
            let percent = if *total == 0 {
                0
            } else {
                downloaded.saturating_mul(100) / total
            };
            format!("Preparing voice · {percent}%")
        }
        VoicePhase::RequestingPermission => "Allow microphone access".to_string(),
        VoicePhase::Loading | VoicePhase::Resuming => "Getting ready…".to_string(),
        VoicePhase::Listening => "Talk to me".to_string(),
        VoicePhase::Transcribing => "Got it…".to_string(),
        VoicePhase::Thinking => "Thinking…".to_string(),
        VoicePhase::Speaking => notice.unwrap_or("Speaking…").to_string(),
        VoicePhase::Error(error) => error.clone(),
    }
}

fn primary_animation(
    items: &[CompanionItem],
    music_animation: Option<CompanionAnimation>,
) -> CompanionAnimation {
    items
        .iter()
        .map(|item| item.animation)
        .min_by_key(|animation| animation.priority())
        .unwrap_or_else(|| music_animation.unwrap_or(CompanionAnimation::Idle))
}

fn companion_bounds(
    display: &dyn PlatformDisplay,
    window_size: gpui::Size<gpui::Pixels>,
) -> Bounds<gpui::Pixels> {
    companion_bounds_in_display(display.bounds(), window_size)
}

fn companion_bounds_in_display(
    display_bounds: Bounds<gpui::Pixels>,
    window_size: gpui::Size<gpui::Pixels>,
) -> Bounds<gpui::Pixels> {
    Bounds::new(
        display_bounds.bottom_right()
            - point(
                window_size.width + px(SCREEN_MARGIN),
                window_size.height + px(SCREEN_MARGIN),
            ),
        window_size,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::AttentionCategory;
    use crate::state::agent_chat::{ChangedFilesSummary, FileChangeStat};

    fn item(animation: CompanionAnimation) -> CompanionItem {
        CompanionItem {
            project_id: ProjectId::new(),
            agent_id: Uuid::new_v4(),
            title: "Agent".to_string(),
            project_name: "Project".to_string(),
            status: "State".to_string(),
            animation,
            created_at: 1,
            acknowledge_on_open: animation != CompanionAnimation::Working,
        }
    }

    fn process(name: &str, cpu: f64, memory_bytes: u64) -> ProcessInfo {
        ProcessInfo {
            pid: memory_bytes as i32,
            memory_bytes,
            cpu,
            project_id: None,
            project: Some("Project".to_string()),
            agent_id: None,
            agent: None,
            name: name.to_string(),
            command: name.to_string(),
        }
    }

    #[test]
    fn completed_work_with_changed_files_uses_done() {
        assert_eq!(
            attention_animation(AttentionCategory::Completed, 2),
            CompanionAnimation::Done
        );
    }

    #[test]
    fn attention_preview_stays_at_three_until_expanded() {
        assert_eq!(visible_item_count(2, false), 2);
        assert_eq!(visible_item_count(8, false), ATTENTION_PREVIEW_LIMIT);
        assert_eq!(visible_item_count(8, true), 8);
        assert_eq!(
            window_height(8, false),
            AVATAR_BASE_HEIGHT
                + ATTENTION_ROW_STEP * ATTENTION_PREVIEW_LIMIT as f32
                + ATTENTION_OVERFLOW_STEP
        );
        assert_eq!(
            window_height(8, true),
            AVATAR_BASE_HEIGHT + ATTENTION_ROW_STEP * 8.0 + ATTENTION_OVERFLOW_STEP
        );
    }

    #[test]
    fn expanded_attention_is_capped_to_the_display() {
        let content_height = window_height(20, true) + MUSIC_MENU_HEIGHT;

        assert_eq!(
            capped_window_height(content_height, Some(900.0)),
            900.0 - SCREEN_MARGIN * 2.0
        );
        assert_eq!(capped_window_height(content_height, None), content_height);
    }

    #[test]
    fn live_processes_are_ranked_by_selected_metric_and_limited_to_five() {
        let processes = vec![
            process("one", 1.0, 60),
            process("two", 6.0, 50),
            process("three", 2.0, 40),
            process("four", 5.0, 30),
            process("five", 3.0, 20),
            process("six", 4.0, 10),
        ];

        let cpu = top_processes(&processes, ProcessMetric::Cpu);
        let memory = top_processes(&processes, ProcessMetric::Memory);

        assert_eq!(cpu.len(), PROCESS_MONITOR_LIMIT);
        assert_eq!(
            cpu.iter()
                .map(|process| process.name.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "four", "six", "five", "three"]
        );
        assert_eq!(
            memory
                .iter()
                .map(|process| process.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two", "three", "four", "five"]
        );
    }

    #[test]
    fn live_process_memory_uses_readable_units() {
        assert_eq!(format_process_memory(512 * 1024 * 1024), "512 MB");
        assert_eq!(format_process_memory(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(process_memory_share(1, 4), 25.0);
        assert_eq!(process_memory_share(1, 0), 0.0);
    }

    #[test]
    fn companion_bounds_respect_display_origin_and_screen_margin() {
        let display_bounds =
            Bounds::new(point(px(-1440.0), px(-200.0)), size(px(1440.0), px(900.0)));
        let window_size = size(px(330.0), px(610.0));

        let bounds = companion_bounds_in_display(display_bounds, window_size);

        assert_eq!(bounds.origin, point(px(-350.0), px(70.0)));
        assert_eq!(bounds.size, window_size);
    }

    #[test]
    fn completed_work_without_changed_files_needs_attention() {
        assert_eq!(
            attention_animation(AttentionCategory::Completed, 0),
            CompanionAnimation::NeedsAttention
        );
        assert_eq!(
            attention_animation(AttentionCategory::Failed, 3),
            CompanionAnimation::NeedsAttention
        );
    }

    #[test]
    fn historical_file_changes_do_not_mark_a_new_text_only_reply_done() {
        let timeline = vec![
            user_message("Change the title", 1),
            changed_files(&["src/title.rs"]),
            assistant_message("Changed it.", 2),
            user_message("What did you change?", 3),
            assistant_message("Only the title.", 4),
        ];

        assert_eq!(current_turn_changed_file_count(&timeline), 0);
    }

    #[test]
    fn file_changes_after_the_latest_user_message_mark_the_turn_done() {
        let timeline = vec![
            user_message("Old edit", 1),
            changed_files(&["src/old.rs"]),
            assistant_message("Done.", 2),
            user_message("Make a fresh edit", 3),
            changed_files(&["src/new.rs", "src/new_test.rs"]),
            assistant_message("Done.", 4),
        ];

        assert_eq!(current_turn_changed_file_count(&timeline), 2);
    }

    #[test]
    fn urgent_attention_drives_the_avatar_when_other_work_is_active() {
        let items = vec![
            item(CompanionAnimation::Working),
            item(CompanionAnimation::Done),
            item(CompanionAnimation::NeedsAttention),
        ];
        assert_eq!(
            primary_animation(&items, Some(CompanionAnimation::HighEnergy)),
            CompanionAnimation::NeedsAttention
        );
    }

    #[test]
    fn no_conversations_uses_idle() {
        assert_eq!(primary_animation(&[], None), CompanionAnimation::Idle);
    }

    #[test]
    fn music_mood_animates_only_when_no_chat_state_has_priority() {
        assert_eq!(
            primary_animation(&[], Some(CompanionAnimation::LofiFlow)),
            CompanionAnimation::LofiFlow
        );
        assert_eq!(
            primary_animation(
                &[item(CompanionAnimation::Working)],
                Some(CompanionAnimation::HighEnergy),
            ),
            CompanionAnimation::Working
        );
    }

    #[test]
    fn each_playlist_slot_maps_to_its_named_music_mood() {
        assert_eq!(music_animation(0), CompanionAnimation::DeepFocus);
        assert_eq!(music_animation(1), CompanionAnimation::LofiFlow);
        assert_eq!(music_animation(2), CompanionAnimation::Calm);
        assert_eq!(music_animation(3), CompanionAnimation::HighEnergy);
    }

    #[test]
    fn every_state_uses_eight_smooth_frames_with_state_specific_pacing() {
        for animation in [
            CompanionAnimation::Idle,
            CompanionAnimation::Working,
            CompanionAnimation::NeedsAttention,
            CompanionAnimation::Done,
            CompanionAnimation::DeepFocus,
            CompanionAnimation::LofiFlow,
            CompanionAnimation::Calm,
            CompanionAnimation::HighEnergy,
            CompanionAnimation::AgentAssistant,
        ] {
            let assets = animation
                .persistent_motion_assets()
                .expect("persistent state should use bounded-motion poses");
            assert_eq!(assets.len(), PERSISTENT_MOTION_FRAME_COUNT);
            assert_eq!(animation.frame_count(), PERSISTENT_MOTION_FRAME_COUNT);
            assert!(animation.persistent_motion_interval().is_some());
            assert_ne!(animation.render_asset(0), animation.render_asset(1));
        }
        assert_eq!(
            CompanionAnimation::Idle.persistent_motion_interval(),
            Some(Duration::from_millis(300))
        );
        assert_eq!(
            CompanionAnimation::HighEnergy.persistent_motion_interval(),
            Some(Duration::from_millis(140))
        );
        assert_eq!(
            CompanionAnimation::NeedsAttention.persistent_motion_interval(),
            Some(Duration::from_millis(140))
        );
        assert_eq!(
            CompanionAnimation::AgentAssistant.persistent_motion_interval(),
            Some(Duration::from_millis(160))
        );
    }

    #[test]
    fn every_animation_prewarms_all_assets_before_becoming_current() {
        let animations = [
            CompanionAnimation::Idle,
            CompanionAnimation::Working,
            CompanionAnimation::NeedsAttention,
            CompanionAnimation::Done,
            CompanionAnimation::DeepFocus,
            CompanionAnimation::LofiFlow,
            CompanionAnimation::Calm,
            CompanionAnimation::HighEnergy,
            CompanionAnimation::AgentAssistant,
        ];
        let mut handoff = AnimationHandoff::new();

        for animation in animations {
            let previous = handoff.current;
            handoff.request(animation);
            assert_eq!(handoff.pending, Some(animation));
            if handoff.current_ready {
                assert_eq!(handoff.current, previous);
            }

            let animation_switch = handoff
                .commit_pending()
                .expect("a ready animation should commit atomically");
            assert_eq!(animation_switch.current, animation);
            assert_eq!(handoff.current, animation);
            assert!(handoff.current_ready);
            assert_eq!(handoff.pending, None);
        }
    }

    #[test]
    fn prewarm_covers_eight_frames_for_every_state() {
        for animation in [
            CompanionAnimation::Idle,
            CompanionAnimation::Working,
            CompanionAnimation::NeedsAttention,
            CompanionAnimation::Done,
            CompanionAnimation::DeepFocus,
            CompanionAnimation::LofiFlow,
            CompanionAnimation::Calm,
            CompanionAnimation::HighEnergy,
            CompanionAnimation::AgentAssistant,
        ] {
            assert_eq!(animation.render_asset_count(), 8);
        }
    }

    #[test]
    fn a_superseded_pending_state_never_replaces_the_visible_state() {
        let mut handoff = AnimationHandoff::new();
        handoff.request(CompanionAnimation::Idle);
        handoff.commit_pending();

        handoff.request(CompanionAnimation::HighEnergy);
        assert_eq!(handoff.current, CompanionAnimation::Idle);
        assert_eq!(
            handoff.request(CompanionAnimation::NeedsAttention),
            Some(CompanionAnimation::HighEnergy)
        );
        assert_eq!(handoff.current, CompanionAnimation::Idle);
        assert_eq!(handoff.pending, Some(CompanionAnimation::NeedsAttention));
    }

    #[test]
    fn animation_memory_ceiling_matches_all_embedded_frames() {
        let animations = [
            CompanionAnimation::Idle,
            CompanionAnimation::Working,
            CompanionAnimation::NeedsAttention,
            CompanionAnimation::Done,
            CompanionAnimation::DeepFocus,
            CompanionAnimation::LofiFlow,
            CompanionAnimation::Calm,
            CompanionAnimation::HighEnergy,
            CompanionAnimation::AgentAssistant,
        ];
        assert_eq!(
            animations
                .iter()
                .map(|item| item.frame_count())
                .sum::<usize>(),
            72
        );
        assert_eq!(
            animations
                .iter()
                .map(|item| item.decoded_frame_ceiling_bytes())
                .sum::<u64>(),
            72 * ANIMATION_CANVAS_WIDTH * ANIMATION_CANVAS_HEIGHT * 4
        );
    }

    fn user_message(text: &str, created_at: u64) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            text: text.to_string(),
            display_text: None,
            tags: Vec::new(),
            created_at,
        })
    }

    fn assistant_message(text: &str, created_at: u64) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            message_id: None,
            text: text.to_string(),
            created_at,
        })
    }

    fn changed_files(paths: &[&str]) -> AgentChatTimelineItem {
        AgentChatTimelineItem::ChangedFiles(ChangedFilesSummary {
            files: paths
                .iter()
                .map(|path| FileChangeStat::new(*path, 1, 0))
                .collect(),
            ..ChangedFilesSummary::default()
        })
    }
}
