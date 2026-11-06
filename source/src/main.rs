#![windows_subsystem = "windows"]

mod engine;
mod platform;
mod strategies;
mod tray;
mod ui;

fn main() {
    if let Err(error) = run() {
        if !std::env::args().any(|arg| arg == "--self-test") {
            platform::error_box(&format!("{error:#}"));
        }
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let root = std::env::current_exe()?.parent().unwrap().to_path_buf();
    // Self-test is deliberately unprivileged and never changes system routes.
    if std::env::args().any(|arg| arg == "--self-test") {
        return engine::self_test(&root);
    }
    if !platform::is_admin() && !std::env::args().any(|arg| arg == "--preview") {
        platform::elevate()?;
        return Ok(());
    }
    let tray_start = std::env::args().any(|arg| arg == "--tray");
    let _instance = match platform::Instance::acquire() {
        Ok(instance) => instance,
        Err(_) => {
            // Уже запущено (возможно, в трее): просто показываем существующее окно.
            if platform::show_main_window(true) {
                return Ok(());
            }
            anyhow::bail!("WARP Unblock уже запущен");
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([620.0, 520.0])
            .with_visible(!tray_start)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(eframe::egui::IconData::default()),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "WARP Unblock",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, root, tray_start)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}
