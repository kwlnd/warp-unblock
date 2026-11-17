//! Linux: процессы, root-хелпер, правила NFQUEUE для nfqws, автозапуск и один экземпляр.
//!
//! Окно, трей и всё остальное работают от обычного пользователя. Права нужны только mihomo
//! (TUN) и nfqws (очередь пакетов): их запускает хелпер — этот же бинарник с ключом `--helper`,
//! поднятый через `pkexec`. Хелпер один на всё время работы программы: пароль спрашивается
//! один раз, а не на каждый процесс. Он принимает построчный JSON по stdin, запускает только
//! файлы из `app/bin/` рядом со своим бинарником и убивает всё, когда stdin закрылся
//! (окно закрыто или упало).
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::AsRawFd,
        net::{UnixListener, UnixStream},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub const MIHOMO: &str = "app/bin/mihomo/mihomo";
pub const WGCF: &str = "app/bin/wgcf/wgcf";
pub const ZAPRET: &str = "app/bin/zapret/nfqws";
pub const OPERA: &str = "app/bin/opera/opera-proxy";
/// Имя исполняемого файла в архиве обновления.
pub const EXE_NAME: &str = "WarpUnblock";
/// Окончание имени архива релиза для этой платформы.
#[cfg(target_arch = "x86_64")]
pub const ARCHIVE_SUFFIX: &str = "-linux-x86_64.tar.gz";
#[cfg(target_arch = "aarch64")]
pub const ARCHIVE_SUFFIX: &str = "-linux-aarch64.tar.gz";
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const ARCHIVE_SUFFIX: &str = "-linux-unsupported.tar.gz";

/// Метка пакетов, которые nfqws вставляет сам: их в очередь возвращать нельзя.
const DESYNC_MARK: &str = "0x40000000";
const TABLE_PREFIX: &str = "warp_unblock_";

/// Файлы, без которых подключение невозможно.
pub fn required_files() -> Vec<&'static str> {
    vec![
        MIHOMO,
        WGCF,
        ZAPRET,
        OPERA,
        "app/bin/zapret/tls_clienthello_www_google_com.bin",
    ]
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Права берёт хелпер в момент запуска mihomo/nfqws, поэтому приложению они не нужны.
pub fn is_admin() -> bool {
    true
}

pub fn error_box(message: &str) {
    eprintln!("WARP Unblock: {message}");
    let shown = Command::new("zenity")
        .args(["--error", "--no-markup", "--title=WARP Unblock", "--text"])
        .arg(message)
        .status()
        .is_ok_and(|s| s.success())
        || Command::new("kdialog")
            .args(["--error"])
            .arg(message)
            .status()
            .is_ok_and(|s| s.success());
    if !shown {
        let _ = Command::new("notify-send")
            .args(["WARP Unblock"])
            .arg(message)
            .status();
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn env_dir(name: &str, fallback: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

fn writable(dir: &Path) -> bool {
    CString::new(dir.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 })
}

/// Корень программы с папками `app/` и `data/`.
///
/// Переносимая установка (архив) — папка с бинарником. Пакетная (`/opt`, `/usr`) для записи
/// недоступна: данные и настраиваемые файлы лежат в `~/.local/share/warp-unblock`, а ядра и
/// ресурсы подключаются ссылками на системную папку. `WARP_ROOT` задаёт корень явно.
pub fn root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("WARP_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let exe = fs::canonicalize(std::env::current_exe()?)?;
    let dir = exe.parent().context("Нет папки программы")?.to_path_buf();
    if dir.join("app").is_dir() && writable(&dir) {
        return Ok(dir);
    }
    let base = env_dir("XDG_DATA_HOME", ".local/share").join("warp-unblock");
    prepare_user_root(&dir, &base)?;
    Ok(base)
}

/// Самообновление возможно только для переносимой установки: пакеты обновляет менеджер пакетов.
pub fn can_self_update() -> bool {
    match (root(), std::env::current_exe().and_then(fs::canonicalize)) {
        (Ok(root), Ok(exe)) => exe.parent() == Some(root.as_path()),
        _ => false,
    }
}

/// Пользовательские списки: при обновлении пакета остаются как есть.
const USER_FILES: [&str; 3] = [
    "lists/proxy-domains.txt",
    "lists/direct-domains.txt",
    "lists/ai-domains.txt",
];

fn prepare_user_root(system: &Path, base: &Path) -> Result<()> {
    fs::create_dir_all(base.join("data"))?;
    fs::create_dir_all(base.join("app"))?;
    for name in ["bin", "assets", "licenses"] {
        let source = system.join("app").join(name);
        let link = base.join("app").join(name);
        if !source.exists() {
            continue;
        }
        if fs::read_link(&link).ok().as_deref() != Some(source.as_path()) {
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&source, &link)
                .with_context(|| format!("Не удалось связать {}", link.display()))?;
        }
    }
    sync_config(&system.join("app/config"), &base.join("app/config"))
}

fn sync_config(source: &Path, target: &Path) -> Result<()> {
    fn walk(source: &Path, target: &Path, relative: &Path) -> Result<()> {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)?.flatten() {
            let name = entry.file_name();
            let (from, to, rel) = (entry.path(), target.join(&name), relative.join(&name));
            if from.is_dir() {
                walk(&from, &to, &rel)?;
                continue;
            }
            let kept = USER_FILES.iter().any(|f| Path::new(f) == rel) && to.exists();
            if !kept && fs::read(&from)? != fs::read(&to).unwrap_or_default() {
                fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }
    walk(source, target, Path::new(""))
}

// ---------- окно: запросы из трея и от второго экземпляра ----------

static REQUEST: AtomicU8 = AtomicU8::new(0);
static HELD: AtomicBool = AtomicBool::new(false);

fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        if dir.is_dir() {
            return dir;
        }
    }
    let dir = std::env::temp_dir().join(format!("warp-unblock-{}", unsafe { libc::getuid() }));
    let _ = fs::create_dir_all(&dir);
    let _ = fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    dir
}

/// Показывает/прячет главное окно. Окно рисует интерфейс: здесь только запрос, который он
/// подхватывает таймером (`take_window_request`). Из второго экземпляра запрос уходит первому
/// через сокет.
pub fn show_main_window(show: bool) -> bool {
    if HELD.load(Ordering::Relaxed) {
        REQUEST.store(if show { 1 } else { 2 }, Ordering::Relaxed);
        return true;
    }
    show && UnixStream::connect(runtime_dir().join("warp-unblock.sock"))
        .and_then(|mut stream| stream.write_all(b"show\n"))
        .is_ok()
}

pub fn take_window_request() -> Option<bool> {
    match REQUEST.swap(0, Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

pub struct Instance {
    _lock: File,
    socket: PathBuf,
}
impl Instance {
    pub fn acquire() -> Result<Self> {
        let dir = runtime_dir();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("warp-unblock.lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("WARP Unblock уже открыт. Используйте существующее окно.");
        }
        HELD.store(true, Ordering::Relaxed);
        let socket = dir.join("warp-unblock.sock");
        let _ = fs::remove_file(&socket);
        if let Ok(listener) = UnixListener::bind(&socket) {
            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    drop(stream);
                    REQUEST.store(1, Ordering::Relaxed);
                }
            });
        }
        Ok(Self {
            _lock: lock,
            socket,
        })
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

// ---------- автозапуск, открытие ссылок ----------

fn desktop_path() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", ".config").join("autostart/warp-unblock.desktop")
}

/// Автозапуск: файл XDG autostart, приложение стартует в трее и подключается само.
pub fn set_autostart(enable: bool, _root: &Path) -> Result<()> {
    let path = desktop_path();
    if !enable {
        match fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => return Ok(()),
        }
    }
    let exe = std::env::current_exe()?.display().to_string();
    if exe.contains(['"', '`', '$', '\\', '\n']) {
        bail!("В пути к программе есть символы, которые нельзя записать в ярлык автозапуска");
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=WARP Unblock\nComment=Обход блокировок через Cloudflare WARP\n\
             Exec=\"{exe}\" --tray\nIcon=warp-unblock\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ),
    )?;
    Ok(())
}

pub fn open(path: &Path) {
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn open_url(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Команда, которая живёт долго: проверка завершения дочерних процессов вместе с заданием.
pub fn sleeper() -> (PathBuf, Vec<String>) {
    (PathBuf::from("/bin/sleep"), vec!["60".into()])
}

// ---------- процессы ----------

/// Задание: при закрытии убивает всё, что в нём запущено (аналог Job Object).
pub struct Job {
    id: u64,
    local: Arc<Mutex<Vec<u32>>>,
}
static JOB_IDS: AtomicU64 = AtomicU64::new(1);
impl Job {
    pub fn new() -> Result<Self> {
        Ok(Self {
            id: JOB_IDS.fetch_add(1, Ordering::Relaxed),
            local: Arc::default(),
        })
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        for pid in self.local.lock().unwrap().drain(..) {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        let helper = HELPER.lock().unwrap().clone();
        if let Some(helper) = helper {
            let _ = helper.request(&Request::KillJob { job: self.id });
        }
    }
}

fn open_log(log: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .with_context(|| format!("Лог {}", log.display()))
}

/// Процесс, запущенный в этом процессе (приложением или хелпером).
struct Local {
    child: Mutex<Child>,
    pid: u32,
    rules: Mutex<Option<Rules>>,
    job: Option<Arc<Mutex<Vec<u32>>>>,
}

static QUEUES: AtomicU16 = AtomicU16::new(0);

impl Local {
    fn spawn(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        log: File,
        safe_paths: &Path,
    ) -> Result<Self> {
        let mut args = args.to_vec();
        let mut rules = None;
        if exe.file_name().is_some_and(|name| name == "nfqws") {
            // Фильтры `--wf-*` относятся к WinDivert; очередь и правила делаем сами.
            args.retain(|arg| !arg.starts_with("--wf-"));
            let queue = 200 + QUEUES.fetch_add(1, Ordering::Relaxed) % 50;
            if !args.iter().any(|arg| arg == "--dry-run") {
                rules = Some(Rules::install(queue, &args)?);
            }
            args.push(format!("--qnum={queue}"));
        }
        let mut command = Command::new(exe);
        command
            .args(&args)
            .current_dir(cwd)
            .env("SAFE_PATHS", safe_paths)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        // Родитель умер — ребёнок тоже: не оставляем mihomo с поднятым TUN.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(
                    libc::PR_SET_PDEATHSIG,
                    libc::SIGKILL as libc::c_ulong,
                    0,
                    0,
                    0,
                );
                Ok(())
            });
        }
        let child = command
            .spawn()
            .with_context(|| format!("Запуск {}", exe.display()))?;
        Ok(Self {
            pid: child.id(),
            child: Mutex::new(child),
            rules: Mutex::new(rules),
            job: None,
        })
    }

    fn exit_code(&self) -> Result<Option<u32>> {
        Ok(self.child.lock().unwrap().try_wait()?.map(|status| {
            status
                .code()
                .map(|code| code as u32)
                .or(status.signal().map(|signal| 128 + signal as u32))
                .unwrap_or(1)
        }))
    }

    /// SIGTERM, чтобы mihomo убрал TUN и маршруты; через 3 секунды SIGKILL.
    fn stop(&self) {
        let mut child = self.child.lock().unwrap();
        if matches!(child.try_wait(), Ok(None)) {
            unsafe {
                libc::kill(self.pid as i32, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        drop(child);
        self.rules.lock().unwrap().take();
        if let Some(job) = &self.job {
            job.lock().unwrap().retain(|pid| *pid != self.pid);
        }
    }
}
impl Drop for Local {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Remote {
    helper: Arc<Helper>,
    pid: u32,
    exited: Mutex<Option<u32>>,
}

enum Inner {
    Local(Local),
    Remote(Remote),
}

pub struct Process {
    inner: Inner,
}

impl Process {
    /// `privileged` — процессу нужен root (mihomo с TUN, nfqws): запускается через хелпер.
    pub fn spawn(
        job: &Job,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        log: &Path,
        safe_paths: &Path,
        privileged: bool,
    ) -> Result<Self> {
        if privileged && !is_root() {
            let helper = helper()?;
            let response = helper.request(&Request::Spawn {
                job: job.id,
                exe: exe.to_owned(),
                args: args.to_vec(),
                cwd: cwd.to_owned(),
                log: log.to_owned(),
                safe_paths: safe_paths.to_owned(),
            })?;
            return Ok(Self {
                inner: Inner::Remote(Remote {
                    helper,
                    pid: response.pid.context("Хелпер не вернул pid")?,
                    exited: Mutex::new(None),
                }),
            });
        }
        let mut local = Local::spawn(exe, args, cwd, open_log(log)?, safe_paths)?;
        job.local.lock().unwrap().push(local.pid);
        local.job = Some(job.local.clone());
        Ok(Self {
            inner: Inner::Local(local),
        })
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        match &self.inner {
            Inner::Local(local) => local.exit_code(),
            Inner::Remote(remote) => {
                if let Some(code) = *remote.exited.lock().unwrap() {
                    return Ok(Some(code));
                }
                let response = remote
                    .helper
                    .request(&Request::Status { pid: remote.pid })?;
                if response.running {
                    return Ok(None);
                }
                let code = response.code.unwrap_or(1);
                *remote.exited.lock().unwrap() = Some(code);
                Ok(Some(code))
            }
        }
    }

    pub fn stop(&mut self) {
        match &self.inner {
            Inner::Local(local) => local.stop(),
            Inner::Remote(remote) => {
                let _ = remote.helper.request(&Request::Kill { pid: remote.pid });
                remote.exited.lock().unwrap().get_or_insert(1);
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------- клиент хелпера ----------

#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum Request {
    Hello,
    Spawn {
        job: u64,
        exe: PathBuf,
        args: Vec<String>,
        cwd: PathBuf,
        log: PathBuf,
        safe_paths: PathBuf,
    },
    Status {
        pid: u32,
    },
    Kill {
        pid: u32,
    },
    KillJob {
        job: u64,
    },
}

#[derive(Default, Serialize, Deserialize)]
struct Response {
    ok: bool,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    running: bool,
    #[serde(default)]
    code: Option<u32>,
    #[serde(default)]
    error: Option<String>,
}

struct Connection {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    dead: bool,
}

struct Helper {
    connection: Mutex<Connection>,
}

static HELPER: Mutex<Option<Arc<Helper>>> = Mutex::new(None);

/// Хелпер с правами root: поднимается один раз (окно ввода пароля polkit) и живёт до выхода.
fn helper() -> Result<Arc<Helper>> {
    let mut slot = HELPER.lock().unwrap();
    if let Some(helper) = slot.as_ref().filter(|helper| helper.alive()) {
        return Ok(helper.clone());
    }
    let helper = Arc::new(Helper::start()?);
    *slot = Some(helper.clone());
    Ok(helper)
}

impl Helper {
    fn start() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let mut child = Command::new("pkexec")
            .arg(&exe)
            .arg("--helper")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("Не найден pkexec (пакет polkit): без него нельзя получить права для TUN")?;
        let helper = Self {
            connection: Mutex::new(Connection {
                input: Some(child.stdin.take().context("pkexec: нет stdin")?),
                output: BufReader::new(child.stdout.take().context("pkexec: нет stdout")?),
                child,
                dead: false,
            }),
        };
        helper.request(&Request::Hello).context(
            "Права администратора не получены: окно ввода пароля закрыто или не найден агент polkit",
        )?;
        Ok(helper)
    }

    fn alive(&self) -> bool {
        let mut connection = self.connection.lock().unwrap();
        !connection.dead && matches!(connection.child.try_wait(), Ok(None))
    }

    fn request(&self, request: &Request) -> Result<Response> {
        let mut connection = self.connection.lock().unwrap();
        if connection.dead {
            bail!("Хелпер с правами root остановлен");
        }
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        let mut reply = String::new();
        let connection = &mut *connection;
        let result = match connection.input.as_mut() {
            Some(input) => input
                .write_all(line.as_bytes())
                .and_then(|_| input.flush())
                .and_then(|_| connection.output.read_line(&mut reply)),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        };
        match result {
            Ok(n) if n > 0 => {}
            _ => {
                connection.dead = true;
                bail!("Хелпер с правами root не отвечает");
            }
        }
        let response: Response = serde_json::from_str(&reply)?;
        if !response.ok {
            bail!(
                "{}",
                response.error.unwrap_or_else(|| "ошибка хелпера".into())
            );
        }
        Ok(response)
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        // Закрытие stdin — сигнал хелперу убрать за собой и выйти. pkexec принадлежит root,
        // убить его нельзя, поэтому только ждём.
        let connection = self.connection.get_mut().unwrap();
        connection.input.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(connection.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

// ---------- сам хелпер (запускается под root: `WarpUnblock --helper`) ----------

struct Entry {
    local: Local,
    job: u64,
    cwd: PathBuf,
}

fn chown_tree(path: &Path, uid: u32) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    if let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) {
        unsafe {
            libc::lchown(c_path.as_ptr(), uid, u32::MAX);
        }
    }
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                chown_tree(&entry.path(), uid);
            }
        }
    }
}

/// Файлы, созданные root в `data/` (кэш mihomo, наборы правил), возвращаем пользователю:
/// следующий запуск без прав (проверка профиля) должен уметь их читать и писать.
fn hand_back(cwd: &Path, uid: u32) {
    let data = cwd.join("data");
    if fs::metadata(&data).is_ok_and(|meta| meta.uid() == uid) {
        chown_tree(&data, uid);
    }
}

fn handle(
    request: Request,
    procs: &mut HashMap<u32, Entry>,
    bin: &Path,
    uid: u32,
) -> Result<Response> {
    let ok = Response {
        ok: true,
        ..Default::default()
    };
    match request {
        Request::Hello => Ok(ok),
        Request::Spawn {
            job,
            exe,
            args,
            cwd,
            log,
            safe_paths,
        } => {
            let exe =
                fs::canonicalize(&exe).with_context(|| format!("Нет файла {}", exe.display()))?;
            if !exe.starts_with(bin) {
                bail!("Запуск разрешён только для app/bin: {}", exe.display());
            }
            // Рабочая папка и логи должны принадлежать пользователю, а не системе.
            if !fs::metadata(&cwd).is_ok_and(|meta| meta.is_dir() && meta.uid() == uid) {
                bail!(
                    "Рабочая папка не принадлежит пользователю: {}",
                    cwd.display()
                );
            }
            if log
                .parent()
                .and_then(|p| p.file_name())
                .is_none_or(|n| n != "logs")
            {
                bail!("Лог должен лежать в data/logs");
            }
            let file = open_log(&log)?;
            unsafe {
                libc::fchown(file.as_raw_fd(), uid, u32::MAX);
            }
            let local = Local::spawn(&exe, &args, &cwd, file, &safe_paths)?;
            let pid = local.pid;
            procs.insert(pid, Entry { local, job, cwd });
            Ok(Response {
                pid: Some(pid),
                ..ok
            })
        }
        Request::Status { pid } => {
            let entry = procs.get(&pid).context("Нет такого процесса")?;
            let code = entry.local.exit_code()?;
            Ok(Response {
                running: code.is_none(),
                code,
                ..ok
            })
        }
        Request::Kill { pid } => {
            if let Some(entry) = procs.remove(&pid) {
                entry.local.stop();
                hand_back(&entry.cwd, uid);
            }
            Ok(ok)
        }
        Request::KillJob { job } => {
            let pids: Vec<u32> = procs
                .iter()
                .filter(|(_, entry)| entry.job == job)
                .map(|(pid, _)| *pid)
                .collect();
            for pid in pids {
                if let Some(entry) = procs.remove(&pid) {
                    entry.local.stop();
                    hand_back(&entry.cwd, uid);
                }
            }
            Ok(ok)
        }
    }
}

/// Точка входа `--helper`: читает команды из stdin, пока родитель не закроет канал.
pub fn run_helper() -> Result<()> {
    if !is_root() {
        bail!("--helper запускается только под root (через pkexec)");
    }
    let uid = std::env::var("PKEXEC_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| {
            std::env::var("SUDO_UID")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .context("Неизвестный пользователь (нет PKEXEC_UID)")?;
    let exe = fs::canonicalize(std::env::current_exe()?)?;
    let bin = exe.parent().context("Нет папки программы")?.join("app/bin");
    let bin = fs::canonicalize(&bin).with_context(|| format!("Нет {}", bin.display()))?;
    Rules::remove_stale();
    let mut procs = HashMap::new();
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle(request, &mut procs, &bin, uid),
            Err(error) => Err(error.into()),
        }
        .unwrap_or_else(|error| Response {
            ok: false,
            error: Some(format!("{error:#}")),
            ..Default::default()
        });
        writeln!(out, "{}", serde_json::to_string(&response)?)?;
        out.flush()?;
    }
    // Родитель ушёл: остановить всё и убрать правила (Drop у Local).
    for (_, entry) in procs.drain() {
        entry.local.stop();
        hand_back(&entry.cwd, uid);
    }
    Ok(())
}

// ---------- правила очереди пакетов для nfqws ----------

enum Backend {
    Nft(String),
    /// Команды удаления: (iptables|ip6tables, аргументы).
    Iptables(Vec<(&'static str, Vec<String>)>),
}

struct Rules(Backend);

/// Фильтры, которые winws получает ключами `--filter-udp/tcp` и `--ipset`.
struct Filter {
    ports: Vec<(&'static str, u16)>,
    v4: Vec<String>,
    v6: Vec<String>,
}

fn parse_filter(args: &[String]) -> Result<Filter> {
    let mut filter = Filter {
        ports: Vec::new(),
        v4: Vec::new(),
        v6: Vec::new(),
    };
    for arg in args {
        if let Some(port) = arg.strip_prefix("--filter-udp=") {
            filter.ports.push(("udp", port.parse()?));
        } else if let Some(port) = arg.strip_prefix("--filter-tcp=") {
            filter.ports.push(("tcp", port.parse()?));
        } else if let Some(path) = arg.strip_prefix("--ipset=") {
            for line in fs::read_to_string(path)
                .with_context(|| format!("Список адресов {path}"))?
                .lines()
            {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (address, prefix) = line.split_once('/').unwrap_or((line, ""));
                if !prefix.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                if address.parse::<std::net::Ipv4Addr>().is_ok() {
                    filter.v4.push(line.to_owned());
                } else if address.parse::<std::net::Ipv6Addr>().is_ok() {
                    filter.v6.push(line.to_owned());
                }
            }
        }
    }
    if filter.ports.is_empty() {
        bail!(
            "У nfqws нет --filter-udp/--filter-tcp: непонятно, какие пакеты направлять в очередь"
        );
    }
    Ok(filter)
}

fn nft_script(table: &str, queue: u16, filter: &Filter) -> String {
    let mut text = format!("table inet {table} {{\n");
    for (name, kind, list) in [
        ("warp4", "ipv4_addr", &filter.v4),
        ("warp6", "ipv6_addr", &filter.v6),
    ] {
        if !list.is_empty() {
            text += &format!(
                "  set {name} {{ type {kind}; flags interval; elements = {{ {} }} }}\n",
                list.join(", ")
            );
        }
    }
    text += "  chain out {\n    type filter hook output priority mangle; policy accept;\n";
    let tail = format!("meta mark & {DESYNC_MARK} == 0 queue num {queue} bypass");
    let limited = !filter.v4.is_empty() || !filter.v6.is_empty();
    for (proto, port) in &filter.ports {
        if !limited {
            text += &format!("    {proto} dport {port} {tail}\n");
            continue;
        }
        if !filter.v4.is_empty() {
            text += &format!("    {proto} dport {port} ip daddr @warp4 {tail}\n");
        }
        if !filter.v6.is_empty() {
            text += &format!("    {proto} dport {port} ip6 daddr @warp6 {tail}\n");
        }
    }
    text + "  }\n}\n"
}

fn run_with_input(program: &str, args: &[&str], input: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(input.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn iptables(program: &str, args: &[String]) -> Result<()> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        bail!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

impl Rules {
    /// nftables, а если его нет — iptables. Очередь с `bypass`: если nfqws упадёт, пакеты идут
    /// как обычно, а не пропадают.
    fn install(queue: u16, args: &[String]) -> Result<Self> {
        let filter = parse_filter(args)?;
        let table = format!("{TABLE_PREFIX}{queue}");
        let script = nft_script(&table, queue, &filter);
        let nft = run_with_input("nft", &["-f", "-"], &script);
        let Err(nft_error) = nft else {
            return Ok(Self(Backend::Nft(table)));
        };
        let mut undo = Vec::new();
        let result = (|| -> Result<()> {
            for (proto, port) in &filter.ports {
                for (tool, list) in [("iptables", &filter.v4), ("ip6tables", &filter.v6)] {
                    let limited = !filter.v4.is_empty() || !filter.v6.is_empty();
                    if limited && list.is_empty() {
                        continue;
                    }
                    for destination in if limited {
                        list.clone()
                    } else {
                        vec![String::new()]
                    } {
                        let mut rule: Vec<String> =
                            ["-t", "mangle", "OUTPUT", "-p", *proto, "--dport"]
                                .map(String::from)
                                .into();
                        rule.push(port.to_string());
                        if !destination.is_empty() {
                            rule.extend(["-d".into(), destination]);
                        }
                        rule.extend(["-m", "mark", "!", "--mark"].map(String::from));
                        rule.push(format!("{DESYNC_MARK}/{DESYNC_MARK}"));
                        rule.extend(["-j", "NFQUEUE", "--queue-num"].map(String::from));
                        rule.push(queue.to_string());
                        rule.push("--queue-bypass".into());
                        let mut add = rule.clone();
                        add.insert(2, "-I".into());
                        let mut delete = rule;
                        delete.insert(2, "-D".into());
                        iptables(tool, &add)?;
                        undo.push((tool, delete));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            Self(Backend::Iptables(undo)).drop_rules();
            bail!("Не удалось настроить очередь пакетов (nft: {nft_error:#}; iptables: {error:#})");
        }
        Ok(Self(Backend::Iptables(undo)))
    }

    fn drop_rules(&self) {
        match &self.0 {
            Backend::Nft(table) => {
                let _ = Command::new("nft")
                    .args(["delete", "table", "inet", table.as_str()])
                    .output();
            }
            Backend::Iptables(undo) => {
                for (tool, args) in undo {
                    let _ = iptables(tool, args);
                }
            }
        }
    }

    /// Таблицы, оставшиеся от упавшего хелпера.
    fn remove_stale() {
        let Ok(output) = Command::new("nft").args(["list", "tables"]).output() else {
            return;
        };
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(name) = line.strip_prefix("table inet ") {
                if name.starts_with(TABLE_PREFIX) {
                    let _ = Command::new("nft")
                        .args(["delete", "table", "inet", name.trim()])
                        .output();
                }
            }
        }
    }
}
impl Drop for Rules {
    fn drop(&mut self) {
        self.drop_rules();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn filter_from_winws_arguments() {
        let dir = std::env::temp_dir().join(format!("wu-ipset-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let ipset = dir.join("ips.txt");
        fs::write(
            &ipset,
            "# c\n162.159.192.0/24\n\n2606:4700:d0::/48\nbad/xx\n",
        )
        .unwrap();
        let filter = parse_filter(&strings(&[
            "--wf-udp=2408",
            "--filter-udp=2408",
            &format!("--ipset={}", ipset.display()),
            "--dpi-desync-any-protocol",
        ]))
        .unwrap();
        assert_eq!(filter.ports, vec![("udp", 2408)]);
        assert_eq!(filter.v4, vec!["162.159.192.0/24"]);
        assert_eq!(filter.v6, vec!["2606:4700:d0::/48"]);
        let script = nft_script("warp_unblock_200", 200, &filter);
        assert!(script.contains("table inet warp_unblock_200"));
        assert!(script.contains(
            "udp dport 2408 ip daddr @warp4 meta mark & 0x40000000 == 0 queue num 200 bypass"
        ));
        assert!(script.contains("udp dport 2408 ip6 daddr @warp6"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tcp_registration_has_no_address_limit() {
        let filter = parse_filter(&strings(&["--filter-tcp=443", "--hostlist=x"])).unwrap();
        let script = nft_script("t", 201, &filter);
        assert!(script.contains("tcp dport 443 meta mark & 0x40000000 == 0 queue num 201 bypass"));
        assert!(!script.contains("set "));
    }

    #[test]
    fn nfqws_without_filter_is_rejected() {
        assert!(parse_filter(&strings(&["--dpi-desync=fake"])).is_err());
    }

    #[test]
    fn requests_roundtrip() {
        let text = serde_json::to_string(&Request::Kill { pid: 7 }).unwrap();
        assert_eq!(text, r#"{"op":"Kill","pid":7}"#);
        assert!(matches!(
            serde_json::from_str::<Request>(&text).unwrap(),
            Request::Kill { pid: 7 }
        ));
    }

    #[test]
    fn job_kills_local_children() {
        let job = Job::new().unwrap();
        let dir = std::env::temp_dir();
        let log = dir.join(format!("wu-job-{}.log", std::process::id()));
        let (exe, args) = sleeper();
        let process = Process::spawn(&job, &exe, &args, &dir, &log, &dir, false).unwrap();
        assert_eq!(process.exit_code().unwrap(), None);
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        while process.exit_code().unwrap().is_none() {
            assert!(Instant::now() < deadline, "child survived job shutdown");
            thread::sleep(Duration::from_millis(50));
        }
        let _ = fs::remove_file(log);
    }

    #[test]
    fn config_sync_keeps_user_lists() {
        let base = std::env::temp_dir().join(format!("wu-sync-{}", std::process::id()));
        let (from, to) = (base.join("from"), base.join("to"));
        fs::create_dir_all(from.join("lists")).unwrap();
        fs::create_dir_all(to.join("lists")).unwrap();
        fs::write(from.join("config.yaml"), "new").unwrap();
        fs::write(from.join("lists/proxy-domains.txt"), "shipped").unwrap();
        fs::write(from.join("lists/warp-ips.txt"), "ips2").unwrap();
        fs::write(to.join("config.yaml"), "old").unwrap();
        fs::write(to.join("lists/proxy-domains.txt"), "mine").unwrap();
        fs::write(to.join("lists/warp-ips.txt"), "ips1").unwrap();
        sync_config(&from, &to).unwrap();
        let read = |p: &str| fs::read_to_string(to.join(p)).unwrap();
        assert_eq!(read("config.yaml"), "new");
        assert_eq!(read("lists/proxy-domains.txt"), "mine");
        assert_eq!(read("lists/warp-ips.txt"), "ips2");
        fs::remove_dir_all(base).unwrap();
    }
}
