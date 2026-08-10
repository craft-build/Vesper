use dioxus::desktop::{Config, WindowBuilder};

fn main() {
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

    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(ui::App);
}
