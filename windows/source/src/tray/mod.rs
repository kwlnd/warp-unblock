//! Иконка в трее: левый клик — открыть окно, правый — меню. Обработчики работают независимо от
//! отрисовки окна, поэтому живут и при скрытом окне. Windows — `tray-icon`, Linux — значок
//! StatusNotifierItem по D-Bus (`ksni`): работает в KDE, Cinnamon, XFCE и в GNOME с расширением
//! AppIndicator. Нет трея — окно просто не прячется.
use std::sync::atomic::AtomicBool;

#[cfg(unix)]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use linux::Tray;
#[cfg(windows)]
pub use windows::Tray;

/// Выбран пункт «Выход» в меню трея.
pub static QUIT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Off,
    Busy,
    Ok,
    Error,
}

/// Цвет значка по состоянию: серый, жёлтый, оранжевый, красный.
pub(crate) fn color(look: Look) -> (u8, u8, u8) {
    match look {
        Look::Off => (128, 134, 146),
        Look::Busy => (240, 180, 41),
        Look::Ok => (246, 130, 31),
        Look::Error => (242, 102, 109),
    }
}
