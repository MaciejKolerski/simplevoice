use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use rodio::{Decoder, OutputStream, Sink, Source};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{Manager, State};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PLAYBACK_POSITION_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAudioStatus {
    id: Option<String>,
    position_sec: f64,
    duration_sec: f64,
    playing: bool,
}

impl HistoryAudioStatus {
    fn idle() -> Self {
        Self {
            id: None,
            position_sec: 0.0,
            duration_sec: 0.0,
            playing: false,
        }
    }
}

type PlayerResult = Result<HistoryAudioStatus, String>;
type Reply = Sender<PlayerResult>;

enum PlayerCommand {
    Play {
        id: String,
        path: PathBuf,
        position: Duration,
        reply: Reply,
    },
    Pause {
        id: String,
        reply: Reply,
    },
    Seek {
        id: String,
        position: Duration,
        reply: Reply,
    },
    Stop {
        id: Option<String>,
        reply: Reply,
    },
    Status {
        id: String,
        reply: Reply,
    },
}

#[derive(Clone)]
pub struct HistoryAudioController {
    commands: Sender<PlayerCommand>,
}

impl HistoryAudioController {
    pub fn new() -> Self {
        let (commands, receiver) = unbounded();
        std::thread::Builder::new()
            .name("history-audio-player".to_string())
            .spawn(move || player_worker(receiver))
            .expect("failed to start history audio player");
        Self { commands }
    }

    fn request(&self, command: impl FnOnce(Reply) -> PlayerCommand) -> PlayerResult {
        let (reply, response) = bounded(1);
        self.commands
            .send(command(reply))
            .map_err(|_| "The audio player is unavailable".to_string())?;
        response
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| "The audio player did not respond".to_string())?
    }

    fn play(&self, id: String, path: PathBuf, position: Duration) -> PlayerResult {
        self.request(|reply| PlayerCommand::Play {
            id,
            path,
            position,
            reply,
        })
    }

    fn pause(&self, id: String) -> PlayerResult {
        self.request(|reply| PlayerCommand::Pause { id, reply })
    }

    fn seek(&self, id: String, position: Duration) -> PlayerResult {
        self.request(|reply| PlayerCommand::Seek {
            id,
            position,
            reply,
        })
    }

    pub fn stop(&self, id: Option<String>) -> PlayerResult {
        self.request(|reply| PlayerCommand::Stop { id, reply })
    }

    fn status(&self, id: String) -> PlayerResult {
        self.request(|reply| PlayerCommand::Status { id, reply })
    }
}

impl Default for HistoryAudioController {
    fn default() -> Self {
        Self::new()
    }
}

struct Playback {
    id: String,
    path: PathBuf,
    duration: Duration,
    _stream: OutputStream,
    sink: Sink,
}

fn player_worker(receiver: Receiver<PlayerCommand>) {
    let mut playback: Option<Playback> = None;

    while let Ok(command) = receiver.recv() {
        match command {
            PlayerCommand::Play {
                id,
                path,
                position,
                reply,
            } => {
                let needs_new_playback = playback.as_ref().is_none_or(|active| {
                    active.id != id || active.path != path || active.sink.empty()
                });

                let result = if needs_new_playback {
                    if let Some(active) = playback.take() {
                        active.sink.stop();
                    }
                    match open_playback(id, path, position) {
                        Ok(active) => {
                            playback = Some(active);
                            Ok(snapshot(playback.as_ref()))
                        }
                        Err(error) => Err(error),
                    }
                } else if let Some(active) = playback.as_ref() {
                    seek_sink(active, position).map(|()| {
                        active.sink.play();
                        snapshot(playback.as_ref())
                    })
                } else {
                    Ok(HistoryAudioStatus::idle())
                };
                let _ = reply.send(result);
            }
            PlayerCommand::Pause { id, reply } => {
                if let Some(active) = playback.as_ref().filter(|active| active.id == id) {
                    active.sink.pause();
                }
                let _ = reply.send(Ok(snapshot_for_id(playback.as_ref(), &id)));
            }
            PlayerCommand::Seek {
                id,
                position,
                reply,
            } => {
                let result = if let Some(active) =
                    playback.as_ref().filter(|active| active.id == id)
                {
                    seek_sink(active, position).map(|()| snapshot_for_id(playback.as_ref(), &id))
                } else {
                    Ok(HistoryAudioStatus::idle())
                };
                let _ = reply.send(result);
            }
            PlayerCommand::Stop { id, reply } => {
                let should_stop = id
                    .as_ref()
                    .is_none_or(|id| playback.as_ref().is_some_and(|active| &active.id == id));
                if should_stop {
                    if let Some(active) = playback.take() {
                        active.sink.stop();
                    }
                }
                let _ = reply.send(Ok(HistoryAudioStatus::idle()));
            }
            PlayerCommand::Status { id, reply } => {
                let _ = reply.send(Ok(snapshot_for_id(playback.as_ref(), &id)));
            }
        }
    }
}

fn open_playback(
    id: String,
    path: PathBuf,
    requested_position: Duration,
) -> Result<Playback, String> {
    let file =
        File::open(&path).map_err(|error| format!("Could not open the recording: {error}"))?;
    let source = Decoder::new(BufReader::new(file))
        .map_err(|error| format!("Could not decode the recording: {error}"))?;
    let duration = source.total_duration().unwrap_or_default();
    let (stream, handle) = OutputStream::try_default()
        .map_err(|error| format!("Could not open the audio output: {error}"))?;
    let sink = Sink::try_new(&handle)
        .map_err(|error| format!("Could not create the audio player: {error}"))?;

    sink.pause();
    sink.append(source);
    let playback = Playback {
        id,
        path,
        duration,
        _stream: stream,
        sink,
    };
    seek_sink(&playback, requested_position)?;
    playback.sink.play();
    Ok(playback)
}

fn seek_sink(playback: &Playback, requested_position: Duration) -> Result<(), String> {
    let position = requested_position.min(playback.duration);
    playback
        .sink
        .try_seek(position)
        .map_err(|error| format!("Could not seek in the recording: {error}"))
}

fn snapshot(playback: Option<&Playback>) -> HistoryAudioStatus {
    let Some(playback) = playback else {
        return HistoryAudioStatus::idle();
    };
    let position = playback.sink.get_pos().min(playback.duration);
    HistoryAudioStatus {
        id: Some(playback.id.clone()),
        position_sec: position.as_secs_f64(),
        duration_sec: playback.duration.as_secs_f64(),
        playing: !playback.sink.is_paused() && !playback.sink.empty(),
    }
}

fn snapshot_for_id(playback: Option<&Playback>, id: &str) -> HistoryAudioStatus {
    playback
        .filter(|active| active.id == id)
        .map_or_else(HistoryAudioStatus::idle, |active| snapshot(Some(active)))
}

fn playback_position(seconds: f64) -> Result<Duration, String> {
    if !seconds.is_finite() || !(0.0..=MAX_PLAYBACK_POSITION_SECS).contains(&seconds) {
        return Err("Invalid playback position".to_string());
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn recording_path(app_handle: &tauri::AppHandle, requested: &str) -> Result<PathBuf, String> {
    let recordings = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("recordings")
        .canonicalize()
        .map_err(|error| format!("Could not access the recordings folder: {error}"))?;
    let requested = Path::new(requested)
        .canonicalize()
        .map_err(|error| format!("Could not access the recording: {error}"))?;

    if !requested.starts_with(&recordings) || !requested.is_file() {
        return Err("The selected file is not a Simplevoice recording".to_string());
    }
    Ok(requested)
}

#[tauri::command]
pub async fn play_history_audio(
    id: String,
    path: String,
    position_sec: f64,
    app_handle: tauri::AppHandle,
    controller: State<'_, HistoryAudioController>,
) -> PlayerResult {
    let controller = controller.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        controller.play(
            id,
            recording_path(&app_handle, &path)?,
            playback_position(position_sec)?,
        )
    })
    .await
    .map_err(|error| format!("The audio player task failed: {error}"))?
}

#[tauri::command]
pub async fn pause_history_audio(
    id: String,
    controller: State<'_, HistoryAudioController>,
) -> PlayerResult {
    let controller = controller.inner().clone();
    tauri::async_runtime::spawn_blocking(move || controller.pause(id))
        .await
        .map_err(|error| format!("The audio player task failed: {error}"))?
}

#[tauri::command]
pub async fn seek_history_audio(
    id: String,
    position_sec: f64,
    controller: State<'_, HistoryAudioController>,
) -> PlayerResult {
    let controller = controller.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        controller.seek(id, playback_position(position_sec)?)
    })
    .await
    .map_err(|error| format!("The audio player task failed: {error}"))?
}

#[tauri::command]
pub async fn stop_history_audio(
    id: Option<String>,
    controller: State<'_, HistoryAudioController>,
) -> PlayerResult {
    let controller = controller.inner().clone();
    tauri::async_runtime::spawn_blocking(move || controller.stop(id))
        .await
        .map_err(|error| format!("The audio player task failed: {error}"))?
}

#[tauri::command]
pub async fn get_history_audio_status(
    id: String,
    controller: State<'_, HistoryAudioController>,
) -> PlayerResult {
    let controller = controller.inner().clone();
    tauri::async_runtime::spawn_blocking(move || controller.status(id))
        .await
        .map_err(|error| format!("The audio player task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::playback_position;

    #[test]
    fn rejects_invalid_playback_positions() {
        assert!(playback_position(-1.0).is_err());
        assert!(playback_position(f64::NAN).is_err());
        assert!(playback_position(f64::INFINITY).is_err());
        assert!(playback_position(f64::MAX).is_err());
    }

    #[test]
    fn accepts_finite_playback_positions() {
        assert_eq!(playback_position(12.5).unwrap().as_secs_f64(), 12.5);
    }
}
