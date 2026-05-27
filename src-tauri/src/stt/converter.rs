use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager};

/// Command to convert a Hugging Face model directory to ONNX format using optimum-cli.
#[tauri::command]
pub async fn convert_model(
    model_path: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let model_dir = std::path::Path::new(&model_path);
    if !model_dir.exists() {
        return Err(format!("Model directory does not exist: {}", model_path));
    }

    // Determine output directory path (e.g. /path/to/model-onnx)
    let folder_name = model_dir.file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| "Invalid model directory path".to_string())?;
    
    let parent_dir = model_dir.parent()
        .ok_or_else(|| "Invalid parent directory path".to_string())?;
    
    let output_dir = parent_dir.join(format!("{}-onnx", folder_name));

    // Run the conversion in a blocking thread to avoid blocking the async executor
    tauri::async_runtime::spawn_blocking(move || {
        let app_local_data: PathBuf = app_handle.path().app_local_data_dir()
            .map_err(|e: tauri::Error| e.to_string())?;
        
        let venv_dir = app_local_data.join("converter_env");
        
        #[cfg(target_os = "windows")]
        let (python_executable, pip_executable, optimum_executable) = {
            (venv_dir.join("Scripts").join("python.exe"),
             venv_dir.join("Scripts").join("pip.exe"),
             venv_dir.join("Scripts").join("optimum-cli.exe"))
        };

        #[cfg(not(target_os = "windows"))]
        let (python_executable, pip_executable, optimum_executable) = {
            (venv_dir.join("bin").join("python"),
             venv_dir.join("bin").join("pip"),
             venv_dir.join("bin").join("optimum-cli"))
        };

        let emit_progress = |status: &str| {
            let _ = app_handle.emit("conversion-progress", status);
        };

        // Step 1: Check Python availability in the system and select the best executable
        emit_progress("Checking Python installation...");
        let executables = ["python3.12", "python3.11", "python3.10", "python3", "python"];
        let mut chosen_python = None;
        let mut python_version_str = String::new();
        let mut is_at_least_3_10 = false;

        for exe in &executables {
            if let Ok(output) = Command::new(exe).arg("--version").output() {
                if output.status.success() {
                    let version_out = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let version_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let version_info = if version_out.is_empty() { version_err } else { version_out };
                    
                    let version_num = version_info.replace("Python ", "");
                    let parts: Vec<&str> = version_num.split('.').collect();
                    let mut current_is_3_10 = false;
                    if parts.len() >= 2 {
                        if let (Ok(major), Ok(minor)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            if major > 3 || (major == 3 && minor >= 10) {
                                current_is_3_10 = true;
                            }
                        }
                    }
                    
                    if chosen_python.is_none() || current_is_3_10 {
                        chosen_python = Some(exe.to_string());
                        python_version_str = version_info.clone();
                        is_at_least_3_10 = current_is_3_10;
                    }
                    
                    if is_at_least_3_10 {
                        break;
                    }
                }
            }
        }

        let python_exe = chosen_python.ok_or_else(|| {
            "Python 3 is required for model conversion but was not found in your system PATH.".to_string()
        })?;

        let mut recreate_venv = false;
        if venv_dir.exists() {
            // Check the python version of the existing venv
            let venv_python = if cfg!(target_os = "windows") {
                venv_dir.join("Scripts").join("python.exe")
            } else {
                venv_dir.join("bin").join("python")
            };

            let mut current_venv_is_ok = false;
            if let Ok(output) = Command::new(&venv_python).arg("--version").output() {
                if output.status.success() {
                    let version_info = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let version_num = version_info.replace("Python ", "");
                    let parts: Vec<&str> = version_num.split('.').collect();
                    if parts.len() >= 2 {
                        if let (Ok(major), Ok(minor)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            if major > 3 || (major == 3 && minor >= 10) {
                                current_venv_is_ok = true;
                            }
                        }
                    }
                }
            }

            let config_path = std::path::Path::new(&model_path).join("config.json");
            let is_parakeet = if config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(config_path) {
                    content.contains("parakeet_tdt") || content.contains("ParakeetForTDT")
                } else {
                    false
                }
            } else {
                false
            };

            if is_parakeet && !current_venv_is_ok {
                if is_at_least_3_10 {
                    emit_progress("Existing Python env is outdated (<3.10). Recreating with newer Python...");
                    let _ = std::fs::remove_dir_all(&venv_dir);
                    recreate_venv = true;
                } else {
                    return Err(format!(
                        "Python version >= 3.10 is required to convert Parakeet TDT models (found system Python {}). Please install a newer Python version (e.g. 'brew install python@3.11') and try again.",
                        python_version_str
                    ));
                }
            }
        }

        // Step 2: Create Python virtual environment if it does not exist or needs recreation
        if !venv_dir.exists() || recreate_venv {
            emit_progress("Creating Python virtual environment...");
            let output = Command::new(&python_exe)
                .arg("-m")
                .arg("venv")
                .arg(&venv_dir)
                .output()
                .map_err(|e| format!("Failed to run venv creation command: {}", e))?;

            if !output.status.success() {
                return Err(format!("Failed to create virtual environment: {}", String::from_utf8_lossy(&output.stderr)));
            }
        }

        // Step 3: Install optimum[onnxruntime] & transformers & torch if not present
        if !optimum_executable.exists() || recreate_venv {
            emit_progress("Installing conversion tools (optimum, transformers, torch)...");
            
            // Upgrade pip first
            let _ = Command::new(&python_executable)
                .arg("-m")
                .arg("pip")
                .arg("install")
                .arg("--upgrade")
                .arg("pip")
                .output();

            let mut pip_cmd = Command::new(&pip_executable);
            pip_cmd.arg("install").arg("optimum[onnxruntime]");
            
            if is_at_least_3_10 {
                pip_cmd.arg("git+https://github.com/huggingface/transformers.git");
            } else {
                pip_cmd.arg("transformers");
            }
            
            let install_output = pip_cmd.arg("torch").output()
                .map_err(|e| format!("Failed to execute pip installation: {}", e))?;

            if !install_output.status.success() {
                return Err(format!("Failed to install Python packages: {}", String::from_utf8_lossy(&install_output.stderr)));
            }
        }

        // Step 4: Run optimum-cli to convert the local HF model to ONNX format
        emit_progress("Converting model to ONNX format...");
        
        let convert_output = Command::new(&optimum_executable)
            .arg("export")
            .arg("onnx")
            .arg("--model")
            .arg(&model_path)
            .arg("--task")
            .arg("automatic-speech-recognition")
            .arg("--trust-remote-code")
            .arg(&output_dir)
            .output()
            .map_err(|e| format!("Failed to execute optimum-cli conversion command: {}", e))?;

        if !convert_output.status.success() {
            return Err(format!("Model conversion failed: {}", String::from_utf8_lossy(&convert_output.stderr)));
        }

        emit_progress("Model conversion completed successfully!");
        Ok(())
    })
    .await
    .map_err(|e| format!("Conversion thread panicked: {}", e))?
}
