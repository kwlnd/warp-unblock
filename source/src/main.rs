#![windows_subsystem = "windows"]

mod engine;
mod platform;
mod strategies;
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
    let _instance = platform::Instance::acquire()?;
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([620.0, 520.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(eframe::egui::IconData::default()),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "WARP Unblock",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, root)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}
