use crate::audio_processing::{AudioProcessingSettings, AudioProcessor, MicrophoneLevel};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Safety net for forgotten recordings (the design target is ~1 h sessions).
/// Checked in the consumer thread regardless of VAD or live mode.
pub(crate) const RECORDING_MAX_SECS: usize = 5400;
/// Warn the user this long before the cap (emits `recording-time-warning`).
pub(crate) const RECORDING_WARNING_SECS: usize = 5100;

pub struct StreamWrapper(pub cpal::Stream);
unsafe impl Send for StreamWrapper {}
unsafe impl Sync for StreamWrapper {}

pub struct AudioState {
    pub is_recording: bool,
    pub is_testing: bool,
    pub microphone_level: Option<MicrophoneLevel>,
    pub processing: AudioProcessingSettings,
    consumer_thread: Option<std::thread::JoinHandle<()>>,
    capture_id: u64,
    pub is_saving: bool,
    pub is_transcribing: bool,
    pub buffer: Vec<f32>,
    pub stream: Option<StreamWrapper>,
    pub selected_device: Option<String>,
    pub recording_start: Option<chrono::DateTime<chrono::Local>>,
    pub vad_enabled: bool,
    pub vad_threshold: f32,
    pub vad_silence_duration_ms: u32,
    pub last_samples: Arc<Vec<f32>>,
    /// Identifiers of media sessions paused on recording start (cross-platform).
    /// Used to selectively resume only what *we* paused.
    pub paused_media_apps: Vec<String>,
    pub cached_devices: Vec<String>,
    /// When `Some`, the consumer thread fans out drained chunks to a live
    /// streaming session. Installed/cleared by the StreamingController wiring.
    pub stream_tx: Option<Sender<Vec<f32>>>,
    /// When true, VAD does NOT auto-stop the recording (the live segmenter owns
    /// utterance boundaries; the session ends on manual stop).
    pub live_mode_active: bool,
}

pub struct AudioController {
    pub state: Arc<Mutex<AudioState>>,
}

fn stop_microphone_test(state: &mut AudioState) {
    if state.is_testing {
        state.capture_id = state.capture_id.wrapping_add(1);
        state.is_testing = false;
        state.microphone_level = None;
        drop(state.stream.take());
        drop(state.consumer_thread.take());
    }
}

pub(crate) fn save_wav_file(
    app_handle: &tauri::AppHandle,
    samples: &[f32],
    start_time: chrono::DateTime<chrono::Local>,
) -> Result<Option<String>, String> {
    // Ok(None) means "nothing to save" (no samples). A real write failure is an
    // Err, never silently reported as success-without-a-path.
    if samples.is_empty() {
        return Ok(None);
    }

    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;

    let dir_name = start_time.format("%Y-%m-%d_%H-%M-%S").to_string();
    let recordings_dir = app_local_data.join("recordings").join(dir_name);
    std::fs::create_dir_all(&recordings_dir).map_err(|e| e.to_string())?;

    let wav_path = recordings_dir.join("output.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav_path, spec).map_err(|e| e.to_string())?;
    for &sample in samples {
        let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(Some(wav_path.to_string_lossy().to_string()))
}

/// Drains DSP state on the consumer before handing automatic-stop persistence to
/// a worker. The recording guard preserves audio when a manual stop wins the race.
fn auto_stop_recording(
    mut s: std::sync::MutexGuard<'_, AudioState>,
    state: &Arc<Mutex<AudioState>>,
    app_handle: &tauri::AppHandle,
    processor: &mut AudioProcessor,
) {
    if !s.is_recording {
        return;
    }
    s.is_recording = false;
    s.is_saving = true;
    if let Some(wrapper) = s.stream.take() {
        let _ = wrapper.0.pause();
    }

    match processor.process(&[], s.processing, true) {
        Ok((tail, _)) => {
            s.buffer.extend_from_slice(&tail);
            if !tail.is_empty() {
                if let Some(tx) = &s.stream_tx {
                    if tx.try_send(tail).is_err() {
                        note_live_drop();
                    }
                }
            }
        }
        Err(error) => {
            tracing::error!("Could not drain audio processing: {}", error);
            let _ = app_handle.emit("recording-error", "audio_processing");
        }
    }

    let paused_apps: Vec<String> = s.paused_media_apps.drain(..).collect();

    let samples = Arc::new(std::mem::take(&mut s.buffer));
    s.last_samples = Arc::clone(&samples);
    let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);

    // Claim the live session's sender under the same lock that arms/disarms it.
    // The save thread can take seconds; if the user starts a new recording in
    // that window, the new session must not be torn down by this stale stopper.
    let live_tx = s.stream_tx.take();
    s.live_mode_active = false;

    if !paused_apps.is_empty() {
        crate::media_control::resume_system_media(&paused_apps);
    }

    drop(s);

    // Refresh overlay visibility only AFTER releasing the audio-state lock:
    // update_recording_window_visibility re-locks it (is_recording / is_saving /
    // is_transcribing), so calling it while `s` was held would deadlock the
    // audio thread. is_saving is still true here, so the overlay stays up
    // through transcription (keeps App Nap away on macOS).
    #[cfg(target_os = "macos")]
    crate::update_recording_window_visibility(app_handle);

    let state_save_clone = Arc::clone(state);
    let app_handle_save_clone = app_handle.clone();
    std::thread::spawn(move || {
        // WAV persistence must not delay stop feedback.
        crate::play_backend_sound(&app_handle_save_clone, "stop");
        let _ = crate::rebuild_tray_menu(&app_handle_save_clone);

        let saved_path = match save_wav_file(&app_handle_save_clone, &samples, start_time) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("save_wav_file failed: {}", e);
                let _ = app_handle_save_clone.emit("recording-save-failed", e);
                None
            }
        };

        {
            let mut s = state_save_clone.lock().unwrap();
            s.is_saving = false;
            s.is_transcribing = true;
        }

        let payload = saved_path.unwrap_or_else(|| "Recording stopped".to_string());
        let _ = app_handle_save_clone.emit("recording-stopped", payload);

        // Finish only the live session this recording owned (no-op otherwise);
        // a newer session started during the WAV save must survive.
        if let Some(tx) = live_tx {
            crate::finish_live_session_for(&app_handle_save_clone, &tx);
        }
    });
}

impl AudioController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AudioState {
                is_recording: false,
                is_testing: false,
                microphone_level: None,
                processing: AudioProcessingSettings::default(),
                consumer_thread: None,
                capture_id: 0,
                is_saving: false,
                is_transcribing: false,
                buffer: Vec::new(),
                stream: None,
                selected_device: None,
                recording_start: None,
                vad_enabled: false,
                vad_threshold: 0.008,
                vad_silence_duration_ms: 1500,
                last_samples: Arc::new(Vec::new()),
                paused_media_apps: Vec::new(),
                cached_devices: Vec::new(),
                stream_tx: None,
                live_mode_active: false,
            })),
        }
    }

    pub fn refresh_devices(&self) -> Result<(), String> {
        let host = cpal::default_host();
        let devices = host.input_devices().map_err(|e| e.to_string())?;
        let mut names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        let mut s = self.state.lock().unwrap();
        s.cached_devices = names;
        Ok(())
    }

    pub fn list_devices(&self) -> Result<Vec<String>, String> {
        let s = self.state.lock().unwrap();
        if s.cached_devices.is_empty() {
            drop(s);
            let _ = self.refresh_devices();
            return Ok(self.state.lock().unwrap().cached_devices.clone());
        }
        Ok(s.cached_devices.clone())
    }

    pub fn set_selected_device(&self, device_name: Option<String>) {
        let mut s = self.state.lock().unwrap();
        stop_microphone_test(&mut s);
        s.selected_device = device_name;
    }

    /// Atomically arm a live session: set `live_mode_active` and install the
    /// fan-out sender under a single lock so the consumer never observes one
    /// without the other.
    pub fn set_live_session(&self, tx: Sender<Vec<f32>>) {
        let mut s = self.state.lock().unwrap();
        s.live_mode_active = true;
        s.stream_tx = Some(tx);
    }

    /// Atomically disarm a live session (single lock), mirroring set_live_session.
    pub fn clear_live_session(&self) {
        let mut s = self.state.lock().unwrap();
        s.live_mode_active = false;
        s.stream_tx = None;
    }

    pub fn start_recording(
        &self,
        app_handle: tauri::AppHandle,
        pause_audio: bool,
    ) -> Result<(), String> {
        self.start_capture(app_handle, pause_audio, false)
    }

    pub fn start_microphone_test(&self, app_handle: tauri::AppHandle) -> Result<(), String> {
        self.start_capture(app_handle, false, true)
    }

    pub fn stop_microphone_test(&self) {
        stop_microphone_test(&mut self.state.lock().unwrap());
    }

    fn start_capture(
        &self,
        app_handle: tauri::AppHandle,
        pause_audio: bool,
        testing: bool,
    ) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if s.is_recording || s.is_saving || (testing && s.is_transcribing) {
            return Err("errors.microphone_busy".to_string());
        }
        stop_microphone_test(&mut s);
        let config: serde_json::Value = crate::load_config(app_handle.clone())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        s.processing = AudioProcessingSettings::from_config(&config)?;

        let device_name = s.selected_device.clone();

        // Resolve the input device BEFORE pausing media, so an unavailable device
        // fails cleanly without leaving the user's media paused.
        //
        // When the user explicitly picked a device, use exactly that one and never
        // silently fall back to the system default. On macOS the default input is
        // often a Bluetooth headset (e.g. AirPods); opening its microphone forces
        // the A2DP -> HFP profile switch that audibly degrades playback. Honoring
        // the explicit choice keeps the headset in high-quality output mode.
        let host = cpal::default_host();
        let device = match &device_name {
            Some(name) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().map(|n| &n == name).unwrap_or(false))
                .ok_or_else(|| {
                    tracing::warn!("Selected microphone '{}' is not available", name);
                    "errors.mic_unavailable".to_string()
                })?,
            None => host
                .default_input_device()
                .ok_or_else(|| "No default input device found".to_string())?,
        };

        let config = choose_input_config(&device)?;
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();

        let channels = stream_config.channels;
        let src_rate = stream_config.sample_rate.0;
        let mut processor = AudioProcessor::new(src_rate)?;

        // Bound native-rate input to two seconds; DSP runs only on the consumer.
        let rb = SharedRb::<Heap<f32>>::new((src_rate * 2) as usize);
        let (mut producer, mut consumer) = rb.split();

        let err_app = app_handle.clone();
        let err_fn = move |err| {
            tracing::error!("an error occurred on stream: {}", err);
            if !testing {
                let _ = err_app.emit("recording-error", "device_lost");
            }
        };

        // Build the capture stream for whatever sample format the device reports.
        // cpal's `to_sample::<f32>` conversion is generic over every integer/float
        // sample type, so one macro covers them all. Some Linux/PipeWire devices
        // default to I32 (or other formats) that a F32/I16/U16-only match rejected
        // with "Unsupported sample format".
        macro_rules! capture_stream {
            ($t:ty) => {
                device.build_input_stream(
                    &stream_config,
                    move |data: &[$t], _: &cpal::InputCallbackInfo| {
                        let f32_data: Vec<f32> = data
                            .iter()
                            .map(|&s| cpal::Sample::to_sample::<f32>(s))
                            .collect();
                        let mono = downmix(&f32_data, channels);
                        let pushed = producer.push_slice(&mono);
                        if pushed < mono.len() {
                            note_ring_overflow(mono.len() - pushed);
                        }
                    },
                    err_fn,
                    None,
                )
            };
        }

        let stream = match sample_format {
            cpal::SampleFormat::I8 => capture_stream!(i8),
            cpal::SampleFormat::I16 => capture_stream!(i16),
            cpal::SampleFormat::I32 => capture_stream!(i32),
            cpal::SampleFormat::I64 => capture_stream!(i64),
            cpal::SampleFormat::U8 => capture_stream!(u8),
            cpal::SampleFormat::U16 => capture_stream!(u16),
            cpal::SampleFormat::U32 => capture_stream!(u32),
            cpal::SampleFormat::U64 => capture_stream!(u64),
            cpal::SampleFormat::F32 => capture_stream!(f32),
            cpal::SampleFormat::F64 => capture_stream!(f64),
            other => return Err(format!("Unsupported sample format: {other:?}")),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        s.stream = Some(StreamWrapper(stream));

        s.capture_id = s.capture_id.wrapping_add(1);
        let capture_id = s.capture_id;
        s.is_testing = testing;
        s.microphone_level = testing.then(MicrophoneLevel::default);
        if !testing {
            s.buffer.clear();
            s.is_recording = true;
            s.recording_start = Some(chrono::Local::now());
            if pause_audio {
                s.paused_media_apps = crate::media_control::pause_system_media();
            } else {
                s.paused_media_apps = Vec::new();
            }
        }

        let state_clone = Arc::clone(&self.state);
        let app_handle_clone = app_handle.clone();
        s.consumer_thread = Some(std::thread::spawn(move || {
            let mut local_buf = vec![0.0; (src_rate / 50) as usize];
            let mut has_spoken = false;
            let mut silence_samples = 0;
            let mut warned_about_cap = false;
            let mut last_audio = std::time::Instant::now();
            let test_start = last_audio;

            loop {
                let (is_recording, vad_enabled, vad_threshold, vad_silence_duration_ms, processing) = {
                    let mut s = state_clone.lock().unwrap();
                    // A stopped test must never append samples to a subsequent recording.
                    if s.capture_id != capture_id {
                        break;
                    }
                    if testing
                        && (test_start.elapsed().as_secs() >= 30
                            || last_audio.elapsed().as_secs() >= 5)
                    {
                        stop_microphone_test(&mut s);
                        break;
                    }
                    (
                        s.is_recording,
                        s.vad_enabled,
                        s.vad_threshold,
                        s.vad_silence_duration_ms,
                        s.processing,
                    )
                };

                let read = consumer.pop_slice(&mut local_buf);
                let finishing = !is_recording && !testing && consumer.is_empty();
                if read > 0 || finishing {
                    if read > 0 {
                        last_audio = std::time::Instant::now();
                    }
                    let (processed, mut level) =
                        match processor.process(&local_buf[..read], processing, finishing) {
                            Ok(result) => result,
                            Err(error) => {
                                tracing::error!("Audio processing failed: {}", error);
                                let mut s = state_clone.lock().unwrap();
                                if s.capture_id != capture_id {
                                    break;
                                }
                                if testing {
                                    stop_microphone_test(&mut s);
                                } else if s.is_recording {
                                    auto_stop_recording(
                                        s,
                                        &state_clone,
                                        &app_handle_clone,
                                        &mut processor,
                                    );
                                }
                                let _ =
                                    app_handle_clone.emit("recording-error", "audio_processing");
                                break;
                            }
                        };
                    if testing {
                        let mut s = state_clone.lock().unwrap();
                        if s.capture_id != capture_id {
                            break;
                        }
                        if let Some(previous) = s.microphone_level {
                            level.input_peak = level.input_peak.max(previous.input_peak);
                            level.peak = level.peak.max(previous.peak);
                            level.clipped |= previous.clipped;
                            level.limited |= previous.limited;
                        }
                        s.microphone_level = Some(level);
                        drop(s);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    let rms = level.rms;
                    let _ = app_handle_clone.emit("audio-amplitude", rms);

                    let mut should_warn = false;
                    {
                        let mut s = state_clone.lock().unwrap();
                        if s.capture_id != capture_id {
                            break;
                        }
                        s.buffer.extend_from_slice(&processed);
                        let buffer_len = s.buffer.len();

                        // Read live state under the same lock as the fan-out so the two stay
                        // consistent with set_live_session / clear_live_session.
                        let live_active = s.live_mode_active;

                        // Live fan-out: hand the chunk to the streaming session. Non-blocking;
                        // the bounded channel returns Full rather than stalling the audio path.
                        if let Some(tx) = &s.stream_tx {
                            if tx.try_send(processed.clone()).is_err() {
                                note_live_drop();
                            }
                        }

                        if finishing {
                            break;
                        }
                        if !s.is_recording {
                            continue;
                        }

                        if buffer_len >= RECORDING_MAX_SECS * 16_000 {
                            auto_stop_recording(s, &state_clone, &app_handle_clone, &mut processor);
                            break;
                        }

                        if vad_enabled && !live_active {
                            if rms >= vad_threshold {
                                has_spoken = true;
                                silence_samples = 0;
                            } else if has_spoken {
                                silence_samples += processed.len();
                                let timeout_samples =
                                    (vad_silence_duration_ms as f32 / 1000.0 * 16000.0) as usize;
                                if silence_samples >= timeout_samples {
                                    auto_stop_recording(
                                        s,
                                        &state_clone,
                                        &app_handle_clone,
                                        &mut processor,
                                    );
                                    break;
                                }
                            }
                        }

                        if !warned_about_cap && buffer_len >= RECORDING_WARNING_SECS * 16_000 {
                            warned_about_cap = true;
                            should_warn = true;
                        }
                    }
                    if should_warn {
                        let _ = app_handle_clone.emit(
                            "recording-time-warning",
                            serde_json::json!({
                                "seconds_left": (RECORDING_MAX_SECS - RECORDING_WARNING_SECS) as u32
                            }),
                        );
                    }
                }

                // Device-disconnect watchdog: if no audio arrived for 5 s while
                // recording (mic unplugged / asleep / Bluetooth dropped), the data
                // callback has gone silent. Stop instead of "recording" dead air.
                if is_recording && last_audio.elapsed() > std::time::Duration::from_secs(5) {
                    let _ = app_handle_clone.emit("recording-error", "device_lost");
                    let s = state_clone.lock().unwrap();
                    auto_stop_recording(s, &state_clone, &app_handle_clone, &mut processor);
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }));

        Ok(())
    }

    pub fn stop_recording(&self, app_handle: &tauri::AppHandle) -> Result<Option<String>, String> {
        let (samples, start_time) = {
            let mut s = self.state.lock().unwrap();
            if !s.is_recording {
                return Ok(None);
            }

            s.is_recording = false;
            s.is_saving = true;

            let paused_apps_stop: Vec<String> = s.paused_media_apps.drain(..).collect();
            if !paused_apps_stop.is_empty() {
                crate::media_control::resume_system_media(&paused_apps_stop);
            }

            drop(s.stream.take());
            let worker = s.consumer_thread.take();
            // Finish the bounded queue and DSP delay before taking the saved/live audio.
            drop(s);
            if let Some(worker) = worker {
                if worker.join().is_err() {
                    tracing::error!("Audio consumer panicked while stopping");
                }
            }

            let mut s = self.state.lock().unwrap();
            let samples = Arc::new(std::mem::take(&mut s.buffer));
            s.last_samples = Arc::clone(&samples);
            let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
            (samples, start_time)
        };

        let _ = crate::rebuild_tray_menu(app_handle);

        // A write failure must not abort transcription: surface it and continue with
        // no path (the samples are still transcribed from memory). Only a genuine
        // 0-sample recording yields Ok(None) now.
        let save_result = match save_wav_file(app_handle, &samples, start_time) {
            Ok(p) => Ok(p),
            Err(e) => {
                tracing::error!("save_wav_file failed: {}", e);
                let _ = app_handle.emit("recording-save-failed", e);
                Ok(None)
            }
        };

        {
            let mut s = self.state.lock().unwrap();
            s.is_saving = false;
            s.is_transcribing = true;
        }

        let _ = crate::rebuild_tray_menu(app_handle);

        save_result
    }

    pub fn is_recording(&self) -> bool {
        self.state.lock().unwrap().is_recording
    }

    pub fn is_saving(&self) -> bool {
        self.state.lock().unwrap().is_saving
    }

    pub fn is_transcribing(&self) -> bool {
        self.state.lock().unwrap().is_transcribing
    }

    pub fn set_transcribing(&self, value: bool) {
        self.state.lock().unwrap().is_transcribing = value;
    }
}

/// Prefer the output rate and the smallest available channel count.
fn choose_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    const TARGET: u32 = 16_000;
    if let Ok(ranges) = device.supported_input_configs() {
        let mut best: Option<cpal::SupportedStreamConfigRange> = None;
        for r in ranges {
            if r.min_sample_rate().0 <= TARGET && TARGET <= r.max_sample_rate().0 {
                let better = best.as_ref().map_or(true, |b| r.channels() < b.channels());
                if better {
                    best = Some(r);
                }
            }
        }
        if let Some(r) = best {
            return Ok(r.with_sample_rate(cpal::SampleRate(TARGET)));
        }
    }
    device.default_input_config().map_err(|e| e.to_string())
}

static RING_DROPPED: AtomicUsize = AtomicUsize::new(0);
static RING_WARNED: AtomicBool = AtomicBool::new(false);

/// Records samples dropped because the consumer fell behind and the ring filled
/// (previously a silent `let _ = push_slice`). Warns once per process so a
/// persistent fault is visible without log spam.
fn note_ring_overflow(dropped: usize) {
    RING_DROPPED.fetch_add(dropped, Ordering::Relaxed);
    if !RING_WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!("audio ring buffer overflow: consumer fell behind, dropping samples");
    }
}

static LIVE_DROPPED: AtomicUsize = AtomicUsize::new(0);
static LIVE_WARNED: AtomicBool = AtomicBool::new(false);

/// Records a live-fan-out chunk dropped because the streaming worker fell behind
/// (the bounded channel returned Full). Warns once per process. With G3 coalescing
/// this should be rare; surfacing it makes a real overload visible.
fn note_live_drop() {
    LIVE_DROPPED.fetch_add(1, Ordering::Relaxed);
    if !LIVE_WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!("live transcription overload: dropping audio chunks (decode too slow)");
    }
}

fn downmix(data: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    let ch = channels as usize;
    let mut mono = Vec::with_capacity(data.len() / ch + 1);
    let mut chunks = data.chunks_exact(ch);
    for chunk in &mut chunks {
        mono.push(chunk.iter().sum::<f32>() / channels as f32);
    }
    // chunks_exact drops the trailing partial frame; average what is present so the
    // last samples of every callback are not silently lost.
    let rem = chunks.remainder();
    if !rem.is_empty() {
        mono.push(rem.iter().sum::<f32>() / rem.len() as f32);
    }
    mono
}

#[cfg(test)]
mod downmix_tests {
    use super::downmix;

    #[test]
    fn mono_passthrough() {
        assert_eq!(downmix(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn stereo_averages_pairs() {
        assert_eq!(downmix(&[0.0, 1.0, 2.0, 3.0], 2), vec![0.5, 2.5]);
    }

    #[test]
    fn keeps_trailing_partial_frame() {
        // 3 channels, 4 samples: one full frame (0+1+2)/3 = 1.0 plus remainder [9.0].
        let out = downmix(&[0.0, 1.0, 2.0, 9.0], 3);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[1] - 9.0).abs() < 1e-6);
    }
}
