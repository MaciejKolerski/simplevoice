use tauri::ipc::InvokeError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Tauri error: {0}")]
    Tauri(String),

    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("Command error: {0}")]
    Command(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("Recording error: {0}")]
    Recording(String),
}

impl From<AppError> for InvokeError {
    fn from(err: AppError) -> Self {
        InvokeError::from_anyhow(anyhow::anyhow!("{}", err))
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Command(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Command(s.to_string())
    }
}
