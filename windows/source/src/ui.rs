use crate::{
    engine::{Command, Event, Settings, Worker},
    platform, strategies, tray,
};
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
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

/// Ренат-мод включён: карточки чуть прозрачнее, чтобы фон был виден.
static RENAT: AtomicBool = AtomicBool::new(false);
/// Текущий оттенок «RGB» (0..1), обновляется каждый кадр.
static RENAT_HUE: AtomicU32 = AtomicU32::new(0);

fn hue() -> f32 {
    f32::from_bits(RENAT_HUE.load(Ordering::Relaxed))
}

/// Акцентный цвет: обычный оранжевый или бегущий по кругу RGB в Ренат-моде.
fn accent() -> Color32 {
    if RENAT.load(Ordering::Relaxed) {
        hsv(hue(), 0.9, 1.0)
    } else {
        ACCENT
    }
}

fn border() -> Color32 {
    if RENAT.load(Ordering::Relaxed) {
        hsv(hue(), 0.85, 0.95)
    } else {
        BORDER
    }
}

fn surface() -> Color32 {
    if RENAT.load(Ordering::Relaxed) {
        Color32::from_rgba_unmultiplied(22, 23, 26, 150)
    } else {
        SURFACE
    }
}

/// Фон при подключении / без подключения. Свои файлы в data/ главнее встроенных в app/assets/.
const RENAT_ON: [&str; 6] = [
    "data/renat-on.png",
    "data/renat-on.jpg",
    "data/renat.png",
    "data/renat.jpg",
    "app/assets/renat-on.jpg",
    "app/assets/renat-on.png",
];
const RENAT_OFF: [&str; 3] = [
    "data/renat-off.png",
    "data/renat-off.jpg",
    "app/assets/renat-off.jpg",
];

pub struct App {
    root: PathBuf,
    preview: bool,
    worker: Worker,
    settings: Settings,
    catalog: Vec<strategies::Strategy>,
    busy: bool,
    stage: Option<(usize, usize, String)>,
    batch: Option<(usize, usize, usize)>,
    stopping: bool,
    running: bool,
    status: String,
    checks: [(String, bool); 4],
    journal: VecDeque<String>,
    engine_log: String,
    /// Результаты последнего подбора: индекс профиля -> задержка (None — не прошёл).
    results: HashMap<usize, Option<u64>>,
    last_tail: Instant,
    last_health: Option<Instant>,
    close_dialog: bool,
    tray: Option<tray::Tray>,
    renat_on: Option<egui::TextureHandle>,
    renat_off: Option<egui::TextureHandle>,
    renat_loaded: bool,
    allow_close: bool,
    tools: crate::features::Tools,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: PathBuf,
        tray_start: bool,
        preview: bool,
    ) -> Self {
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
        let root_for_results = root.clone();
        let worker = Worker::new(root.clone());
        worker.send(Command::Check);
        let tray = if preview {
            None
        } else {
            tray::Tray::new(cc.egui_ctx.clone(), worker.handle(), root.clone())
        };
        if tray_start && !preview {
            if tray.is_some() {
                // Тихий старт из автозапуска: окно скрыто, подключаемся сразу.
                worker.send(Command::Start(settings.clone()));
            } else {
                // Без иконки в трее скрытое окно не вернуть — показываем его.
                platform::show_main_window(true);
            }
        }
        if settings.autostart && !preview {
            // Обновить путь в задаче, если папку с программой перенесли.
            let root = root.clone();
            std::thread::spawn(move || {
                let _ = platform::set_autostart(true, &root);
            });
        }
        Self {
            root,
            preview,
            worker,
            settings,
            catalog,
            busy: true,
            stage: None,
            batch: None,
            stopping: false,
            running: false,
            status: "Проверяю файлы…".into(),
            checks: [
                ("Ожидание проверки".into(), false),
                ("Ожидание проверки".into(), false),
                ("Тоннель выключен".into(), false),
                ("Opera EU выключена".into(), false),
            ],
            journal: VecDeque::new(),
            engine_log: String::new(),
            results: load_results(&root_for_results),
            last_tail: Instant::now(),
            last_health: None,
            close_dialog: false,
            tray,
            renat_on: None,
            renat_off: None,
            renat_loaded: false,
            allow_close: false,
            tools: Default::default(),
        }
    }

    fn action(&mut self, command: Command, status: &str) {
        self.stopping = matches!(command, Command::Stop);
        self.busy = true;
        self.stage = None;
        self.batch = None;
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
        if self.preview {
            ui.label("Предпросмотр интерфейса · подключение недоступно");
            return;
        }
        if self.tools.locked() {
            ui.label("Завершите операцию с настройками…");
            return;
        }
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
        if self.settings.renat && !self.stopping {
            let label = if self.running {
                "Перестать моггать"
            } else if self.busy {
                "Отменить"
            } else {
                "Моггать"
            };
            if rainbow_button(ui, label) {
                if active {
                    self.action(Command::Stop, "Отключаю…");
                } else {
                    self.action(Command::Start(self.settings.clone()), "Подключаю…");
                }
            }
            return;
        }
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
                .stroke(egui::Stroke::new(
                    1.0,
                    if active { BORDER } else { ACCENT_HOVER },
                ))
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

    fn progress(&self, ui: &mut egui::Ui) {
        let bar = |ui: &mut egui::Ui, fraction: f32, height: f32| {
            ui.add(
                egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                    .fill(accent())
                    .desired_height(height)
                    .corner_radius(height / 2.0),
            );
        };
        if let Some((done, total, failed)) = self.batch {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Подбор профиля").size(14.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("проверено {done} из {total}"))
                            .size(14.0)
                            .color(TEXT),
                    );
                    if failed > 0 {
                        ui.label(
                            RichText::new(format!("не прошло {failed}  "))
                                .size(14.0)
                                .color(ERROR),
                        );
                    }
                });
            });
            bar(ui, done as f32 / total.max(1) as f32, 10.0);
        }
        if let Some((done, total, text)) = &self.stage {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(text).size(14.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("этап {} из {total}", (done + 1).min(*total)))
                            .size(14.0)
                            .color(MUTED),
                    );
                });
            });
            bar(ui, *done as f32 / (*total).max(1) as f32, 6.0);
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
        let response =
            egui::CollapsingHeader::new(RichText::new("Профиль WARP").size(16.0).strong())
                .id_salt("profile")
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.add_enabled_ui(
                        !self.preview && !self.busy && !self.running && !self.tools.locked(),
                        |ui| {
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
                                        let (color, label) =
                                            mark(self.results.get(&index), &profile.name);
                                        let selected = !self.settings.custom
                                            && self.settings.strategy == index;
                                        let clicked = ui
                                            .horizontal(|ui| {
                                                let x = ui.cursor().min.x;
                                                ui.add_space(14.0);
                                                let row = ui.selectable_label(selected, label);
                                                ui.painter().circle_filled(
                                                    egui::pos2(x + 5.0, row.rect.center().y),
                                                    4.5,
                                                    color,
                                                );
                                                row.clicked()
                                            })
                                            .inner;
                                        if clicked {
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
                                    self.action(
                                        Command::Test(self.settings.clone()),
                                        "Проверяю профиль…",
                                    );
                                }
                                if ui.button("Выбрать лучший").clicked() {
                                    self.action(
                                        Command::Best(self.settings.clone()),
                                        "Сравниваю все профили…",
                                    );
                                }
                                if ui.button("Подобрать автоматически").clicked()
                                {
                                    self.action(
                                        Command::Auto(self.settings.clone()),
                                        "Подбираю профиль WARP…",
                                    );
                                }
                            });
                            if let Some(stat) =
                                crate::features::stats(&self.root).get(&self.settings.strategy)
                            {
                                ui.label(format!(
                                    "Успешно {}/{} · средняя задержка {} мс",
                                    stat.successes,
                                    stat.attempts,
                                    stat.total_ms / stat.successes.max(1)
                                ));
                            }
                            ui.label(
                                RichText::new(
                                    "Проверка без TUN. Перед тестом отключите другой VPN.",
                                )
                                .size(14.0)
                                .color(MUTED),
                            );
                        },
                    );
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
                        for (index, title) in ["Компоненты", "Профиль", "Тоннель", "Нейросети"]
                            .iter()
                            .enumerate()
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

    fn options(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Настройки").size(16.0).strong())
            .id_salt("options")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.add_enabled_ui(!self.tools.locked(), |ui| {
                    let mut changed = false;
                    let mut autostart = self.settings.autostart;
                    if switch_row(
                        ui,
                        "Запускать вместе с Windows",
                        "Тихо, в трее, с автоподключением",
                        &mut autostart,
                    ) {
                        match platform::set_autostart(autostart, &self.root) {
                            Ok(()) => {
                                self.settings.autostart = autostart;
                                changed = true;
                            }
                            Err(error) => {
                                self.journal(format!("Автозапуск не изменён: {error:#}"));
                            }
                        }
                    }
                    if switch_row(
                        ui,
                        "Крестик сворачивает в трей",
                        "Выключено — крестик закрывает приложение",
                        &mut self.settings.close_to_tray,
                    ) {
                        changed = true;
                    }
                    let hint = if self.renat_on.is_some() || self.renat_off.is_some() {
                        "Радужная кнопка, фон зависит от подключения"
                    } else {
                        "Свои картинки: data/renat-on.png и renat-off.png"
                    };
                    if switch_row(ui, "Ренат мод", hint, &mut self.settings.renat) {
                        changed = true;
                    }
                    if changed {
                        let _ = self.settings.save(&self.root);
                    }
                });
                self.tools.show(
                    ui,
                    &self.root,
                    &self.settings,
                    self.busy || self.running,
                    self.running,
                );
            });
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
                        self.tools.open_editor();
                    }
                    if ui.button("Инструкция").clicked() {
                        platform::open(&self.root.join("README.md"));
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
                                            RichText::new(&self.engine_log)
                                                .monospace()
                                                .size(13.0)
                                                .color(MUTED),
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
        if let Some(settings) = self.tools.poll(&self.root) {
            self.settings = settings;
        }
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
                        self.stage = None;
                        self.batch = None;
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
                Event::Stage(done, total, text) => self.stage = Some((done, total, text)),
                Event::Batch(batch) => self.batch = batch,
                Event::Selected(index) => {
                    self.settings.strategy = index;
                    self.settings.custom = false;
                }
            }
        }
        if self.last_tail.elapsed() > Duration::from_secs(1) {
            self.results = load_results(&self.root);
            self.engine_log = tail(&self.root.join("data/logs/mihomo.log"));
            self.last_tail = Instant::now();
        }
        if tray::QUIT.load(std::sync::atomic::Ordering::Relaxed) {
            self.allow_close = true;
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.settings.close_to_tray && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                platform::show_main_window(false);
            } else if self.running || self.busy {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_dialog = true;
            }
        }
        let healthy = self.healthy();
        if let Some(tray) = &mut self.tray {
            let look = if self.busy {
                tray::Look::Busy
            } else if healthy {
                tray::Look::Ok
            } else if self.running {
                tray::Look::Error
            } else {
                tray::Look::Off
            };
            let tip = if self.busy {
                self.status.as_str()
            } else if self.running && healthy {
                "Обход работает"
            } else if self.running {
                "Нет связи с WARP"
            } else {
                "Отключено"
            };
            tray.update(look, tip);
        }
        let renat = self.settings.renat;
        RENAT.store(renat, Ordering::Relaxed);
        let hue_now = (ctx.input(|input| input.time) as f32 * 0.25).rem_euclid(1.0);
        RENAT_HUE.store(hue_now.to_bits(), Ordering::Relaxed);
        ctx.style_mut(|style| {
            let color = accent();
            style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, color);
            style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, color);
            style.visuals.selection.stroke = egui::Stroke::new(1.0, color);
            style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border());
        });
        if renat && !self.renat_loaded {
            self.renat_loaded = true;
            self.renat_on = load_renat(ctx, &self.root, "renat-on", &RENAT_ON);
            self.renat_off = load_renat(ctx, &self.root, "renat-off", &RENAT_OFF);
        }
        let panel_fill = if renat {
            Color32::from_black_alpha(40)
        } else {
            BACKGROUND
        };
        if renat {
            let backdrop = if self.running {
                self.renat_on.as_ref()
            } else {
                self.renat_off.as_ref()
            };
            paint_backdrop(ctx, backdrop);
            // Радужная кнопка анимируется, поэтому перерисовываем чаще.
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        egui::TopBottomPanel::bottom("connection")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(panel_fill).inner_margin(20.0))
            .show(ctx, |ui| self.connection_button(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(panel_fill).inner_margin(20.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(surface())
                    .stroke(egui::Stroke::new(1.0, border()))
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
                                } else if self.running && self.checks[2].1 {
                                    "Нейросети: нет связи с Opera"
                                } else if self.running {
                                    "Нет связи с WARP"
                                } else {
                                    &self.status
                                };
                                ui.label(RichText::new(title).size(22.0).strong());
                                if self.running && !self.busy {
                                    ui.label(
                                        RichText::new(if self.checks[2].1 {
                                            &self.checks[3].0
                                        } else {
                                            &self.checks[2].0
                                        })
                                        .size(13.0)
                                        .color(MUTED),
                                    );
                                }
                            });
                        });
                        if self.busy {
                            self.progress(ui);
                        }
                    });
                ui.add_space(8.0);
                if ui.button("Редактировать списки проксирования").clicked()
                {
                    self.tools.open_editor();
                }
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("sections")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        card(ui, |ui| self.profiles(ui));
                        card(ui, |ui| self.details(ui));
                        card(ui, |ui| self.options(ui));
                        card(ui, |ui| self.logs(ui));
                    });
            });
        self.tools
            .editor_window(ctx, &self.root, self.busy || self.running);
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

/// Строка с подписью и тумблером справа. Возвращает true при переключении.
fn switch_row(ui: &mut egui::Ui, title: &str, hint: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(16.0));
            ui.label(RichText::new(hint).size(14.0).color(MUTED));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(46.0, 26.0), egui::Sense::click());
            if response.clicked() {
                *value = !*value;
                changed = true;
            }
            let fill = if *value { accent() } else { SURFACE_HOVER };
            ui.painter().rect(
                rect,
                13.0,
                fill,
                egui::Stroke::new(1.0, if *value { accent() } else { border() }),
                egui::StrokeKind::Inside,
            );
            let knob_x = if *value {
                rect.right() - 13.0
            } else {
                rect.left() + 13.0
            };
            ui.painter().circle_filled(
                egui::pos2(knob_x, rect.center().y),
                9.0,
                if *value { ON_ACCENT } else { MUTED },
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *value, title)
            });
        });
    });
    changed
}

/// Цвет маркера и текст строки списка: название профиля + итог подбора.
fn mark(result: Option<&Option<u64>>, name: &str) -> (Color32, egui::text::LayoutJob) {
    let (color, note) = match result {
        Some(Some(ms)) => (OK, format!("  ·  {ms} мс")),
        Some(None) => (ERROR, "  ·  не прошёл".to_owned()),
        None => (BORDER, String::new()),
    };
    let font = egui::FontId::proportional(16.0);
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat::simple(font.clone(), TEXT));
    job.append(&note, 0.0, egui::TextFormat::simple(font, color));
    (color, job)
}

fn load_results(root: &Path) -> HashMap<usize, Option<u64>> {
    let Ok(data) = fs::read(root.join("data/strategy-results.json")) else {
        return HashMap::new();
    };
    let Ok(items) = serde_json::from_slice::<Vec<serde_json::Value>>(&data) else {
        return HashMap::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let index = item["index"].as_u64()? as usize;
            Some((index, item["delay_ms"].as_u64()))
        })
        .collect()
}

fn hsv(hue: f32, saturation: f32, value: f32) -> Color32 {
    let c = egui::ecolor::Hsva::new(hue.rem_euclid(1.0), saturation, value, 1.0);
    Color32::from(c)
}

/// Кнопка целиком мигает всеми цветами RGB (без градиента). Возвращает true по клику.
fn rainbow_button(ui: &mut egui::Ui, label: &str) -> bool {
    let size = egui::vec2(ui.available_width(), 56.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let value = if response.hovered() { 1.0 } else { 0.92 };
    let fill = hsv(hue(), 0.95, value);
    ui.painter().rect(
        rect,
        12.0,
        fill,
        egui::Stroke::new(2.0, Color32::from_white_alpha(120)),
        egui::StrokeKind::Inside,
    );
    let font = egui::FontId::proportional(20.0);
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 1.0),
        egui::Align2::CENTER_CENTER,
        label,
        font.clone(),
        Color32::from_white_alpha(110),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        Color32::from_rgb(15, 10, 25),
    );
    response.clicked()
}

fn load_renat(
    ctx: &egui::Context,
    root: &Path,
    name: &str,
    candidates: &[&str],
) -> Option<egui::TextureHandle> {
    let path = candidates
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())?;
    let image = image::open(path).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Тёмная подложка и картинка по центру окна (режим cover) под всеми панелями.
fn paint_backdrop(ctx: &egui::Context, texture: Option<&egui::TextureHandle>) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let screen = ctx.content_rect();
    painter.rect_filled(screen, 0.0, BACKGROUND);
    let Some(texture) = texture else { return };
    let tex = texture.size_vec2();
    let scale = (screen.width() / tex.x).max(screen.height() / tex.y);
    let shown = tex * scale;
    // Портрет в широком окне режется по высоте: прижимаем кадр вверх, чтобы лицо оставалось видно.
    let excess = (shown.y - screen.height()).max(0.0);
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            screen.center().x - shown.x / 2.0,
            screen.top() - excess * 0.3,
        ),
        shown,
    );
    painter.with_clip_rect(screen).image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::from_white_alpha(175),
    );
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(surface())
        .stroke(egui::Stroke::new(1.0, border()))
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
