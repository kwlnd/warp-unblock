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
    let warp = root.join("data/warp");
    let (Ok(conf), Ok(reserved)) = (
        fs::read_to_string(warp.join("wgcf-profile.conf")),
        fs::read_to_string(warp.join("reserved.txt")),
    ) else {
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
    let reserved = reserved.trim();
    let bytes: Vec<u8> = serde_json::from_str(reserved).unwrap_or_default();
    if bytes.len() != 3 {
        return Ok(false);
    }
    let yaml = [
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
        format!("    reserved: [{}, {}, {}]", bytes[0], bytes[1], bytes[2]),
        String::new(),
    ]
    .join("
");
    fs::create_dir_all(root.join("data/mihomo"))?;
    fs::write(root.join("data/mihomo/warp.yaml"), yaml)?;
    Ok(true)
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
            strategy: 3,
            custom_args: String::new(),
            custom: false,
            autostart: false,
            close_to_tray: true,
            renat: false,
            check_updates: true,
            skip_version: String::new(),
            theme: system_theme(),
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
    Selected(usize),
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
                            engine.refresh_health();
                            last_health = Instant::now();
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
        self.job()?;
        Process::spawn(
            self.job.as_ref().unwrap(),
            &self.root.join(relative),
            args,
            &self.root,
            &self.root.join(format!("data/logs/{log_name}")),
            &self.root.join("app/config/lists"),
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
    fn script(&mut self, script: &str, extra: &[&str], log: &str) -> Result<()> {
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
        )?;
        self.wait(process, Duration::from_secs(600))
    }
    fn prepare(&mut self) -> Result<()> {
        self.cancelled()?;
        fs::create_dir_all(self.root.join("data/logs"))?;
        fs::create_dir_all(self.root.join("data/mihomo"))?;
        self.job()?;
        let required = required_files();
        if required.iter().any(|p| !self.root.join(p).is_file()) {
            self.log("Не хватает зависимостей. Скачиваю официальные сборки…");
            self.script("setup.ps1", &[], "setup.log")?;
        }
        for file in required {
            if !self.root.join(file).is_file() {
                bail!("Нет файла {file}");
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
            vec![port, 7891]
        } else {
            vec![port, 7890, opera::PORT, opera::UNDERLAY_PORT]
        };
        for port in ports {
            let _probe = TcpListener::bind(("127.0.0.1", port)).with_context(|| {
                format!("Порт {port} занят. Отключите другой прокси перед запуском")
            })?;
        }
        self.stage(0, "Подготовка компонентов");
        self.prepare()?;
        let catalog = strategies::load(&self.root)?;
        let profile = catalog
            .get(settings.strategy)
            .context("Неизвестный профиль WARP")?;
        self.log(&format!(
            "Профиль: {} · {}:{}",
            profile.name, profile.endpoint, profile.port
        ));
        let args = strategy_args(settings, &self.root)?;
        self.stage(1, "Запуск zapret");
        if !args.is_empty() {
            self.log("Запускаю zapret…");
            let process = self.launch("app/bin/zapret/winws.exe", &args, "zapret.log")?;
            self.pause(Duration::from_secs(1))?;
            if let Some(code) = process.exit_code()? {
                bail!("winws не запустился (код {code}), см. zapret.log");
            }
            self.children.push(process);
        } else {
            self.log("Zapret отключён; использую WARP с маскировкой handshake.");
        }
        // TCP desync is only for first-time account registration, never website traffic.
        let bootstrap = if !self.root.join("data/warp/wgcf-account.toml").is_file() {
            Some(self.launch(
                "app/bin/zapret/winws.exe",
                &[
                    "--wf-tcp=443".into(),
                    "--filter-tcp=443".into(),
                    format!(
                        "--hostlist={}",
                        self.root.join("app/config/lists/warp-hosts.txt").display()
                    ),
                    format!(
                            "--dpi-desync-fake-tls={}",
                            self.root
                                .join("app/bin/zapret/tls_clienthello_www_google_com.bin")
                                .display()
                        ),
                    "--dpi-desync=fake,disorder2".into(),
                    "--dpi-desync-split-pos=1".into(),
                    "--dpi-desync-ttl=6".into(),
                    "--dpi-desync-fooling=badsum".into(),
                ],
                "registration.log",
            )?)
        } else {
            None
        };
        self.stage(2, "Настройка профиля WARP");
        if bootstrap.is_none() && write_profile(&self.root, profile)? {
            self.item(1, "Профиль WARP готов", true);
        } else {
            self.profile_script(profile)?;
            self.item(1, "Профиль WARP готов", true);
        }
        drop(bootstrap);
        self.finish_start(test, port, budget, validate, settings)
    }

    fn profile_script(&mut self, profile: &strategies::Strategy) -> Result<()> {
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
        )
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
            let process =
                self.launch("app/bin/mihomo/mihomo.exe", &validation, "validation.log")?;
            self.wait(process, Duration::from_secs(5))?;
        }
        self.log(if test {
            "Проверяю стратегию без TUN…"
        } else {
            "Поднимаю TUN. Обычный трафик — DIRECT."
        });
        self.stage(4, "Запуск mihomo");
        let process = self.launch("app/bin/mihomo/mihomo.exe", &args, "mihomo.log")?;
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

    fn auto_select(&mut self, mut settings: Settings, best: bool) -> Result<()> {
        if !platform::is_admin() {
            bail!("Подбор требует прав администратора");
        }
        self.prepare()?;
        let catalog = strategies::load(&self.root)?;
        let original = settings.clone();
        let preferred = settings.strategy.min(catalog.len() - 1);
        let order =
            std::iter::once(preferred).chain((0..catalog.len()).filter(|i| *i != preferred));
        let mut results = Vec::new();
        let mut failed = 0usize;
        let mut candidates = Vec::new();
        settings.custom = false;
        let budget = pick_budget(&self.root);
        self.log(&format!(
            "Подбор: на ответ WARP {:.1} с на профиль.",
            budget.as_secs_f32()
        ));
        for (attempt, index) in order.enumerate() {
            self.cancelled()?;
            settings.strategy = index;
            let _ = self
                .events
                .send(Event::Batch(Some((attempt, catalog.len(), failed))));
            self.state(
                true,
                &format!(
                    "Подбор WARP · {}/{} · {}",
                    attempt + 1,
                    catalog.len(),
                    catalog[index].name
                ),
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
            }
            results.push(serde_json::json!({"name": catalog[index].name, "index": index, "delay_ms": delay, "error": error}));
            fs::write(
                self.root.join("data/strategy-results.json"),
                serde_json::to_vec_pretty(&results)?,
            )?;
            if result.is_err() {
                failed += 1;
            }
            let _ = self
                .events
                .send(Event::Batch(Some((attempt + 1, catalog.len(), failed))));
            if result.is_ok() && !best {
                settings.save(&self.root)?;
                let _ = self.events.send(Event::Selected(index));
                self.log(&format!(
                    "Рабочий профиль сохранён: {}. Нажмите «Подключить».",
                    catalog[index].name
                ));
                self.state(false, "Профиль WARP подобран");
                return Ok(());
            }
            self.log(&format!(
                "{}: {}",
                if result.is_ok() {
                    "Прошёл"
                } else {
                    "Не прошёл"
                },
                catalog[index].name
            ));
            self.cancelled()?;
        }
        if best {
            if let Some(index) = crate::features::best(&self.root, &candidates) {
                settings.strategy = index;
                settings.save(&self.root)?;
                let _ = self.events.send(Event::Selected(index));
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
                    required_files().iter().all(|p| self.root.join(p).is_file()),
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
            .proxy(reqwest::Proxy::all("http://127.0.0.1:7890")?)
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

    fn refresh_health(&mut self) {
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
            }
            Err(error) if !self.cancel.load(Ordering::Relaxed) => {
                self.item(2, "WARP не отвечает", false);
                self.log(&format!("Проверка соединения: {error:#}"));
                self.state(false, "Нет связи с WARP");
            }
            Err(_) => {}
        }
    }

    fn start_opera(&mut self) -> Result<()> {
        self.cancelled()?;
        self.opera.take();
        self.item(3, "Подключаю Opera EU через WARP…", false);
        self.log("Запускаю Opera EU; API и HTTPS-туннель Opera идут через WARP.");
        let process = self.launch(
            "app/bin/opera/opera-proxy.exe",
            &opera::launch_args(),
            "opera-proxy.log",
        )?;
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

pub fn required_files() -> [&'static str; 8] {
    [
        "app/bin/mihomo/mihomo.exe",
        "app/bin/wgcf/wgcf.exe",
        "app/bin/zapret/winws.exe",
        "app/bin/zapret/WinDivert.dll",
        "app/bin/zapret/WinDivert64.sys",
        "app/bin/zapret/cygwin1.dll",
        "app/bin/zapret/tls_clienthello_www_google_com.bin",
        "app/bin/opera/opera-proxy.exe",
    ]
}

fn strategy_args(settings: &Settings, root: &Path) -> Result<Vec<String>> {
    if settings.custom {
        return parse_args(&settings.custom_args);
    }
    let catalog = strategies::load(root)?;
    let profile = catalog
        .get(settings.strategy)
        .context("Неизвестный профиль WARP")?;
    Ok(strategies::args(profile, root))
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
        for file in required_files() {
            if !root.join(file).is_file() {
                bail!("Missing {file}");
            }
        }
        let config = fs::read_to_string(root.join("app/config/config.yaml"))?;
        if !config.contains("  - MATCH,DIRECT") || config.contains("path: ../lists/") {
            bail!("Config paths/default route invalid");
        }
        let job = Job::new()?;
        for profile in strategies::load(root)? {
            let mut args = strategies::args(&profile, root);
            if args.is_empty() {
                continue;
            }
            args.insert(0, "--dry-run".into());
            let process = Process::spawn(
                &job,
                &root.join("app/bin/zapret/winws.exe"),
                &args,
                root,
                &root.join("data/logs/self-test.log"),
                &root.join("app/config/lists"),
            )?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while process.exit_code()?.is_none() {
                if Instant::now() > deadline {
                    bail!("winws dry-run timed out: {}", profile.name);
                }
                thread::sleep(Duration::from_millis(50));
            }
            if process.exit_code()? != Some(0) {
                bail!("winws rejected profile: {}", profile.name);
            }
        }
        let process = Process::spawn(
            &job,
            &root.join("app/bin/mihomo/mihomo.exe"),
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
        let sleeper = Process::spawn(
            &job,
            &platform::powershell(),
            &[
                "-NoProfile".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 60".into(),
            ],
            root,
            &root.join("data/logs/self-test.log"),
            &root.join("app/config/lists"),
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
            Ok(()) => "PASS: components, all WARP strategy arguments (winws dry-run), config validation, process isolation and job cleanup\n"
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
            winws: false,
            repeats: 0,
            ttl: 0,
            cutoff: 0,
        };
        assert!(
            !write_profile(&root, &profile).unwrap(),
            "без аккаунта нужен warp.ps1"
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
            "без reserved нужен warp.ps1"
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
