#![windows_subsystem = "windows"]

mod engine;
mod features;
mod glow;
mod opera;
mod platform;
mod strategies;
mod tray;
mod ui;
mod update;

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
    let preview = std::env::args().any(|arg| arg == "--preview");
    let after_update = std::env::args().any(|arg| arg == "--after-update");
    let _instance = if preview {
        None
    } else {
        Some(acquire_instance(after_update)?)
    };
    if !preview {
        update::cleanup(&root);
    }
    ui::run(root, tray_start, preview);
    Ok(())
}

fn acquire_instance(after_update: bool) -> anyhow::Result<platform::Instance> {
    // После обновления старый экземпляр ещё закрывается: ждём, пока он отпустит mutex.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match platform::Instance::acquire() {
            Ok(instance) => return Ok(instance),
            Err(_) if after_update && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(_) => {
                // Уже запущено (возможно, в трее): просто показываем существующее окно.
                if platform::show_main_window(true) {
                    std::process::exit(0);
                }
                anyhow::bail!("WARP Unblock уже запущен");
            }
        }
    }
}
