slint::include_modules!();

// Standalone metric panel: `cargo run -p beca-panel`.
// The window itself lives in beca-ui (`ui/panel/metric_panel_window.slint`).
fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    ui.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });

    ui.run()
}
