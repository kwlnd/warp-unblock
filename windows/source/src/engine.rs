use crate::platform::{self, Job, Process};
use crate::{opera, strategies};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

const STAGES: usize = 5;

/// Сколько ждать, пока mihomo поднимет API: это запуск программы, а не ответ WARP.
const READY_TIMEOUT: Duration = Duration::from_secs(3);
/// Ответ WARP при подключении и «Тесте профиля»: одна полная попытка рукопожатия WireGuard.
pub const CONNECT_BUDGET: Duration = Duration::from_secs(5);
/// Ответ WARP при подборе: рабочий профиль отвечает за 0,2–0,9 с.
const PICK_BUDGET_MIN: Duration = Duration::from_millis(1500);

/// Бюджет на ответ WARP при подборе: 1,5 с, а на медленной сети — 2,5× от лучшей средней
/// задержки из истории успешных проверок, но не больше полной попытки рукопожатия.
pub fn pick_budget(root: &Path) -> Duration {
    let fastest = crate::features::stats(root)
        .values()
        .filter(|s| s.successes > 0)
        .map(|s| s.total_ms / s.successes)
        .min();
    match fastest {
        Some(ms) => Duration::from_millis(ms * 5 / 2).clamp(PICK_BUDGET_MIN, CONNECT_BUDGET),
        None => PICK_BUDGET_MIN,
    }
}

/// Сначала ждёт API mihomo (до [`READY_TIMEOUT`]), затем отдельно отмеряет `budget` на
/// ответ WARP через delay-тест ядра.
fn probe_delay(
    port: u16,
    ready: Duration,
    budget: Duration,
    check: impl Fn() -> Result<()>,
) -> Result<u64> {
    let client = reqwest::blocking::Client::builder().no_proxy().build()?;
    let ready_deadline = Instant::now() + ready;
    loop {
        check()?;
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!(
                "mihomo не запустился за {} с; см. mihomo.log",
                ready.as_secs_f32()
            );
        }
        let up = client
            .get(format!("http://127.0.0.1:{port}/version"))
            .timeout(remaining.min(Duration::from_millis(300)))
            .send()
            .is_ok_and(|r| r.status().is_success());
        if up {
            break;
        }
        thread::sleep(Duration::from_millis(50).min(remaining));
    }
    let deadline = Instant::now() + budget;
    loop {
        check()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let kernel_ms = remaining.as_millis().saturating_sub(50).max(1).to_string();
        let response = client
            .get(format!("http://127.0.0.1:{port}/proxies/WARP/delay"))
            .timeout(remaining)
            .query(&[
                ("timeout", kernel_ms.as_str()),
                ("url", "https://www.gstatic.com/generate_204"),
            ])
            .send();
        if let Ok(response) = response {
            if response.status().is_success() {
                if let Ok(value) = response.json::<serde_json::Value>() {
                    if Instant::now() < deadline {
                        if let Some(delay) = value["delay"].as_u64().filter(|d| *d > 0) {
                            return Ok(delay);
                        }
                    }
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(Duration::from_millis(50).min(remaining));
    }
    bail!(
        "WARP не ответил за {} с. Попробуйте другую стратегию.",
        budget.as_secs_f32()
    )
}

/// Пишет data/mihomo/warp.yaml для профиля без PowerShell, если аккаунт и reserved уже есть.
/// false — нужен warp.ps1 (регистрация аккаунта или первое получение reserved).
fn write_profile(root: &Path, profile: &strategies::Strategy) -> Result<bool> {
    write_profile_with(root, profile, true)
}

/// `reserved_required = false` — писать профиль и без reserved (WARP обычно работает и так).
fn write_profile_with(
    root: &Path,
    profile: &strategies::Strategy,
    reserved_required: bool,
) -> Result<bool> {
    let warp = root.join("data/warp");
    let Ok(conf) = fs::read_to_string(warp.join("wgcf-profile.conf")) else {
        return Ok(false);
    };
    if !warp.join("wgcf-account.toml").is_file() {
        return Ok(false);
    }
    let field = |name: &str| -> Option<String> {
        conf.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let (Some(private), Some(public), Some(addresses)) =
        (field("PrivateKey"), field("PublicKey"), field("Address"))
    else {
        return Ok(false);
    };
    let pick = |v6: bool| {
        addresses
            .split(',')
            .map(|a| a.trim().split('/').next().unwrap_or("").to_owned())
            .find(|a| a.contains(':') == v6 && !a.is_empty())
    };
    let (Some(ipv4), Some(ipv6)) = (pick(false), pick(true)) else {
        return Ok(false);
    };
    let bytes: Vec<u8> = fs::read_to_string(warp.join("reserved.txt"))
        .ok()
        .and_then(|text| serde_json::from_str(text.trim()).ok())
        .unwrap_or_default();
    if bytes.len() != 3 && reserved_required {
        return Ok(false);
    }
    let mut lines = vec![
        "# Сгенерировано WarpUnblock из data/warp/wgcf-profile.conf, пересоздаётся при каждом запуске."
            .to_owned(),
        "proxies:".into(),
        "  - name: WARP-WG".into(),
        "    type: wireguard".into(),
        format!("    server: {}", profile.endpoint),
        format!("    port: {}", profile.port),
        format!("    ip: {ipv4}"),
        format!("    ipv6: {ipv6}"),
        format!("    private-key: {private}"),
        format!("    public-key: {public}"),
        "    mtu: 1280".into(),
        "    udp: true".into(),
        "    amnezia-wg-option:".into(),
        format!("      jc: {}", profile.jc),
        format!("      jmin: {}", profile.jmin),
        format!("      jmax: {}", profile.jmax),
    ];
    if bytes.len() == 3 {
        lines.push(format!(
            "    reserved: [{}, {}, {}]",
            bytes[0], bytes[1], bytes[2]
        ));
    }
    lines.push(String::new());
    fs::create_dir_all(root.join("data/mihomo"))?;
    fs::write(root.join("data/mihomo/warp.yaml"), lines.join("\n"))?;
    Ok(true)
}

/// Значение ключа из wgcf-account.toml: `key = 'value'`.
#[cfg(unix)]
fn toml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches(['\'', '"']).to_owned())
    })
}

/// 3 байта client_id (reserved) из API Cloudflare: с ними WARP отвечает надёжнее.
#[cfg(unix)]
fn fetch_reserved(account: &str) -> Result<String> {
    use base64::Engine;
    let device = toml_value(account, "device_id").context("В аккаунте нет device_id")?;
    let token = toml_value(account, "access_token").context("В аккаунте нет access_token")?;
    let value: serde_json::Value = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .get(format!(
            "https://api.cloudflareclient.com/v0a1922/reg/{device}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("CF-Client-Version", "a-6.3-1922")
        .header("User-Agent", "okhttp/3.12.1")
        .send()?
        .error_for_status()?
        .json()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(
        value["config"]["client_id"]
            .as_str()
            .context("Нет client_id")?,
    )?;
    if bytes.len() < 3 {
        bail!("client_id короче трёх байт");
    }
    Ok(format!("[{}, {}, {}]", bytes[0], bytes[1], bytes[2]))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub strategy: usize,
    pub custom_args: String,
    pub custom: bool,
    /// Запускать вместе с Windows: тихо, в трей, с автоподключением.
    #[serde(default)]
    pub autostart: bool,
    /// Крестик сворачивает в трей, а не закрывает приложение.
    #[serde(default = "yes")]
    pub close_to_tray: bool,
    /// «Ренат мод»: радужная кнопка и картинка на фоне (data/renat.png).
    #[serde(default)]
    pub renat: bool,
    /// Проверять обновления при запуске и предлагать установку.
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Версия, которую пользователь попросил не предлагать.
    #[serde(default)]
    pub skip_version: String,
    /// Тема интерфейса: system, light или dark.
    #[serde(default = "system_theme")]
    pub theme: String,
    /// WARP перестал отвечать — подобрать другой профиль и переподключиться самому.
    #[serde(default = "yes")]
    pub auto_switch: bool,
    /// Рабочий адрес WARP из пула каталога («ip:порт»), найденный подбором.
    #[serde(default)]
    pub endpoint: String,
}
fn system_theme() -> String {
    "system".into()
}
fn yes() -> bool {
    true
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            strategy: 0,
            custom_args: String::new(),
            custom: false,
            autostart: false,
            close_to_tray: true,
            renat: false,
            check_updates: true,
            skip_version: String::new(),
            theme: system_theme(),
            auto_switch: true,
            endpoint: String::new(),
        }
    }
}
impl Settings {
    pub fn load(root: &Path) -> Self {
        fs::read(root.join("data/settings.json"))
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        fs::create_dir_all(root.join("data"))?;
        fs::write(
            root.join("data/settings.json"),
            serde_json::to_vec_pretty(self)?,
        )?;
        Ok(())
    }
}

pub enum Command {
    Start(Settings),
    Test(Settings),
    Auto(Settings),
    Best(Settings),
    Check,
    Stop,
    Shutdown,
}
pub enum Event {
    Log(String),
    State {
        busy: bool,
        running: bool,
        status: String,
    },
    Check(usize, String, bool),
    /// Подбор выбрал профиль и адрес из пула.
    Selected(usize, String),
    /// Этап одной проверки/подключения: (сделано, всего, название).
    Stage(usize, usize, String),
    /// Автоподбор: (пройдено, всего, не прошло). None — подбор не идёт.
    Batch(Option<(usize, usize, usize)>),
}

pub struct Worker {
    pub tx: mpsc::Sender<Command>,
    pub rx: mpsc::Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(root: PathBuf) -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let handle = thread::spawn(move || {
            let mut engine = Engine {
                root,
                events,
                cancel: stop,
                job: None,
                children: Vec::new(),
                opera: None,
                running: false,
            };
            let mut last_health = Instant::now();
            // Сколько проверок подряд WARP не ответил.
            let mut failures = 0u32;
            loop {
                match commands.recv_timeout(Duration::from_secs(1)) {
                    Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Command::Stop) => {
                        engine.stop();
                        engine.state(false, "Отключено");
                    }
                    Ok(Command::Start(settings))
                    | Ok(Command::Test(settings))
                    | Ok(Command::Auto(settings))
                    | Ok(Command::Best(settings))
                        if engine.running
                            || crate::features::FILES_BUSY.load(Ordering::Acquire) =>
                    {
                        let _ = settings;
                        engine.log("Завершите текущую операцию или остановите подключение.");
                    }
                    Ok(command) => {
                        engine.cancel.store(false, Ordering::Relaxed);
                        engine.state(true, "Проверяю…");
                        let result = match command {
                            Command::Start(settings) => engine.start(settings, false),
                            Command::Test(settings) => engine.start(settings, true),
                            Command::Auto(settings) => engine.auto_select(settings, false),
                            Command::Best(settings) => engine.auto_select(settings, true),
                            Command::Check => engine.check(),
                            _ => unreachable!(),
                        };
                        last_health = Instant::now();
                        failures = 0;
                        if let Err(error) = result {
                            engine.log(&format!("Ошибка: {error:#}"));
                            // A failed health check must not tear down a working TUN.
                            if engine.cancel.load(Ordering::Relaxed) {
                                engine.stop();
                            }
                            engine.state(
                                false,
                                if engine.running {
                                    "Требуется внимание"
                                } else {
                                    "Отключено · см. журнал"
                                },
                            );
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if engine.running
                            && engine
                                .children
                                .iter()
                                .any(|child| !matches!(child.exit_code(), Ok(None)))
                        {
                            engine.log("Один из компонентов завершился. Подключение остановлено; подробности в логах.");
                            engine.stop();
                            engine.state(false, "Компонент завершился");
                        } else if engine.running && last_health.elapsed() >= Duration::from_secs(30)
                        {
                            last_health = Instant::now();
                            if engine.refresh_health() {
                                failures = 0;
                            } else {
                                failures += 1;
                                // Один промах бывает от сети; второй проверяем уже через 10 с.
                                last_health -= Duration::from_secs(20);
                                let settings = Settings::load(&engine.root);
                                if failures >= 2 && settings.auto_switch && !settings.custom {
                                    failures = 0;
                                    engine.cancel.store(false, Ordering::Relaxed);
                                    if let Err(error) = engine.switch_profile(settings) {
                                        engine.log(&format!("Автопереключение: {error:#}"));
                                        engine.stop();
                                        engine.state(false, "Отключено · см. журнал");
                                    }
                                    last_health = Instant::now();
                                }
                            }
                        }
                    }
                }
            }
            engine.stop();
        });
        Self {
            tx,
            rx,
            cancel,
            handle: Some(handle),
        }
    }
    pub fn send(&self, command: Command) {
        self.handle().send(command);
    }
    /// Дешёвая копия для потоков трея: отправка команд без владения воркером.
    pub fn handle(&self) -> Handle {
        Handle {
            tx: self.tx.clone(),
            cancel: self.cancel.clone(),
        }
    }
}
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Command>,
    cancel: Arc<AtomicBool>,
}
impl Handle {
    pub fn send(&self, command: Command) {
        if crate::features::FILES_BUSY.load(Ordering::Acquire)
            && matches!(
                command,
                Command::Start(_) | Command::Test(_) | Command::Auto(_) | Command::Best(_)
            )
        {
            return;
        }
        if matches!(command, Command::Stop | Command::Shutdown) {
            self.cancel.store(true, Ordering::Relaxed);
        }
        let _ = self.tx.send(command);
    }
}
// Keep all process ownership inside the worker, including shutdown.
impl Drop for Worker {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Engine {
    root: PathBuf,
    events: mpsc::Sender<Event>,
    cancel: Arc<AtomicBool>,
    job: Option<Job>,
    children: Vec<Process>,
    opera: Option<Process>,
    running: bool,
}
impl Engine {
    fn log(&self, text: &str) {
        let _ = self.events.send(Event::Log(text.to_owned()));
        if fs::create_dir_all(self.root.join("data/logs")).is_ok() {
            if let Ok(mut file) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.join("data/logs/launcher.log"))
            {
                let _ = writeln!(file, "{text}");
            }
        }
    }
    fn state(&self, busy: bool, status: &str) {
        let _ = self.events.send(Event::State {
            busy,
            running: self.running,
            status: status.to_owned(),
        });
    }
    fn stage(&self, done: usize, text: &str) {
        let _ = self
            .events
            .send(Event::Stage(done, STAGES, text.to_owned()));
    }
    fn item(&self, index: usize, text: &str, ok: bool) {
        let _ = self.events.send(Event::Check(index, text.to_owned(), ok));
    }
    fn cancelled(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("Операция отменена");
        }
        Ok(())
    }
    fn pause(&self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.cancelled()?;
            thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        self.cancelled()
    }
    fn job(&mut self) -> Result<&Job> {
        if self.job.is_none() {
            self.job = Some(Job::new()?);
        }
        Ok(self.job.as_ref().unwrap())
    }
    fn launch(&mut self, relative: &str, args: &[String], log_name: &str) -> Result<Process> {
        let cwd = self.root.clone();
        self.launch_at(relative, args, &cwd, log_name)
    }
    fn launch_at(
        &mut self,
        relative: &str,
        args: &[String],
        cwd: &Path,
        log_name: &str,
    ) -> Result<Process> {
        self.job()?;
        Process::spawn(
            self.job.as_ref().unwrap(),
            &self.root.join(relative),
            args,
            cwd,
            &self.root.join(format!("data/logs/{log_name}")),
            &self.root.join("app/config/lists"),
            needs_root(relative, args),
        )
    }
    fn wait(&self, process: Process, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            self.cancelled()?;
            if let Some(code) = process.exit_code()? {
                if code != 0 {
                    bail!("Компонент завершился с кодом {code}. Подробности в data/logs.");
                }
                return Ok(());
            }
            if start.elapsed() > timeout {
                bail!("Истекло время ожидания компонента");
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(windows)]
    fn script(&mut self, script: &str, extra: &[&str], log: &str, timeout: Duration) -> Result<()> {
        self.log(&format!("{script}…"));
        self.job()?;
        let args = [
            "-NoProfile".to_string(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            self.root
                .join(format!("app/scripts/{script}"))
                .display()
                .to_string(),
        ]
        .into_iter()
        .chain(extra.iter().map(|s| s.to_string()))
        .collect::<Vec<_>>();
        let process = Process::spawn(
            self.job.as_ref().unwrap(),
            &platform::powershell(),
            &args,
            &self.root,
            &self.root.join(format!("data/logs/{log}")),
            &self.root.join("app/config/lists"),
            false,
        )?;
        self.wait(process, timeout)
    }
    fn prepare(&mut self) -> Result<()> {
        self.cancelled()?;
        fs::create_dir_all(self.root.join("data/logs"))?;
        fs::create_dir_all(self.root.join("data/mihomo"))?;
        self.job()?;
        let required = platform::required_files();
        #[cfg(windows)]
        if required.iter().any(|p| !self.root.join(p).is_file()) {
            self.log("Не хватает зависимостей. Скачиваю официальные сборки…");
            self.script("setup.ps1", &[], "setup.log", Duration::from_secs(600))?;
        }
        for file in required {
            if !self.root.join(file).is_file() {
                bail!("Нет файла {file}. Переустановите программу: ядра идут в комплекте.");
            }
        }
        self.item(0, "Все компоненты на месте", true);
        Ok(())
    }
    fn start(&mut self, settings: Settings, test: bool) -> Result<()> {
        if !platform::is_admin() {
            bail!("Подключение требует прав администратора");
        }
        let result = self.start_inner(&settings, test, CONNECT_BUDGET, true);
        if result.is_err() || test {
            self.stop();
        }
        if result.is_ok() {
            self.state(
                false,
                if test {
                    "Проверка стратегии завершена"
                } else {
                    "Подключено"
                },
            );
        }
        result.map(|_| ())
    }
    /// `budget` — сколько ждать ответа WARP; `validate` — прогнать `mihomo -t` (при подборе
    /// конфиг не меняется между профилями, достаточно проверить его один раз).
    fn start_inner(
        &mut self,
        settings: &Settings,
        test: bool,
        budget: Duration,
        validate: bool,
    ) -> Result<u64> {
        let port = if test { 9091 } else { 9090 };
        let ports = if test {
            vec![port, 5631]
        } else {
            vec![port, 5630, opera::PORT, opera::UNDERLAY_PORT]
        };
        for port in ports {
            let _probe = TcpListener::bind(("127.0.0.1", port)).with_context(|| {
                format!("Порт {port} занят. Отключите другой прокси перед запуском")
            })?;
        }
        self.stage(0, "Подготовка компонентов");
        self.prepare()?;
        let catalog = strategies::load(&self.root)?;
        if settings.strategy >= catalog.len() {
            bail!("Неизвестный профиль WARP");
        }
        let profile = &catalog.resolve(
            settings.strategy,
            catalog.endpoint_index(&settings.endpoint),
        );
        self.log(&format!(
            "Профиль: {} · {}:{}",
            profile.name, profile.endpoint, profile.port
        ));
        let args = strategy_args(settings, &self.root)?;
        self.stage(1, "Запуск zapret");
        if !args.is_empty() {
            self.log("Запускаю zapret…");
            let process = self.launch(platform::ZAPRET, &args, "zapret.log")?;
            self.pause(Duration::from_secs(1))?;
            if let Some(code) = process.exit_code()? {
                bail!("winws не запустился (код {code}), см. zapret.log");
            }
            self.children.push(process);
        } else {
            self.log("Zapret отключён; использую WARP с маскировкой handshake.");
        }
        self.stage(2, "Настройка профиля WARP");
        self.ensure_profile(profile)?;
        self.item(1, "Профиль WARP готов", true);
        self.finish_start(test, port, budget, validate, settings)
    }

    /// Нет аккаунта или профиля WireGuard — получаем их у API Cloudflare с теми же обходами DPI,
    /// что и при регистрации: у части провайдеров API без них не отвечает (TLS-таймаут).
    fn ensure_profile(&mut self, profile: &strategies::Strategy) -> Result<()> {
        if self.root.join("data/warp/wgcf-account.toml").is_file()
            && write_profile(&self.root, profile)?
        {
            return Ok(());
        }
        self.register_account(profile)
    }

    /// Регистрация аккаунта и получение профиля WireGuard: сначала напрямую, затем по очереди
    /// способами обхода DPI из общего `registration.json`. TCP-обход действует только на это время.
    fn register_account(&mut self, profile: &strategies::Strategy) -> Result<()> {
        let methods = crate::registration::load(&self.root)?;
        let attempts: Vec<(String, Option<Vec<String>>)> =
            std::iter::once(("напрямую".to_string(), None))
                .chain(
                    methods
                        .into_iter()
                        .filter_map(|m| m.winws.map(|args| (m.name, Some(args)))),
                )
                .collect();
        let total = attempts.len();
        for (index, (name, method)) in attempts.into_iter().enumerate() {
            self.cancelled()?;
            self.state(
                true,
                &format!("Получаю профиль WARP · {}/{total} · {name}", index + 1),
            );
            let bypass = match &method {
                Some(args) => {
                    let args = crate::registration::winws_args(args, &self.root);
                    let process = self.launch(platform::ZAPRET, &args, "registration.log")?;
                    self.pause(Duration::from_millis(500))?;
                    Some(process)
                }
                None => None,
            };
            let result = self.profile_script(profile, Duration::from_secs(30));
            drop(bypass);
            if result.is_ok()
                && self.root.join("data/warp/wgcf-account.toml").is_file()
                && self.root.join("data/warp/wgcf-profile.conf").is_file()
            {
                self.log(&format!("Аккаунт и профиль WARP получены ({name})."));
                return Ok(());
            }
            self.log(&format!("Регистрация не прошла ({name})."));
        }
        bail!(
            "Не удалось зарегистрировать аккаунт WARP ни одним способом.              Импортируйте копию аккаунта или профиль с другого устройства."
        )
    }

    #[cfg(windows)]
    fn profile_script(&mut self, profile: &strategies::Strategy, timeout: Duration) -> Result<()> {
        let parameters = [
            "-Action".to_string(),
            "register".into(),
            "-EndpointIp".into(),
            profile.endpoint.clone(),
            "-EndpointPort".into(),
            profile.port.to_string(),
            "-Jc".into(),
            profile.jc.to_string(),
            "-Jmin".into(),
            profile.jmin.to_string(),
            "-Jmax".into(),
            profile.jmax.to_string(),
        ];
        self.script(
            "warp.ps1",
            &parameters.iter().map(String::as_str).collect::<Vec<_>>(),
            "profile.log",
            timeout,
        )
    }

    /// Linux: wgcf и запрос reserved делаем сами, вместо PowerShell-скрипта.
    #[cfg(unix)]
    fn profile_script(&mut self, profile: &strategies::Strategy, timeout: Duration) -> Result<()> {
        let warp = self.root.join("data/warp");
        fs::create_dir_all(&warp)?;
        let steps: [(&str, &[&str]); 2] = [
            ("wgcf-account.toml", &["register", "--accept-tos"]),
            ("wgcf-profile.conf", &["generate"]),
        ];
        for (file, args) in steps {
            if warp.join(file).is_file() {
                continue;
            }
            self.log(&format!("wgcf {}…", args[0]));
            let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
            let process = self.launch_at(platform::WGCF, &args, &warp, "profile.log")?;
            self.wait(process, timeout)?;
            if !warp.join(file).is_file() {
                bail!("wgcf не создал {file}; см. profile.log");
            }
        }
        if !warp.join("reserved.txt").is_file() {
            let account = fs::read_to_string(warp.join("wgcf-account.toml")).unwrap_or_default();
            match fetch_reserved(&account) {
                Ok(reserved) => fs::write(warp.join("reserved.txt"), reserved)?,
                Err(error) => self.log(&format!(
                    "reserved получить не удалось, работаю без него: {error:#}"
                )),
            }
        }
        if !write_profile_with(&self.root, profile, false)? {
            bail!("Не удалось собрать профиль WARP из wgcf-profile.conf");
        }
        Ok(())
    }

    fn finish_start(
        &mut self,
        test: bool,
        port: u16,
        budget: Duration,
        validate: bool,
        settings: &Settings,
    ) -> Result<u64> {
        let config = if test {
            "app/config/warp-test.yaml"
        } else {
            "app/config/config.yaml"
        };
        let args = vec![
            "-d".into(),
            self.root.join("data/mihomo").display().to_string(),
            "-f".into(),
            self.root.join(config).display().to_string(),
        ];
        self.stage(3, "Проверка конфигурации");
        if validate {
            let mut validation = vec!["-t".to_string()];
            validation.extend(args.clone());
            self.log("Проверяю конфигурацию mihomo…");
            let process = self.launch(platform::MIHOMO, &validation, "validation.log")?;
            self.wait(process, Duration::from_secs(5))?;
        }
        self.log(if test {
            "Проверяю стратегию без TUN…"
        } else {
            "Поднимаю TUN. Обычный трафик — DIRECT."
        });
        self.stage(4, "Запуск mihomo");
        let process = self.launch(platform::MIHOMO, &args, "mihomo.log")?;
        self.children.push(process);
        self.running = !test;
        self.state(true, "Проверяю тоннель WARP…");
        self.stage(4, "Проверка тоннеля WARP");
        let delay = self.delay(port, budget)?;
        self.stage(STAGES, "Готово");
        self.item(2, &format!("WARP отвечает · {delay} мс"), true);
        self.log(&format!("WARP доступен, задержка {delay} мс."));
        if test {
            self.log("Стратегия работает. Сетевые маршруты не менялись.");
        } else {
            self.state(true, "Подключаю Opera EU для нейросетей…");
            self.start_opera()?;
        }
        settings.save(&self.root)?;
        Ok(delay)
    }

    /// Подбор: пары «профиль × адрес» в порядке [`strategies::search_order`] — сначала все адреса
    /// текущего профиля, потом остальные профили. `best` — у каждого профиля найти первый рабочий
    /// адрес и выбрать лучший по истории; иначе остановиться на первой рабочей паре.
    fn auto_select(&mut self, mut settings: Settings, best: bool) -> Result<()> {
        if !platform::is_admin() {
            bail!("Подбор требует прав администратора");
        }
        self.prepare()?;
        let catalog = strategies::load(&self.root)?;
        let original = settings.clone();
        let order = strategies::search_order(
            &catalog,
            settings.strategy,
            catalog.endpoint_index(&settings.endpoint),
        );
        let total = order.len();
        // Аккаунт и профиль — один раз до перебора: без них не пройдёт ни одна пара, а повторять
        // регистрацию со всеми обходами на каждой паре — это часы.
        self.ensure_profile(&catalog.resolve(order[0].0, order[0].1))?;
        let mut results = Vec::new();
        let mut failed = 0usize;
        let mut candidates = Vec::new();
        let mut found = std::collections::BTreeMap::new();
        settings.custom = false;
        let budget = pick_budget(&self.root);
        self.log(&format!(
            "Подбор: {} профилей × {} адресов, на ответ WARP {:.1} с.",
            catalog.len(),
            catalog.endpoints.len(),
            budget.as_secs_f32()
        ));
        for (attempt, (index, endpoint)) in order.into_iter().enumerate() {
            self.cancelled()?;
            // В режиме «лучший» у профиля достаточно одного рабочего адреса.
            if best && found.contains_key(&index) {
                continue;
            }
            settings.strategy = index;
            settings.endpoint = catalog.endpoints[endpoint].clone();
            let _ = self
                .events
                .send(Event::Batch(Some((attempt, total, failed))));
            let name = format!("{} · {}", catalog.profiles[index].name, settings.endpoint);
            self.state(
                true,
                &format!("Подбор WARP · {}/{} · {name}", attempt + 1, total),
            );
            let result = self.start_inner(&settings, true, budget, attempt == 0);
            // A fresh WireGuard instance for each trial avoids testing an old handshake.
            self.stop();
            let (delay, error) = match &result {
                Ok(delay) => (Some(*delay), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
            self.cancelled()?;
            crate::features::record(&self.root, index, delay)?;
            if let Some(ms) = delay {
                candidates.push((index, ms));
                found.insert(index, settings.endpoint.clone());
            }
            results.push(serde_json::json!({"name": catalog.profiles[index].name, "index": index, "endpoint": settings.endpoint, "delay_ms": delay, "error": error}));
            fs::write(
                self.root.join("data/strategy-results.json"),
                serde_json::to_vec_pretty(&results)?,
            )?;
            if result.is_err() {
                failed += 1;
            }
            let _ = self
                .events
                .send(Event::Batch(Some((attempt + 1, total, failed))));
            if result.is_ok() && !best {
                settings.save(&self.root)?;
                let _ = self
                    .events
                    .send(Event::Selected(index, settings.endpoint.clone()));
                self.log(&format!("Рабочая пара сохранена: {name}."));
                self.state(false, "Профиль WARP подобран");
                return Ok(());
            }
            self.log(&format!(
                "{}: {name}",
                if result.is_ok() {
                    "Прошёл"
                } else {
                    "Не прошёл"
                }
            ));
            self.cancelled()?;
        }
        if best {
            if let Some(index) = crate::features::best(&self.root, &candidates) {
                settings.strategy = index;
                settings.endpoint = found[&index].clone();
                settings.save(&self.root)?;
                let _ = self
                    .events
                    .send(Event::Selected(index, settings.endpoint.clone()));
                self.state(false, "Лучший профиль сохранён");
                return Ok(());
            }
        }
        original.save(&self.root)?;
        bail!("Ни один профиль не прошёл проверку. См. data/strategy-results.json")
    }
    fn delay(&self, port: u16, timeout: Duration) -> Result<u64> {
        let result = probe_delay(port, READY_TIMEOUT, timeout, || {
            self.cancelled()?;
            if self
                .children
                .iter()
                .any(|p| !matches!(p.exit_code(), Ok(None)))
            {
                bail!("Компонент завершился во время проверки; см. логи");
            }
            Ok(())
        });
        if result.is_err() {
            self.item(2, "WARP не отвечает", false);
        }
        result
    }
    fn check(&mut self) -> Result<()> {
        if !self.running {
            for (index, (label, ok)) in [
                (
                    "Компоненты на месте",
                    platform::required_files()
                        .iter()
                        .all(|p| self.root.join(p).is_file()),
                ),
                (
                    "Профиль WARP сохранён",
                    self.root.join("data/mihomo/warp.yaml").is_file(),
                ),
                ("Тоннель выключен", false),
                ("Opera EU выключена", false),
            ]
            .iter()
            .enumerate()
            {
                self.item(index, label, *ok);
            }
            self.state(false, "Отключено");
            return Ok(());
        }
        let delay = self.delay(9090, CONNECT_BUDGET)?;
        self.item(2, &format!("WARP отвечает · {delay} мс"), true);
        self.log(&format!("Проверка WARP: {delay} мс."));
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:5630")?)
            .timeout(Duration::from_secs(12))
            .build()?;
        let response = client
            .get("https://discord.com/api/v10/gateway")
            .send()?
            .error_for_status()?;
        let value: serde_json::Value = response.json()?;
        if value["url"].as_str().is_none() {
            bail!("Discord API вернул неожиданный ответ");
        }
        self.log("Discord API доступен через правила маршрутизации.");
        self.state(false, "Подключено · проверка пройдена");
        Ok(())
    }
    fn stop(&mut self) {
        self.opera.take();
        while let Some(mut child) = self.children.pop() {
            child.stop();
        }
        self.job.take();
        self.running = false;
        self.item(2, "Тоннель выключен", false);
        self.item(3, "Opera EU выключена", false);
    }

    /// WARP перестал отвечать: подбор с того же профиля на следующем адресе пула (сломанный адрес —
    /// последним), затем другие профили, и подключение с найденной парой. Провайдер обычно режет
    /// конкретный адрес, а маскировка продолжает работать на соседнем.
    fn switch_profile(&mut self, mut settings: Settings) -> Result<()> {
        let catalog = strategies::load(&self.root)?;
        let broken = catalog.endpoint_index(&settings.endpoint);
        self.log(&format!(
            "WARP не отвечает: {} · {}. Ищу другой адрес…",
            catalog.profiles[settings.strategy.min(catalog.len() - 1)].name,
            catalog.endpoints[broken]
        ));
        self.state(true, "WARP пропал · ищу другой адрес…");
        self.stop();
        settings.endpoint = catalog.endpoints[(broken + 1) % catalog.endpoints.len()].clone();
        self.auto_select(settings, false)?;
        let settings = Settings::load(&self.root);
        self.log(&format!(
            "Переключаюсь: {} · {}.",
            catalog.profiles[settings.strategy.min(catalog.len() - 1)].name,
            settings.endpoint
        ));
        self.start(settings, false)
    }

    /// Проверка живого подключения; `true`, если WARP ответил.
    fn refresh_health(&mut self) -> bool {
        match self.delay(9090, CONNECT_BUDGET) {
            Ok(delay) => {
                self.item(2, &format!("WARP отвечает · {delay} мс"), true);
                let result = if self
                    .opera
                    .as_ref()
                    .is_some_and(|process| matches!(process.exit_code(), Ok(None)))
                {
                    self.opera_country(Duration::from_secs(6))
                        .map(|country| self.item(3, &format!("Opera EU · {country}"), true))
                } else {
                    self.start_opera()
                };
                match result {
                    Ok(()) => self.state(false, "Подключено"),
                    Err(error) if !self.cancel.load(Ordering::Relaxed) => {
                        self.item(3, "Opera EU недоступна", false);
                        self.log(&format!("Opera: {error:#}"));
                        self.state(false, "Нейросети: нет связи с Opera");
                    }
                    Err(_) => {}
                }
                true
            }
            Err(error) if !self.cancel.load(Ordering::Relaxed) => {
                self.item(2, "WARP не отвечает", false);
                self.log(&format!("Проверка соединения: {error:#}"));
                self.state(false, "Нет связи с WARP");
                false
            }
            // Отмена (пользователь остановил подключение) — не повод переключать профиль.
            Err(_) => true,
        }
    }

    fn start_opera(&mut self) -> Result<()> {
        self.cancelled()?;
        self.opera.take();
        self.item(3, "Подключаю Opera EU через WARP…", false);
        self.log("Запускаю Opera EU; API и HTTPS-туннель Opera идут через WARP.");
        let process = self.launch(platform::OPERA, &opera::launch_args(), "opera-proxy.log")?;
        self.opera = Some(process);
        match self.opera_country(Duration::from_secs(60)) {
            Ok(country) => {
                self.item(3, &format!("Opera EU · {country}"), true);
                self.log(&format!("Opera EU доступна, страна выхода: {country}. AI HTTPS направляется через Opera."));
                Ok(())
            }
            Err(error) => {
                self.item(3, "Opera EU недоступна", false);
                Err(error)
            }
        }
    }

    fn opera_country(&self, timeout: Duration) -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!(
                "http://127.0.0.1:{}",
                opera::PORT
            ))?)
            .timeout(Duration::from_secs(6))
            .build()?;
        let deadline = Instant::now() + timeout;
        let mut last_error = "Прокси ещё не готов".to_owned();
        while Instant::now() < deadline {
            self.cancelled()?;
            let process = self.opera.as_ref().context("Opera не запущена")?;
            if let Some(code) = process.exit_code()? {
                bail!("Opera завершилась с кодом {code}. См. opera-proxy.log");
            }
            match client
                .get("https://www.cloudflare.com/cdn-cgi/trace")
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.text())
            {
                Ok(trace) => return opera::country_from_trace(&trace),
                Err(error) => last_error = error.to_string(),
            }
            self.pause(Duration::from_millis(500))?;
        }
        bail!("Opera EU не ответила: {last_error}")
    }
}

/// Правам root нужны mihomo с TUN и nfqws; проверка конфигурации и пробный запуск без TUN — нет.
/// На Windows весь процесс уже запущен с правами администратора, там флаг не используется.
fn needs_root(relative: &str, args: &[String]) -> bool {
    relative == platform::ZAPRET
        || (relative == platform::MIHOMO
            && args.first().is_none_or(|arg| arg != "-t")
            && !args.iter().any(|arg| arg.ends_with("warp-test.yaml")))
}

fn strategy_args(settings: &Settings, root: &Path) -> Result<Vec<String>> {
    if settings.custom {
        return parse_args(&settings.custom_args);
    }
    let catalog = strategies::load(root)?;
    if settings.strategy >= catalog.len() {
        bail!("Неизвестный профиль WARP");
    }
    let profile = catalog.resolve(
        settings.strategy,
        catalog.endpoint_index(&settings.endpoint),
    );
    Ok(strategies::args(&profile, root))
}
// Only split arguments; never execute a shell or interpolate custom input.
fn parse_args(input: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in input.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quoted {
        bail!("В аргументах не закрыта кавычка");
    }
    if !current.is_empty() {
        result.push(current);
    }
    if result.is_empty() {
        bail!("Введите аргументы своей стратегии");
    }
    Ok(result)
}

pub fn self_test(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("data/logs"))?;
    let result = (|| -> Result<()> {
        for file in platform::required_files() {
            if !root.join(file).is_file() {
                bail!("Missing {file}");
            }
        }
        let config = fs::read_to_string(root.join("app/config/config.yaml"))?;
        if !config.contains("  - MATCH,DIRECT") || config.contains("path: ../lists/") {
            bail!("Config paths/default route invalid");
        }
        let job = Job::new()?;
        // Профили WARP и способы регистрации: winws должен принять каждый набор аргументов.
        let catalog = strategies::load(root)?;
        let profiles = (0..catalog.len()).map(|i| {
            let p = catalog.resolve(i, 0);
            (p.name.clone(), strategies::args(&p, root))
        });
        let methods = crate::registration::load(root)?
            .into_iter()
            .filter_map(|m| {
                let args = crate::registration::winws_args(m.winws.as_deref()?, root);
                Some((format!("регистрация: {}", m.name), args))
            });
        for (name, mut args) in profiles.chain(methods) {
            if args.is_empty() {
                continue;
            }
            args.insert(0, "--dry-run".into());
            let process = Process::spawn(
                &job,
                &root.join(platform::ZAPRET),
                &args,
                root,
                &root.join("data/logs/self-test.log"),
                &root.join("app/config/lists"),
                false,
            )?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while process.exit_code()?.is_none() {
                if Instant::now() > deadline {
                    bail!("winws dry-run timed out: {name}");
                }
                thread::sleep(Duration::from_millis(50));
            }
            if process.exit_code()? != Some(0) {
                bail!("winws rejected profile: {name}");
            }
        }
        let process = Process::spawn(
            &job,
            &root.join(platform::MIHOMO),
            &[
                "-t".into(),
                "-d".into(),
                root.join("data/mihomo").display().to_string(),
                "-f".into(),
                root.join("app/config/config.yaml").display().to_string(),
            ],
            root,
            &root.join("data/logs/self-test.log"),
            &root.join("app/config/lists"),
            false,
        )?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while process.exit_code()?.is_none() {
            if Instant::now() > deadline {
                bail!("Config validation timed out");
            }
            thread::sleep(Duration::from_millis(100));
        }
        if process.exit_code()? != Some(0) {
            bail!("mihomo config validation failed");
        }
        let (sleeper_exe, sleeper_args) = platform::sleeper();
        let sleeper = Process::spawn(
            &job,
            &sleeper_exe,
            &sleeper_args,
            root,
            &root.join("data/logs/self-test.log"),
            &root.join("app/config/lists"),
            false,
        )?;
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        while sleeper.exit_code()?.is_none() {
            if Instant::now() > deadline {
                bail!("Child survived job shutdown");
            }
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    fs::write(
        root.join("data/logs/self-test-result.txt"),
        match &result {
            Ok(()) => "PASS: components, all WARP strategy arguments (zapret dry-run), config validation, process isolation and job cleanup\n"
                .to_string(),
            Err(error) => format!("FAIL: {error:#}\n"),
        },
    )?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stalled_probe_obeys_total_budget() {
        use std::io::Read;
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            let _ = socket.read(&mut [0; 2048]);
            thread::sleep(Duration::from_secs(1));
        });
        let started = Instant::now();
        let budget = Duration::from_millis(250);
        assert!(probe_delay(port, budget, budget, || Ok(())).is_err());
        assert!(started.elapsed() < Duration::from_millis(750));
        worker.join().unwrap();
    }
    #[test]
    fn available_probe_returns_immediately() {
        use std::io::Read;
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        // Два запроса: /version (готовность) и delay-тест.
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = server.accept().unwrap();
                let _ = socket.read(&mut [0; 2048]);
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"delay\":42}").unwrap();
            }
        });
        let second = Duration::from_secs(1);
        assert_eq!(probe_delay(port, second, second, || Ok(())).unwrap(), 42);
        worker.join().unwrap();
    }
    #[test]
    fn tunnel_budget_starts_after_core_is_ready() {
        use std::io::Read;
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            // /version отвечает сразу, delay-тест зависает — как WARP, который молчит.
            let (mut socket, _) = server.accept().unwrap();
            let _ = socket.read(&mut [0; 2048]);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
            let (mut socket, _) = server.accept().unwrap();
            let _ = socket.read(&mut [0; 2048]);
            thread::sleep(Duration::from_secs(1));
        });
        let started = Instant::now();
        let error = probe_delay(
            port,
            Duration::from_secs(1),
            Duration::from_millis(300),
            || Ok(()),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("WARP не ответил"), "{error}");
        assert!(started.elapsed() < Duration::from_millis(900));
        worker.join().unwrap();
    }
    #[test]
    fn pick_budget_follows_history() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-pick-budget");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert_eq!(pick_budget(&root), PICK_BUDGET_MIN);
        crate::features::record(&root, 0, Some(300)).unwrap();
        assert_eq!(pick_budget(&root), PICK_BUDGET_MIN);
        crate::features::record(&root, 1, Some(1200)).unwrap();
        assert_eq!(
            pick_budget(&root),
            PICK_BUDGET_MIN,
            "берётся лучший профиль"
        );
        let slow = root.join("slow");
        fs::create_dir_all(&slow).unwrap();
        crate::features::record(&slow, 0, Some(1000)).unwrap();
        assert_eq!(pick_budget(&slow), Duration::from_millis(2500));
        crate::features::record(&slow, 1, Some(9000)).unwrap();
        assert_eq!(pick_budget(&slow), Duration::from_millis(2500));
        let very = root.join("very");
        fs::create_dir_all(&very).unwrap();
        crate::features::record(&very, 0, Some(4000)).unwrap();
        assert_eq!(pick_budget(&very), CONNECT_BUDGET);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn profile_without_powershell() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-write-profile");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/warp")).unwrap();
        let profile = strategies::Strategy {
            name: "t".into(),
            endpoint: "162.159.192.1".into(),
            port: 2408,
            jc: 4,
            jmin: 40,
            jmax: 70,
            zapret: None,
        };
        assert!(
            !write_profile(&root, &profile).unwrap(),
            "без аккаунта нужна регистрация"
        );
        let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        fs::write(root.join("data/warp/wgcf-account.toml"), "device_id = 'x'").unwrap();
        fs::write(
            root.join("data/warp/wgcf-profile.conf"),
            format!("[Interface]\nPrivateKey = {key}\nAddress = 172.16.0.2/32, 2606:4700:110::1/128\n[Peer]\nPublicKey = {key}\n"),
        )
        .unwrap();
        assert!(
            !write_profile(&root, &profile).unwrap(),
            "без reserved нужно его получить"
        );
        fs::write(root.join("data/warp/reserved.txt"), "[1, 2, 3]\n").unwrap();
        assert!(write_profile(&root, &profile).unwrap());
        let yaml = fs::read_to_string(root.join("data/mihomo/warp.yaml")).unwrap();
        for part in [
            "
  - name: WARP-WG
",
            "
    server: 162.159.192.1
",
            "
    port: 2408
",
            "
    ip: 172.16.0.2
",
            "
    ipv6: 2606:4700:110::1
",
            "
    private-key: AAAA",
            "
    amnezia-wg-option:
      jc: 4
      jmin: 40
      jmax: 70
",
            "
    reserved: [1, 2, 3]
",
        ] {
            assert!(yaml.contains(part), "{part}\n{yaml}");
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn custom_arguments_are_data_not_shell_commands() {
        assert_eq!(
            parse_args("--hostlist=\"D:\\my lists\\domains.txt\" --wf-tcp=443").unwrap(),
            vec!["--hostlist=D:\\my lists\\domains.txt", "--wf-tcp=443"]
        );
        assert!(parse_args("\"unfinished").is_err());
        assert!(parse_args("").is_err());
        assert!(strategy_args(
            &Settings {
                strategy: 0,
                ..Default::default()
            },
            Path::new(".")
        )
        .unwrap()
        .is_empty());
    }
}
