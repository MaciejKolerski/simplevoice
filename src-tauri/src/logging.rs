//! Structured logging (H5): a rolling daily file under `<app_data>/logs/` plus
//! stderr, via `tracing`. Initialised once at startup.
//!
//! Fault-tolerant by design: any failure to set up file logging falls back to
//! stderr-only (or nothing) and NEVER panics — logging must not be able to stop
//! the app from starting. The non-blocking writer's `WorkerGuard` is parked in a
//! `static` so it lives for the whole process (dropping it would silently stop
//! the file writer).

use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialise logging. Writes a daily-rotated `simplevoice.log.<date>` in
/// `<app_data>/logs/` and mirrors to stderr at INFO and above. Safe and cheap to
/// call more than once — only the first call installs the subscriber.
pub fn init(app_data_dir: &Path) {
    if GUARD.get().is_some() {
        return;
    }

    let log_dir = app_data_dir.join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        // No writable log dir: stderr-only, still better than nothing.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_target(false)
            .try_init();
        return;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "simplevoice.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(non_blocking);
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr);

    let installed = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .is_ok();

    // Keep the writer alive for the process lifetime, but only if we actually
    // installed our subscriber (otherwise the appender is unused).
    if installed {
        let _ = GUARD.set(guard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installs a process-global subscriber, so it's ignored by default; run it
    /// alone: `cargo test --lib logging -- --ignored --nocapture`.
    #[test]
    #[ignore = "installs a global tracing subscriber"]
    fn init_writes_a_log_file() {
        let d = tempfile::tempdir().unwrap();
        init(d.path());
        tracing::info!("hello from the logging test");
        // The non-blocking writer flushes on its own thread; give it a moment.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let logs = d.path().join("logs");
        let files: Vec<_> = std::fs::read_dir(&logs)
            .expect("logs dir exists")
            .filter_map(|e| e.ok())
            .collect();
        assert!(!files.is_empty(), "expected a rolling log file to be created");
        let content = std::fs::read_to_string(files[0].path()).unwrap_or_default();
        assert!(
            content.contains("hello from the logging test"),
            "log file should contain the event; got: {content:?}"
        );
    }
}
