use tauri::AppHandle;

/// Compatibility command that reports unsupported local conversion.
/// Prebuilt models avoid executing unpinned converters or remote model code.
#[tauri::command]
pub async fn convert_model(_model_path: String, _app_handle: AppHandle) -> Result<(), String> {
    Err("On-device conversion has been removed. Download a prebuilt ONNX model \
         (e.g. Parakeet) from the model list instead."
        .to_string())
}
