use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use moonshine_rs::{ModelArch, Transcriber};

use super::model::VoiceModelManager;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const SPEECH_RMS: f32 = 0.012;
const NOISE_CALIBRATION_TICKS: usize = 4;
const SPEECH_ONSET_TICKS: usize = 3;
const NOISE_THRESHOLD_MULTIPLIER: f32 = 2.4;
const SPEECH_THRESHOLD_CEILING: f32 = 0.06;
const MIN_TURN_DURATION: Duration = Duration::from_millis(480);
const SMART_TURN_SILENCE: Duration = Duration::from_millis(500);
const FORCED_TURN_SILENCE: Duration = Duration::from_millis(1_200);
const MAX_UTTERANCE: Duration = Duration::from_secs(30);
const CONFIRMATION_MIN_TURN_DURATION: Duration = Duration::from_millis(240);
const CONFIRMATION_TURN_SILENCE: Duration = Duration::from_millis(360);
const CONFIRMATION_MAX_UTTERANCE: Duration = Duration::from_secs(4);
const PUSH_TO_TALK_POLL_INTERVAL: Duration = Duration::from_millis(20);
const PUSH_TO_TALK_MAX_DURATION: Duration = Duration::from_secs(120);
const MIN_PUSH_TO_TALK_AUDIO: Duration = Duration::from_millis(80);
const CONTINUOUS_POLL_INTERVAL: Duration = Duration::from_millis(40);
const CONTINUOUS_MIN_TURN_DURATION: Duration = Duration::from_millis(160);
const CONTINUOUS_TURN_SILENCE: Duration = Duration::from_millis(200);
const CONTINUOUS_IDLE_PRE_ROLL: Duration = Duration::from_millis(500);
const CONTINUATION_GRACE_TICKS: usize = 5;
const CONTINUATION_VARIATION_RMS: f32 = 0.0025;

type CachedTranscriber = Option<(std::path::PathBuf, Arc<Transcriber>)>;
static TRANSCRIBER: OnceLock<Mutex<CachedTranscriber>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VoiceInputDevice {
    pub(crate) name: String,
    pub(crate) is_default: bool,
}

#[derive(Debug)]
pub(super) enum RecognitionEvent {
    RequestingPermission,
    PermissionGranted,
    Level(f32),
    Transcribing,
    Transcript(String),
    UnrecognizedSpeech,
    SilenceTimeout,
    Cancelled,
    Error(String),
}

#[derive(Default)]
struct SpeechGate {
    calibration_ticks: usize,
    noise_floor: f32,
    onset_ticks: usize,
    continuation_grace_ticks: usize,
    last_rms: f32,
}

impl SpeechGate {
    fn observe(&mut self, rms: f32) -> bool {
        if self.calibration_ticks < NOISE_CALIBRATION_TICKS {
            self.calibration_ticks += 1;
            self.noise_floor += (rms - self.noise_floor) / self.calibration_ticks.max(1) as f32;
            self.last_rms = rms;
            return false;
        }

        if self.is_voice(rms) {
            self.onset_ticks += 1;
            self.last_rms = rms;
            if self.onset_ticks >= SPEECH_ONSET_TICKS {
                self.continuation_grace_ticks = CONTINUATION_GRACE_TICKS;
                return true;
            }
            return false;
        }

        self.onset_ticks = 0;
        self.noise_floor += (rms - self.noise_floor) * 0.05;
        self.last_rms = rms;
        false
    }

    fn is_voice(&self, rms: f32) -> bool {
        rms >= self.voice_threshold()
    }

    fn voice_threshold(&self) -> f32 {
        (self.noise_floor * NOISE_THRESHOLD_MULTIPLIER)
            .max(SPEECH_RMS)
            .min(SPEECH_THRESHOLD_CEILING)
    }

    /// Once speech has started, require either a clearly stronger signal or
    /// speech-like variation. A few grace ticks preserve quiet word endings,
    /// but steady room noise can no longer extend a turn forever.
    fn observe_continuation(&mut self, rms: f32) -> bool {
        self.observe_continuation_inner(rms, true)
    }

    /// Hands-free dictation already has an explicit 200 ms phrase boundary,
    /// so it does not add the conversational grace window on top of that.
    fn observe_continuation_strict(&mut self, rms: f32) -> bool {
        self.observe_continuation_inner(rms, false)
    }

    fn observe_continuation_inner(&mut self, rms: f32, allow_grace: bool) -> bool {
        let above_voice_floor = self.is_voice(rms);
        let varied = rms - self.last_rms >= CONTINUATION_VARIATION_RMS;
        let clear_threshold = (self.voice_threshold() * 1.35).min(0.08);
        let crossed_clear_threshold = rms >= clear_threshold && self.last_rms < clear_threshold;
        let clear = crossed_clear_threshold || (above_voice_floor && varied);
        self.last_rms = rms;

        if clear {
            self.continuation_grace_ticks = CONTINUATION_GRACE_TICKS;
            return true;
        }
        if allow_grace && above_voice_floor && self.continuation_grace_ticks > 0 {
            self.continuation_grace_ticks -= 1;
            return true;
        }
        self.continuation_grace_ticks = 0;
        false
    }

    fn reset_turn(&mut self) {
        self.onset_ticks = 0;
        self.continuation_grace_ticks = 0;
        self.last_rms = 0.0;
    }
}

pub(super) fn recognize_one_turn(
    models: VoiceModelManager,
    patient_turn_taking: bool,
    confirmation_only: bool,
    push_to_talk: bool,
    continuous_dictation: bool,
    input_device: Option<String>,
    silence_timeout: Duration,
    cancelled: Arc<AtomicBool>,
    finish_requested: Arc<AtomicBool>,
    events: mpsc::Sender<RecognitionEvent>,
) {
    if let Err(error) = recognize_one_turn_inner(
        &models,
        patient_turn_taking,
        confirmation_only,
        push_to_talk,
        continuous_dictation,
        input_device.as_deref(),
        silence_timeout,
        cancelled.clone(),
        finish_requested,
        &events,
    ) {
        if cancelled.load(Ordering::Relaxed) {
            let _ = events.send(RecognitionEvent::Cancelled);
        } else {
            let _ = events.send(RecognitionEvent::Error(format!("{error:#}")));
        }
    }
}

fn recognize_one_turn_inner(
    models: &VoiceModelManager,
    patient_turn_taking: bool,
    confirmation_only: bool,
    push_to_talk: bool,
    continuous_dictation: bool,
    input_device: Option<&str>,
    silence_timeout: Duration,
    cancelled: Arc<AtomicBool>,
    finish_requested: Arc<AtomicBool>,
    events: &mpsc::Sender<RecognitionEvent>,
) -> Result<()> {
    anyhow::ensure!(models.is_installed(), "voice models are not installed");
    ensure_microphone_permission(&cancelled, events)?;
    let mut turn_predictor = patient_turn_taking
        .then(|| smart_turn_rs::SmartTurnPredictor::new(&models.smart_turn_path()))
        .transpose()
        .context("could not load Smart Turn v3.2")?;

    let host = cpal::default_host();
    let device = if let Some(input_device) = input_device {
        host.input_devices()
            .context("could not list microphone input devices")?
            .find(|device| device.name().is_ok_and(|name| name == input_device))
            .with_context(|| {
                format!(
                    "microphone “{input_device}” is no longer available; choose another microphone"
                )
            })?
    } else {
        host.default_input_device()
            .context("no microphone input device is available")?
    };
    let supported = device
        .default_input_config()
        .context("could not read the default microphone format")?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let stream_config: StreamConfig = supported.clone().into();
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let stream_failure = Arc::new(Mutex::new(None::<String>));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream(
            &device,
            &stream_config,
            channels,
            samples.clone(),
            |sample: f32| sample,
            stream_failure.clone(),
        )?,
        SampleFormat::I16 => build_stream(
            &device,
            &stream_config,
            channels,
            samples.clone(),
            |sample: i16| sample as f32 / i16::MAX as f32,
            stream_failure.clone(),
        )?,
        SampleFormat::U16 => build_stream(
            &device,
            &stream_config,
            channels,
            samples.clone(),
            |sample: u16| (sample as f32 - 32_768.0) / 32_768.0,
            stream_failure.clone(),
        )?,
        format => anyhow::bail!("unsupported microphone sample format {format:?}"),
    };
    stream.play().context("could not start the microphone")?;

    // Open the microphone before loading Moonshine. On a cold first use this
    // lets push-to-talk capture the opening words while the model warms up.
    let transcriber = cached_transcriber(models)?;

    let started = Instant::now();
    let mut heard_speech = false;
    let mut last_speech = Instant::now();
    let mut speech_started_at = None;
    let mut speech_start_sample = 0;
    let mut speech_gate = SpeechGate::default();
    let mut last_smart_turn_check = Instant::now();
    let min_turn_duration = if continuous_dictation {
        CONTINUOUS_MIN_TURN_DURATION
    } else if confirmation_only {
        CONFIRMATION_MIN_TURN_DURATION
    } else {
        MIN_TURN_DURATION
    };
    let turn_silence = if continuous_dictation {
        CONTINUOUS_TURN_SILENCE
    } else if confirmation_only {
        CONFIRMATION_TURN_SILENCE
    } else {
        SMART_TURN_SILENCE
    };
    let max_utterance = if confirmation_only {
        CONFIRMATION_MAX_UTTERANCE
    } else {
        MAX_UTTERANCE
    };
    loop {
        std::thread::sleep(if push_to_talk {
            PUSH_TO_TALK_POLL_INTERVAL
        } else if continuous_dictation {
            CONTINUOUS_POLL_INTERVAL
        } else {
            Duration::from_millis(80)
        });
        if cancelled.load(Ordering::Relaxed) {
            drop(stream);
            let _ = events.send(RecognitionEvent::Cancelled);
            return Ok(());
        }
        if let Some(error) = stream_failure.lock().unwrap().take() {
            anyhow::bail!("microphone stream stopped: {error}");
        }
        if push_to_talk {
            let finish = finish_requested.load(Ordering::Relaxed)
                || started.elapsed() >= PUSH_TO_TALK_MAX_DURATION;
            let (rms, captured) = {
                let samples = samples.lock().unwrap();
                let recent_count = (sample_rate as usize / 10).min(samples.len());
                let recent = &samples[samples.len().saturating_sub(recent_count)..];
                let rms = sample_rms(recent);
                let captured = finish.then(|| samples.clone());
                (rms, captured)
            };
            let _ = events.send(RecognitionEvent::Level((rms / 0.12).clamp(0.0, 1.0)));
            if let Some(captured) = captured {
                drop(stream);
                if captured.len()
                    < (sample_rate as usize * MIN_PUSH_TO_TALK_AUDIO.as_millis() as usize) / 1_000
                {
                    let _ = events.send(RecognitionEvent::UnrecognizedSpeech);
                    return Ok(());
                }
                let _ = events.send(RecognitionEvent::Transcribing);
                return transcribe_and_emit(&transcriber, &captured, sample_rate, events);
            }
            continue;
        }
        let snapshot = samples.lock().unwrap().clone();
        let recent_count = (sample_rate as usize / 10).min(snapshot.len());
        let recent = &snapshot[snapshot.len().saturating_sub(recent_count)..];
        let rms = sample_rms(recent);
        let _ = events.send(RecognitionEvent::Level((rms / 0.12).clamp(0.0, 1.0)));
        if !heard_speech && speech_gate.observe(rms) {
            let now = Instant::now();
            heard_speech = true;
            speech_started_at = Some(now);
            speech_start_sample = snapshot
                .len()
                .saturating_sub((sample_rate as usize * 350) / 1_000);
            last_speech = now;
        } else if heard_speech
            && if continuous_dictation {
                speech_gate.observe_continuation_strict(rms)
            } else {
                speech_gate.observe_continuation(rms)
            }
        {
            last_speech = Instant::now();
        }
        if !heard_speech {
            if continuous_dictation {
                retain_recent_samples(&samples, sample_rate, CONTINUOUS_IDLE_PRE_ROLL);
            } else if started.elapsed() >= silence_timeout {
                drop(stream);
                let _ = events.send(RecognitionEvent::SilenceTimeout);
                return Ok(());
            }
            continue;
        }
        let utterance_elapsed = speech_started_at.map_or(Duration::ZERO, |start| start.elapsed());
        if utterance_elapsed < min_turn_duration {
            continue;
        }
        let turn_samples = &snapshot[speech_start_sample.min(snapshot.len())..];
        let silence = last_speech.elapsed();
        let should_finish = if !patient_turn_taking {
            silence >= turn_silence || utterance_elapsed >= max_utterance
        } else if silence >= FORCED_TURN_SILENCE || utterance_elapsed >= max_utterance {
            true
        } else if silence >= turn_silence
            && last_smart_turn_check.elapsed() >= Duration::from_millis(350)
        {
            last_smart_turn_check = Instant::now();
            turn_predictor
                .as_mut()
                .and_then(|predictor| {
                    smart_turn_complete(predictor, turn_samples, sample_rate).ok()
                })
                .unwrap_or(false)
        } else {
            false
        };
        if should_finish {
            if continuous_dictation {
                let processed_samples = snapshot.len();
                let turn_samples = turn_samples.to_vec();
                let _ = events.send(RecognitionEvent::Transcribing);
                transcribe_and_emit(&transcriber, &turn_samples, sample_rate, events)?;
                {
                    let mut samples = samples.lock().unwrap();
                    let processed_samples = processed_samples.min(samples.len());
                    samples.drain(..processed_samples);
                }
                heard_speech = false;
                speech_started_at = None;
                speech_start_sample = 0;
                last_speech = Instant::now();
                last_smart_turn_check = Instant::now();
                speech_gate.reset_turn();
                continue;
            }
            drop(stream);
            return transcribe_and_emit(&transcriber, turn_samples, sample_rate, events);
        }
    }
}

pub(super) fn warm_transcriber(models: &VoiceModelManager) -> Result<()> {
    cached_transcriber(models).map(|_| ())
}

fn sample_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|value| value * value).sum::<f32>() / samples.len() as f32).sqrt()
}

fn retain_recent_samples(samples: &Arc<Mutex<Vec<f32>>>, sample_rate: u32, duration: Duration) {
    let keep = (u64::from(sample_rate) * duration.as_millis() as u64 / 1_000) as usize;
    let mut samples = samples.lock().unwrap();
    let discard = samples.len().saturating_sub(keep);
    if discard > 0 {
        samples.drain(..discard);
    }
}

fn transcribe_and_emit(
    transcriber: &Transcriber,
    samples: &[f32],
    sample_rate: u32,
    events: &mpsc::Sender<RecognitionEvent>,
) -> Result<()> {
    let pcm = resample_mono(samples, sample_rate, TARGET_SAMPLE_RATE);
    let transcript = transcriber
        .transcribe(&pcm, TARGET_SAMPLE_RATE)
        .context("Moonshine could not transcribe the turn")?
        .text()
        .trim()
        .to_string();
    if transcript.is_empty() {
        let _ = events.send(RecognitionEvent::UnrecognizedSpeech);
    } else {
        let _ = events.send(RecognitionEvent::Transcript(transcript));
    }
    Ok(())
}

fn cached_transcriber(models: &VoiceModelManager) -> Result<Arc<Transcriber>> {
    let model_path = models.moonshine_dir();
    let mut cache = TRANSCRIBER.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some((cached_path, transcriber)) = cache.as_ref() {
        if *cached_path == model_path {
            return Ok(transcriber.clone());
        }
    }
    let transcriber = Arc::new(
        Transcriber::from_files(&model_path, ModelArch::SmallStreaming, None)
            .context("could not load Moonshine Small Streaming")?,
    );
    *cache = Some((model_path, transcriber.clone()));
    Ok(transcriber)
}

#[cfg(target_os = "macos")]
fn ensure_microphone_permission(
    cancelled: &AtomicBool,
    events: &mpsc::Sender<RecognitionEvent>,
) -> Result<()> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    let media_type =
        unsafe { AVMediaTypeAudio }.context("macOS did not provide the audio media type")?;
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    match status {
        AVAuthorizationStatus::Authorized => return Ok(()),
        AVAuthorizationStatus::Denied => anyhow::bail!(
            "Microphone access is off. Enable this Choro app in System Settings → Privacy & Security → Microphone, then try again."
        ),
        AVAuthorizationStatus::Restricted => anyhow::bail!(
            "Microphone access is restricted by macOS. Check System Settings → Privacy & Security → Microphone."
        ),
        AVAuthorizationStatus::NotDetermined => {}
        _ => anyhow::bail!("macOS returned an unknown microphone permission status"),
    }

    let _ = events.send(RecognitionEvent::RequestingPermission);
    let (permission_tx, permission_rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = permission_tx.send(granted.as_bool());
    });
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler);
    }

    loop {
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("microphone permission request was cancelled");
        }
        match permission_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(true) => {
                let _ = events.send(RecognitionEvent::PermissionGranted);
                return Ok(());
            }
            Ok(false) => anyhow::bail!(
                "Microphone access was not allowed. Enable this Choro app in System Settings → Privacy & Security → Microphone, then try again."
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("macOS closed the microphone permission request unexpectedly")
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_microphone_permission(
    _cancelled: &AtomicBool,
    _events: &mpsc::Sender<RecognitionEvent>,
) -> Result<()> {
    Ok(())
}

pub(crate) fn input_devices() -> Result<Vec<VoiceInputDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let mut devices = host
        .input_devices()
        .context("could not list microphone input devices")?
        .filter_map(|device| device.name().ok())
        .map(|name| VoiceInputDevice {
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    devices.dedup_by(|left, right| left.name == right.name);
    Ok(devices)
}

fn build_stream<T: cpal::SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    samples: Arc<Mutex<Vec<f32>>>,
    convert: impl Fn(T) -> f32 + Send + 'static,
    stream_failure: Arc<Mutex<Option<String>>>,
) -> Result<Stream> {
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut output = samples.lock().unwrap();
                output.extend(
                    data.chunks(channels)
                        .filter_map(|frame| frame.first().copied())
                        .map(&convert),
                );
            },
            move |error| {
                *stream_failure.lock().unwrap() = Some(error.to_string());
            },
            None,
        )
        .context("could not open the microphone")
}

fn smart_turn_complete(
    predictor: &mut smart_turn_rs::SmartTurnPredictor,
    samples: &[f32],
    source_rate: u32,
) -> Result<bool> {
    let mut pcm = resample_mono(samples, source_rate, TARGET_SAMPLE_RATE);
    const WINDOW: usize = 8 * TARGET_SAMPLE_RATE as usize;
    if pcm.len() > WINDOW {
        pcm = pcm.split_off(pcm.len() - WINDOW);
    } else if pcm.len() < WINDOW {
        let mut padded = vec![0.0; WINDOW - pcm.len()];
        padded.extend(pcm);
        pcm = padded;
    }
    let features = smart_turn_rs::features::log_mel_spectrogram(&pcm)?;
    Ok(predictor.predict(features)?.probability >= 0.70)
}

fn resample_mono(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == target_rate {
        return input.to_vec();
    }
    let output_len =
        ((input.len() as u64 * u64::from(target_rate)) / u64::from(source_rate)) as usize;
    let ratio = source_rate as f64 / target_rate as f64;
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let left = position.floor() as usize;
            let right = (left + 1).min(input.len() - 1);
            let fraction = (position - left as f64) as f32;
            input[left] * (1.0 - fraction) + input[right] * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_preserves_duration() {
        let input = vec![0.0; 48_000];
        assert_eq!(resample_mono(&input, 48_000, 16_000).len(), 16_000);
    }

    #[test]
    fn speech_gate_ignores_steady_background_noise() {
        let mut gate = SpeechGate::default();
        for _ in 0..20 {
            assert!(!gate.observe(0.02));
        }
    }

    #[test]
    fn speech_gate_requires_sustained_voice_above_the_noise_floor() {
        let mut gate = SpeechGate::default();
        for _ in 0..NOISE_CALIBRATION_TICKS {
            assert!(!gate.observe(0.003));
        }
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.003));
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.04));
        assert!(gate.observe(0.04));
    }

    #[test]
    fn speech_gate_stops_extending_for_low_steady_noise_after_speech() {
        let mut gate = SpeechGate::default();
        for _ in 0..NOISE_CALIBRATION_TICKS {
            assert!(!gate.observe(0.003));
        }
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.04));
        assert!(gate.observe(0.04));

        for _ in 0..CONTINUATION_GRACE_TICKS {
            assert!(gate.observe_continuation(0.013));
        }
        assert!(!gate.observe_continuation(0.013));
    }

    #[test]
    fn speech_gate_stops_extending_for_loud_steady_noise_after_speech() {
        let mut gate = SpeechGate::default();
        for _ in 0..NOISE_CALIBRATION_TICKS {
            assert!(!gate.observe(0.003));
        }
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.04));
        assert!(gate.observe(0.04));

        for _ in 0..CONTINUATION_GRACE_TICKS {
            assert!(gate.observe_continuation(0.04));
        }
        assert!(!gate.observe_continuation(0.04));
    }

    #[test]
    fn strict_continuation_starts_the_hands_free_pause_immediately() {
        let mut gate = SpeechGate::default();
        for _ in 0..NOISE_CALIBRATION_TICKS {
            assert!(!gate.observe(0.003));
        }
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.04));
        assert!(gate.observe(0.04));

        assert!(!gate.observe_continuation_strict(0.013));
    }

    #[test]
    fn hands_free_idle_audio_keeps_only_the_pre_roll_window() {
        let samples = Arc::new(Mutex::new(vec![0.0; 48_000 * 10]));
        retain_recent_samples(&samples, 48_000, CONTINUOUS_IDLE_PRE_ROLL);
        assert_eq!(samples.lock().unwrap().len(), 24_000);
    }

    #[test]
    fn speech_gate_keeps_variable_voice_active() {
        let mut gate = SpeechGate::default();
        for _ in 0..NOISE_CALIBRATION_TICKS {
            assert!(!gate.observe(0.003));
        }
        assert!(!gate.observe(0.04));
        assert!(!gate.observe(0.04));
        assert!(gate.observe(0.04));

        for rms in [0.018, 0.026, 0.019, 0.029, 0.017, 0.025] {
            assert!(gate.observe_continuation(rms));
        }
    }
}
