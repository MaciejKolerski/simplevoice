use std::path::PathBuf;
use std::fs;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, serde::Serialize)]
struct DownloadPayload {
    repo_id: String,
    file: String,
    progress: f64,
    current_file_index: usize,
    total_files: usize,
}

/// Command to asynchronously download model files from Hugging Face Hub
/// and stream the download progress to the frontend UI.
#[tauri::command]
pub async fn download_model(
    repo_id: String,
    files: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let app_local_data: PathBuf = app_handle.path().app_local_data_dir()
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

    let client = reqwest::Client::new();
    let total_files = files.len();

    for (index, file_path) in files.iter().enumerate() {
        let file_url = format!("https://huggingface.co/{}/resolve/main/{}", repo_id, file_path);
        let dest_path = model_dir.join(file_path);

        // Ensure parent directories exist (e.g. for onnx/encoder_model.onnx)
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent directories for {}: {}", file_path, e))?;
        }

        let emit_progress = |progress: f64| {
            let _ = app_handle.emit("download-progress", DownloadPayload {
                repo_id: repo_id.clone(),
                file: file_path.clone(),
                progress,
                current_file_index: index + 1,
                total_files,
            });
        };

        emit_progress(0.0);

        let mut response = client.get(&file_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download file {}: {}", file_path, e))?;

        if !response.status().is_success() {
            return Err(format!("Server returned error {} for {}", response.status(), file_path));
        }

        let content_length = response.content_length().unwrap_or(0);
        let mut dest_file = fs::File::create(&dest_path)
            .map_err(|e| format!("Failed to create local file {}: {}", file_path, e))?;

        let mut downloaded: u64 = 0;
        
        while let Some(chunk) = response.chunk().await.map_err(|e| format!("Error while downloading chunk: {}", e))? {
            dest_file.write_all(&chunk)
                .map_err(|e| format!("Failed to write chunk to file: {}", e))?;
            
            downloaded += chunk.len() as u64;
            if content_length > 0 {
                let progress = (downloaded as f64 / content_length as f64) * 100.0;
                emit_progress(progress);
            }
        }
        
        emit_progress(100.0);
    }

    Ok(())
}
