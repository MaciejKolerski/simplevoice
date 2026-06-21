use tauri::AppHandle;

/// On-device model conversion has been removed. It previously built a venv and ran
/// `optimum-cli export onnx --trust-remote-code`, installing `optimum`/`torch`/
/// `transformers` from git main with no pinned versions — non-reproducible and a
/// supply-chain / RCE surface. SimpleVoice ships curated prebuilt ONNX models (e.g.
/// Parakeet) instead. The command is retained so the existing UI resolves it and
/// shows an actionable message rather than a missing-command error.
#[tauri::command]
pub async fn convert_model(_model_path: String, _app_handle: AppHandle) -> Result<(), String> {
    Err("On-device conversion has been removed. Download a prebuilt ONNX model \
         (e.g. Parakeet) from the model list instead."
        .to_string())
}
