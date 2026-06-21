use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Per-download control flags toggled by `pause_download` / `cancel_download`.
///
/// These are standalone booleans with no data dependency on other state; the
/// download loop only polls them between chunks and a brief delay in observing a
/// change is fine, so `Ordering::Relaxed` is sufficient. If a future change makes
/// a flag guard related data, upgrade these accesses to `AcqRel`/`SeqCst`.
#[derive(Default)]
pub struct DownloadControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
}

/// Tracks in-flight downloads by id so they can be paused or cancelled.
#[derive(Default)]
pub struct DownloadRegistry {
    map: Mutex<HashMap<String, Arc<DownloadControl>>>,
}

#[derive(Clone, serde::Serialize)]
struct DownloadPayload {
    download_id: String,
    repo_id: String,
    file: String,
    progress: f64,
    current_file_index: usize,
    total_files: usize,
}

/// Partial files are downloaded to `<name>.part` and renamed to their final
/// name only once complete, so a half-finished download is never picked up as
/// an installed model by `scan_models`.
fn part_path_for(final_path: &Path) -> PathBuf {
    let mut os = final_path.to_path_buf().into_os_string();
    os.push(".part");
    PathBuf::from(os)
}

/// Reject file paths that could escape the models directory. The recommended
/// model list is trusted, but `download_model` is an exposed command, so we
/// defend in depth: a relative path with no `..` / root / prefix components.
fn is_safe_relative(file_path: &str) -> bool {
    use std::path::Component;
    let p = Path::new(file_path);
    !file_path.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Authoritative total size from a `Content-Range: bytes start-end/total`
/// header, when present and numeric (the server may send `*` for an unknown
/// total, which we treat as absent).
fn content_range_total(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()
}

fn cleanup_cancelled(is_single_file: bool, model_dir: &Path, part_path: &Path) {
    if is_single_file {
        let _ = fs::remove_file(part_path);
    } else {
        // Multi-file models live in their own folder; drop the whole thing
        // (partial `.part` plus any files already finished this run).
        let _ = fs::remove_dir_all(model_dir);
    }
}

/// Command to asynchronously download model files from Hugging Face Hub
/// and stream the download progress to the frontend UI.
///
/// Returns `"completed"`, `"paused"`, or `"cancelled"`. Pausing keeps the
/// partial `.part` file so a later call with the same `download_id` resumes via
/// an HTTP Range request; cancelling discards partial data.
#[tauri::command]
pub async fn download_model(
    repo_id: String,
    files: Vec<String>,
    download_id: String,
    registry: State<'_, DownloadRegistry>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let control = Arc::new(DownloadControl::default());
    registry
        .map
        .lock()
        .unwrap()
        .insert(download_id.clone(), control.clone());

    let result = run_download(&repo_id, &files, &download_id, &control, &app_handle).await;

    registry.map.lock().unwrap().remove(&download_id);
    result
}

async fn run_download(
    repo_id: &str,
    files: &[String],
    download_id: &str,
    control: &Arc<DownloadControl>,
    app_handle: &AppHandle,
) -> Result<String, String> {
    let app_local_data: PathBuf = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let models_dir = app_local_data.join("models");
    let folder_name = repo_id.replace("/", "--");

    let is_single_file = files.len() == 1;
    let model_dir = if is_single_file {
        models_dir.clone()
    } else {
        models_dir.join(&folder_name)
    };

    fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model directory: {}", e))?;

    // Bound connection establishment so a half-open socket cannot hang a download
    // forever. No total request timeout: model files are large and a slow-but-
    // progressing transfer must not be cut off (F4).
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let total_files = files.len();

    for (index, file_path) in files.iter().enumerate() {
        if !is_safe_relative(file_path) {
            return Err(format!("Refusing unsafe file path: {}", file_path));
        }

        let file_url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo_id, file_path
        );
        let dest_path = model_dir.join(file_path);

        // Ensure parent directories exist (e.g. for onnx/encoder_model.onnx)
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!("Failed to create parent directories for {}: {}", file_path, e)
            })?;
        }

        let part_path = part_path_for(&dest_path);

        let emit_progress = |progress: f64| {
            let _ = app_handle.emit(
                "download-progress",
                DownloadPayload {
                    download_id: download_id.to_string(),
                    repo_id: repo_id.to_string(),
                    file: file_path.clone(),
                    progress,
                    current_file_index: index + 1,
                    total_files,
                },
            );
        };

        // Already finished on a previous run.
        if dest_path.exists() {
            emit_progress(100.0);
            continue;
        }

        let mut resume_from = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

        let mut request = client.get(&file_url);
        if resume_from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
        }

        let mut response = request
            .send()
            .await
            .map_err(|e| format!("Failed to download file {}: {}", file_path, e))?;

        // A 416 means our `.part` is at least as large as the server's file, so it
        // is stale/corrupt (a partial left exactly at the final byte is rare and
        // indistinguishable from corruption). Discard it and re-download cleanly
        // rather than risk finalizing bad bytes.
        if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let _ = fs::remove_file(&part_path);
            resume_from = 0;
            response = client
                .get(&file_url)
                .send()
                .await
                .map_err(|e| format!("Failed to download file {}: {}", file_path, e))?;
        }

        let status = response.status();
        if !status.is_success() {
            return Err(format!("Server returned error {} for {}", status, file_path));
        }

        // Only append when the server actually honored our Range request.
        let resumed = resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        // Prefer the authoritative total from Content-Range; fall back to
        // resume_from + body length (206) or body length (200).
        let body_len = response.content_length().unwrap_or(0);
        let total = content_range_total(&response)
            .unwrap_or(if resumed { resume_from + body_len } else { body_len });

        let mut dest_file = if resumed {
            fs::OpenOptions::new()
                .append(true)
                .open(&part_path)
                .map_err(|e| format!("Failed to open partial file {}: {}", file_path, e))?
        } else {
            // Fresh start, or server ignored Range (200) -> truncate and restart.
            fs::File::create(&part_path)
                .map_err(|e| format!("Failed to create local file {}: {}", file_path, e))?
        };

        let mut downloaded: u64 = if resumed { resume_from } else { 0 };
        if total > 0 {
            emit_progress((downloaded as f64 / total as f64) * 100.0);
        } else {
            emit_progress(0.0);
        }

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Error while downloading chunk: {}", e))?
        {
            if control.cancelled.load(Ordering::Relaxed) {
                drop(dest_file);
                cleanup_cancelled(is_single_file, &model_dir, &part_path);
                return Ok("cancelled".to_string());
            }
            if control.paused.load(Ordering::Relaxed) {
                let _ = dest_file.flush();
                drop(dest_file);
                return Ok("paused".to_string());
            }

            dest_file
                .write_all(&chunk)
                .map_err(|e| format!("Failed to write chunk to file: {}", e))?;

            downloaded += chunk.len() as u64;
            if total > 0 {
                emit_progress((downloaded as f64 / total as f64) * 100.0);
            }
        }

        dest_file
            .flush()
            .map_err(|e| format!("Failed to flush file {}: {}", file_path, e))?;
        drop(dest_file);

        // Guard against a silently truncated stream: if the expected size is known
        // and we got a different count, keep the `.part` so a retry can resume.
        if total > 0 && downloaded != total {
            return Err(format!(
                "Incomplete download for {} ({} of {} bytes)",
                file_path, downloaded, total
            ));
        }

        fs::rename(&part_path, &dest_path)
            .map_err(|e| format!("Failed to finalize {}: {}", file_path, e))?;
        emit_progress(100.0);
    }

    Ok("completed".to_string())
}

/// Signal a running download to pause (keeps partial data for later resume).
#[tauri::command]
pub fn pause_download(download_id: String, registry: State<'_, DownloadRegistry>) -> Result<(), String> {
    if let Some(control) = registry.map.lock().unwrap().get(&download_id) {
        control.paused.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Signal a running download to cancel (the download loop discards partial data).
#[tauri::command]
pub fn cancel_download(download_id: String, registry: State<'_, DownloadRegistry>) -> Result<(), String> {
    if let Some(control) = registry.map.lock().unwrap().get(&download_id) {
        control.cancelled.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Remove partial data for a download that is paused (i.e. has no active task,
/// so `cancel_download`'s flag would never be observed). Used when the user
/// cancels a paused download.
///
/// If a task with this `download_id` is somehow still active (e.g. a resume
/// raced this call), do nothing — deleting its `.part` out from under it would
/// corrupt the in-flight write. Cancelling an active download goes through
/// `cancel_download` instead.
#[tauri::command]
pub fn discard_download(
    repo_id: String,
    files: Vec<String>,
    download_id: String,
    registry: State<'_, DownloadRegistry>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if registry.map.lock().unwrap().contains_key(&download_id) {
        return Ok(());
    }

    let app_local_data: PathBuf = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;
    let models_dir = app_local_data.join("models");

    if files.len() == 1 {
        let dest_path = models_dir.join(&files[0]);
        let _ = fs::remove_file(part_path_for(&dest_path));
    } else {
        let folder_name = repo_id.replace("/", "--");
        let _ = fs::remove_dir_all(models_dir.join(folder_name));
    }
    Ok(())
}
