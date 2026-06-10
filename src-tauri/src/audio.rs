use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use ringbuf::{storage::Heap, traits::*, SharedRb};
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

pub(crate) fn save_wav_file(
    app_handle: &tauri::AppHandle,
    samples: &[f32],
    start_time: chrono::DateTime<chrono::Local>,
) -> Result<Option<String>, String> {
    let pcm_len = samples.len();
    if pcm_len > 0 {
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
        if let Ok(mut writer) = hound::WavWriter::create(&wav_path, spec) {
            for &sample in samples {
                let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                let _ = writer.write_sample(sample_i16);
            }
            let _ = writer.finalize();
            let path_str = wav_path.to_string_lossy().to_string();
            return Ok(Some(path_str));
        }
    }
    Ok(None)
}

/// Completes an automatic stop (VAD silence or the max-duration cap) from the
/// consumer thread. Consumes the held state guard so the lock is released
/// before any blocking work; WAV save and notifications run on a new thread,
/// mirroring the manual stop path. The is_recording guard makes it a no-op
/// when a manual stop won the race in between two consumer iterations —
/// without it, this would overwrite last_samples (the real recording) with
/// the ≤1024-sample residue drained after the manual stop.
fn auto_stop_recording(
    mut s: std::sync::MutexGuard<'_, AudioState>,
    state: &Arc<Mutex<AudioState>>,
    app_handle: &tauri::AppHandle,
) {
    if !s.is_recording {
        return;
    }
    s.is_recording = false;
    s.is_saving = true;
    if let Some(wrapper) = s.stream.take() {
        let _ = wrapper.0.pause();
    }

    let paused_apps: Vec<String> = s.paused_media_apps.drain(..).collect();

    let samples = Arc::new(std::mem::take(&mut s.buffer));
    s.last_samples = Arc::clone(&samples);
    let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);

    // Resume media before dropping the lock
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
        let saved_path = save_wav_file(&app_handle_save_clone, &samples, start_time)
            .ok()
            .flatten();

        {
            let mut s = state_save_clone.lock().unwrap();
            s.is_saving = false;
            s.is_transcribing = true;
        }

        let _ = crate::rebuild_tray_menu(&app_handle_save_clone);
        crate::play_backend_sound(&app_handle_save_clone, "stop");

        let payload = saved_path.unwrap_or_else(|| "Recording stopped".to_string());
        let _ = app_handle_save_clone.emit("recording-stopped", payload);

        // A max-duration auto-stop can fire while a live session is active;
        // finish it so `transcription-final` is emitted. No-op when inactive
        // (the VAD path never runs in live mode).
        crate::end_live_session(&app_handle_save_clone);
    });
}

impl AudioController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AudioState {
                is_recording: false,
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
        let mut s = self.state.lock().unwrap();
        if s.is_recording {
            return Err("Already recording".to_string());
        }

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
                    eprintln!("Selected microphone '{}' is not available", name);
                    "errors.mic_unavailable".to_string()
                })?,
            None => host
                .default_input_device()
                .ok_or_else(|| "No default input device found".to_string())?,
        };

        // Cross-platform media pause (macOS / Windows / Linux)
        if pause_audio {
            s.paused_media_apps = crate::media_control::pause_system_media();
        } else {
            s.paused_media_apps = Vec::new();
        }

        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();

        let channels = stream_config.channels;
        let src_rate = stream_config.sample_rate.0;
        let dst_rate = 16000;

        // Create the ring buffer (10 seconds capacity)
        let rb = SharedRb::<Heap<f32>>::new((dst_rate * 10) as usize);
        let (mut producer, mut consumer) = rb.split();

        s.buffer.clear();
        s.is_recording = true;
        s.recording_start = Some(chrono::Local::now());

        // Spawn consumer thread to move data from ring buffer to AudioState buffer
        let state_clone = Arc::clone(&self.state);
        let app_handle_clone = app_handle.clone();
        std::thread::spawn(move || {
            let mut local_buf = vec![0.0; 1024];
            let mut has_spoken = false;
            let mut silence_samples = 0;
            let mut warned_about_cap = false;

            loop {
                // Check state at start of loop iteration
                let (is_recording, vad_enabled, vad_threshold, vad_silence_duration_ms) = {
                    let s = state_clone.lock().unwrap();
                    (
                        s.is_recording,
                        s.vad_enabled,
                        s.vad_threshold,
                        s.vad_silence_duration_ms,
                    )
                };

                if !is_recording {
                    // Drain any remaining samples directly into buffer
                    let mut s = state_clone.lock().unwrap();
                    while !consumer.is_empty() {
                        let read = consumer.pop_slice(&mut local_buf);
                        s.buffer.extend_from_slice(&local_buf[..read]);
                    }
                    break;
                }

                // Read from consumer
                let read = consumer.pop_slice(&mut local_buf);
                if read > 0 {
                    // Compute RMS of the newly read samples for visualizer
                    let mut sum_sq = 0.0;
                    for &sample in &local_buf[..read] {
                        sum_sq += sample * sample;
                    }
                    let rms = (sum_sq / read as f32).sqrt();
                    let _ = app_handle_clone.emit("audio-amplitude", rms);

                    let mut should_warn = false;
                    {
                        let mut s = state_clone.lock().unwrap();
                        s.buffer.extend_from_slice(&local_buf[..read]);
                        let buffer_len = s.buffer.len();

                        // Read live state under the same lock as the fan-out so the two stay
                        // consistent with set_live_session / clear_live_session.
                        let live_active = s.live_mode_active;

                        // Live fan-out: hand the chunk to the streaming session. Non-blocking;
                        // the bounded channel returns Full rather than stalling the audio path.
                        if let Some(tx) = &s.stream_tx {
                            let _ = tx.try_send(local_buf[..read].to_vec());
                        }

                        if buffer_len >= RECORDING_MAX_SECS * 16_000 {
                            auto_stop_recording(s, &state_clone, &app_handle_clone);
                            break;
                        }

                        if vad_enabled && !live_active {
                            if rms >= vad_threshold {
                                has_spoken = true;
                                silence_samples = 0;
                            } else if has_spoken {
                                silence_samples += read;
                                let timeout_samples =
                                    (vad_silence_duration_ms as f32 / 1000.0 * 16000.0) as usize;
                                if silence_samples >= timeout_samples {
                                    auto_stop_recording(s, &state_clone, &app_handle_clone);
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

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        // Create the resampler and downmixer
        let mut resampler = Resampler::new(src_rate, dst_rate);

        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono = downmix(data, channels);
                    let resampled = resampler.process(&mono);
                    let _ = producer.push_slice(&resampled);
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let f32_data: Vec<f32> = data
                        .iter()
                        .map(|&s| cpal::Sample::to_sample::<f32>(s))
                        .collect();
                    let mono = downmix(&f32_data, channels);
                    let resampled = resampler.process(&mono);
                    let _ = producer.push_slice(&resampled);
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let f32_data: Vec<f32> = data
                        .iter()
                        .map(|&s| cpal::Sample::to_sample::<f32>(s))
                        .collect();
                    let mono = downmix(&f32_data, channels);
                    let resampled = resampler.process(&mono);
                    let _ = producer.push_slice(&resampled);
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported sample format".to_string()),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        s.stream = Some(StreamWrapper(stream));

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

            // Cross-platform media resume
            let paused_apps_stop: Vec<String> = s.paused_media_apps.drain(..).collect();
            if !paused_apps_stop.is_empty() {
                crate::media_control::resume_system_media(&paused_apps_stop);
            }

            if let Some(wrapper) = s.stream.as_ref() {
                let _ = wrapper.0.pause();
            }

            // Consumer thread will drain remaining samples. We drop lock immediately
            // to avoid deadlock with VAD path. No artificial sleep.
            drop(s);

            let mut s = self.state.lock().unwrap();
            let samples = Arc::new(std::mem::take(&mut s.buffer));
            s.last_samples = Arc::clone(&samples);
            let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
            (samples, start_time)
        };

        let _ = crate::rebuild_tray_menu(app_handle);

        // Run the saving logic in a helper to safely catch early-return errors
        let save_result = save_wav_file(app_handle, &samples, start_time);


        // WAV saved - switch to transcribing state (blue dot stays on)
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

fn downmix(data: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return data.to_vec();
    }
    let mut mono = Vec::with_capacity(data.len() / channels as usize);
    for chunk in data.chunks_exact(channels as usize) {
        let sum: f32 = chunk.iter().sum();
        mono.push(sum / channels as f32);
    }
    mono
}

pub struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    buffer: Vec<f32>,
    pos: f64,
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate,
            dst_rate,
            buffer: Vec::new(),
            pos: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.src_rate == self.dst_rate {
            return input.to_vec();
        }

        self.buffer.extend_from_slice(input);
        let mut output = Vec::new();
        let ratio = self.src_rate as f64 / self.dst_rate as f64;

        while (self.pos + 1.0) < self.buffer.len() as f64 {
            let idx = self.pos as usize;
            let frac = self.pos - idx as f64;
            let sample =
                self.buffer[idx] * (1.0 - frac as f32) + self.buffer[idx + 1] * frac as f32;
            output.push(sample);
            self.pos += ratio;
        }

        let remove_count = (self.pos.floor() as usize).min(self.buffer.len());
        if remove_count > 0 {
            self.buffer.drain(0..remove_count);
            self.pos -= remove_count as f64;
        }

        output
    }
}
