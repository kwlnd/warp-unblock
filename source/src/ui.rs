use crate::{
    engine::{Command, Event, Settings, Worker},
    platform, strategies,
};
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

// Нейтральный тёмный фон и оранжевый акцент Cloudflare WARP.
const BACKGROUND: Color32 = Color32::from_rgb(22, 23, 26);
const SURFACE: Color32 = Color32::from_rgb(33, 35, 40);
const SURFACE_HOVER: Color32 = Color32::from_rgb(48, 51, 58);
const BORDER: Color32 = Color32::from_rgb(58, 62, 71);
const TEXT: Color32 = Color32::from_rgb(240, 242, 245);
const MUTED: Color32 = Color32::from_rgb(177, 183, 194);
const ACCENT: Color32 = Color32::from_rgb(246, 130, 31);
const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 152, 64);
const ON_ACCENT: Color32 = Color32::from_rgb(28, 16, 4);
const OK: Color32 = Color32::from_rgb(76, 195, 138);
const WARN: Color32 = Color32::from_rgb(240, 180, 41);
const ERROR: Color32 = Color32::from_rgb(242, 102, 109);

pub struct App {
    root: PathBuf,
    worker: Worker,
    settings: Settings,
    catalog: Vec<strategies::Strategy>,
    busy: bool,
    stopping: bool,
    running: bool,
    status: String,
    checks: [(String, bool); 3],
    journal: VecDeque<String>,
    engine_log: String,
    last_tail: Instant,
    last_health: Option<Instant>,
    close_dialog: bool,
    allow_close: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = SURFACE;
        visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = Color32::from_rgb(120, 66, 20);
        visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
        visuals.extreme_bg_color = Color32::from_rgb(17, 18, 20);
        visuals.collapsing_header_frame = false;
        let radius = egui::CornerRadius::same(8);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        for (state, fill) in [
            (&mut widgets.inactive, SURFACE_HOVER),
            (&mut widgets.hovered, Color32::from_rgb(62, 66, 75)),
            (&mut widgets.active, Color32::from_rgb(72, 77, 87)),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = egui::Stroke::new(1.0, BORDER);
            state.corner_radius = radius;
        }
        widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
        widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
        widgets.open.corner_radius = radius;
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.interaction.selectable_labels = false;
        style.interaction.multi_widget_text_select = false;
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
        cc.egui_ctx.set_style(style);
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        if let Ok(font) = fs::read(PathBuf::from(windows).join("Fonts/segoeui.ttf")) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("segoe".into(), egui::FontData::from_owned(font).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe".into());
            cc.egui_ctx.set_fonts(fonts);
        }
        let mut settings = Settings::load(&root);
        let catalog = strategies::load(&root).unwrap_or_else(|_| {
            serde_json::from_str(include_str!("../../app/config/strategies.json"))
                .expect("built-in catalog")
        });
        settings.strategy = settings.strategy.min(catalog.len() - 1);
        let worker = Worker::new(root.clone());
        worker.send(Command::Check);
        Self {
            root,
            worker,
            settings,
            catalog,
            busy: true,
            stopping: false,
            running: false,
            status: "Проверяю файлы…".into(),
            checks: [
                ("Ожидание проверки".into(), false),
                ("Ожидание проверки".into(), false),
                ("Тоннель выключен".into(), false),
            ],
            journal: VecDeque::new(),
            engine_log: String::new(),
            last_tail: Instant::now(),
            last_health: None,
            close_dialog: false,
            allow_close: false,
        }
    }

    fn action(&mut self, command: Command, status: &str) {
        self.stopping = matches!(command, Command::Stop);
        self.busy = true;
        self.status = status.into();
        self.worker.send(command);
    }

    fn journal(&mut self, text: String) {
        if self.journal.len() >= 300 {
            self.journal.pop_front();
        }
        self.journal.push_back(text);
    }

    fn healthy(&self) -> bool {
        self.running && self.checks.iter().all(|(_, ok)| *ok)
    }

    fn color(&self) -> Color32 {
        if self.busy {
            WARN
        } else if self.healthy() {
            OK
        } else if self.running {
            ERROR
        } else {
            MUTED
        }
    }

    fn connection_button(&mut self, ui: &mut egui::Ui) {
        let active = self.running || self.busy;
        let label = if self.stopping {
            "Отключаю…"
        } else if self.running {
            "Отключить"
        } else if self.busy {
            "Отменить"
        } else {
            "Подключить"
        };
        let fill = if active { SURFACE_HOVER } else { ACCENT };
        let text_color = if active { TEXT } else { ON_ACCENT };
        if ui
            .add_enabled(
                !self.stopping,
                egui::Button::new((
                    egui::Atom::grow(),
                    RichText::new(label).size(19.0).strong().color(text_color),
                    egui::Atom::grow(),
                ))
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, if active { BORDER } else { ACCENT_HOVER }))
                .corner_radius(10.0)
                .min_size(egui::vec2(ui.available_width(), 56.0)),
            )
            .clicked()
        {
            if active {
                self.action(Command::Stop, "Отключаю…");
            } else {
                self.action(Command::Start(self.settings.clone()), "Подключаю…");
            }
        }
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        let current = if self.settings.custom {
            "Свои аргументы".to_owned()
        } else {
            self.catalog[self.settings.strategy]
                .name
                .replace("WARP · ", "")
        };
        let right = ui.max_rect().right();
        let response = egui::CollapsingHeader::new(RichText::new("Профиль WARP").size(16.0).strong())
            .id_salt("profile")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.add_enabled_ui(!self.busy && !self.running, |ui| {
                    egui::ComboBox::from_id_salt("strategy")
                        .width(ui.available_width() - 12.0)
                        .height(220.0)
                        .selected_text(if self.settings.custom {
                            "Свои аргументы winws"
                        } else {
                            &self.catalog[self.settings.strategy].name
                        })
                        .show_ui(ui, |ui| {
                            for (index, profile) in self.catalog.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        !self.settings.custom && self.settings.strategy == index,
                                        &profile.name,
                                    )
                                    .clicked()
                                {
                                    self.settings.strategy = index;
                                    self.settings.custom = false;
                                }
                            }
                            ui.selectable_value(
                                &mut self.settings.custom,
                                true,
                                "Свои аргументы winws",
                            );
                        });
                    if self.settings.custom {
                        ui.label("Аргументы winws");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.settings.custom_args)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Тест профиля").clicked() {
                            self.action(Command::Test(self.settings.clone()), "Проверяю профиль…");
                        }
                        if ui.button("Подобрать автоматически").clicked() {
                            self.action(
                                Command::Auto(self.settings.clone()),
                                "Подбираю профиль WARP…",
                            );
                        }
                    });
                    ui.label(
                        RichText::new("Проверка без TUN. Перед тестом отключите другой VPN.")
                            .size(14.0)
                            .color(MUTED),
                    );
                });
            });
        ui.painter().text(
            egui::pos2(right, response.header_response.rect.center().y),
            egui::Align2::RIGHT_CENTER,
            current,
            egui::FontId::proportional(14.0),
            MUTED,
        );
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let color = self.color();
        let right = ui.max_rect().right();
        let response = egui::CollapsingHeader::new(RichText::new("Статус").size(16.0).strong())
            .id_salt("status")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                egui::Grid::new("checks")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        for (index, title) in
                            ["Компоненты", "Профиль", "Тоннель"].iter().enumerate()
                        {
                            ui.label(RichText::new(*title).color(MUTED));
                            ui.label(
                                RichText::new(&self.checks[index].0)
                                    .color(if self.checks[index].1 { OK } else { MUTED }),
                            );
                            ui.end_row();
                        }
                    });
                if self.running {
                    if let Some(last) = self.last_health {
                        ui.label(
                            RichText::new(format!(
                                "Проверено {} с назад · обновляется автоматически",
                                last.elapsed().as_secs()
                            ))
                            .size(14.0)
                            .color(MUTED),
                        );
                    }
                }
            });
        ui.painter().circle_filled(
            egui::pos2(right - 6.0, response.header_response.rect.center().y),
            5.0,
            color,
        );
    }

    fn logs(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Журнал").size(16.0).strong())
            .id_salt("journal")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Открыть логи").clicked() {
                        let path = self.root.join("data/logs");
                        let _ = fs::create_dir_all(&path);
                        platform::open(&path);
                    }
                    if ui.button("Списки доменов").clicked() {
                        platform::open(&self.root.join("app/config/lists"));
                    }
                    if ui.button("Инструкция").clicked() {
                        platform::open(&self.root.join("docs/README.md"));
                    }
                });
                if self.journal.is_empty() {
                    ui.label(RichText::new("Событий пока нет").size(14.0).color(MUTED));
                }
                egui::ScrollArea::vertical()
                    .id_salt("events")
                    .max_height(170.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for text in &self.journal {
                            ui.label(RichText::new(text).size(14.0).color(MUTED));
                        }
                    });
                if !self.engine_log.is_empty() {
                    egui::CollapsingHeader::new("Подробный лог mihomo")
                        .id_salt("engine-log")
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("engine-log-scroll")
                                .max_height(170.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&self.engine_log).monospace().size(13.0).color(MUTED),
                                        )
                                        .wrap(),
                                    );
                                });
                        });
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.worker.rx.try_recv() {
            match event {
                Event::Log(text) => self.journal(text),
                Event::State {
                    busy,
                    running,
                    status,
                } => {
                    self.busy = busy;
                    self.running = running;
                    self.status = status;
                    if !busy {
                        self.stopping = false;
                    }
                }
                Event::Check(index, text, ok) => {
                    if index == 2 {
                        self.last_health = Some(Instant::now());
                    }
                    if let Some(check) = self.checks.get_mut(index) {
                        *check = (text, ok);
                    }
                }
                Event::Selected(index) => {
                    self.settings.strategy = index;
                    self.settings.custom = false;
                }
            }
        }
        if self.last_tail.elapsed() > Duration::from_secs(1) {
            self.engine_log = tail(&self.root.join("data/logs/mihomo.log"));
            self.last_tail = Instant::now();
        }
        if ctx.input(|input| input.viewport().close_requested())
            && (self.running || self.busy)
            && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_dialog = true;
        }
        egui::TopBottomPanel::bottom("connection")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(20.0))
            .show(ctx, |ui| self.connection_button(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(20.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(12.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if self.busy {
                                ui.spinner();
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(12.0, 22.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(rect.center(), 4.5, self.color());
                            }
                            ui.vertical(|ui| {
                                let title = if self.busy {
                                    &self.status
                                } else if self.running && self.healthy() {
                                    "Обход работает"
                                } else if self.running {
                                    "Нет связи с WARP"
                                } else {
                                    &self.status
                                };
                                ui.label(RichText::new(title).size(22.0).strong());
                                if self.running && !self.busy {
                                    ui.label(
                                        RichText::new(&self.checks[2].0).size(14.0).color(MUTED),
                                    );
                                }
                            });
                        });
                    });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("sections")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        card(ui, |ui| self.profiles(ui));
                        card(ui, |ui| self.details(ui));
                        card(ui, |ui| self.logs(ui));
                    });
            });
        if self.close_dialog {
            egui::Window::new("Закрыть приложение?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Подключение будет остановлено.");
                    ui.horizontal(|ui| {
                        if ui.button("Остановить и закрыть").clicked() {
                            self.worker.send(Command::Shutdown);
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Остаться").clicked() {
                            self.close_dialog = false;
                        }
                    });
                });
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn tail(path: &Path) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(48 * 1024)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .take(50)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}
