use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, EventEmitter};
use ide_core::local_store::StoredVoiceTurn;
use ide_core::{ProjectId, VoiceAnnouncements};

use crate::state::{AgentChatState, AgentRecords, Workspace};

use super::audio::{self, RecognitionEvent};
use super::commands::{
    command_label, parse_draft_confirmation, parse_project_conversation_command,
    parse_voice_command, split_dictation_send_command, VoiceCommand,
};
use super::coordinator::{self, VoiceAction, VoiceConversationTurn, VoiceDecision};
use super::model::{VoiceModelManager, VoiceModelProgress, VoiceModelStatus};

const SEND_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(30);
const QUICK_RESUME_DELAY: Duration = Duration::from_millis(180);
const MAX_PROJECT_CONVERSATION_TURNS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMode {
    Director,
    Dictation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoicePhase {
    Idle,
    Downloading { downloaded: u64, total: u64 },
    RequestingPermission,
    Loading,
    Listening,
    Transcribing,
    Thinking,
    Speaking,
    Resuming,
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceSessionMode {
    #[default]
    Conversation,
    Writing,
    AwaitingSend,
}

#[derive(Clone, Debug)]
pub enum VoiceEvent {
    Decision(VoiceDecision),
    CreatePlan {
        project_id: ProjectId,
        prompt: String,
    },
    Dictation {
        agent_id: Option<uuid::Uuid>,
        text: String,
        insert_at_cursor: bool,
    },
    SendDraft {
        agent_id: uuid::Uuid,
        fallback_text: String,
    },
    DiscardDraft {
        agent_id: uuid::Uuid,
        text: String,
    },
}

enum WorkerEvent {
    Download(u64, VoiceModelProgress),
    ModelsReady(u64),
    Recognition(u64, RecognitionEvent),
    Decision(u64, ProjectId, anyhow::Result<VoiceDecision>),
}

pub struct VoiceState {
    workspace: Entity<Workspace>,
    agents: Entity<AgentRecords>,
    chats: Entity<AgentChatState>,
    models: VoiceModelManager,
    model_status: VoiceModelStatus,
    phase: VoicePhase,
    requested_mode: Option<VoiceMode>,
    dictation_target: Option<uuid::Uuid>,
    session_active: bool,
    session_mode: VoiceSessionMode,
    open_chat_target: Option<uuid::Uuid>,
    draft_target: Option<uuid::Uuid>,
    draft_text: Option<String>,
    last_command: Option<String>,
    session_notice: Option<String>,
    resume_at: Option<Instant>,
    confirmation_expires_at: Option<Instant>,
    level: f32,
    history: Vec<StoredVoiceTurn>,
    project_conversations: HashMap<ProjectId, Vec<VoiceConversationTurn>>,
    events_tx: mpsc::Sender<WorkerEvent>,
    events_rx: Arc<Mutex<mpsc::Receiver<WorkerEvent>>>,
    cancellation: Option<Arc<AtomicBool>>,
    finish_requested: Option<Arc<AtomicBool>>,
    push_to_talk_mode: bool,
    push_to_talk_held: bool,
    continuous_dictation: bool,
    operation_id: u64,
    last_file: Option<String>,
    input_devices: Vec<audio::VoiceInputDevice>,
    selected_input_device: Option<String>,
    #[cfg(target_os = "macos")]
    #[allow(deprecated)]
    speaker: Option<objc2::rc::Retained<objc2_app_kit::NSSpeechSynthesizer>>,
}

impl EventEmitter<VoiceEvent> for VoiceState {}

impl VoiceState {
    pub fn view(
        workspace: Entity<Workspace>,
        agents: Entity<AgentRecords>,
        chats: Entity<AgentChatState>,
        cx: &mut App,
    ) -> Entity<Self> {
        let models = VoiceModelManager::default();
        let model_status = models.status();
        let (retention, selected_input_device) = {
            let voice = &workspace.read(cx).voice;
            (voice.transcript_retention_days, voice.input_device.clone())
        };
        let input_devices = audio::input_devices().unwrap_or_default();
        if model_status == VoiceModelStatus::Ready {
            let warm_models = models.clone();
            std::thread::spawn(move || {
                if let Err(error) = audio::warm_transcriber(&warm_models) {
                    eprintln!("could not warm the local voice model: {error:#}");
                }
            });
        }
        let history = ide_core::local_store::LocalStore::open_default()
            .and_then(|store| {
                store.prune_voice_turns(retention)?;
                store.load_voice_turns(80)
            })
            .unwrap_or_default();
        let (events_tx, events_rx) = mpsc::channel();
        let events_rx = Arc::new(Mutex::new(events_rx));
        cx.new(|cx| {
            let mut state = Self {
                workspace,
                agents,
                chats,
                models,
                model_status,
                phase: VoicePhase::Idle,
                requested_mode: None,
                dictation_target: None,
                session_active: false,
                session_mode: VoiceSessionMode::Conversation,
                open_chat_target: None,
                draft_target: None,
                draft_text: None,
                last_command: None,
                session_notice: None,
                resume_at: None,
                confirmation_expires_at: None,
                level: 0.0,
                history,
                project_conversations: HashMap::new(),
                events_tx,
                events_rx,
                cancellation: None,
                finish_requested: None,
                push_to_talk_mode: false,
                push_to_talk_held: false,
                continuous_dictation: false,
                operation_id: 0,
                last_file: None,
                input_devices,
                selected_input_device,
                #[cfg(target_os = "macos")]
                speaker: None,
            };
            state.start_event_pump(cx);
            state
        })
    }

    pub fn phase(&self) -> &VoicePhase {
        &self.phase
    }

    pub fn model_status(&self) -> VoiceModelStatus {
        self.model_status
    }

    pub fn dictation_active(&self) -> bool {
        self.requested_mode == Some(VoiceMode::Dictation)
    }

    pub fn push_to_talk_held(&self) -> bool {
        self.push_to_talk_held
    }

    pub fn continuous_dictation_active(&self) -> bool {
        self.continuous_dictation && self.dictation_active()
    }

    pub fn control_active(&self) -> bool {
        self.session_active || self.dictation_active()
    }

    pub fn set_open_chat_target(&mut self, agent_id: Option<uuid::Uuid>, cx: &mut Context<Self>) {
        if self.open_chat_target == agent_id {
            return;
        }
        self.open_chat_target = agent_id;
        cx.notify();
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn history(&self) -> &[StoredVoiceTurn] {
        &self.history
    }

    pub(crate) fn input_devices(&self) -> &[audio::VoiceInputDevice] {
        &self.input_devices
    }

    pub fn selected_input_device(&self) -> Option<&str> {
        self.selected_input_device.as_deref()
    }

    pub fn refresh_input_devices(&mut self, cx: &mut Context<Self>) {
        self.input_devices = audio::input_devices().unwrap_or_default();
        if self.selected_input_device.as_ref().is_some_and(|selected| {
            !self
                .input_devices
                .iter()
                .any(|device| device.name.as_str() == selected.as_str())
        }) {
            self.selected_input_device = None;
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_voice_input_device(None, cx)
            });
        }
        cx.notify();
    }

    pub fn select_input_device(&mut self, input_device: Option<String>, cx: &mut Context<Self>) {
        self.selected_input_device = input_device.clone();
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_voice_input_device(input_device, cx)
        });
        self.refresh_input_devices(cx);
        if self.session_active
            && matches!(
                self.phase,
                VoicePhase::RequestingPermission
                    | VoicePhase::Loading
                    | VoicePhase::Listening
                    | VoicePhase::Resuming
                    | VoicePhase::Error(_)
            )
        {
            self.restart_session_recognition(cx);
        } else {
            cx.notify();
        }
    }

    pub fn toggle_director(&mut self, cx: &mut Context<Self>) {
        if self.session_active {
            self.stop(cx);
            self.session_notice = Some("Voice session ended".to_string());
            cx.notify();
        } else {
            self.start_session(cx);
        }
    }

    pub fn activate_dictation_for(&mut self, agent_id: uuid::Uuid, cx: &mut Context<Self>) {
        self.dictation_target = Some(agent_id);
        self.activate(VoiceMode::Dictation, false, false, cx);
    }

    pub fn begin_push_to_talk_for(&mut self, agent_id: uuid::Uuid, cx: &mut Context<Self>) {
        if self.push_to_talk_held || self.dictation_active() {
            return;
        }
        self.dictation_target = Some(agent_id);
        self.activate(VoiceMode::Dictation, true, false, cx);
    }

    pub fn begin_continuous_dictation_for(&mut self, agent_id: uuid::Uuid, cx: &mut Context<Self>) {
        self.dictation_target = Some(agent_id);
        self.activate(VoiceMode::Dictation, false, true, cx);
    }

    pub fn report_missing_dictation_target(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        let message = "Open an agent chat before using voice dictation.".to_string();
        self.phase = VoicePhase::Error(message.clone());
        self.session_notice = Some(message);
        cx.notify();
    }

    pub fn finish_push_to_talk(&mut self, cx: &mut Context<Self>) {
        if !self.push_to_talk_held || !self.dictation_active() {
            return;
        }
        self.push_to_talk_held = false;
        if self.model_status == VoiceModelStatus::Installing {
            self.requested_mode = None;
            self.dictation_target = None;
            self.push_to_talk_mode = false;
            self.session_notice =
                Some("Voice is still installing. Hold ⌘L again when it is ready.".to_string());
        } else if let Some(finish_requested) = self.finish_requested.as_ref() {
            finish_requested.store(true, Ordering::Relaxed);
            self.phase = VoicePhase::Transcribing;
            self.level = 0.0;
        } else {
            self.requested_mode = None;
            self.dictation_target = None;
            self.phase = VoicePhase::Idle;
        }
        cx.notify();
    }

    pub fn prepare_models(&mut self, cx: &mut Context<Self>) {
        if self.model_status == VoiceModelStatus::Ready {
            return;
        }
        self.requested_mode = None;
        self.install_models(cx);
    }

    #[allow(deprecated)]
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.operation_id = self.operation_id.wrapping_add(1);
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.store(true, Ordering::Relaxed);
        }
        self.finish_requested = None;
        self.push_to_talk_mode = false;
        self.push_to_talk_held = false;
        self.continuous_dictation = false;
        #[cfg(target_os = "macos")]
        if let Some(speaker) = self.speaker.take() {
            speaker.stopSpeaking();
        }
        self.requested_mode = None;
        self.dictation_target = None;
        self.session_active = false;
        self.session_mode = VoiceSessionMode::Conversation;
        self.draft_target = None;
        self.draft_text = None;
        self.resume_at = None;
        self.confirmation_expires_at = None;
        if self.model_status == VoiceModelStatus::Installing {
            self.model_status = self.models.status();
            self.last_file = None;
        }
        self.phase = VoicePhase::Idle;
        self.level = 0.0;
        cx.notify();
    }

    pub fn remove_models(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.stop(cx);
        self.models.remove()?;
        self.model_status = VoiceModelStatus::NotInstalled;
        cx.notify();
        Ok(())
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        ide_core::local_store::LocalStore::open_default()?.clear_voice_turns()?;
        self.history.clear();
        cx.notify();
        Ok(())
    }

    fn activate(
        &mut self,
        mode: VoiceMode,
        push_to_talk: bool,
        continuous_dictation: bool,
        cx: &mut Context<Self>,
    ) {
        self.refresh_input_devices(cx);
        let dictation_target = (mode == VoiceMode::Dictation)
            .then(|| self.dictation_target.take())
            .flatten();
        self.stop(cx);
        self.dictation_target = dictation_target;
        self.requested_mode = Some(mode);
        self.push_to_talk_mode = push_to_talk;
        self.push_to_talk_held = push_to_talk;
        self.continuous_dictation = continuous_dictation;
        if self.model_status != VoiceModelStatus::Ready {
            self.install_models(cx);
            return;
        }
        self.start_recognition(cx);
    }

    fn start_session(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        let has_active_project = {
            let workspace = self.workspace.read(cx);
            workspace.active.is_some_and(|project_id| {
                workspace
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
            })
        };
        if !has_active_project {
            let message = "Open a project before starting Project Talk.".to_string();
            self.phase = VoicePhase::Error(message.clone());
            self.session_notice = Some(message);
            cx.notify();
            return;
        }
        self.refresh_input_devices(cx);
        self.session_active = true;
        self.session_mode = VoiceSessionMode::Conversation;
        self.session_notice = None;
        self.last_command = None;
        self.confirmation_expires_at = None;
        self.requested_mode = Some(VoiceMode::Director);
        if self.model_status != VoiceModelStatus::Ready {
            self.install_models(cx);
        } else {
            self.start_recognition(cx);
        }
    }

    #[allow(deprecated)]
    fn restart_session_recognition(&mut self, cx: &mut Context<Self>) {
        self.operation_id = self.operation_id.wrapping_add(1);
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.store(true, Ordering::Relaxed);
        }
        self.finish_requested = None;
        self.push_to_talk_mode = false;
        self.push_to_talk_held = false;
        self.continuous_dictation = false;
        #[cfg(target_os = "macos")]
        if let Some(speaker) = self.speaker.take() {
            speaker.stopSpeaking();
        }
        self.resume_at = None;
        self.requested_mode = Some(VoiceMode::Director);
        self.start_recognition(cx);
    }

    fn install_models(&mut self, cx: &mut Context<Self>) {
        if self.model_status == VoiceModelStatus::Installing {
            return;
        }
        self.model_status = VoiceModelStatus::Installing;
        self.phase = VoicePhase::Downloading {
            downloaded: 0,
            total: super::model::MODEL_DOWNLOAD_BYTES,
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        self.cancellation = Some(cancellation.clone());
        self.operation_id = self.operation_id.wrapping_add(1);
        let operation_id = self.operation_id;
        let manager = self.models.clone();
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = manager.install(&cancellation, |progress| {
                let _ = tx.send(WorkerEvent::Download(operation_id, progress));
            });
            match result {
                Ok(()) => {
                    let _ = tx.send(WorkerEvent::ModelsReady(operation_id));
                }
                Err(error) => {
                    let event = if cancellation.load(Ordering::Relaxed) {
                        RecognitionEvent::Cancelled
                    } else {
                        RecognitionEvent::Error(format!(
                            "Voice model installation failed: {error:#}"
                        ))
                    };
                    let _ = tx.send(WorkerEvent::Recognition(operation_id, event));
                }
            }
        });
        cx.notify();
    }

    fn start_recognition(&mut self, cx: &mut Context<Self>) {
        let cancellation = Arc::new(AtomicBool::new(false));
        self.cancellation = Some(cancellation.clone());
        let finish_requested = Arc::new(AtomicBool::new(false));
        self.finish_requested = Some(finish_requested.clone());
        self.operation_id = self.operation_id.wrapping_add(1);
        let operation_id = self.operation_id;
        self.phase = VoicePhase::Loading;
        self.level = 0.0;
        let models = self.models.clone();
        let confirmation_only = self.session_mode == VoiceSessionMode::AwaitingSend;
        let push_to_talk =
            self.push_to_talk_mode && self.requested_mode == Some(VoiceMode::Dictation);
        let continuous_dictation =
            self.continuous_dictation && self.requested_mode == Some(VoiceMode::Dictation);
        let patient = self.workspace.read(cx).voice.patient_turn_taking
            && !confirmation_only
            && !continuous_dictation;
        let input_device = self.selected_input_device.clone();
        let silence_timeout =
            if self.session_active && self.requested_mode == Some(VoiceMode::Director) {
                Duration::from_secs(120)
            } else {
                Duration::from_secs(30)
            };
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let (recognition_tx, recognition_rx) = mpsc::channel();
            let forward = tx.clone();
            std::thread::spawn(move || {
                let mut saw_terminal_event = false;
                while let Ok(event) = recognition_rx.recv() {
                    let terminal = matches!(
                        event,
                        RecognitionEvent::Cancelled | RecognitionEvent::Error(_)
                    ) || (!continuous_dictation
                        && matches!(
                            event,
                            RecognitionEvent::Transcript(_)
                                | RecognitionEvent::UnrecognizedSpeech
                                | RecognitionEvent::SilenceTimeout
                        ));
                    let _ = forward.send(WorkerEvent::Recognition(operation_id, event));
                    if terminal {
                        saw_terminal_event = true;
                        break;
                    }
                }
                if !saw_terminal_event {
                    let _ = forward.send(WorkerEvent::Recognition(
                        operation_id,
                        RecognitionEvent::Error(
                            "Voice recognition stopped unexpectedly. Please try again.".to_string(),
                        ),
                    ));
                }
            });
            let panic_tx = recognition_tx.clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                audio::recognize_one_turn(
                    models,
                    patient,
                    confirmation_only,
                    push_to_talk,
                    continuous_dictation,
                    input_device,
                    silence_timeout,
                    cancellation,
                    finish_requested,
                    recognition_tx,
                );
            }))
            .is_err()
            {
                let _ = panic_tx.send(RecognitionEvent::Error(
                    "The local voice engine could not start. Please try again.".to_string(),
                ));
            }
        });
        cx.notify();
    }

    fn start_event_pump(&mut self, cx: &mut Context<Self>) {
        let rx = self.events_rx.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let events = {
                let receiver = rx.lock().unwrap();
                std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>()
            };
            if events.is_empty() {
                if this.upgrade().is_none() {
                    break;
                }
                let _ = this.update(cx, |state, cx| state.advance_session(cx));
                continue;
            }
            if this
                .update(cx, |state, cx| {
                    for event in events {
                        state.handle_worker_event(event, cx);
                    }
                    state.advance_session(cx);
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    fn handle_worker_event(&mut self, event: WorkerEvent, cx: &mut Context<Self>) {
        match event {
            WorkerEvent::Download(operation_id, progress) => {
                if operation_id != self.operation_id {
                    return;
                }
                if self.model_status != VoiceModelStatus::Installing {
                    return;
                }
                self.last_file = Some(progress.file);
                self.phase = VoicePhase::Downloading {
                    downloaded: progress.downloaded_bytes,
                    total: progress.total_bytes,
                };
            }
            WorkerEvent::ModelsReady(operation_id) => {
                // A download may finish in the narrow race after cancellation.
                // Record the verified installation, but never resume listening
                // for an operation the user already stopped.
                if operation_id != self.operation_id {
                    self.model_status = self.models.status();
                    return;
                }
                self.cancellation = None;
                self.model_status = VoiceModelStatus::Ready;
                self.last_file = None;
                if self.requested_mode.is_some() {
                    self.start_recognition(cx);
                } else {
                    self.phase = VoicePhase::Idle;
                }
            }
            WorkerEvent::Recognition(operation_id, event) => {
                if operation_id != self.operation_id {
                    return;
                }
                self.handle_recognition_event(event, cx);
            }
            WorkerEvent::Decision(operation_id, project_id, result) => {
                if operation_id != self.operation_id {
                    return;
                }
                self.handle_decision(project_id, result, cx);
            }
        }
        cx.notify();
    }

    fn handle_recognition_event(&mut self, event: RecognitionEvent, cx: &mut Context<Self>) {
        match event {
            RecognitionEvent::RequestingPermission => {
                self.phase = VoicePhase::RequestingPermission;
                self.level = 0.0;
            }
            RecognitionEvent::PermissionGranted => {
                self.phase = VoicePhase::Loading;
            }
            RecognitionEvent::Level(level) => {
                if matches!(self.phase, VoicePhase::Loading) {
                    self.phase = VoicePhase::Listening;
                }
                self.level = level;
            }
            RecognitionEvent::Transcribing => {
                self.phase = VoicePhase::Transcribing;
                self.level = 0.0;
            }
            RecognitionEvent::Cancelled => {
                if self.model_status == VoiceModelStatus::Installing {
                    self.model_status = self.models.status();
                    self.last_file = None;
                }
                self.phase = VoicePhase::Idle;
                self.level = 0.0;
                self.finish_requested = None;
                self.push_to_talk_mode = false;
                self.push_to_talk_held = false;
                self.continuous_dictation = false;
            }
            RecognitionEvent::SilenceTimeout => {
                let kept_draft = self.draft_text.is_some();
                let confirmation_timed_out = self.session_mode == VoiceSessionMode::AwaitingSend;
                self.session_active = false;
                self.session_mode = VoiceSessionMode::Conversation;
                self.draft_target = None;
                self.draft_text = None;
                self.requested_mode = None;
                self.cancellation = None;
                self.finish_requested = None;
                self.push_to_talk_mode = false;
                self.push_to_talk_held = false;
                self.continuous_dictation = false;
                self.resume_at = None;
                self.confirmation_expires_at = None;
                self.phase = VoicePhase::Idle;
                self.level = 0.0;
                self.session_notice = Some(if confirmation_timed_out && kept_draft {
                    "No send answer after 30 seconds · draft kept in the composer".to_string()
                } else if kept_draft {
                    "Stopped after two minutes without speech · draft kept in the composer"
                        .to_string()
                } else {
                    "Stopped after two minutes without speech".to_string()
                });
            }
            RecognitionEvent::UnrecognizedSpeech => {
                if self.continuous_dictation_active() {
                    self.level = 0.0;
                    self.phase = VoicePhase::Listening;
                    self.session_notice = Some("No words caught · still listening".to_string());
                    return;
                }
                self.cancellation = None;
                self.finish_requested = None;
                self.push_to_talk_mode = false;
                self.push_to_talk_held = false;
                self.continuous_dictation = false;
                self.level = 0.0;
                self.session_notice = Some("No words caught · still listening".to_string());
                if self.session_active && self.requested_mode == Some(VoiceMode::Director) {
                    self.schedule_resume(Duration::from_millis(180), cx);
                } else {
                    self.requested_mode = None;
                    self.phase = VoicePhase::Idle;
                }
            }
            RecognitionEvent::Error(error) => {
                if self.model_status == VoiceModelStatus::Installing {
                    self.model_status = VoiceModelStatus::NotInstalled;
                }
                self.phase = VoicePhase::Error(error);
                self.level = 0.0;
                self.cancellation = None;
                self.finish_requested = None;
                self.push_to_talk_mode = false;
                self.push_to_talk_held = false;
                self.continuous_dictation = false;
                self.session_active = false;
                self.requested_mode = None;
                self.resume_at = None;
                self.confirmation_expires_at = None;
            }
            RecognitionEvent::Transcript(text) => {
                self.level = 0.0;
                self.session_notice = None;
                let mode = self.requested_mode.unwrap_or(VoiceMode::Director);
                match mode {
                    VoiceMode::Dictation => {
                        let continuous = self.continuous_dictation_active();
                        let target = self.dictation_target;
                        let (text, send_after_insert) = split_dictation_send_command(&text);
                        if !text.is_empty() {
                            self.save_turn("dictation", "user", &text, target);
                            cx.emit(VoiceEvent::Dictation {
                                agent_id: target,
                                text: text.clone(),
                                insert_at_cursor: true,
                            });
                        }
                        if send_after_insert {
                            if let Some(agent_id) = target {
                                self.save_turn("command", "user", "send", Some(agent_id));
                                cx.emit(VoiceEvent::SendDraft {
                                    agent_id,
                                    fallback_text: text,
                                });
                            }
                        }

                        if continuous && send_after_insert {
                            self.stop(cx);
                            self.session_notice =
                                Some("Sent. Hands-free dictation stopped.".to_string());
                        } else if continuous {
                            self.phase = VoicePhase::Listening;
                        } else {
                            self.cancellation = None;
                            self.finish_requested = None;
                            self.push_to_talk_mode = false;
                            self.push_to_talk_held = false;
                            self.continuous_dictation = false;
                            self.requested_mode = None;
                            self.dictation_target = None;
                            self.phase = VoicePhase::Idle;
                        }
                    }
                    VoiceMode::Director => {
                        self.cancellation = None;
                        self.finish_requested = None;
                        self.push_to_talk_mode = false;
                        self.push_to_talk_held = false;
                        self.continuous_dictation = false;
                        self.handle_session_utterance(text, cx);
                    }
                }
            }
        }
    }

    fn handle_session_utterance(&mut self, text: String, cx: &mut Context<Self>) {
        if !self.session_active {
            return;
        }
        if self.session_mode == VoiceSessionMode::AwaitingSend {
            if let Some(command) = parse_draft_confirmation(&text) {
                self.last_command = Some(command_label(&command).to_string());
                self.save_turn("command", "user", &text, self.draft_target);
                self.handle_voice_command(command, cx);
            } else {
                // Confirmation mode is deliberately narrow: unrelated speech
                // is ignored and never becomes dictation or a conversation.
                self.last_command = None;
                self.session_notice =
                    Some("Send the draft? Say yes or no. ⌘L stops Voice.".to_string());
                self.schedule_resume(QUICK_RESUME_DELAY, cx);
            }
            return;
        }
        self.last_command = None;
        match self.session_mode {
            VoiceSessionMode::Conversation => {
                if let Some(command) = parse_project_conversation_command(&text) {
                    self.last_command = Some(command_label(&command).to_string());
                    self.save_turn("command", "user", &text, None);
                    self.handle_voice_command(command, cx);
                    return;
                }
                let Some(project_id) = self.workspace.read(cx).active else {
                    self.show_and_continue("Open a project to ask about it.", None, cx);
                    return;
                };
                let prior_conversation = self
                    .project_conversations
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_default();
                self.append_project_conversation(project_id, "user", text.clone());
                self.save_turn("project_talk", "user", &text, None);
                self.begin_coordination(project_id, prior_conversation, text, cx);
            }
            VoiceSessionMode::Writing => {
                if let Some(command) = parse_voice_command(&text) {
                    self.last_command = Some(command_label(&command).to_string());
                    self.save_turn("command", "user", &text, self.draft_target);
                    self.handle_voice_command(command, cx);
                    return;
                }
                let Some(agent_id) = self.draft_target else {
                    self.session_mode = VoiceSessionMode::Conversation;
                    self.show_and_continue(
                        "I lost the target chat. Open a chat and say please write again.",
                        None,
                        cx,
                    );
                    return;
                };
                if self.open_chat_target != Some(agent_id) {
                    self.session_mode = VoiceSessionMode::Conversation;
                    self.draft_target = None;
                    self.show_and_continue(
                        "That agent chat is no longer open. Open it, then say please write again.",
                        None,
                        cx,
                    );
                    return;
                }
                self.save_turn("dictation", "user", &text, Some(agent_id));
                self.write_draft(agent_id, text, cx);
            }
            VoiceSessionMode::AwaitingSend => unreachable!("handled before general commands"),
        }
    }

    fn handle_voice_command(&mut self, command: VoiceCommand, cx: &mut Context<Self>) {
        match command {
            VoiceCommand::CreateProjectPlan(topic) => {
                let project = {
                    let workspace = self.workspace.read(cx);
                    workspace
                        .active
                        .and_then(|project_id| {
                            workspace
                                .projects
                                .iter()
                                .find(|project| project.id == project_id)
                        })
                        .cloned()
                };
                let Some(project) = project else {
                    self.show_and_continue("Open a project before creating a plan.", None, cx);
                    return;
                };
                let conversation = self
                    .project_conversations
                    .get(&project.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let prompt = project_plan_prompt(&project.name, conversation, topic.as_deref());
                cx.emit(VoiceEvent::CreatePlan {
                    project_id: project.id,
                    prompt,
                });
                self.end_voice_session(
                    &format!("Opened a plan agent for {}.", project.name),
                    None,
                    cx,
                );
            }
            VoiceCommand::StartWriting => {
                if self.session_mode == VoiceSessionMode::AwaitingSend {
                    self.show_and_continue(
                        "There is already a draft waiting. Say send or cancel draft first.",
                        self.draft_target,
                        cx,
                    );
                    return;
                }
                match self.selected_writable_chat(cx) {
                    Ok((agent_id, title)) => {
                        self.session_mode = VoiceSessionMode::Writing;
                        self.draft_target = Some(agent_id);
                        self.draft_text = None;
                        self.confirmation_expires_at = None;
                        self.show_and_continue(
                            &format!("Write mode for {title}. Tell me the message."),
                            Some(agent_id),
                            cx,
                        );
                    }
                    Err(message) => {
                        self.session_mode = VoiceSessionMode::Conversation;
                        self.draft_target = None;
                        self.draft_text = None;
                        self.confirmation_expires_at = None;
                        self.show_and_continue(&message, None, cx);
                    }
                }
            }
            VoiceCommand::Write(text) => {
                if self.session_mode == VoiceSessionMode::AwaitingSend {
                    self.show_and_continue(
                        "There is already a draft waiting. Say send or cancel draft first.",
                        self.draft_target,
                        cx,
                    );
                    return;
                }
                match self.selected_writable_chat(cx) {
                    Ok((agent_id, _)) => self.write_draft(agent_id, text, cx),
                    Err(message) => {
                        self.session_mode = VoiceSessionMode::Conversation;
                        self.draft_target = None;
                        self.draft_text = None;
                        self.confirmation_expires_at = None;
                        self.show_and_continue(&message, None, cx);
                    }
                }
            }
            VoiceCommand::WriteAndSend(text) => match self.selected_writable_chat(cx) {
                Ok((agent_id, _)) => self.write_and_send(agent_id, text, cx),
                Err(message) => {
                    self.session_mode = VoiceSessionMode::Conversation;
                    self.draft_target = None;
                    self.draft_text = None;
                    self.confirmation_expires_at = None;
                    self.show_and_continue(&message, None, cx);
                }
            },
            VoiceCommand::SendDraft => {
                let (Some(agent_id), Some(fallback_text)) =
                    (self.draft_target, self.draft_text.clone())
                else {
                    self.show_and_continue(
                        "There is no voice draft to send. Say please write first.",
                        None,
                        cx,
                    );
                    return;
                };
                cx.emit(VoiceEvent::SendDraft {
                    agent_id,
                    fallback_text,
                });
                self.end_voice_session("Sent. Voice stopped.", Some(agent_id), cx);
            }
            VoiceCommand::KeepDraft => {
                if self.draft_text.is_some() {
                    let agent_id = self.draft_target;
                    self.end_voice_session(
                        "Not sent. The draft is still in the composer.",
                        agent_id,
                        cx,
                    );
                } else {
                    self.show_and_continue("There is no voice draft to keep.", None, cx);
                }
            }
            VoiceCommand::CancelDraft => {
                if let (Some(agent_id), Some(text)) = (self.draft_target, self.draft_text.take()) {
                    cx.emit(VoiceEvent::DiscardDraft { agent_id, text });
                    self.end_voice_session("Draft cancelled. Voice stopped.", None, cx);
                } else if self.session_mode == VoiceSessionMode::Writing {
                    self.session_mode = VoiceSessionMode::Conversation;
                    self.draft_target = None;
                    self.confirmation_expires_at = None;
                    self.show_and_continue("Write mode cancelled. Conversation mode.", None, cx);
                } else {
                    self.show_and_continue("There is no voice draft to cancel.", None, cx);
                }
            }
            VoiceCommand::StopListening => {
                let message = if self.draft_text.is_some() {
                    "Voice stopped. Your draft is still in the composer."
                } else {
                    "Voice stopped."
                };
                self.end_voice_session(message, None, cx);
            }
            VoiceCommand::Help => {
                let message = if self.session_mode == VoiceSessionMode::Conversation {
                    "Ask about this project, recent commits, or an approach. Say create a plan for that to hand the discussion to a new plan agent, or stop listening to end Project Talk."
                } else {
                    "Say send to submit the draft, cancel draft to remove it, or stop listening to end the session."
                };
                self.show_and_continue(message, self.draft_target, cx);
            }
        }
    }

    fn selected_writable_chat(&self, cx: &App) -> Result<(uuid::Uuid, String), String> {
        let project_id = self.workspace.read(cx).active.ok_or_else(|| {
            "No agent chat is open. Open a chat, then say please write again.".to_string()
        })?;
        let agent_id = self.open_chat_target.ok_or_else(|| {
            "No agent chat is open. Open a chat, then say please write again.".to_string()
        })?;
        let agents = self.agents.read(cx);
        let agent = agents
            .agent(agent_id)
            .filter(|agent| {
                agent.project_id == project_id && agent.runtime == ide_core::AgentRuntimeKind::Chat
            })
            .ok_or_else(|| {
                "No agent chat is open. Open a chat, then say please write again.".to_string()
            })?;
        let title = agent.title.clone();

        if self
            .chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                session.pending_user_input.is_some() || session.pending_approval.is_some()
            })
        {
            return Err(
                "That chat is waiting for a visible answer or approval. Use its on-screen controls first."
                    .to_string(),
            );
        }
        Ok((agent_id, title))
    }

    fn write_draft(&mut self, agent_id: uuid::Uuid, text: String, cx: &mut Context<Self>) {
        self.draft_target = Some(agent_id);
        self.draft_text = Some(text.clone());
        self.session_mode = VoiceSessionMode::AwaitingSend;
        self.confirmation_expires_at = Some(Instant::now() + SEND_CONFIRMATION_TIMEOUT);
        cx.emit(VoiceEvent::Dictation {
            agent_id: Some(agent_id),
            text,
            insert_at_cursor: false,
        });
        self.show_and_continue(
            "Draft ready. Say yes to send or no to keep it. ⌘L stops Voice.",
            Some(agent_id),
            cx,
        );
    }

    fn write_and_send(&mut self, agent_id: uuid::Uuid, text: String, cx: &mut Context<Self>) {
        self.save_turn("dictation", "user", &text, Some(agent_id));
        cx.emit(VoiceEvent::Dictation {
            agent_id: Some(agent_id),
            text: text.clone(),
            insert_at_cursor: false,
        });
        cx.emit(VoiceEvent::SendDraft {
            agent_id,
            fallback_text: text,
        });
        self.end_voice_session("Sent. Voice stopped.", Some(agent_id), cx);
    }

    #[allow(deprecated)]
    fn end_voice_session(
        &mut self,
        message: &str,
        agent_id: Option<uuid::Uuid>,
        cx: &mut Context<Self>,
    ) {
        self.operation_id = self.operation_id.wrapping_add(1);
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.store(true, Ordering::Relaxed);
        }
        self.finish_requested = None;
        self.push_to_talk_mode = false;
        self.push_to_talk_held = false;
        self.continuous_dictation = false;
        #[cfg(target_os = "macos")]
        if let Some(speaker) = self.speaker.take() {
            speaker.stopSpeaking();
        }
        self.session_active = false;
        self.session_mode = VoiceSessionMode::Conversation;
        self.requested_mode = None;
        self.resume_at = None;
        self.confirmation_expires_at = None;
        self.draft_target = None;
        self.draft_text = None;
        self.level = 0.0;
        self.phase = VoicePhase::Idle;
        self.save_turn("command", "assistant", message, agent_id);
        self.session_notice = Some(message.to_string());
        cx.notify();
    }

    fn show_and_continue(
        &mut self,
        message: &str,
        agent_id: Option<uuid::Uuid>,
        cx: &mut Context<Self>,
    ) {
        self.save_turn("director", "assistant", message, agent_id);
        self.session_notice = Some(message.to_string());
        self.finish_turn(false, cx);
    }

    fn finish_turn(&mut self, speaking: bool, cx: &mut Context<Self>) {
        if !self.session_active {
            self.phase = if speaking {
                VoicePhase::Speaking
            } else {
                VoicePhase::Idle
            };
            return;
        }
        if speaking {
            self.phase = VoicePhase::Speaking;
            self.resume_at = None;
        } else {
            self.schedule_resume(QUICK_RESUME_DELAY, cx);
        }
    }

    fn schedule_resume(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if !self.session_active {
            self.phase = VoicePhase::Idle;
            self.resume_at = None;
            return;
        }
        self.phase = VoicePhase::Resuming;
        self.resume_at = Some(Instant::now() + delay);
        cx.notify();
    }

    fn handle_decision(
        &mut self,
        project_id: ProjectId,
        result: anyhow::Result<VoiceDecision>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(decision) => {
                let agent_id = match decision.action {
                    VoiceAction::None => None,
                    VoiceAction::FocusChat { agent_id }
                    | VoiceAction::SendPrompt { agent_id, .. }
                    | VoiceAction::StopRun { agent_id }
                    | VoiceAction::ShowPendingInput { agent_id } => Some(agent_id),
                };
                self.append_project_conversation(project_id, "assistant", decision.speech.clone());
                self.save_turn("project_talk", "assistant", &decision.speech, agent_id);
                let speaking = if self.session_active {
                    self.speak_session_response(&decision.speech, cx)
                } else {
                    self.speak_if_enabled(&decision.speech, &decision.action, true, cx)
                };
                cx.emit(VoiceEvent::Decision(decision));
                if self.session_active {
                    self.finish_turn(speaking, cx);
                } else {
                    self.phase = if speaking {
                        VoicePhase::Speaking
                    } else {
                        VoicePhase::Idle
                    };
                }
            }
            Err(error) => {
                eprintln!("Project Talk failed: {error:#}");
                self.session_notice =
                    Some("I couldn’t answer from the project · still listening".to_string());
                self.finish_turn(false, cx);
            }
        }
    }

    fn begin_coordination(
        &mut self,
        project_id: ProjectId,
        conversation: Vec<VoiceConversationTurn>,
        utterance: String,
        cx: &mut Context<Self>,
    ) {
        self.phase = VoicePhase::Thinking;
        let project_context = {
            let workspace = self.workspace.read(cx);
            workspace
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .cloned()
                .map(|project| (project, workspace.generation_agent.clone()))
        };
        let Some((project, generation_agent)) = project_context else {
            self.show_and_continue("That project is no longer open.", None, cx);
            return;
        };
        let tx = self.events_tx.clone();
        let operation_id = self.operation_id;
        std::thread::spawn(move || {
            let result = coordinator::coordinate(
                &generation_agent,
                &project.name,
                &project.path,
                &conversation,
                &utterance,
            );
            let _ = tx.send(WorkerEvent::Decision(operation_id, project_id, result));
        });
    }

    fn append_project_conversation(
        &mut self,
        project_id: ProjectId,
        role: impl Into<String>,
        text: impl Into<String>,
    ) {
        let conversation = self.project_conversations.entry(project_id).or_default();
        conversation.push(VoiceConversationTurn::new(role, text));
        if conversation.len() > MAX_PROJECT_CONVERSATION_TURNS {
            conversation.drain(..conversation.len() - MAX_PROJECT_CONVERSATION_TURNS);
        }
    }

    fn save_turn(&mut self, mode: &str, role: &str, text: &str, agent_id: Option<uuid::Uuid>) {
        match ide_core::local_store::LocalStore::open_default()
            .and_then(|store| store.save_voice_turn(mode, role, text, agent_id))
        {
            Ok(turn) => {
                self.history.push(turn);
                if self.history.len() > 80 {
                    self.history.remove(0);
                }
            }
            Err(error) => eprintln!("could not save voice transcript: {error:#}"),
        }
    }

    fn speak_session_response(&mut self, text: &str, cx: &App) -> bool {
        self.speak_if_enabled(text, &VoiceAction::None, true, cx)
    }

    #[allow(deprecated)]
    fn speak_if_enabled(
        &mut self,
        text: &str,
        action: &VoiceAction,
        important: bool,
        cx: &App,
    ) -> bool {
        let settings = &self.workspace.read(cx).voice;
        let should_speak = match settings.announcements {
            VoiceAnnouncements::Off => false,
            VoiceAnnouncements::All => true,
            VoiceAnnouncements::Balanced => important || !matches!(action, VoiceAction::None),
            VoiceAnnouncements::Minimal => important,
        };
        if !should_speak {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            use objc2::AnyThread;
            use objc2_app_kit::NSSpeechSynthesizer;
            use objc2_foundation::NSString;

            if let Some(existing) = self.speaker.take() {
                existing.stopSpeaking();
            }
            let voice = settings.system_voice.as_deref().map(NSString::from_str);
            let Some(synth) =
                NSSpeechSynthesizer::initWithVoice(NSSpeechSynthesizer::alloc(), voice.as_deref())
            else {
                return false;
            };
            synth.setRate(180.0 * settings.speech_rate.clamp(0.6, 1.5));
            let spoken = NSString::from_str(text);
            if !synth.startSpeakingString(&spoken) {
                return false;
            }
            self.speaker = Some(synth);
            return true;
        }
        #[cfg(not(target_os = "macos"))]
        false
    }

    #[allow(deprecated)]
    fn advance_session(&mut self, cx: &mut Context<Self>) {
        if self.session_mode == VoiceSessionMode::AwaitingSend
            && self
                .confirmation_expires_at
                .is_some_and(|expires_at| Instant::now() >= expires_at)
        {
            self.end_voice_session(
                "No send answer after 30 seconds. The draft is still in the composer.",
                self.draft_target,
                cx,
            );
            return;
        }

        if matches!(self.phase, VoicePhase::Speaking) {
            #[cfg(target_os = "macos")]
            if self
                .speaker
                .as_ref()
                .is_some_and(|speaker| speaker.isSpeaking())
            {
                return;
            }
            #[cfg(target_os = "macos")]
            {
                self.speaker = None;
            }
            if self.session_active {
                // Keep the input stream closed briefly so the tail of native
                // speech output cannot become the next dictated turn.
                self.schedule_resume(Duration::from_millis(650), cx);
            } else {
                self.phase = VoicePhase::Idle;
                cx.notify();
            }
            return;
        }

        if matches!(self.phase, VoicePhase::Resuming)
            && self
                .resume_at
                .is_some_and(|resume_at| Instant::now() >= resume_at)
        {
            self.resume_at = None;
            self.requested_mode = Some(VoiceMode::Director);
            self.start_recognition(cx);
        }
    }
}

fn project_plan_prompt(
    project_name: &str,
    conversation: &[VoiceConversationTurn],
    topic: Option<&str>,
) -> String {
    let discussion = conversation
        .iter()
        .map(|turn| format!("{}: {}", turn.role, turn.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let discussion = discussion.chars().take(12_000).collect::<String>();
    let topic = topic
        .filter(|topic| !topic.trim().is_empty())
        .map(|topic| format!("\nThe user specifically asked to plan: {}\n", topic.trim()))
        .unwrap_or_default();
    format!(
        "Create an implementation plan for the active project, {project_name}. Inspect the project and Git history before proposing the plan. Use the prior Project Talk discussion below as context, resolve references such as ‘that’ from it, and ask only if a decision would materially change the plan. Do not implement anything; return a concrete, ordered plan with likely files, risks, and verification.\n{topic}\nProject Talk discussion:\n{discussion}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_handoff_contains_project_discussion_and_stays_plan_only() {
        let conversation = vec![
            VoiceConversationTurn::new("user", "How should caching work?"),
            VoiceConversationTurn::new("assistant", "Use a bounded local cache."),
        ];
        let prompt = project_plan_prompt("Example", &conversation, None);
        assert!(prompt.contains("Example"));
        assert!(prompt.contains("How should caching work?"));
        assert!(prompt.contains("Do not implement anything"));
    }
}
