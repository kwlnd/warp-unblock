//! Всё, что зависит от операционной системы: запуск процессов, права, окно, автозапуск.
//! Одинаковый набор функций на Windows (WinAPI) и Linux (процессы, хелпер под pkexec, XDG).
#[cfg(unix)]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;
