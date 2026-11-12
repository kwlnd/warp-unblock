//! Трей Linux: StatusNotifierItem через `ksni`, свой поток и D-Bus-соединение.
use super::{color, Look, QUIT};
use crate::{
    engine::{Command, Handle as Engine, Settings},
    platform,
};
use ksni::{blocking::TrayMethods, menu::StandardItem, MenuItem};
use std::{path::PathBuf, sync::atomic::Ordering};

/// Щит из assets/icons/tray-mask.png, окрашенный в цвет состояния (формат пикселей ARGB32).
fn render(look: Look) -> ksni::Icon {
    let (r, g, b) = color(look);
    let mask = image::load_from_memory(include_bytes!("../../assets/icons/tray-mask.png"))
        .expect("tray mask")
        .to_rgba8();
    let (width, height) = mask.dimensions();
    ksni::Icon {
        width: width as i32,
        height: height as i32,
        data: mask.pixels().flat_map(|p| [p.0[3], r, g, b]).collect(),
    }
}

struct Item {
    engine: Engine,
    root: PathBuf,
    look: Look,
    tip: String,
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "warp-unblock".into()
    }

    fn title(&self) -> String {
        "WARP Unblock".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![render(self.look)]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "WARP Unblock".into(),
            description: self.tip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        platform::show_main_window(true);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Открыть".into(),
                activate: Box::new(|_| {
                    platform::show_main_window(true);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Подключить".into(),
                activate: Box::new(|this: &mut Self| {
                    this.engine.send(Command::Start(Settings::load(&this.root)))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Отключить".into(),
                activate: Box::new(|this: &mut Self| this.engine.send(Command::Stop)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Выход".into(),
                // Выход подхватывает таймер интерфейса: он останавливает соединение и закрывает окно.
                activate: Box::new(|_| QUIT.store(true, Ordering::Relaxed)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct Tray {
    handle: ksni::blocking::Handle<Item>,
    look: Look,
    tip: String,
}

impl Tray {
    /// `None`, если в сессии нет StatusNotifierWatcher (например, GNOME без расширения).
    pub fn new(engine: Engine, root: PathBuf) -> Option<Self> {
        let handle = Item {
            engine,
            root,
            look: Look::Off,
            tip: String::new(),
        }
        .spawn()
        .ok()?;
        Some(Self {
            handle,
            look: Look::Off,
            tip: String::new(),
        })
    }

    pub fn update(&mut self, look: Look, tip: &str) {
        if self.look == look && self.tip == tip {
            return;
        }
        self.look = look;
        self.tip = tip.to_owned();
        let tip = tip.to_owned();
        self.handle.update(move |item| {
            item.look = look;
            item.tip = tip;
        });
    }
}
