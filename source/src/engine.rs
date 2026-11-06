use crate::platform::{self, Job, Process};
use crate::strategies;
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
                        if engine.running =>
                    {
                        let _ = settings;
                        engine.log("Сначала остановите текущее подключение.");
                    }
                    Ok(command) => {
                        engine.cancel.store(false, Ordering::Relaxed);
                        engine.state(true, "Проверяю…");
                        let result = match command {
                            Command::Start(settings) => engine.start(settings, false),
                            Command::Test(settings) => engine.start(settings, true),
                            Command::Auto(settings) => engine.auto_select(settings),
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
            thread::sleep(Duration::from_millis(100));
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
        let result = self.start_inner(&settings, test);
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
    fn start_inner(&mut self, settings: &Settings, test: bool) -> Result<u64> {
        let port = if test { 9091 } else { 9090 };
        for port in [port, if test { 7891 } else { 7890 }] {
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
        )?;
        drop(bootstrap);
        self.item(1, "Профиль WARP готов", true);
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
        let mut validation = vec!["-t".to_string()];
        validation.extend(args.clone());
        self.log("Проверяю конфигурацию mihomo…");
        let process = self.launch("app/bin/mihomo/mihomo.exe", &validation, "validation.log")?;
        self.wait(process, Duration::from_secs(30))?;
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
        let delay = self.delay(port, Duration::from_secs(40))?;
        self.stage(STAGES, "Готово");
        self.item(2, &format!("WARP отвечает · {delay} мс"), true);
        self.log(&format!("WARP доступен, задержка {delay} мс."));
        if test {
            self.log("Стратегия работает. Сетевые маршруты не менялись.");
        }
        settings.save(&self.root)?;
        Ok(delay)
    }

    fn auto_select(&mut self, mut settings: Settings) -> Result<()> {
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
        settings.custom = false;
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
            let result = self.start_inner(&settings, true);
            // A fresh WireGuard instance for each trial avoids testing an old handshake.
            self.stop();
            let (delay, error) = match &result {
                Ok(delay) => (Some(*delay), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
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
            if result.is_ok() {
                settings.save(&self.root)?;
                let _ = self.events.send(Event::Selected(index));
                self.log(&format!(
                    "Рабочий профиль сохранён: {}. Нажмите «Подключить».",
                    catalog[index].name
                ));
                self.state(false, "Профиль WARP подобран");
                return Ok(());
            }
            self.log(&format!("Не прошёл: {}", catalog[index].name));
            self.cancelled()?;
            self.pause(Duration::from_secs(3))?;
        }
        original.save(&self.root)?;
        bail!("Ни один профиль не прошёл проверку. См. data/strategy-results.json")
    }
    fn delay(&self, port: u16, timeout: Duration) -> Result<u64> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(4))
            .build()?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.cancelled()?;
            if self
                .children
                .iter()
                .any(|p| !matches!(p.exit_code(), Ok(None)))
            {
                bail!("Компонент завершился во время проверки; см. логи");
            }
            let response = client
                .get(format!("http://127.0.0.1:{port}/proxies/WARP/delay"))
                .query(&[
                    ("timeout", "3000"),
                    ("url", "https://www.gstatic.com/generate_204"),
                ])
                .send();
            if let Ok(response) = response {
                if response.status().is_success() {
                    if let Ok(value) = response.json::<serde_json::Value>() {
                        if let Some(delay) = value["delay"].as_u64().filter(|d| *d > 0) {
                            return Ok(delay);
                        }
                    }
                }
            }
            self.pause(Duration::from_millis(300))?;
        }
        self.item(2, "WARP не отвечает", false);
        bail!(
            "WARP не ответил за {} с. Попробуйте другую стратегию.",
            timeout.as_secs()
        )
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
            ]
            .iter()
            .enumerate()
            {
                self.item(index, label, *ok);
            }
            self.state(false, "Отключено");
            return Ok(());
        }
        let delay = self.delay(9090, Duration::from_secs(15))?;
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
        while let Some(mut child) = self.children.pop() {
            child.stop();
        }
        self.job.take();
        self.running = false;
        self.item(2, "Тоннель выключен", false);
    }

    fn refresh_health(&mut self) {
        match self.delay(9090, Duration::from_secs(6)) {
            Ok(delay) => {
                self.item(2, &format!("WARP отвечает · {delay} мс"), true);
                self.state(false, "Подключено");
            }
            Err(error) if !self.cancel.load(Ordering::Relaxed) => {
                self.item(2, "WARP не отвечает", false);
                self.log(&format!("Проверка соединения: {error:#}"));
                self.state(false, "Нет связи с WARP");
            }
            Err(_) => {}
        }
    }
}

pub fn required_files() -> [&'static str; 7] {
    [
        "app/bin/mihomo/mihomo.exe",
        "app/bin/wgcf/wgcf.exe",
        "app/bin/zapret/winws.exe",
        "app/bin/zapret/WinDivert.dll",
        "app/bin/zapret/WinDivert64.sys",
        "app/bin/zapret/cygwin1.dll",
        "app/bin/zapret/tls_clienthello_www_google_com.bin",
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
