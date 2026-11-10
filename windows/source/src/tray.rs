//! Иконка в трее: левый клик — открыть окно, правый — меню. Обработчики работают на потоке
//! событий Windows и не зависят от отрисовки окна, поэтому живут и при скрытом окне.
use crate::{
    engine::{Command, Handle, Settings},
    platform,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Выбран пункт «Выход» в меню трея.
pub static QUIT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Off,
    Busy,
    Ok,
    Error,
}

pub struct Tray {
    icon: TrayIcon,
    look: Look,
    tip: String,
}

fn render(look: Look) -> Icon {
    let (r, g, b) = match look {
        Look::Off => (128, 134, 146),
        Look::Busy => (240, 180, 41),
        Look::Ok => (246, 130, 31),
        Look::Error => (242, 102, 109),
    };
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let distance = (dx * dx + dy * dy).sqrt();
            // Кольцо с точкой внутри, сглаженные края.
            let ring = (1.0 - (distance - 13.0).max(0.0)).clamp(0.0, 1.0)
                * (distance - 8.5).clamp(0.0, 1.0);
            let dot = (1.0 - (distance - 5.0).max(0.0)).clamp(0.0, 1.0);
            let alpha = (ring + dot).clamp(0.0, 1.0);
            rgba.extend([r, g, b, (alpha * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, size, size).expect("valid icon")
}

impl Tray {
    pub fn new(handle: Handle, root: PathBuf) -> Option<Self> {
        let open = MenuItem::new("Открыть", true, None);
        let connect = MenuItem::new("Подключить", true, None);
        let disconnect = MenuItem::new("Отключить", true, None);
        let quit = MenuItem::new("Выход", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &connect,
            &disconnect,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .ok()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("WARP Unblock")
            .with_icon(render(Look::Off))
            .build()
            .ok()?;

        TrayIconEvent::set_event_handler(Some(move |event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                platform::show_main_window(true);
            }
        }));
        let (open, connect, disconnect, quit) = (
            open.id().clone(),
            connect.id().clone(),
            disconnect.id().clone(),
            quit.id().clone(),
        );
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == open {
                platform::show_main_window(true);
            } else if event.id == connect {
                handle.send(Command::Start(Settings::load(&root)));
            } else if event.id == disconnect {
                handle.send(Command::Stop);
            } else if event.id == quit {
                // Выход подхватывает таймер интерфейса: он останавливает соединение и закрывает окно.
                QUIT.store(true, Ordering::Relaxed);
            }
        }));
        Some(Self {
            icon,
            look: Look::Off,
            tip: String::new(),
        })
    }

    pub fn update(&mut self, look: Look, tip: &str) {
        if self.look != look {
            self.look = look;
            let _ = self.icon.set_icon(Some(render(look)));
        }
        if self.tip != tip {
            self.tip = tip.to_owned();
            let _ = self.icon.set_tooltip(Some(format!("WARP Unblock — {tip}")));
        }
    }
}
