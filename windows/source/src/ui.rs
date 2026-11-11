//! Интерфейс на GPUI: боковая навигация, главный экран со статусом и кнопкой подключения,
//! страницы профилей, списков доменов, настроек и журнала, модальные окна поверх.
use crate::{
    engine::{Command, Event, Settings, Worker},
    features, glow, platform, strategies, tray, update,
};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

// Нейтральный тёмный фон и оранжевый акцент Cloudflare WARP.
const BACKGROUND: u32 = 0x131418;
const SIDEBAR: u32 = 0x18191e;
const SURFACE: u32 = 0x1f2127;
const SURFACE_HOVER: u32 = 0x2a2d35;
const BORDER: u32 = 0x33363f;
const TEXT: u32 = 0xf0f2f5;
const MUTED: u32 = 0x9aa1ad;
const ACCENT: u32 = 0xf6821f;
const ACCENT_HOVER: u32 = 0xff9840;
const ON_ACCENT: u32 = 0x1c1004;
const OK: u32 = 0x4cc38a;
const WARN: u32 = 0xf0b429;
const ERROR: u32 = 0xf2666d;
/// Manrope вшит в программу, поэтому выглядит одинаково на любой Windows.
const FONT: &str = "Manrope";
const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Manrope-Regular.ttf"),
    include_bytes!("../assets/fonts/Manrope-Medium.ttf"),
    include_bytes!("../assets/fonts/Manrope-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Manrope-Bold.ttf"),
];

/// Светлая тема активна (системная светлая или выбранная вручную).
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

// Иконки Lucide, которых нет в базовом наборе gpui-kit.
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Power,
        Route,
        ListFilter,
        ScrollText,
        ShieldCheck,
        Download,
        Clock,
        Activity
    ]
);

struct AppAssets;
impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// Иконка Lucide из набора gpui-kit.
fn icon(name: &str, size: f32, color: Hsla) -> Icon {
    Icon::default()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .text_color(color)
}

/// Фон Ренат-мода при подключении / без подключения. Свои файлы в data/ главнее встроенных.
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

/// Цвет палитры: константы заданы для тёмной темы, светлая подставляет пару.
fn c(hex: u32) -> Hsla {
    let hex = if light() {
        match hex {
            BACKGROUND => 0xf4f5f7,
            SIDEBAR => 0xeaecf0,
            SURFACE => 0xffffff,
            SURFACE_HOVER => 0xe6e8ed,
            BORDER => 0xd7dae1,
            TEXT => 0x15171c,
            MUTED => 0x5c6370,
            ACCENT => 0xe8700f,
            ACCENT_HOVER => 0xf6821f,
            ON_ACCENT => 0xffffff,
            OK => 0x1a9a5e,
            WARN => 0xb7791f,
            ERROR => 0xd4343d,
            other => other,
        }
    } else {
        hex
    };
    rgb(hex).into()
}

pub fn run(root: PathBuf, tray_start: bool, preview: bool) {
    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let _ = cx.text_system().add_fonts(
                FONT_FILES
                    .iter()
                    .map(|f| std::borrow::Cow::Borrowed(*f))
                    .collect(),
            );
            let bounds = Bounds::centered(None, size(px(980.), px(760.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Своя плашка: системный заголовок прозрачный, кнопки окна рисуем сами.
                titlebar: Some(TitlebarOptions {
                    title: Some("WARP Unblock".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_min_size: Some(size(px(720.), px(540.))),
                icon: image::load_from_memory(include_bytes!("../assets/icons/app.png"))
                    .ok()
                    .map(|icon| std::sync::Arc::new(icon.to_rgba8())),
                show: !tray_start || preview,
                focus: !tray_start || preview,
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Main::new(root.clone(), tray_start, preview, window, cx))
            });
            match opened {
                Ok((_, view)) => {
                    let weak = view.downgrade();
                    let window = cx.windows().first().cloned();
                    if let Some(window) = window {
                        let _ = window.update(cx, |_, window, cx| {
                            window.on_window_should_close(cx, move |window, cx| {
                                weak.update(cx, |this, cx| this.should_close(window, cx))
                                    .unwrap_or(true)
                            });
                        });
                    }
                }
                Err(error) => {
                    platform::error_box(&format!("Не удалось открыть окно: {error:#}"));
                    cx.quit();
                }
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
        });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Home,
    Profiles,
    Lists,
    Settings,
    Journal,
}

const PAGES: [(Page, &str, &str); 5] = [
    (Page::Home, "Связь", "power"),
    (Page::Profiles, "Профили", "route"),
    (Page::Lists, "Списки", "list-filter"),
    (Page::Settings, "Настройки", "settings-2"),
    (Page::Journal, "Журнал", "scroll-text"),
];

enum Modal {
    Close,
    Update(update::Release),
    Installing(String, f32),
    Qr(qrcode::QrCode),
}

enum UpdateMsg {
    Checked(Result<Option<update::Release>, String>, bool),
    Progress(String, f32),
    Done(Result<usize, String>),
}

struct Main {
    root: PathBuf,
    preview: bool,
    worker: Worker,
    settings: Settings,
    catalog: Vec<strategies::Strategy>,
    page: Page,
    busy: bool,
    stage: Option<(usize, usize, String)>,
    batch: Option<(usize, usize, usize)>,
    stopping: bool,
    running: bool,
    status: String,
    checks: [(String, bool); 4],
    journal: VecDeque<String>,
    engine_log: String,
    results: HashMap<usize, Option<u64>>,
    last_tail: Instant,
    last_health: Option<Instant>,
    tray: Option<tray::Tray>,
    modal: Option<Modal>,
    allow_close: bool,
    started: Instant,
    // Списки доменов: три редактора и сообщение о сохранении.
    lists: Option<[Entity<TextareaState>; 3]>,
    list_tab: usize,
    list_message: String,
    custom_args: Entity<TextareaState>,
    // Фоновые задачи настроек.
    task_rx: Option<mpsc::Receiver<anyhow::Result<String>>>,
    task_message: String,
    services_rx: Option<mpsc::Receiver<Vec<features::Service>>>,
    services: Vec<features::Service>,
    update_rx: Option<mpsc::Receiver<UpdateMsg>>,
    update_message: String,
    pending_update: Option<update::Release>,
    install_after_stop: Option<update::Release>,
    theme_applied: bool,
    /// Запечённая светящаяся сетка главной карточки.
    glow_cache: std::rc::Rc<std::cell::RefCell<glow::Cache>>,
    connected_since: Option<Instant>,
    /// Какую команду ждём от движка — для всплывашки с итогом.
    pending: Option<&'static str>,
    toasts: Vec<Toast>,
    toast_seq: usize,
}

struct Toast {
    id: usize,
    text: String,
    color: u32,
    born: Instant,
}

impl Main {
    fn new(
        root: PathBuf,
        tray_start: bool,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = Settings::load(&root);
        let catalog = strategies::load(&root).unwrap_or_else(|_| {
            serde_json::from_str(include_str!("../../app/config/strategies.json"))
                .expect("built-in catalog")
        });
        settings.strategy = settings.strategy.min(catalog.len() - 1);
        let catalog_changed = !preview && features::sync_catalog(&root, &catalog, &mut settings);
        let worker = Worker::new(root.clone());
        worker.send(Command::Check);
        let tray = if preview {
            None
        } else {
            tray::Tray::new(worker.handle(), root.clone())
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
        let custom_args = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx);
            state.set_value(settings.custom_args.clone(), window, cx);
            state
        });
        let results = load_results(&root);
        let mut this = Self {
            root,
            preview,
            worker,
            settings,
            catalog,
            page: Page::Home,
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
            results,
            last_tail: Instant::now(),
            last_health: None,
            tray,
            modal: None,
            allow_close: false,
            started: Instant::now(),
            lists: None,
            list_tab: 0,
            list_message: String::new(),
            custom_args,
            task_rx: None,
            task_message: String::new(),
            services_rx: None,
            services: Vec::new(),
            update_rx: None,
            update_message: String::new(),
            pending_update: None,
            install_after_stop: None,
            theme_applied: false,
            glow_cache: Default::default(),
            connected_since: None,
            pending: None,
            toasts: Vec::new(),
            toast_seq: 0,
        };
        if catalog_changed {
            this.log(
                "Каталог профилей WARP обновился: статистика и выбор профиля сброшены.".into(),
            );
        }
        if this.settings.check_updates
            && (!preview || std::env::var_os("WARP_PREVIEW_UPDATE").is_some())
        {
            this.check_updates(false);
        }
        // Опрос движка и анимация: таймер на главном потоке, живёт и при скрытом окне.
        cx.spawn_in(window, async move |this, cx| loop {
            let renat = this
                .update_in(cx, |this, window, cx| {
                    this.tick(window, cx);
                    // Радужный режим и светящаяся сетка на главной анимируются ~30 кадров/с.
                    this.settings.renat || (this.page == Page::Home && window.is_window_active())
                })
                .unwrap_or(false);
            if this.upgrade().is_none() {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(if renat { 33 } else { 120 }))
                .await;
        })
        .detach();
        this
    }

    // ---------- состояние ----------

    fn action(&mut self, command: Command, status: &str) {
        self.pending = Some(match command {
            Command::Start(_) => "start",
            Command::Stop | Command::Shutdown => "stop",
            Command::Test(_) => "test",
            Command::Auto(_) | Command::Best(_) => "pick",
            Command::Check => "check",
        });
        self.stopping = matches!(command, Command::Stop);
        self.busy = true;
        self.stage = None;
        self.batch = None;
        self.status = status.into();
        self.worker.send(command);
    }

    fn connect_settings(&mut self, cx: &App) -> Settings {
        self.settings.custom_args = self.custom_args.read(cx).value().to_string();
        let _ = self.settings.save(&self.root);
        self.settings.clone()
    }

    fn toast(&mut self, text: impl Into<String>, color: u32) {
        self.toast_seq += 1;
        self.toasts.push(Toast {
            id: self.toast_seq,
            text: text.into(),
            color,
            born: Instant::now(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn log(&mut self, text: String) {
        if self.journal.len() >= 300 {
            self.journal.pop_front();
        }
        self.journal.push_back(text);
    }

    fn healthy(&self) -> bool {
        self.running && self.checks.iter().all(|(_, ok)| *ok)
    }

    fn locked(&self) -> bool {
        self.task_rx.is_some()
    }

    fn state_color(&self) -> Hsla {
        if self.busy {
            c(WARN)
        } else if self.healthy() {
            c(OK)
        } else if self.running {
            c(ERROR)
        } else {
            c(MUTED)
        }
    }

    fn title(&self) -> String {
        if self.busy {
            self.status.clone()
        } else if self.running && self.healthy() {
            "Обход работает".into()
        } else if self.running && self.checks[2].1 {
            "Нейросети: нет связи с Opera".into()
        } else if self.running {
            "Нет связи с WARP".into()
        } else {
            self.status.clone()
        }
    }

    fn hue(&self) -> f32 {
        (self.started.elapsed().as_secs_f32() * 0.25).rem_euclid(1.0)
    }

    fn accent(&self) -> Hsla {
        if self.settings.renat {
            hsla(self.hue(), 0.9, 0.55, 1.0)
        } else {
            c(ACCENT)
        }
    }

    /// Тема: «system» следует за Windows, «light»/«dark» — выбор пользователя.
    fn sync_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = match self.settings.theme.as_str() {
            "light" => true,
            "dark" => false,
            _ => matches!(
                window.appearance(),
                WindowAppearance::Light | WindowAppearance::VibrantLight
            ),
        };
        if want != light() || !self.theme_applied {
            LIGHT.store(want, std::sync::atomic::Ordering::Relaxed);
            let mode = if want {
                gpui_kit::component::ThemeMode::Light
            } else {
                gpui_kit::component::ThemeMode::Dark
            };
            gpui_kit::component::Theme::change(mode, Some(window), cx);
            self.theme_applied = true;
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_theme(window, cx);
        while let Ok(event) = self.worker.rx.try_recv() {
            match event {
                Event::Log(text) => self.log(text),
                Event::State {
                    busy,
                    running,
                    status,
                } => {
                    if running && !self.running {
                        self.connected_since = Some(Instant::now());
                    }
                    if !running {
                        self.connected_since = None;
                    }
                    self.busy = busy;
                    self.running = running;
                    if !busy {
                        self.stage = None;
                        self.batch = None;
                        self.stopping = false;
                        match self.pending.take() {
                            Some("start") if running => {
                                self.toast(format!("Подключено · {}", self.profile_name()), OK)
                            }
                            Some("start") => self.toast(status.clone(), ERROR),
                            Some("stop") => self.toast("Отключено", MUTED),
                            Some("test") | Some("pick") => self.toast(status.clone(), ACCENT),
                            _ => {}
                        }
                    }
                    self.status = status;
                }
                Event::Check(index, text, ok) => {
                    if index == 2 && !ok && self.checks[2].1 && self.running && !self.busy {
                        self.toast("Связь с WARP пропала", ERROR);
                    }
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
        if let Some(result) = self.task_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.task_rx = None;
            let (message, color) = match result {
                Ok(text) => {
                    if text.starts_with("Импортировано.") {
                        self.settings = Settings::load(&self.root);
                        self.lists = None;
                    }
                    (text, OK)
                }
                Err(error) => (format!("Ошибка: {error:#}"), ERROR),
            };
            self.toast(message.clone(), color);
            self.task_message = message;
        }
        if let Some(result) = self.services_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            let ok = result.iter().filter(|s| s.ok).count();
            let total = result.len();
            self.toast(
                format!("Сервисы через WARP: доступно {ok} из {total}"),
                if ok == total { OK } else { WARN },
            );
            self.services = result;
            self.services_rx = None;
        }
        self.toasts
            .retain(|toast| toast.born.elapsed() < Duration::from_millis(4500));
        while let Some(message) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.on_update(message);
        }
        if let Some(release) = self.install_after_stop.take() {
            if self.running || self.busy {
                self.install_after_stop = Some(release);
            } else {
                self.start_install(release);
            }
        }
        if self.last_tail.elapsed() > Duration::from_secs(1) {
            self.results = load_results(&self.root);
            self.engine_log = tail(&self.root.join("data/logs/mihomo.log"));
            self.last_tail = Instant::now();
        }
        if tray::QUIT.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.quit(cx);
            return;
        }
        // Только предпросмотр: WARP_PREVIEW_STATE=busy|on|error показывает интерфейс в этом состоянии.
        if self.preview {
            if let Ok(state) = std::env::var("WARP_PREVIEW_STATE") {
                let all = |ok| std::array::from_fn(|i| (format!("Проверка {i}"), ok));
                match state.as_str() {
                    "busy" => {
                        (self.busy, self.running, self.status) = (true, false, "Подключаю…".into())
                    }
                    "on" => {
                        (self.busy, self.running) = (false, true);
                        self.checks = all(true);
                    }
                    "error" => {
                        (self.busy, self.running) = (false, true);
                        self.checks = all(false);
                    }
                    _ => {}
                }
            }
        }
        if self.page == Page::Lists && self.lists.is_none() {
            self.load_lists(window, cx);
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
        cx.notify();
    }

    fn should_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.allow_close {
            return true;
        }
        if self.settings.close_to_tray && self.tray.is_some() {
            platform::show_main_window(false);
            return false;
        }
        if self.running || self.busy {
            self.modal = Some(Modal::Close);
            cx.notify();
            return false;
        }
        self.worker.send(Command::Shutdown);
        true
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.allow_close = true;
        self.worker.send(Command::Shutdown);
        // Дать движку остановить дочерние процессы; Job Object всё равно завершит их при выходе.
        std::thread::sleep(Duration::from_millis(300));
        cx.quit();
    }

    // ---------- обновления ----------

    fn check_updates(&mut self, manual: bool) {
        if self.update_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        if manual {
            self.update_message = "Проверяю обновления…".into();
        }
        std::thread::spawn(move || {
            let result = update::check().map_err(|e| format!("{e:#}"));
            let _ = tx.send(UpdateMsg::Checked(result, manual));
        });
    }

    fn on_update(&mut self, message: UpdateMsg) {
        match message {
            UpdateMsg::Checked(result, manual) => {
                self.update_rx = None;
                match result {
                    Ok(Some(release)) => {
                        self.update_message = format!("Доступна {}", release.tag);
                        self.pending_update = Some(release.clone());
                        let skipped = !manual && self.settings.skip_version == release.tag;
                        if !skipped && self.modal.is_none() {
                            self.modal = Some(Modal::Update(release));
                        }
                    }
                    Ok(None) => {
                        self.update_message = format!("Версия {} актуальна", update::VERSION);
                        if manual {
                            self.toast(self.update_message.clone(), OK);
                        }
                    }
                    Err(error) if manual => {
                        self.update_message = format!("Не удалось проверить: {error}");
                        self.toast(self.update_message.clone(), ERROR);
                    }
                    Err(_) => {}
                }
            }
            UpdateMsg::Progress(text, fraction) => {
                self.modal = Some(Modal::Installing(text, fraction));
            }
            UpdateMsg::Done(result) => {
                self.update_rx = None;
                match result {
                    Ok(_) => match update::restart() {
                        Ok(()) => {
                            self.allow_close = true;
                            self.worker.send(Command::Shutdown);
                            std::process::exit(0);
                        }
                        Err(error) => {
                            self.modal = None;
                            self.update_message =
                                format!("Обновление установлено, перезапустите вручную: {error:#}");
                        }
                    },
                    Err(error) => {
                        self.modal = None;
                        self.update_message = format!("Обновление не установлено: {error}");
                        self.log(self.update_message.clone());
                    }
                }
            }
        }
    }

    fn begin_install(&mut self, release: update::Release) {
        self.modal = Some(Modal::Installing("Останавливаю соединение…".into(), 0.0));
        if self.running || self.busy {
            self.action(Command::Stop, "Отключаю для обновления…");
            self.install_after_stop = Some(release);
        } else {
            self.start_install(release);
        }
    }

    fn start_install(&mut self, release: update::Release) {
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        let root = self.root.clone();
        std::thread::spawn(move || {
            let progress = |step: update::Step| {
                let message = match step {
                    update::Step::Download(done, total) => UpdateMsg::Progress(
                        format!(
                            "Скачиваю {:.1} из {:.1} МБ",
                            done as f64 / 1048576.0,
                            total as f64 / 1048576.0
                        ),
                        if total > 0 {
                            done as f32 / total as f32 * 0.85
                        } else {
                            0.0
                        },
                    ),
                    update::Step::Verify => UpdateMsg::Progress("Проверяю SHA-256…".into(), 0.9),
                    update::Step::Install => {
                        UpdateMsg::Progress("Устанавливаю файлы…".into(), 0.95)
                    }
                };
                let _ = tx.send(message);
            };
            let result = features::guarded(|| {
                update::install(&root, &release, &progress).map(|n| n.to_string())
            });
            let _ = tx.send(UpdateMsg::Done(
                result
                    .map(|n| n.parse().unwrap_or(0))
                    .map_err(|e| format!("{e:#}")),
            ));
        });
    }

    // ---------- настройки ----------

    fn task(&mut self, task: impl FnOnce() -> anyhow::Result<String> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        self.task_message = "Выполняю…".into();
        std::thread::spawn(move || {
            let _ = tx.send(features::guarded(task));
        });
    }

    fn save_settings(&mut self) {
        let _ = self.settings.save(&self.root);
    }

    fn load_lists(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match features::read_lists(&self.root) {
            Ok(values) => {
                let editors = values.map(|text| {
                    cx.new(|cx| {
                        let mut state = TextareaState::new(window, cx);
                        state.set_value(text, window, cx);
                        state
                    })
                });
                self.lists = Some(editors);
            }
            Err(error) => self.list_message = format!("Не удалось прочитать списки: {error}"),
        }
    }

    fn save_lists(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editors) = &self.lists else { return };
        let values: [String; 3] = std::array::from_fn(|i| editors[i].read(cx).value().to_string());
        match features::save_lists(&self.root, &values) {
            Ok(saved) => {
                for (editor, text) in editors.iter().zip(saved) {
                    editor.update(cx, |state, cx| state.set_value(text, window, cx));
                }
                self.list_message =
                    "Списки сохранены. Применятся при следующем подключении.".into();
                self.toast("Списки доменов сохранены", OK);
            }
            Err(error) => {
                self.list_message = format!("Не сохранено: {error:#}");
                self.toast(self.list_message.clone(), ERROR);
            }
        }
    }
}

// ---------- элементы ----------

fn text(value: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(c(color))
        .child(value.into())
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p(px(18.))
        .rounded(px(14.))
        .bg(c(SURFACE))
        .border_1()
        .border_color(c(BORDER))
}

fn dot(color: Hsla, size: f32) -> Div {
    div().size(px(size)).rounded_full().bg(color).flex_none()
}

/// Обычная кнопка: `primary` — с акцентной заливкой.
fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    enabled: bool,
    accent: Hsla,
) -> Stateful<Div> {
    let base = div()
        .id(id.into())
        .flex()
        .items_center()
        .justify_center()
        .px(px(16.))
        .h(px(36.))
        .rounded(px(9.))
        .text_size(px(14.))
        .border_1()
        .child(label.into());
    let base = if primary {
        base.bg(accent)
            .border_color(accent)
            .text_color(c(ON_ACCENT))
    } else {
        base.bg(c(SURFACE_HOVER))
            .border_color(c(BORDER))
            .text_color(c(TEXT))
    };
    if enabled {
        base.cursor_pointer().hover(move |s| {
            if primary {
                s.bg(c(ACCENT_HOVER))
            } else {
                s.border_color(accent)
            }
        })
    } else {
        // Неактивная кнопка: без заливки, приглушённый текст, но контур виден в обеих темах.
        base.bg(transparent_black())
            .border_color(c(BORDER))
            .text_color(c(MUTED))
    }
}

/// Подсказка при наведении на неактивный элемент: почему он недоступен.
fn why(element: Stateful<Div>, reason: Option<&'static str>) -> Stateful<Div> {
    match reason {
        Some(reason) => element.tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(reason).build(window, cx)
        }),
        None => element,
    }
}

fn toggle(id: impl Into<ElementId>, on: bool, enabled: bool, accent: Hsla) -> Stateful<Div> {
    let track = div()
        .id(id.into())
        .w(px(46.))
        .h(px(26.))
        .flex_none()
        .rounded_full()
        .border_1()
        .flex()
        .items_center()
        .px(px(3.))
        .bg(if on { accent } else { c(SURFACE_HOVER) })
        .border_color(if on { accent } else { c(BORDER) })
        .child(dot(if on { c(ON_ACCENT) } else { c(MUTED) }, 18.));
    let track = if on {
        track.justify_end()
    } else {
        track.justify_start()
    };
    if enabled {
        track.cursor_pointer()
    } else {
        track.opacity(0.45)
    }
}

fn progress_bar(fraction: f32, height: f32, color: Hsla) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(c(SURFACE_HOVER))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

fn section_title(title: &str, hint: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(text(title.to_owned(), 22., TEXT).font_weight(FontWeight::SEMIBOLD))
        .child(text(hint.to_owned(), 14., MUTED))
}

// ---------- страницы ----------

impl Main {
    /// Своя плашка окна: логотип, статус по центру, кнопки окна. Перетаскивание,
    /// двойной клик и Snap обрабатывает Windows по зонам `WindowControlArea`.
    fn title_bar(&self, window: &Window) -> impl IntoElement {
        let color = self.state_color();
        let status = if self.busy {
            self.status.clone()
        } else if self.running && self.healthy() {
            "Обход работает".into()
        } else if self.running {
            self.title()
        } else {
            "Отключено".to_owned()
        };
        let pill_bg = if self.busy {
            c(WARN)
        } else if self.healthy() {
            c(OK)
        } else if self.running {
            c(ERROR)
        } else {
            c(MUTED)
        }
        .opacity(0.14);
        let control = |id: &'static str, glyph: &str, area: WindowControlArea, close: bool| {
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .window_control_area(area)
                .hover(move |s| {
                    if close {
                        s.bg(rgb(0xc42b1c))
                    } else {
                        s.bg(c(SURFACE_HOVER))
                    }
                })
                .child(icon(glyph, 16., c(MUTED)))
        };
        let maximize = if window.is_maximized() {
            "window-restore"
        } else {
            "window-maximize"
        };
        div()
            .relative()
            .flex()
            .flex_none()
            .h(px(40.))
            .bg(if self.settings.renat {
                hsla(0., 0., 0.05, 0.55)
            } else {
                c(SIDEBAR)
            })
            .border_b_1()
            .border_color(c(BORDER))
            .child(
                div()
                    .id("drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(16.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(icon("shield-check", 18., c(ACCENT)))
                    .child(text("WARP Unblock", 13., TEXT).font_weight(FontWeight::BOLD)),
            )
            // Статус строго по центру окна; слой тоже тянет окно, кнопки лежат поверх.
            .child(
                div()
                    .id("drag-center")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(180.))
                    .right(px(180.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px(px(12.))
                            .py(px(4.))
                            .rounded_full()
                            .bg(pill_bg)
                            .child(dot(color, 7.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if self.running || self.busy {
                                        color
                                    } else {
                                        c(MUTED)
                                    })
                                    .max_w(px(320.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(status),
                            ),
                    ),
            )
            .child(control(
                "min",
                "window-minimize",
                WindowControlArea::Min,
                false,
            ))
            .child(control("max", maximize, WindowControlArea::Max, false))
            .child(control(
                "close",
                "window-close",
                WindowControlArea::Close,
                true,
            ))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.accent();
        let item = |id: SharedString, glyph: &str, label: &str, active: bool| {
            div()
                .id(id)
                .w(px(68.))
                .py(px(9.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .rounded(px(12.))
                .cursor_pointer()
                .when(active, |s| s.bg(c(SURFACE_HOVER)))
                .when(!active, |s| s.hover(|s| s.bg(c(SURFACE))))
                .child(icon(glyph, 21., if active { accent } else { c(MUTED) }))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if active { c(TEXT) } else { c(MUTED) })
                        .child(label.to_owned()),
                )
        };
        let mut nav = div().flex().flex_col().items_center().gap_1();
        for (page, label, glyph) in PAGES {
            nav = nav.child(
                item(
                    format!("nav-{label}").into(),
                    glyph,
                    label,
                    self.page == page,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                })),
            );
        }
        let update_badge = self.pending_update.as_ref().map(|release| {
            let release = release.clone();
            item("update-badge".into(), "download", "Обновить", false)
                .relative()
                .child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(16.))
                        .child(dot(accent, 8.)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.modal = Some(Modal::Update(release.clone()));
                    cx.notify();
                }))
        });
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .w(px(84.))
            .flex_none()
            .h_full()
            .py(px(14.))
            .bg(if self.settings.renat {
                hsla(0., 0., 0.05, 0.55)
            } else {
                c(SIDEBAR)
            })
            .border_r_1()
            .border_color(c(BORDER))
            .child(div().flex().flex_col().items_center().gap_4().child(nav))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .children(update_badge)
                    .child(
                        text(
                            format!("v{}", update::VERSION.trim_start_matches('v')),
                            11.,
                            MUTED,
                        )
                        .w_full()
                        .text_center()
                        .font_weight(FontWeight::BOLD),
                    ),
            )
    }

    /// Почему кнопка подключения сейчас недоступна; None — доступна.
    fn power_blocked(&self) -> Option<&'static str> {
        if self.preview {
            Some("В предпросмотре подключение недоступно")
        } else if self.stopping {
            Some("Идёт отключение")
        } else if self.locked() || self.install_after_stop.is_some() {
            Some("Дождитесь окончания текущей операции")
        } else {
            None
        }
    }

    fn power_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let size = 128.;
        let renat = self.settings.renat && !self.stopping;
        let (ring, fill, glyph) = if renat {
            let rainbow = hsla(self.hue(), 0.95, 0.55, 1.0);
            (rainbow, rainbow, c(0x0f0a19))
        } else if self.healthy() {
            (self.accent(), self.accent(), c(ON_ACCENT))
        } else if self.busy {
            (c(WARN), c(SURFACE_HOVER), c(WARN))
        } else if self.running {
            (c(ERROR), c(SURFACE_HOVER), c(ERROR))
        } else {
            (c(BORDER), c(SURFACE_HOVER), self.accent())
        };
        let blocked = self.power_blocked();
        let button = div()
            .id("power")
            .size(px(size))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .border_2()
            .border_color(ring)
            .child(icon("power", 50., glyph))
            .when(blocked.is_none(), |s| {
                s.cursor_pointer().hover(|s| s.opacity(0.88))
            })
            .when(blocked.is_some(), |s| s.opacity(0.6))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.power_blocked().is_some() {
                    return;
                }
                if this.running || this.busy {
                    this.action(Command::Stop, "Отключаю…");
                } else {
                    let settings = this.connect_settings(cx);
                    this.action(Command::Start(settings), "Подключаю…");
                }
                cx.notify();
            }));
        let button = why(button, blocked);
        let halo = div()
            .absolute()
            .size(px(size + 26.))
            .rounded_full()
            .border_2()
            .border_color(ring.opacity(if self.healthy() { 0.35 } else { 0.25 }));
        // Пока идёт подключение или проверка, внешнее кольцо дышит.
        let halo = if self.busy {
            halo.with_animation(
                "power-pulse",
                Animation::new(Duration::from_millis(1400))
                    .repeat()
                    .with_easing(|t| 1.0 - (2.0 * t - 1.0).abs()),
                |el, t| el.opacity(0.25 + 0.75 * t),
            )
            .into_any_element()
        } else {
            halo.into_any_element()
        };
        div()
            .relative()
            .size(px(size + 26.))
            .flex()
            .items_center()
            .justify_center()
            .child(halo)
            .child(button)
            .into_any_element()
    }

    /// Светящаяся сетка за главной карточкой (см. glow.rs). Кадры даёт таймер `tick`.
    fn glow(&self) -> AnyElement {
        let mode = if self.busy {
            glow::Mode::Waves
        } else if self.healthy() {
            glow::Mode::On
        } else if self.running {
            glow::Mode::Error
        } else {
            glow::Mode::Idle
        };
        let color = match mode {
            glow::Mode::Waves => c(WARN).blend(self.accent().opacity(0.55)),
            glow::Mode::Error => c(ERROR),
            _ => self.accent(),
        };
        let frame = glow::Frame {
            mode,
            color,
            star: Hsla {
                l: (color.l + 0.28).min(0.94),
                s: color.s * 0.85,
                ..color
            },
            floor: if light() { 0.035 } else { 0.045 },
            time: self.started.elapsed().as_secs_f32(),
        };
        let cache = self.glow_cache.clone();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                glow::paint(window, bounds, &mut cache.borrow_mut(), &frame)
            },
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }

    fn ping(&self) -> Option<String> {
        let (text, ok) = &self.checks[2];
        if !*ok || !self.running {
            return None;
        }
        text.rsplit('·').next().map(|v| v.trim().to_owned())
    }

    fn uptime(&self) -> Option<String> {
        let secs = self.connected_since?.elapsed().as_secs();
        Some(format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        ))
    }

    fn home(&self, cx: &mut Context<Self>) -> Div {
        let accent = self.accent();
        let hint = if let Some(reason) = self.power_blocked() {
            reason.to_owned()
        } else if self.busy {
            "Нажмите, чтобы отменить".into()
        } else if self.running && self.checks[2].1 {
            self.checks[3].0.clone()
        } else if self.running {
            self.checks[2].0.clone()
        } else if self.settings.renat {
            "Нажмите, чтобы моггать".into()
        } else {
            "Нажмите, чтобы подключиться".into()
        };
        // Без фона и рамки: их даёт рамка ниже, под ней же лежит светящаяся сетка.
        let mut hero = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .px(px(18.))
            .py(px(26.))
            .child(self.power_button(cx))
            .child(
                text(self.title(), 24., TEXT)
                    .font_weight(FontWeight::BOLD)
                    .mt(px(6.)),
            )
            .child(text(hint, 13., MUTED));
        if self.busy {
            let mut progress = div().w_full().max_w(px(520.)).flex().flex_col().gap_3();
            if let Some((done, total, failed)) = self.batch {
                progress = progress.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(text("Подбор профиля", 13., MUTED))
                                .child(text(
                                    if failed > 0 {
                                        format!("проверено {done} из {total} · не прошло {failed}")
                                    } else {
                                        format!("проверено {done} из {total}")
                                    },
                                    13.,
                                    TEXT,
                                )),
                        )
                        .child(progress_bar(done as f32 / total.max(1) as f32, 8., accent)),
                );
            }
            if let Some((done, total, label)) = &self.stage {
                progress = progress.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(text(label.clone(), 13., MUTED))
                                .child(text(
                                    format!("этап {} из {total}", (done + 1).min(*total)),
                                    13.,
                                    MUTED,
                                )),
                        )
                        .child(progress_bar(
                            *done as f32 / (*total).max(1) as f32,
                            6.,
                            accent,
                        )),
                );
            }
            hero = hero.child(progress);
        }
        let metric = |label: &str, value: String, glyph: &str| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_3()
                .p(px(12.))
                .rounded(px(12.))
                .bg(c(SURFACE_HOVER))
                .child(icon(glyph, 18., c(MUTED)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(text(label.to_owned(), 12., MUTED))
                        .child(
                            text(value, 15., TEXT)
                                .font_weight(FontWeight::SEMIBOLD)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap(),
                        ),
                )
        };
        hero = hero.child(
            div()
                .w_full()
                .flex()
                .gap_3()
                .mt(px(8.))
                .child(metric(
                    "Время работы",
                    self.uptime().unwrap_or_else(|| "—".into()),
                    "clock",
                ))
                .child(metric("Профиль", self.profile_name(), "route"))
                .child(metric(
                    "Пинг WARP",
                    self.ping().unwrap_or_else(|| "—".into()),
                    "activity",
                )),
        );

        let titles = ["Компоненты", "Профиль", "Тоннель", "Нейросети"];
        let mut tiles = div().flex().gap_3();
        for (index, title) in titles.iter().enumerate() {
            let (value, ok) = &self.checks[index];
            tiles = tiles.child(
                card()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .p(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(dot(if *ok { c(OK) } else { c(BORDER) }, 8.))
                            .child(text(*title, 13., MUTED)),
                    )
                    .child(text(value.clone(), 14., if *ok { TEXT } else { MUTED })),
            );
        }
        let health = match (self.running, self.last_health) {
            (true, Some(last)) => format!(
                "Состояние проверено {} с назад, обновляется само",
                last.elapsed().as_secs()
            ),
            _ => String::new(),
        };

        let can_check = self.running && !self.busy && !self.locked() && self.services_rx.is_none();
        let mut services = card().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            text("Проверка сервисов", 16., TEXT).font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(text(
                            "HTTPS через WARP. Голос и видео проверяйте отдельно.",
                            13.,
                            MUTED,
                        )),
                )
                .child(why(
                    button("services", "Проверить", false, can_check, accent).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if !can_check {
                                return;
                            }
                            this.services.clear();
                            let (tx, rx) = mpsc::channel();
                            this.services_rx = Some(rx);
                            std::thread::spawn(move || {
                                let _ = tx.send(features::check_services());
                            });
                            cx.notify();
                        }),
                    ),
                    (!can_check).then_some(if self.services_rx.is_some() {
                        "Проверка уже идёт"
                    } else {
                        "Сначала подключитесь"
                    }),
                )),
        );
        if self.services_rx.is_some() {
            services = services.child(text("Проверяю сервисы через WARP…", 14., MUTED));
        }
        if !self.services.is_empty() {
            let mut row = div().flex().gap_3();
            for service in &self.services {
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p(px(10.))
                        .rounded(px(10.))
                        .bg(c(SURFACE_HOVER))
                        .child(dot(if service.ok { c(OK) } else { c(ERROR) }, 8.))
                        .child(text(service.name, 14., TEXT))
                        .child(
                            text(service.detail.clone(), 13., MUTED)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap(),
                        ),
                );
            }
            services = services.child(row);
        }

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .relative()
                    .overflow_hidden()
                    .rounded(px(14.))
                    .bg(c(SURFACE))
                    .border_1()
                    .border_color(c(BORDER))
                    .children((!self.settings.renat).then(|| self.glow()))
                    .child(hero),
            )
            .child(tiles)
            .when(!health.is_empty(), |s| s.child(text(health, 13., MUTED)))
            .child(services)
    }

    fn profile_name(&self) -> String {
        if self.settings.custom {
            "свои аргументы winws".into()
        } else {
            self.catalog[self.settings.strategy]
                .name
                .replace("WARP · ", "")
        }
    }

    fn profiles(&self, cx: &mut Context<Self>) -> Div {
        let accent = self.accent();
        let enabled = !self.preview && !self.busy && !self.running && !self.locked();
        let profile_reason = if self.preview {
            Some("В предпросмотре проверки недоступны")
        } else if self.running {
            Some("Сначала отключитесь")
        } else if !enabled {
            Some("Дождитесь окончания текущей операции")
        } else {
            None
        };
        let mut list = div().flex().flex_col().gap_1();
        let rows = self
            .catalog
            .iter()
            .map(|p| (p.name.clone(), false))
            .chain([("Свои аргументы winws".to_owned(), true)]);
        for (index, (name, custom)) in rows.enumerate() {
            let selected = if custom {
                self.settings.custom
            } else {
                !self.settings.custom && self.settings.strategy == index
            };
            let (mark, note) = match (custom, self.results.get(&index)) {
                (true, _) => (c(BORDER), String::new()),
                (_, Some(Some(ms))) => (c(OK), format!("{ms} мс")),
                (_, Some(None)) => (c(ERROR), "не прошёл".into()),
                _ => (c(BORDER), String::new()),
            };
            let stat = if custom {
                None
            } else {
                features::stats(&self.root).get(&index).cloned()
            };
            list = list.child(
                div()
                    .id(("profile", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px(px(12.))
                    .h(px(40.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(if selected {
                        accent
                    } else {
                        transparent_black()
                    })
                    .when(selected, |s| s.bg(c(SURFACE_HOVER)))
                    .when(enabled, |s| {
                        s.cursor_pointer().hover(|s| s.bg(c(SURFACE_HOVER)))
                    })
                    .child(dot(mark, 9.))
                    .child(text(name, 14., TEXT).flex_1().min_w_0())
                    .when_some(stat, |s, stat| {
                        s.child(text(
                            format!(
                                "{}/{} · {} мс",
                                stat.successes,
                                stat.attempts,
                                stat.total_ms / stat.successes.max(1)
                            ),
                            12.,
                            MUTED,
                        ))
                    })
                    .child(text(note, 13., if mark == c(ERROR) { ERROR } else { OK }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !enabled {
                            return;
                        }
                        if custom {
                            this.settings.custom = true;
                        } else {
                            this.settings.custom = false;
                            this.settings.strategy = index;
                        }
                        this.save_settings();
                        cx.notify();
                    })),
            );
        }
        let actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                why(
                    button("test", "Тест профиля", false, enabled, accent),
                    profile_reason,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        let s = this.connect_settings(cx);
                        this.action(Command::Test(s), "Проверяю профиль…");
                        this.page = Page::Home;
                    }
                })),
            )
            .child(
                why(
                    button("best", "Выбрать лучший", false, enabled, accent),
                    profile_reason,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        let s = this.connect_settings(cx);
                        this.action(Command::Best(s), "Сравниваю все профили…");
                        this.page = Page::Home;
                    }
                })),
            )
            .child(
                why(
                    button("auto", "Подобрать автоматически", true, enabled, accent),
                    profile_reason,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        let s = this.connect_settings(cx);
                        this.action(Command::Auto(s), "Подбираю профиль WARP…");
                        this.page = Page::Home;
                    }
                })),
            );
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title(
                "Профили WARP",
                "Как WireGuard доезжает до WARP: порт, эндпоинт, мусорные пакеты. Проверка идёт без TUN — перед тестом отключите другой VPN.",
            ))
            .child(card().child(actions).when(!enabled && !self.preview, |s| {
                s.child(text("Менять профиль можно при отключённом соединении.", 13., MUTED))
            }))
            .child(card().gap_1().p(px(8.)).child(list));
        if self.settings.custom {
            body = body.child(
                card().child(text("Аргументы winws", 15., TEXT)).child(
                    Textarea::new(&self.custom_args)
                        .h(px(90.))
                        .disabled(!enabled),
                ),
            );
        }
        body
    }

    fn lists_page(&self, cx: &mut Context<Self>) -> Div {
        let accent = self.accent();
        let locked = self.busy || self.running || self.locked();
        let mut tabs = div().flex().gap_2();
        for (index, title) in features::LIST_TITLES.iter().enumerate() {
            let active = self.list_tab == index;
            tabs = tabs.child(
                div()
                    .id(("list-tab", index))
                    .px(px(14.))
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .rounded(px(9.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .border_1()
                    .when(active, |s| {
                        s.bg(c(SURFACE_HOVER))
                            .border_color(accent)
                            .text_color(c(TEXT))
                    })
                    .when(!active, |s| s.border_color(c(BORDER)).text_color(c(MUTED)))
                    .child(*title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.list_tab = index;
                        cx.notify();
                    })),
            );
        }
        let editor = match &self.lists {
            Some(editors) => {
                let lines = editors[self.list_tab].read(cx).value().lines().count();
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div().flex_1().min_h(px(260.)).child(
                            Textarea::new(&editors[self.list_tab])
                                .h_full()
                                .disabled(locked),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                why(
                                    button(
                                        "save-lists",
                                        "Сохранить все списки",
                                        true,
                                        !locked,
                                        accent,
                                    ),
                                    locked.then_some(
                                        "Отключите соединение и дождитесь окончания операции",
                                    ),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if !locked {
                                            this.save_lists(window, cx);
                                            cx.notify();
                                        }
                                    },
                                )),
                            )
                            .child(text(format!("Строк: {lines}"), 13., MUTED)),
                    )
            }
            None => div().child(text("Загружаю списки…", 14., MUTED)),
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .size_full()
            .child(section_title(
                "Списки доменов",
                "Один домен на строку, +.example.com включает поддомены. «Напрямую» главнее правил обхода.",
            ))
            .child(
                card()
                    .flex_1()
                    .min_h_0()
                    .child(tabs)
                    .when(locked, |s| {
                        s.child(text(
                            "Для сохранения отключите соединение и дождитесь завершения операции.",
                            13.,
                            WARN,
                        ))
                    })
                    .child(editor)
                    .when(!self.list_message.is_empty(), |s| {
                        s.child(text(self.list_message.clone(), 13., MUTED))
                    }),
            )
    }

    fn switch_row(
        &self,
        id: &'static str,
        title: &str,
        hint: &str,
        on: bool,
        cx: &mut Context<Self>,
        apply: fn(&mut Main, bool),
    ) -> impl IntoElement {
        // В предпросмотре нельзя трогать систему (задачу планировщика автозапуска).
        let enabled = !(self.locked() || self.preview && id == "autostart");
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(text(title.to_owned(), 15., TEXT))
                    .child(text(hint.to_owned(), 13., MUTED)),
            )
            .child(toggle(id, on, enabled, self.accent()).on_click(cx.listener(
                move |this, _, _, cx| {
                    if enabled {
                        apply(this, !on);
                        cx.notify();
                    }
                },
            )))
    }

    fn theme_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.accent();
        let mut options = div()
            .flex()
            .gap_1()
            .p(px(3.))
            .rounded(px(10.))
            .bg(c(SURFACE_HOVER));
        for (key, label) in [
            ("system", "Как в Windows"),
            ("light", "Светлая"),
            ("dark", "Тёмная"),
        ] {
            let active = self.settings.theme == key;
            options = options.child(
                div()
                    .id(SharedString::from(format!("theme-{key}")))
                    .px(px(12.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .when(active, |s| {
                        s.bg(c(SURFACE))
                            .text_color(c(TEXT))
                            .font_weight(FontWeight::SEMIBOLD)
                            .border_1()
                            .border_color(accent)
                    })
                    .when(!active, |s| {
                        s.text_color(c(MUTED)).hover(|s| s.text_color(c(TEXT)))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.settings.theme = key.into();
                        this.save_settings();
                        this.sync_theme(window, cx);
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(text("Тема", 15., TEXT))
                    .child(text(
                        "«Как в Windows» переключается вместе с системой",
                        13.,
                        MUTED,
                    )),
            )
            .child(options)
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> Div {
        let accent = self.accent();
        let files_locked = self.busy || self.running || self.locked();
        let general = card()
            .gap_4()
            .child(text("Общие", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(self.theme_picker(cx))
            .child(self.switch_row(
                "autostart",
                "Запускать вместе с Windows",
                "Тихо, в трее, с автоподключением",
                self.settings.autostart,
                cx,
                |this, value| match platform::set_autostart(value, &this.root) {
                    Ok(()) => {
                        this.settings.autostart = value;
                        this.save_settings();
                    }
                    Err(error) => this.log(format!("Автозапуск не изменён: {error:#}")),
                },
            ))
            .child(self.switch_row(
                "tray",
                "Крестик сворачивает в трей",
                "Выключено — крестик закрывает приложение",
                self.settings.close_to_tray,
                cx,
                |this, value| {
                    this.settings.close_to_tray = value;
                    this.save_settings();
                },
            ))
            .child(self.switch_row(
                "renat",
                "Ренат мод",
                "Радужная кнопка, фон зависит от подключения. Свои картинки: data/renat-on.png и renat-off.png",
                self.settings.renat,
                cx,
                |this, value| {
                    this.settings.renat = value;
                    this.save_settings();
                },
            ));

        let checking = self.update_rx.is_some();
        let updates = card()
            .gap_4()
            .child(text("Обновления", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(self.switch_row(
                "check-updates",
                "Проверять при запуске",
                "Если вышла новая версия, появится окно с предложением установить",
                self.settings.check_updates,
                cx,
                |this, value| {
                    this.settings.check_updates = value;
                    this.save_settings();
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        button("check", "Проверить сейчас", false, !checking, accent).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if !checking {
                                    this.check_updates(true);
                                    cx.notify();
                                }
                            }),
                        ),
                    )
                    .when_some(self.pending_update.clone(), |s, release| {
                        s.child(
                            button(
                                "install",
                                format!("Установить {}", release.tag),
                                true,
                                !checking,
                                accent,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.modal = Some(Modal::Update(release.clone()));
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .child(
                        button("releases", "Релизы на GitHub", false, true, accent).on_click(
                            |_, _, _| {
                                let _ = open_url("https://github.com/kwlnd/warp-unblock/releases");
                            },
                        ),
                    ),
            )
            .child(text(
                if self.update_message.is_empty() {
                    format!("Установлена версия {}", update::VERSION)
                } else {
                    self.update_message.clone()
                },
                13.,
                MUTED,
            ));

        let backup = card()
            .gap_4()
            .child(text("Резервная копия и телефон", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(text(
                "Копия и QR содержат приватный ключ WARP — не публикуйте их.",
                13.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        why(
                            button(
                                "export",
                                "Экспорт настроек и аккаунта",
                                false,
                                !files_locked,
                                accent,
                            ),
                            files_locked
                                .then_some("Отключите соединение и дождитесь окончания операции"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !files_locked {
                                let root = this.root.clone();
                                let settings = this.connect_settings(cx);
                                this.task(move || features::export_backup(&root, settings));
                            }
                        })),
                    )
                    .child(
                        why(
                            button("import", "Импорт копии", false, !files_locked, accent),
                            files_locked
                                .then_some("Отключите соединение и дождитесь окончания операции"),
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            if !files_locked {
                                let root = this.root.clone();
                                this.task(move || features::import_backup(&root));
                            }
                        })),
                    )
                    .child(
                        why(
                            button("qr", "QR для телефона", false, !files_locked, accent),
                            files_locked
                                .then_some("Отключите соединение и дождитесь окончания операции"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if files_locked {
                                return;
                            }
                            match features::qr(&this.root) {
                                Ok(code) => this.modal = Some(Modal::Qr(code)),
                                Err(e) => this.task_message = format!("Ошибка: {e:#}"),
                            }
                            cx.notify();
                        })),
                    ),
            )
            .when(!self.task_message.is_empty(), |s| {
                s.child(text(self.task_message.clone(), 13., MUTED))
            });

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title(
                "Настройки",
                "Поведение приложения, обновления и перенос аккаунта.",
            ))
            .child(general)
            .child(updates)
            .child(backup)
    }

    fn journal_page(&self, cx: &mut Context<Self>) -> Div {
        let accent = self.accent();
        let root = self.root.clone();
        // README лежит в архиве рядом с папкой программы; если его нет — открываем GitHub.
        let readme = self.root.parent().map(|dir| dir.join("README.md"));
        let mut events = div().flex().flex_col().gap_1();
        if self.journal.is_empty() {
            events = events.child(text("Событий пока нет", 14., MUTED));
        }
        for line in self.journal.iter().rev().take(200) {
            events = events.child(text(line.clone(), 13., MUTED));
        }
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title(
                "Журнал",
                "Логи могут содержать домены и IP. Ключи из папки data/ публиковать нельзя.",
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("logs", "Открыть папку логов", false, true, accent).on_click(
                            move |_, _, _| {
                                let path = root.join("data/logs");
                                let _ = fs::create_dir_all(&path);
                                platform::open(&path);
                            },
                        ),
                    )
                    .child(
                        button("readme", "Инструкция", false, true, accent)
                            .on_click(move |_, _, _| match readme.as_ref().filter(|p| p.is_file()) {
                                Some(path) => platform::open(path),
                                None => {
                                    let _ = open_url(
                                        "https://github.com/kwlnd/warp-unblock/blob/main/windows/README.md",
                                    );
                                }
                            }),
                    )
                    .child(
                        button("to-lists", "Списки доменов", false, true, accent).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.page = Page::Lists;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                card()
                    .child(text("События (новые сверху)", 15., TEXT))
                    .child(events),
            )
            .when(!self.engine_log.is_empty(), |s| {
                s.child(
                    card().child(text("Подробный лог mihomo", 15., TEXT)).child(
                        div()
                            .font_family("Consolas")
                            .text_size(px(12.))
                            .text_color(c(MUTED))
                            .child(self.engine_log.clone()),
                    ),
                )
            })
    }

    // ---------- модальные окна ----------

    fn modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let accent = self.accent();
        let modal = self.modal.as_ref()?;
        let body = match modal {
            Modal::Close => card()
                .w(px(420.))
                .child(text("Закрыть приложение?", 20., TEXT).font_weight(FontWeight::BOLD))
                .child(text("Подключение будет остановлено.", 14., MUTED))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(button("stay", "Остаться", false, true, accent).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.modal = None;
                                cx.notify();
                            }),
                        ))
                        .child(
                            button("close", "Остановить и закрыть", true, true, accent)
                                .on_click(cx.listener(|this, _, _, cx| this.quit(cx))),
                        ),
                ),
            Modal::Update(release) => {
                let notes = if release.notes.is_empty() {
                    "Описание изменений не указано.".to_owned()
                } else {
                    plain_notes(&release.notes)
                };
                let mb = release.zip.size as f64 / 1048576.0;
                let install = release.clone();
                let skip = release.tag.clone();
                let page = release.page.clone();
                card()
                    .w(px(560.))
                    .max_h(px(560.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(dot(accent, 10.))
                            .child(
                                text(format!("Доступна версия {}", release.tag), 20., TEXT)
                                    .font_weight(FontWeight::BOLD),
                            ),
                    )
                    .child(text(
                        format!(
                            "У вас {}. Архив {:.1} МБ, проверяется по SHA-256. Ваши настройки, аккаунт и списки доменов сохранятся.",
                            update::VERSION,
                            mb
                        ),
                        13.,
                        MUTED,
                    ))
                    .child(
                        div()
                            .id("notes")
                            .flex_1()
                            .min_h(px(80.))
                            .max_h(px(300.))
                            .overflow_y_scroll()
                            .p(px(12.))
                            .rounded(px(10.))
                            .bg(c(BACKGROUND))
                            .text_size(px(13.))
                            .text_color(c(TEXT))
                            .child(notes),
                    )
                    .when(self.running || self.busy, |s| {
                        s.child(text(
                            "Соединение будет отключено на время установки, затем программа перезапустится.",
                            13.,
                            WARN,
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .child(button("skip", "Пропустить версию", false, true, accent).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.settings.skip_version = skip.clone();
                                    this.save_settings();
                                    this.modal = None;
                                    cx.notify();
                                }),
                            ))
                            .child(button("page", "Что нового", false, true, accent).on_click(
                                move |_, _, _| {
                                    let _ = open_url(&page);
                                },
                            ))
                            .child(button("later", "Позже", false, true, accent).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.modal = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                why(button("update-now", "Обновить", true, !self.preview, accent), self.preview.then_some("В предпросмотре установка недоступна"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.preview {
                                            this.begin_install(install.clone());
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
            }
            Modal::Installing(label, fraction) => card()
                .w(px(440.))
                .child(text("Обновление", 20., TEXT).font_weight(FontWeight::BOLD))
                .child(text(label.clone(), 14., MUTED))
                .child(progress_bar(*fraction, 10., accent))
                .child(text(
                    "Не закрывайте программу — она перезапустится сама.",
                    13.,
                    MUTED,
                )),
            Modal::Qr(code) => {
                let n = code.width();
                let cell = (320.0 / (n + 8) as f32).floor().max(2.0);
                let mut grid = div()
                    .flex()
                    .flex_col()
                    .p(px(cell * 4.))
                    .bg(gpui_kit::white());
                for y in 0..n {
                    let mut row = div().flex();
                    for x in 0..n {
                        let dark = code[(x, y)] == qrcode::Color::Dark;
                        row =
                            row.child(div().size(px(cell)).when(dark, |s| s.bg(gpui_kit::black())));
                    }
                    grid = grid.child(row);
                }
                card()
                    .items_center()
                    .child(
                        text("Профиль WARP для телефона", 20., TEXT).font_weight(FontWeight::BOLD),
                    )
                    .child(text(
                        "Содержит приватный ключ. Импортируйте в приложении Android: Профили → QR.",
                        13.,
                        MUTED,
                    ))
                    .child(grid)
                    .child(button("qr-close", "Закрыть", false, true, accent).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.modal = None;
                            cx.notify();
                        }),
                    ))
            }
        };
        Some(
            div()
                .id("modal-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., if light() { 0.35 } else { 0.6 }))
                .occlude()
                .child(body.shadow_lg().with_animation(
                    "modal-in",
                    Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t).mt(px((1.0 - t) * 12.)),
                ))
                .into_any_element(),
        )
    }

    fn toasts_view(&self) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let mut column = div()
            .absolute()
            .bottom(px(18.))
            .right(px(18.))
            .w(px(340.))
            .flex()
            .flex_col()
            .gap_2();
        for toast in &self.toasts {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(12.))
                    .bg(c(SURFACE))
                    .border_1()
                    .border_color(c(BORDER))
                    .shadow_lg()
                    .child(dot(c(toast.color), 9.))
                    .child(text(toast.text.clone(), 13., TEXT).flex_1().min_w_0())
                    .with_animation(
                        ("toast", toast.id),
                        Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                        |el, t| el.opacity(t).ml(px((1.0 - t) * 24.)),
                    ),
            );
        }
        Some(column.into_any_element())
    }

    fn backdrop(&self) -> Option<AnyElement> {
        if !self.settings.renat {
            return None;
        }
        let candidates: &[&str] = if self.running { &RENAT_ON } else { &RENAT_OFF };
        let path = candidates
            .iter()
            .map(|name| self.root.join(name))
            .find(|path| path.is_file())?;
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    img(path)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(0.7),
                )
                .into_any_element(),
        )
    }
}

impl Render for Main {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.page {
            Page::Home => self.home(cx),
            Page::Profiles => self.profiles(cx),
            Page::Lists => self.lists_page(cx),
            Page::Settings => self.settings_page(cx),
            Page::Journal => self.journal_page(cx),
        };
        // Страница появляется с коротким сдвигом снизу; id меняется вместе со страницей.
        let page = page.with_animation(
            SharedString::from(format!("page-{:?}", self.page)),
            Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
            |el, t| el.opacity(t).mt(px((1.0 - t) * 10.)),
        );
        let content = if self.page == Page::Lists {
            div().flex_1().min_w_0().h_full().p(px(28.)).child(page)
        } else {
            div().flex_1().min_w_0().h_full().child(
                div()
                    .id("page-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(28.))
                    .child(page),
            )
        };
        let modal = self.modal(cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(FONT)
            .text_color(c(TEXT))
            .bg(c(BACKGROUND))
            .children(self.backdrop())
            .child(self.title_bar(window))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(content),
            )
            .children(modal)
            .children(self.toasts_view())
    }
}

/// Markdown релиза в простой текст: без решёток заголовков и звёздочек выделения.
fn plain_notes(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            line.trim_start_matches('#')
                .trim_start()
                .replace("**", "")
                .replace('`', "")
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

fn open_url(url: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .arg(url)
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
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
