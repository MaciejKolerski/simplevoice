use std::sync::{Arc, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{storage::Heap, traits::*, SharedRb};
use tauri::{Manager, Emitter};

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
    pub last_samples: Vec<f32>,
}

pub struct AudioController {
    pub state: Arc<Mutex<AudioState>>,
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
                last_samples: Vec::new(),
            })),
        }
    }

    pub fn list_devices(&self) -> Result<Vec<String>, String> {
        let host = cpal::default_host();
        let devices = host.input_devices().map_err(|e| e.to_string())?;
        let mut names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        Ok(names)
    }

    pub fn set_selected_device(&self, device_name: Option<String>) {
        let mut s = self.state.lock().unwrap();
        s.selected_device = device_name;
    }

    pub fn start_recording(&self, app_handle: tauri::AppHandle) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if s.is_recording {
            return Err("Already recording".to_string());
        }

        let device_name = s.selected_device.clone();
        
        // Resolve device
        let host = cpal::default_host();
        let device = if let Some(name) = &device_name {
            let mut devices = host.input_devices().map_err(|e| e.to_string())?;
            if let Some(d) = devices.find(|d| d.name().map(|n| &n == name).unwrap_or(false)) {
                d
            } else {
                println!("Selected device '{}' not found, falling back to default device.", name);
                host.default_input_device()
                    .ok_or_else(|| "No default input device found".to_string())?
            }
        } else {
            host.default_input_device()
                .ok_or_else(|| "No default input device found".to_string())?
        };

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

            loop {
                // Check state at start of loop iteration
                let (is_recording, vad_enabled, vad_threshold, vad_silence_duration_ms) = {
                    let s = state_clone.lock().unwrap();
                    (s.is_recording, s.vad_enabled, s.vad_threshold, s.vad_silence_duration_ms)
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
                    let mut s = state_clone.lock().unwrap();
                    s.buffer.extend_from_slice(&local_buf[..read]);

                    if vad_enabled {
                        // Compute RMS of the newly read samples
                        let mut sum_sq = 0.0;
                        for &sample in &local_buf[..read] {
                            sum_sq += sample * sample;
                        }
                        let rms = (sum_sq / read as f32).sqrt();

                        if rms >= vad_threshold {
                            has_spoken = true;
                            silence_samples = 0;
                        } else if has_spoken {
                            silence_samples += read;
                            let timeout_samples = (vad_silence_duration_ms as f32 / 1000.0 * 16000.0) as usize;
                            if silence_samples >= timeout_samples {
                                println!("VAD: Silence detected. Auto-stopping recording after {} ms.", vad_silence_duration_ms);
                                
                                // Transition recording state to stop
                                s.is_recording = false;
                                s.is_saving = true;
                                if let Some(wrapper) = s.stream.take() {
                                    let _ = wrapper.0.pause();
                                }
                                
                                 s.last_samples = s.buffer.clone();
                                let samples = std::mem::take(&mut s.buffer);
                                let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
                                
                                // Drop the lock before running disk I/O to write file & rebuilding tray (to avoid deadlocks)
                                drop(s);

                                // Save WAV file and notify frontend / rebuild tray on a separate thread
                                let state_save_clone = Arc::clone(&state_clone);
                                let app_handle_save_clone = app_handle_clone.clone();
                                std::thread::spawn(move || {
                                    let pcm_len = samples.len();
                                    let mut saved_path = None;
                                     if pcm_len > 0 {
                                         let app_local_data = app_handle_save_clone
                                             .path()
                                             .app_local_data_dir();
                                         
                                         if let Ok(app_local_data) = app_local_data {
                                             let dir_name = start_time.format("%Y-%m-%d_%H-%M-%S").to_string();
                                             let recordings_dir = app_local_data.join("recordings").join(dir_name);
                                             let _ = std::fs::create_dir_all(&recordings_dir);
                                             let wav_path = recordings_dir.join("output.wav");
                                             let spec = hound::WavSpec {
                                                 channels: 1,
                                                 sample_rate: 16000,
                                                 bits_per_sample: 16,
                                                 sample_format: hound::SampleFormat::Int,
                                             };
                                             if let Ok(mut writer) = hound::WavWriter::create(&wav_path, spec) {
                                                 for sample in samples {
                                                     let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                                                     let _ = writer.write_sample(sample_i16);
                                                 }
                                                 let _ = writer.finalize();
                                                 let path_str = wav_path.to_string_lossy().to_string();
                                                 println!("Saved VAD debug WAV file to {}", path_str);
                                                 saved_path = Some(path_str);
                                             }
                                         }
                                     }

                                    {
                                        let mut s = state_save_clone.lock().unwrap();
                                        s.is_saving = false;
                                        s.is_transcribing = true;
                                    }

                                    let _ = crate::rebuild_tray_menu(&app_handle_save_clone);
                                    
                                    // Emit recording-stopped event with the path
                                    let payload = saved_path.unwrap_or_else(|| "Recording stopped".to_string());
                                    let _ = app_handle_save_clone.emit("recording-stopped", payload);
                                });

                                break;
                            }
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        // Create the resampler and downmixer
        let mut resampler = Resampler::new(src_rate, dst_rate);

        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mono = downmix(data, channels);
                        let resampled = resampler.process(&mono);
                        let _ = producer.push_slice(&resampled);
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                device.build_input_stream(
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
                )
            }
            cpal::SampleFormat::U16 => {
                device.build_input_stream(
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
                )
            }
            _ => return Err("Unsupported sample format".to_string()),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;
        s.stream = Some(StreamWrapper(stream));

        Ok(())
    }

    fn save_wav_file(&self, app_handle: &tauri::AppHandle, samples: &[f32], start_time: chrono::DateTime<chrono::Local>) -> Result<Option<String>, String> {
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
                println!("Saved debug WAV file to {}", path_str);
                return Ok(Some(path_str));
            }
        }
        Ok(None)
    }

    pub fn stop_recording(&self, app_handle: &tauri::AppHandle) -> Result<Option<String>, String> {
        let (samples, start_time) = {
            let mut s = self.state.lock().unwrap();
            if !s.is_recording {
                return Ok(None);
            }

            s.is_recording = false;
            s.is_saving = true;
            
            if let Some(wrapper) = s.stream.take() {
                let _ = wrapper.0.pause();
            }

            s.last_samples = s.buffer.clone();
            let samples = std::mem::take(&mut s.buffer);
            let start_time = s.recording_start.take().unwrap_or_else(chrono::Local::now);
            (samples, start_time)
        };

        let _ = crate::rebuild_tray_menu(app_handle);

        let pcm_len = samples.len();
        println!("Recorded {} samples (16kHz)", pcm_len);

        // Run the saving logic in a helper to safely catch early-return errors
        let save_result = self.save_wav_file(app_handle, &samples, start_time);

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
