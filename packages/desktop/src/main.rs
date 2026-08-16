use std::time::Instant;

use dioxus::desktop::{Config, WindowBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Cold-start span: everything before the Dioxus launch call is handed the
/// window config. Logged once so a regression in init shows up as a bigger
/// number next to `startup_span=launch` (checkpoint 11 §C: <3s budget on
/// a warm store; the store-open half is measured by the UI's restore log).
fn main() {
    let boot = Instant::now();

    // Structured logging (checkpoint 11 §D): stdout for `dx serve`, plus a
    // rolling daily file under <data_dir>/logs/ the user can attach to an
    // issue. `RUST_LOG=info,ui=debug` overrides the defaults; matrix-sdk
    // is noisy at info so it stays at warn in the default filter.
    let _log_guard = init_tracing(); // Option<WorkerGuard>: None = stdout-only
    install_panic_hook();
    tracing::info!(
        target: "vesper_startup",
        elapsed_ms = boot.elapsed().as_millis() as u64,
        "logging initialized"
    );

    // Fully borderless on every platform: the app draws its own window controls
    // (see `window_chrome` in the ui crate) — a single `_ / ▢ / ×` cluster at
    // the top-right of every screen.
    let mut window = WindowBuilder::new()
        .with_title("Vesper")
        .with_decorations(false);

    // macOS: borderless windows are sharp-cornered by default. Make the window
    // transparent so the app can paint its own rounded corners via CSS on the
    // root container (matching the native macOS window look).
    #[cfg(target_os = "macos")]
    {
        window = window.with_transparent(true);
    }

    tracing::info!(
        target: "vesper_startup",
        elapsed_ms = boot.elapsed().as_millis() as u64,
        "handing off to dioxus launch"
    );
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(ui::App);
}

/// Two fmt layers over one registry: stdout (for `dx serve` / terminal
/// runs) and a non-blocking rolling daily file. The `WorkerGuard` MUST
/// outlive the app — returned to `main` and dropped at exit — or the file
/// writer stops flushing.
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,matrix_sdk=warn,ui=debug"));

    let stdout = tracing_subscriber::fmt::layer().with_filter(filter.clone());

    // File logging is best-effort: a data dir we can't write to must not
    // take the app down — stdout logging still works, and the settings'
    // "copy diagnostics" just reports the absence.
    let dir = client::diagnostics::logs_dir()
        .map_err(|e| (std::path::PathBuf::from("?"), format!("{e}")))
        .and_then(|dir| {
            std::fs::create_dir_all(&dir)
                .map_err(|e| (dir.clone(), format!("{e}")))
                .map(|_| dir)
        });
    match dir {
        Ok(dir) => {
            let appender = tracing_appender::rolling::daily(&dir, "vesper.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            tracing_subscriber::registry()
                .with(stdout)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(writer)
                        .with_filter(filter),
                )
                .init();
            eprintln!("vesper: file logging enabled in {}", dir.display());
            Some(guard)
        }
        Err((dir, e)) => {
            eprintln!("vesper: no file logging ({}: {e})", dir.display());
            tracing_subscriber::registry().with(stdout).init();
            None
        }
    }
}

/// Panic capture (checkpoint 11 §D): every panic lands in the log file
/// with the coarse screen the user was on (a name — never message
/// content), then the default hook takes over for the crash reporter.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "unknown".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        tracing::error!(
            target: "vesper_panic",
            screen = client::diagnostics::last_screen(),
            location = %location,
            "panic: {payload}"
        );
        default_hook(info);
    }));
}
