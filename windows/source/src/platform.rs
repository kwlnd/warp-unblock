use anyhow::{bail, Context, Result};
use std::{
    ffi::OsStr, fs::OpenOptions, os::windows::ffi::OsStrExt, os::windows::io::AsRawHandle,
    path::Path, ptr,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, SetHandleInformation, ERROR_ALREADY_EXISTS, HANDLE,
        HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{
            CreateMutexW, CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess,
            WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
        },
    },
    UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::{
            FindWindowW, MessageBoxW, SetForegroundWindow, ShowWindow, MB_ICONERROR, MB_OK,
            SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
        },
    },
};

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn error_box(message: &str) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(message).as_ptr(),
            wide("WARP Unblock").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn elevate() -> Result<()> {
    let exe = std::env::current_exe()?;
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            wide("runas").as_ptr(),
            wide(&exe).as_ptr(),
            // Флаги вроде --tray должны пережить перезапуск с правами администратора.
            wide(std::env::args().skip(1).collect::<Vec<_>>().join(" ")).as_ptr(),
            wide(exe.parent().unwrap()).as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        bail!("Запуск от администратора отменён или недоступен");
    }
    Ok(())
}

const WINDOW_TITLE: &str = "WARP Unblock";
const TASK_NAME: &str = "WarpUnblock";

/// Показывает/прячет главное окно штатными средствами Windows — без участия кадров egui,
/// поэтому работает и когда окно скрыто в трей.
pub fn show_main_window(show: bool) -> bool {
    unsafe {
        let hwnd = FindWindowW(ptr::null(), wide(WINDOW_TITLE).as_ptr());
        if hwnd.is_null() {
            return false;
        }
        if show {
            ShowWindow(hwnd, SW_RESTORE);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        } else {
            ShowWindow(hwnd, SW_HIDE);
        }
        true
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn schtasks(args: &[&str]) -> Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::process::Command::new(Path::new(&system).join("System32/schtasks.exe"))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("Не удалось запустить schtasks")
}

/// Автозапуск через планировщик: задача «при входе» с наивысшими правами запускает приложение
/// без UAC-запроса. Реестровый Run потребовал бы подтверждение при каждом входе.
pub fn set_autostart(enable: bool, root: &Path) -> Result<()> {
    if !enable {
        if schtasks(&["/Query", "/TN", TASK_NAME])?.status.success() {
            let out = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
            if !out.status.success() {
                bail!("{}", String::from_utf8_lossy(&out.stdout).trim());
            }
        }
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").context("Неизвестное имя пользователя")?
    );
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>--tray</Arguments><WorkingDirectory>{dir}</WorkingDirectory></Exec></Actions>
</Task>"#,
        user = xml_escape(&user),
        exe = xml_escape(&exe.display().to_string()),
        dir = xml_escape(&exe.parent().unwrap().display().to_string()),
    );
    let path = root.join("data/autostart.xml");
    std::fs::create_dir_all(root.join("data"))?;
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&path, bytes)?;
    let out = schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/XML",
        &path.display().to_string(),
        "/F",
    ])?;
    let _ = std::fs::remove_file(&path);
    if !out.status.success() {
        bail!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub fn open(path: &Path) {
    unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            wide("open").as_ptr(),
            wide(path).as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn powershell() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe")
}

pub struct Instance(HANDLE);
impl Instance {
    pub fn acquire() -> Result<Self> {
        let handle = unsafe {
            CreateMutexW(
                ptr::null(),
                0,
                wide("Local\\WarpUnblock.NativeLauncher").as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            bail!("WARP Unblock уже открыт. Используйте существующее окно.");
        }
        Ok(Self(handle))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct Job(HANDLE);
impl Job {
    pub fn new() -> Result<Self> {
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let job = Self(handle);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(job)
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct Process {
    handle: HANDLE,
}
impl Process {
    pub fn spawn(
        job: &Job,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        log: &Path,
        safe_paths: &Path,
    ) -> Result<Self> {
        let output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .with_context(|| format!("Лог {}", log.display()))?;
        let input = OpenOptions::new().read(true).open("NUL")?;
        for file in [&output, &input] {
            if unsafe {
                SetHandleInformation(
                    file.as_raw_handle() as _,
                    HANDLE_FLAG_INHERIT,
                    HANDLE_FLAG_INHERIT,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = std::mem::size_of_val(&startup) as u32;
        startup.dwFlags = STARTF_USESTDHANDLES;
        startup.hStdOutput = output.as_raw_handle() as _;
        startup.hStdError = output.as_raw_handle() as _;
        startup.hStdInput = input.as_raw_handle() as _;
        let command = std::iter::once(exe.to_string_lossy().to_string())
            .chain(args.iter().cloned())
            .map(|arg| quote(&arg))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide(command);
        let mut vars = std::env::vars_os()
            .filter(|(key, _)| !key.eq_ignore_ascii_case(OsStr::new("SAFE_PATHS")))
            .collect::<Vec<_>>();
        vars.push(("SAFE_PATHS".into(), safe_paths.as_os_str().to_os_string()));
        vars.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
        let mut env = Vec::new();
        for (key, value) in vars {
            env.extend(key.encode_wide());
            env.push('=' as u16);
            env.extend(value.encode_wide());
            env.push(0);
        }
        env.push(0);
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            CreateProcessW(
                wide(exe).as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                env.as_ptr() as _,
                wide(cwd).as_ptr(),
                &startup,
                &mut pi,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("Запуск {}", exe.display()));
        }
        let process = Self {
            handle: pi.hProcess,
        };
        // Assign before resuming: even fast helper subprocesses remain in this job.
        if unsafe { AssignProcessToJobObject(job.0, pi.hProcess) } == 0 {
            unsafe {
                CloseHandle(pi.hThread);
            }
            return Err(std::io::Error::last_os_error())
                .context("Не удалось изолировать дочерний процесс");
        }
        let resumed = unsafe { ResumeThread(pi.hThread) };
        unsafe {
            CloseHandle(pi.hThread);
        }
        if resumed == u32::MAX {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(process)
    }

    pub fn exit_code(&self) -> Result<Option<u32>> {
        if unsafe { WaitForSingleObject(self.handle, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Some(code))
    }

    pub fn stop(&mut self) {
        if matches!(self.exit_code(), Ok(None)) {
            unsafe {
                TerminateProcess(self.handle, 1);
                WaitForSingleObject(self.handle, 3000);
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

// Windows CommandLineToArgvW quoting, including trailing backslashes.
fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            output.push_str(&"\\".repeat(slashes * 2 + 1));
        } else {
            output.push_str(&"\\".repeat(slashes));
        }
        slashes = 0;
        output.push(c);
    }
    output.push_str(&"\\".repeat(slashes * 2));
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::quote;
    #[test]
    fn windows_paths_and_quotes() {
        assert_eq!(quote("D:\\Новая папка\\"), "\"D:\\Новая папка\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote(""), "\"\"");
    }
}
