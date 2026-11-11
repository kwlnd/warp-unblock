//! Обновление из GitHub Releases: поиск свежего `v*`, загрузка ZIP, проверка подписи Ed25519
//! списка сумм `SHA256SUMS.txt` и SHA-256 архива, замена файлов рядом с exe и перезапуск.
//! Данные (`data/`) и свои списки доменов не трогаются. Если GitHub напрямую не отвечает, а
//! подключение активно, запросы повторяются через WARP (вход ядра `opera-underlay`, 17892).
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const REPO: &str = "kwlnd/warp-unblock";
/// Пользовательские списки: при обновлении остаются как есть.
const KEEP: [&str; 3] = [
    "app/config/lists/proxy-domains.txt",
    "app/config/lists/direct-domains.txt",
    "app/config/lists/ai-domains.txt",
];
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
/// Публичный ключ Ed25519 подписи обновлений (секретный — в секретах CI, `UPDATE_SIGNING_KEY`).
const UPDATE_KEY: &str = "gwjccy9ryFjmeGN6zMYamhwpoOWzMvNI+JYL/xF85yc=";
/// Вход ядра mihomo, весь трафик которого идёт через WARP (`opera-underlay` в config.yaml).
const WARP_INPUT: &str = "127.0.0.1:17892";
const EXE: &str = "WarpUnblock.exe";

pub const VERSION: &str = match option_env!("WARP_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub page: String,
    pub zip: Asset,
    /// `SHA256SUMS.txt` и его подпись `SHA256SUMS.txt.sig`; без них установка отменяется.
    pub sums: Option<Asset>,
    pub sig: Option<Asset>,
}

pub fn version(tag: &str) -> Option<[u64; 3]> {
    let p: Vec<_> = tag.strip_prefix('v').unwrap_or(tag).split('.').collect();
    if p.len() != 3 {
        return None;
    }
    Some([p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?])
}

fn client(timeout: u64) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .user_agent("warp-unblock")
        .build()?)
}

/// Клиент через WARP, если подключение активно и вход ядра слушает; иначе None.
fn warp_client(timeout: u64) -> Option<reqwest::blocking::Client> {
    let address = WARP_INPUT.parse().ok()?;
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(300)).ok()?;
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .user_agent("warp-unblock")
        .proxy(reqwest::Proxy::all(format!("http://{WARP_INPUT}")).ok()?)
        .build()
        .ok()
}

/// Сначала напрямую; при ошибке — ещё раз через WARP, если подключение активно.
fn with_fallback<T>(
    timeout: u64,
    request: impl Fn(&reqwest::blocking::Client) -> Result<T>,
) -> Result<T> {
    match request(&client(timeout)?) {
        Ok(value) => Ok(value),
        Err(direct) => match warp_client(timeout) {
            Some(warp) => request(&warp)
                .with_context(|| format!("напрямую: {direct:#}; через WARP тоже не вышло")),
            None => Err(direct),
        },
    }
}

/// Подпись Ed25519 (base64) над точными байтами списка сумм.
fn verify_signature(public_key: &[u8], message: &[u8], signature_b64: &str) -> bool {
    use base64::Engine;
    let Ok(signature) = base64::engine::general_purpose::STANDARD.decode(signature_b64.trim())
    else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(message, &signature)
        .is_ok()
}

/// Ожидаемый SHA-256 файла из `SHA256SUMS.txt` (формат `sha256sum`).
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let hash = hash.to_ascii_lowercase();
        (file.trim().trim_start_matches('*') == name
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(hash)
    })
}

fn asset(item: &serde_json::Value) -> Option<Asset> {
    Some(Asset {
        name: item["name"].as_str()?.to_owned(),
        url: item["browser_download_url"].as_str()?.to_owned(),
        size: item["size"].as_u64().unwrap_or(0),
    })
}

/// Самый свежий стабильный Windows-релиз из списка GitHub API (без черновиков и пререлизов).
fn pick(items: &[serde_json::Value]) -> Option<([u64; 3], Release)> {
    items
        .iter()
        .filter(|item| {
            item["draft"].as_bool() == Some(false) && item["prerelease"].as_bool() == Some(false)
        })
        .filter_map(|item| {
            let tag = item["tag_name"].as_str()?;
            let parsed = version(tag.strip_prefix('v')?)?;
            let assets: Vec<_> = item["assets"]
                .as_array()
                .map(|list| list.iter().filter_map(asset).collect())
                .unwrap_or_default();
            let zip = assets
                .iter()
                .find(|a| a.name.ends_with("-win-x64.zip"))?
                .clone();
            let sums = assets.iter().find(|a| a.name == "SHA256SUMS.txt").cloned();
            let sig = assets
                .iter()
                .find(|a| a.name == "SHA256SUMS.txt.sig")
                .cloned();
            Some((
                parsed,
                Release {
                    tag: tag.to_owned(),
                    notes: item["body"].as_str().unwrap_or("").trim().to_owned(),
                    page: item["html_url"].as_str().unwrap_or("").to_owned(),
                    zip,
                    sums,
                    sig,
                },
            ))
        })
        .max_by_key(|(v, _)| *v)
}

/// Some — доступна более новая версия, None — текущая актуальна.
pub fn check() -> Result<Option<Release>> {
    let mut best: Option<([u64; 3], Release)> = None;
    for page in 1..=10 {
        let items: Vec<serde_json::Value> = with_fallback(15, |client| {
            Ok(client
                .get(format!(
                    "https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}"
                ))
                .send()?
                .error_for_status()?
                .json()?)
        })?;
        if let Some(found) = pick(&items) {
            if best.as_ref().is_none_or(|(v, _)| found.0 > *v) {
                best = Some(found);
            }
        }
        if items.len() < 100 {
            break;
        }
    }
    let current = version(VERSION).context("Неизвестная текущая версия")?;
    Ok(best.filter(|(v, _)| *v > current).map(|(_, r)| r))
}

fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    target: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<()> {
    let mut response = client.get(url).send()?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);
    if total > MAX_DOWNLOAD {
        bail!("Файл обновления слишком большой");
    }
    let mut file = fs::File::create(target)?;
    let mut buffer = vec![0; 64 * 1024];
    let mut done = 0u64;
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        done += read as u64;
        if done > MAX_DOWNLOAD {
            bail!("Файл обновления слишком большой");
        }
        file.write_all(&buffer[..read])?;
        progress(done, total);
    }
    file.sync_all()?;
    Ok(())
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Распаковывает архив релиза в `staging`, срезая верхнюю папку `WarpUnblock/`.
fn extract(zip: &Path, staging: &Path) -> Result<Vec<PathBuf>> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip)?)?;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .enclosed_name()
            .context("Небезопасный путь в архиве")?;
        let relative: PathBuf = name.components().skip(1).collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = staging.join(&relative);
        fs::create_dir_all(target.parent().unwrap())?;
        let mut out = fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        files.push(relative);
    }
    if !files.iter().any(|f| f == Path::new(EXE)) {
        bail!("В архиве нет {EXE}");
    }
    Ok(files)
}

fn same(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => fs::read(a).ok() == fs::read(b).ok(),
        _ => false,
    }
}

fn keep(relative: &Path) -> bool {
    let text = relative.to_string_lossy().replace('\\', "/");
    KEEP.contains(&text.as_str()) || text.starts_with("data/")
}

/// Заменяет файлы: старый переименовывается в `*.old` (работает и для запущенного exe),
/// новый ставится на место. При любой ошибке всё возвращается обратно.
fn apply(root: &Path, exe: &Path, staging: &Path, files: &[PathBuf]) -> Result<usize> {
    let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let result = (|| -> Result<()> {
        for relative in files {
            let target = if relative == Path::new(EXE) {
                exe.to_owned()
            } else {
                root.join(relative)
            };
            let source = staging.join(relative);
            if (keep(relative) && target.exists()) || same(&source, &target) {
                continue;
            }
            fs::create_dir_all(target.parent().unwrap())?;
            let old = if target.exists() {
                let old = old_name(&target);
                let _ = fs::remove_file(&old);
                fs::rename(&target, &old)
                    .with_context(|| format!("Файл занят: {}", relative.display()))?;
                Some(old)
            } else {
                None
            };
            moved.push((target.clone(), old));
            fs::rename(&source, &target)
                .or_else(|_| fs::copy(&source, &target).map(|_| ()))
                .with_context(|| format!("Не удалось записать {}", relative.display()))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        for (target, old) in moved.into_iter().rev() {
            let _ = fs::remove_file(&target);
            if let Some(old) = old {
                let _ = fs::rename(old, &target);
            }
        }
        return Err(error);
    }
    Ok(moved.len())
}

fn old_name(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".old");
    path.with_file_name(name)
}

#[derive(Clone, Copy, Debug)]
pub enum Step {
    Download(u64, u64),
    Verify,
    Install,
}

/// Скачивает, проверяет и ставит релиз. Соединение должно быть остановлено заранее.
pub fn install(root: &Path, release: &Release, progress: &dyn Fn(Step)) -> Result<usize> {
    let (Some(sums), Some(sig)) = (&release.sums, &release.sig) else {
        bail!("У релиза нет подписанных контрольных сумм — установка отменена");
    };
    // Сначала подпись: дальше доверяем только подписанному списку сумм.
    let text = |url: &str| -> Result<Vec<u8>> {
        with_fallback(30, |client| {
            Ok(client
                .get(url)
                .send()?
                .error_for_status()?
                .bytes()?
                .to_vec())
        })
    };
    let sums_bytes = text(&sums.url)?;
    let sig_text = String::from_utf8(text(&sig.url)?).unwrap_or_default();
    use base64::Engine;
    let key = base64::engine::general_purpose::STANDARD.decode(UPDATE_KEY)?;
    if !verify_signature(&key, &sums_bytes, &sig_text) {
        bail!("Подпись обновления не сошлась — файл подменён, установка отменена");
    }
    let expected = expected_sum(&String::from_utf8_lossy(&sums_bytes), &release.zip.name)
        .context("В подписанном списке нет суммы архива")?;
    let work = root.join("data/update");
    let _ = fs::remove_dir_all(&work);
    let staging = work.join("staging");
    fs::create_dir_all(&staging)?;
    let zip = work.join(&release.zip.name);
    with_fallback(600, |client| {
        download(client, &release.zip.url, &zip, &|done, total| {
            progress(Step::Download(done, total.max(release.zip.size)))
        })
    })?;
    progress(Step::Verify);
    if sha256(&zip)? != expected {
        bail!("Контрольная сумма не совпала — файл повреждён или подменён");
    }
    progress(Step::Install);
    let files = extract(&zip, &staging)?;
    let exe = std::env::current_exe()?;
    let changed = apply(root, &exe, &staging, &files)?;
    let _ = fs::remove_dir_all(&work);
    Ok(changed)
}

/// Запускает новую версию; она дождётся, пока текущий экземпляр освободит mutex.
pub fn restart() -> Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .arg("--after-update")
        .spawn()?;
    Ok(())
}

/// Удаляет `*.old`, оставшиеся после прошлого обновления.
pub fn cleanup(root: &Path) {
    fn walk(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "data" {
                    walk(&path);
                }
            } else if path.extension().is_some_and(|e| e == "old") {
                let _ = fs::remove_file(path);
            }
        }
    }
    walk(root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str, draft: bool, pre: bool, assets: &[&str]) -> serde_json::Value {
        json!({
            "tag_name": tag, "draft": draft, "prerelease": pre, "body": "notes",
            "html_url": format!("https://example/{tag}"),
            "assets": assets.iter().map(|n| json!({"name": n, "browser_download_url": format!("https://dl/{n}"), "size": 10})).collect::<Vec<_>>()
        })
    }

    #[test]
    fn versions_are_numeric_and_stable_only() {
        assert!(version("android-v0.2.0").is_none());
        assert!(version("v1.2.3-rc1").is_none());
        assert!(version("v0.10.0") > version("v0.9.9"));
    }

    #[test]
    fn picks_newest_windows_release_with_zip() {
        let items = vec![
            release(
                "v0.5.1",
                false,
                false,
                &[
                    "WarpUnblock-v0.5.1-win-x64.zip",
                    "WarpUnblock-v0.5.1-win-x64.zip.sha256",
                ],
            ),
            release("v0.10.0", false, true, &["WarpUnblock-v0.10.0-win-x64.zip"]),
            release("android-v9.0.0", false, false, &["a.apk"]),
            release(
                "v0.6.0",
                false,
                false,
                &[
                    "WarpUnblock-v0.6.0-win-x64.zip",
                    "WarpUnblock-v0.6.0-win-x64.zip.sha256",
                    "SHA256SUMS.txt",
                    "SHA256SUMS.txt.sig",
                ],
            ),
            release("v0.7.0", false, false, &["other.txt"]),
        ];
        let (v, r) = pick(&items).unwrap();
        assert_eq!(v, [0, 6, 0]);
        assert_eq!(r.zip.name, "WarpUnblock-v0.6.0-win-x64.zip");
        assert_eq!(r.sums.unwrap().name, "SHA256SUMS.txt");
        assert_eq!(r.sig.unwrap().name, "SHA256SUMS.txt.sig");
    }

    #[test]
    fn apply_replaces_files_keeps_user_lists_and_data() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-update-apply");
        let _ = fs::remove_dir_all(&base);
        let (root, staging) = (base.join("root"), base.join("staging"));
        let put = |dir: &Path, rel: &str, text: &str| {
            let p = dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        put(&root, EXE, "old exe");
        put(&root, "app/bin/core.exe", "old core");
        put(&root, "app/config/lists/proxy-domains.txt", "user list");
        put(&root, "data/settings.json", "mine");
        let files: Vec<PathBuf> = [
            EXE,
            "app/bin/core.exe",
            "app/config/lists/proxy-domains.txt",
            "app/config/lists/warp-ips.txt",
            "data/settings.json",
        ]
        .iter()
        .map(|rel| {
            put(&staging, rel, &format!("new {rel}"));
            PathBuf::from(rel)
        })
        .collect();
        let changed = apply(&root, &root.join(EXE), &staging, &files).unwrap();
        assert_eq!(changed, 3);
        let read = |rel: &str| fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(read(EXE), format!("new {EXE}"));
        assert_eq!(read("app/bin/core.exe"), "new app/bin/core.exe");
        assert_eq!(read("app/config/lists/proxy-domains.txt"), "user list");
        assert_eq!(
            read("app/config/lists/warp-ips.txt"),
            "new app/config/lists/warp-ips.txt"
        );
        assert_eq!(read("data/settings.json"), "mine");
        assert!(root.join("app/bin/core.exe.old").exists());
        cleanup(&root);
        assert!(!root.join("app/bin/core.exe.old").exists());
        fs::remove_dir_all(base).unwrap();
    }

    /// Живая проверка: скачать опубликованный релиз, сверить SHA-256 и распаковать.
    /// Запуск: cargo test -- --ignored live_release_download
    #[test]
    #[ignore]
    fn live_release_download() {
        let client = client(600).unwrap();
        let items: Vec<serde_json::Value> = client
            .get(format!(
                "https://api.github.com/repos/{REPO}/releases?per_page=100"
            ))
            .send()
            .unwrap()
            .json()
            .unwrap();
        let (_, release) = pick(&items).unwrap();
        let work = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-update-live");
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(work.join("staging")).unwrap();
        let zip = work.join(&release.zip.name);
        download(&client, &release.zip.url, &zip, &|_, _| {}).unwrap();
        // Релизы до v0.7.6 ещё без подписи: проверяем подпись, только если она есть.
        let sums = client
            .get(&release.sums.as_ref().unwrap().url)
            .send()
            .unwrap()
            .bytes()
            .unwrap();
        if let Some(sig) = &release.sig {
            use base64::Engine;
            let key = base64::engine::general_purpose::STANDARD
                .decode(UPDATE_KEY)
                .unwrap();
            let sig = client.get(&sig.url).send().unwrap().text().unwrap();
            assert!(verify_signature(&key, &sums, &sig));
        }
        let expected = expected_sum(&String::from_utf8_lossy(&sums), &release.zip.name).unwrap();
        assert_eq!(sha256(&zip).unwrap(), expected);
        let files = extract(&zip, &work.join("staging")).unwrap();
        assert!(files.iter().any(|f| f.starts_with("app")));
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn signature_must_match_the_exact_sums() {
        use base64::Engine;
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let sums = b"0123abcd  WarpUnblock-v1.0.0-win-x64.zip\n";
        let sig = base64::engine::general_purpose::STANDARD.encode(pair.sign(sums).as_ref());
        let public = pair.public_key().as_ref();
        assert!(verify_signature(public, sums, &sig));
        assert!(!verify_signature(
            public,
            b"0123abce  WarpUnblock-v1.0.0-win-x64.zip\n",
            &sig
        ));
        assert!(!verify_signature(public, sums, "не base64"));
        let ours = base64::engine::general_purpose::STANDARD
            .decode(UPDATE_KEY)
            .unwrap();
        assert_eq!(ours.len(), 32);
        assert!(!verify_signature(&ours, sums, &sig));
    }

    #[test]
    fn expected_sum_reads_sha256sum_format() {
        let sums = format!("{}  a.zip\n{} *b.apk\n", "A".repeat(64), "b".repeat(64));
        assert_eq!(expected_sum(&sums, "a.zip"), Some("a".repeat(64)));
        assert_eq!(expected_sum(&sums, "b.apk"), Some("b".repeat(64)));
        assert_eq!(expected_sum(&sums, "c.zip"), None);
        assert_eq!(expected_sum("xyz  a.zip", "a.zip"), None);
    }

    #[test]
    fn apply_rolls_back_on_failure() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-update-rollback");
        let _ = fs::remove_dir_all(&base);
        let (root, staging) = (base.join("root"), base.join("staging"));
        fs::create_dir_all(root.join("app")).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(root.join(EXE), "old").unwrap();
        fs::write(staging.join(EXE), "new").unwrap();
        // Второго файла в staging нет — установка должна упасть и вернуть exe.
        let files = vec![PathBuf::from(EXE), PathBuf::from("app/missing.bin")];
        assert!(apply(&root, &root.join(EXE), &staging, &files).is_err());
        assert_eq!(fs::read_to_string(root.join(EXE)).unwrap(), "old");
        fs::remove_dir_all(base).unwrap();
    }
}
