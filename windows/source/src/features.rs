use crate::engine::Settings;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, time::Duration};

const LISTS: [&str; 3] = ["proxy-domains.txt", "direct-domains.txt", "ai-domains.txt"];
const ACCOUNT: [&str; 3] = ["wgcf-account.toml", "wgcf-profile.conf", "reserved.txt"];
pub static FILES_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Portable domain-list v1: ASCII DNS names, optional +./*. suffix wildcard, # comments.
pub fn domains(text: &str) -> Result<String> {
    if text.len() > 256 * 1024 {
        bail!("Список больше 256 КиБ");
    }
    let mut entries = std::collections::BTreeSet::new();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let value = line
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if value.is_empty() {
            continue;
        }
        let wildcard = value.starts_with("+.") || value.starts_with("*.");
        let name = if wildcard { &value[2..] } else { &value };
        if name.len() > 253
            || (!wildcard && !name.contains('.'))
            || name.bytes().all(|c| c.is_ascii_digit() || c == b'.')
            || name.split('.').any(|p| {
                p.is_empty()
                    || p.len() > 63
                    || p.starts_with('-')
                    || p.ends_with('-')
                    || !p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
        {
            bail!(
                "Строка {}: нужен домен (например +.example.com), без URL и IP",
                index + 1
            );
        }
        entries.insert(format!("{}{name}", if wildcard { "+." } else { "" }));
    }
    Ok(entries.into_iter().map(|v| v + "\n").collect())
}

/// Каталог профилей сменился (новая версия программы или правка strategies.json): статистика
/// и выбор профиля привязаны к номерам, поэтому они сбрасываются. true — сброс был.
pub fn sync_catalog(
    root: &Path,
    catalog: &crate::strategies::Catalog,
    settings: &mut Settings,
) -> bool {
    let id = crate::strategies::fingerprint(catalog);
    let path = root.join("data/catalog-id.txt");
    let old = fs::read_to_string(&path).ok();
    if old.as_deref().map(str::trim) == Some(id.as_str()) {
        return false;
    }
    let stats = root.join("data/profile-stats.json");
    let results = root.join("data/strategy-results.json");
    let had_state = old.is_some() || stats.exists() || results.exists();
    if had_state {
        let _ = fs::remove_file(stats);
        let _ = fs::remove_file(results);
        settings.strategy = 0;
        settings.endpoint.clear();
        let _ = settings.save(root);
    }
    let _ = fs::create_dir_all(root.join("data"));
    let _ = fs::write(path, id);
    had_state
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Stat {
    pub attempts: u64,
    pub successes: u64,
    pub total_ms: u64,
}
pub fn stats(root: &Path) -> BTreeMap<usize, Stat> {
    fs::read(root.join("data/profile-stats.json"))
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}
pub fn record(root: &Path, index: usize, delay: Option<u64>) -> Result<()> {
    let mut all = stats(root);
    let item = all.entry(index).or_default();
    item.attempts += 1;
    if let Some(ms) = delay {
        item.successes += 1;
        item.total_ms += ms;
    }
    fs::create_dir_all(root.join("data"))?;
    fs::write(
        root.join("data/profile-stats.json"),
        serde_json::to_vec_pretty(&all)?,
    )?;
    Ok(())
}
pub fn best(root: &Path, candidates: &[(usize, u64)]) -> Option<usize> {
    let all = stats(root);
    candidates
        .iter()
        .min_by(|(a, ams), (b, bms)| {
            let a = all.get(a).cloned().unwrap_or_default();
            let b = all.get(b).cloned().unwrap_or_default();
            // Reliability first; current latency breaks ties. Only candidates that passed this run qualify.
            (b.successes * a.attempts)
                .cmp(&(a.successes * b.attempts))
                .then(ams.cmp(bms))
        })
        .map(|v| v.0)
}

#[derive(Serialize, Deserialize)]
struct Backup {
    format: u32,
    settings: Settings,
    lists: BTreeMap<String, String>,
    account: BTreeMap<String, String>,
}
fn export(root: &Path, settings: Settings) -> Result<String> {
    let Some(path) = rfd::FileDialog::new()
        .set_file_name("warp-backup.json")
        .add_filter("WARP backup", &["json"])
        .save_file()
    else {
        return Ok("Экспорт отменён".into());
    };
    let mut backup = Backup {
        format: 1,
        settings,
        lists: BTreeMap::new(),
        account: BTreeMap::new(),
    };
    for name in LISTS {
        backup.lists.insert(
            name.into(),
            fs::read_to_string(root.join("app/config/lists").join(name))?,
        );
    }
    for name in ACCOUNT {
        let file = root.join("data/warp").join(name);
        if file.is_file() {
            backup
                .account
                .insert(name.into(), fs::read_to_string(file)?);
        }
    }
    fs::write(path, serde_json::to_vec_pretty(&backup)?)?;
    Ok("Настройки и аккаунт экспортированы. Файл содержит приватный ключ.".into())
}
fn import(root: &Path) -> Result<String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("WARP backup", &["json"])
        .pick_file()
    else {
        return Ok("Импорт отменён".into());
    };
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 2 * 1024 * 1024 {
        bail!("Слишком большой файл");
    }
    apply_backup(root, serde_json::from_reader(file)?)
}
fn apply_backup(root: &Path, mut backup: Backup) -> Result<String> {
    if backup.format != 1 || backup.settings.strategy >= crate::strategies::load(root)?.len() {
        bail!("Неподдерживаемая резервная копия");
    }
    for (name, content) in &mut backup.lists {
        if !LISTS.contains(&name.as_str()) {
            bail!("Неизвестный список");
        }
        *content = domains(content)?;
    }
    for (name, content) in &backup.account {
        if !ACCOUNT.contains(&name.as_str()) || content.len() > 64 * 1024 {
            bail!("Некорректный аккаунт");
        }
    }
    if !backup.account.is_empty()
        && (!backup.account.contains_key("wgcf-account.toml")
            || !backup.account.contains_key("wgcf-profile.conf"))
    {
        bail!("Копия аккаунта неполная");
    }
    if let Some(profile) = backup.account.get("wgcf-profile.conf") {
        profile_text(profile)?;
    }
    if let Some(reserved) = backup.account.get("reserved.txt") {
        let bytes: Vec<u8> = serde_json::from_str(reserved.trim())?;
        if bytes.len() != 3 {
            bail!("Некорректный reserved");
        }
    }
    // Keep an exact rollback copy before touching any current account/settings/list files.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let saved = root.join(format!("data/import-backups/{stamp}"));
    fs::create_dir_all(&saved)?;
    let mut writes: Vec<_> = backup
        .lists
        .iter()
        .map(|(name, text)| {
            (
                format!("app/config/lists/{name}"),
                Some(text.as_bytes().to_vec()),
            )
        })
        .collect();
    writes.extend(
        backup
            .account
            .iter()
            .map(|(name, text)| (format!("data/warp/{name}"), Some(text.as_bytes().to_vec()))),
    );
    // Autostart is machine-local and must stay in sync with the scheduler on this machine.
    backup.settings.autostart = Settings::load(root).autostart;
    writes.push((
        "data/settings.json".into(),
        Some(serde_json::to_vec_pretty(&backup.settings)?),
    ));
    writes.push(("data/mihomo/warp.yaml".into(), None));
    if !backup.account.is_empty() && !backup.account.contains_key("reserved.txt") {
        writes.push(("data/warp/reserved.txt".into(), None));
    }
    for relative in ["data/mihomo/warp.yaml", "data/warp/reserved.txt"] {
        let file = root.join(relative);
        if file.is_file() {
            let target = saved.join(relative);
            fs::create_dir_all(target.parent().unwrap())?;
            fs::copy(file, target)?;
        }
    }
    for (relative, _) in &writes {
        let file = root.join(relative);
        if file.is_file() {
            let target = saved.join(relative);
            fs::create_dir_all(target.parent().unwrap())?;
            fs::copy(file, target)?;
        }
    }
    let mut applied = Vec::new();
    for (relative, bytes) in &writes {
        let target = root.join(relative);
        let result = (|| -> Result<()> {
            fs::create_dir_all(target.parent().unwrap())?;
            if let Some(bytes) = bytes {
                fs::write(&target, bytes)?;
            } else if target.exists() {
                fs::remove_file(&target)?;
            }
            Ok(())
        })();
        applied.push(relative);
        if let Err(error) = result {
            for relative in applied {
                let old = saved.join(relative);
                let target = root.join(relative);
                if old.exists() {
                    fs::copy(old, target)?;
                } else if target.exists() {
                    fs::remove_file(target)?;
                }
            }
            return Err(error);
        }
    }
    Ok("Импортировано. Предыдущее состояние сохранено в data/import-backups/.".into())
}
fn profile_text(text: &str) -> Result<String> {
    use base64::Engine;
    let field = |name: &str| -> Result<&str> {
        let fields: Vec<_> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim())
            .collect();
        if fields.len() != 1 || fields[0].is_empty() {
            bail!("Некорректное поле {name}");
        }
        Ok(fields[0])
    };
    for name in ["PrivateKey", "PublicKey"] {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(field(name)?)
            .map_err(|_| anyhow::anyhow!("Некорректное поле {name}"))?;
        if decoded.len() != 32 {
            bail!("Некорректное поле {name}");
        }
    }
    let addresses = field("Address")?
        .split(',')
        .map(|v| {
            v.trim()
                .split('/')
                .next()
                .unwrap()
                .parse::<std::net::IpAddr>()
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !addresses.iter().any(|v| v.is_ipv4()) || !addresses.iter().any(|v| v.is_ipv6()) {
        bail!("Нужны IPv4 и IPv6 WARP");
    }
    Ok(text.into())
}
fn profile(root: &Path) -> Result<String> {
    let mut text = profile_text(&fs::read_to_string(
        root.join("data/warp/wgcf-profile.conf"),
    )?)?;
    if let Ok(reserved) = fs::read_to_string(root.join("data/warp/reserved.txt")) {
        let bytes: Vec<u8> = serde_json::from_str(reserved.trim())?;
        if bytes.len() != 3 {
            bail!("Некорректный reserved");
        }
        text.push_str(&format!(
            "\nReserved = {}, {}, {}\n",
            bytes[0], bytes[1], bytes[2]
        ));
    }
    Ok(text)
}

pub const LIST_TITLES: [&str; 3] = ["Через WARP", "Напрямую", "AI через Opera"];

pub fn read_lists(root: &Path) -> Result<[String; 3]> {
    let values = LISTS
        .iter()
        .map(|name| {
            Ok(fs::read_to_string(
                root.join("app/config/lists").join(name),
            )?)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(values.try_into().unwrap())
}

/// Нормализует и сохраняет все три списка; возвращает сохранённый вид.
pub fn save_lists(root: &Path, lists: &[String; 3]) -> Result<[String; 3]> {
    let normalized = lists
        .iter()
        .zip(LIST_TITLES)
        .map(|(text, title)| domains(text).with_context(|| format!("«{title}»")))
        .collect::<Result<Vec<_>>>()?;
    for (name, text) in LISTS.iter().zip(&normalized) {
        fs::write(root.join("app/config/lists").join(name), text)?;
    }
    Ok(normalized.try_into().unwrap())
}

/// Выполняет файловую операцию (экспорт/импорт), пока движок не трогает те же файлы.
pub fn guarded(task: impl FnOnce() -> Result<String>) -> Result<String> {
    struct Unlock;
    impl Drop for Unlock {
        fn drop(&mut self) {
            FILES_BUSY.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    FILES_BUSY.store(true, std::sync::atomic::Ordering::Release);
    let _unlock = Unlock;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Операция завершилась неожиданно")))
}

pub fn export_backup(root: &Path, settings: Settings) -> Result<String> {
    export(root, settings)
}

pub fn import_backup(root: &Path) -> Result<String> {
    import(root)
}

/// QR с профилем WARP для импорта на телефон (содержит приватный ключ).
pub fn qr(root: &Path) -> Result<qrcode::QrCode> {
    Ok(qrcode::QrCode::new(profile(root)?.as_bytes())?)
}

pub struct Service {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// HTTPS-доступность сервисов через группу WARP работающего mihomo.
pub fn check_services() -> Vec<Service> {
    let check = |name: &'static str, url: &str| {
        let result = (|| -> Result<u64> {
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(12))
                .build()?;
            let value: serde_json::Value = client
                .get("http://127.0.0.1:9090/proxies/WARP/delay")
                .query(&[("timeout", "8000"), ("url", url)])
                .send()?
                .error_for_status()?
                .json()?;
            value["delay"].as_u64().context("Нет ответа через WARP")
        })();
        match result {
            Ok(ms) => Service {
                name,
                ok: true,
                detail: format!("{ms} мс"),
            },
            Err(e) => Service {
                name,
                ok: false,
                detail: e.to_string(),
            },
        }
    };
    vec![
        check("Discord", "https://discord.com/api/v10/gateway"),
        check("YouTube", "https://www.youtube.com/"),
        check("Telegram", "https://telegram.org/"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_domains() {
        assert_eq!(
            domains("\u{feff}# note\n*.Example.COM\n+.example.com # same\nfoo.example\n").unwrap(),
            "+.example.com\nfoo.example\n"
        );
        for bad in [
            "https://a.com",
            "1.1.1.1",
            "bad..com",
            "a-.com",
            "a.com,REJECT",
            "пример.рф",
            "a.com/path",
        ] {
            assert!(domains(bad).is_err(), "{bad}");
        }
        assert_eq!(domains("").unwrap(), "");
        assert_eq!(domains("+.ru\n+.su").unwrap(), "+.ru\n+.su\n");
    }
    #[test]
    fn imported_profile_requires_real_keys_and_both_addresses() {
        let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let text = format!("[Interface]\nPrivateKey = {key}\nAddress = 172.16.0.2/32, 2606:4700:110::1/128\n[Peer]\nPublicKey = {key}\n");
        assert!(profile_text(&text).is_ok());
        assert!(profile_text(&text.replace(key, "bad")).is_err());
        assert!(profile_text(&text.replace(", 2606:4700:110::1/128", "")).is_err());
    }
    #[test]
    fn shipped_lists_are_editable() {
        for text in [
            include_str!("../../app/config/lists/direct-domains.txt"),
            include_str!("../../app/config/lists/proxy-domains.txt"),
            include_str!("../../app/config/lists/ai-domains.txt"),
        ] {
            domains(text).unwrap();
        }
    }
    #[test]
    fn backup_validates_before_writing_and_preserves_previous_settings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-settings-backup");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("app/config")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/config/strategies.json"),
            root.join("app/config/strategies.json"),
        )
        .unwrap();
        let old = Settings {
            autostart: true,
            ..Default::default()
        };
        old.save(&root).unwrap();
        let bad = Backup {
            format: 1,
            settings: Settings::default(),
            lists: BTreeMap::from([("../outside.txt".into(), "a.com".into())]),
            account: BTreeMap::new(),
        };
        assert!(apply_backup(&root, bad).is_err());
        assert!(!root.join("data/import-backups").exists());
        let good = Backup {
            format: 1,
            settings: Settings {
                strategy: 1,
                ..Default::default()
            },
            lists: BTreeMap::from([("proxy-domains.txt".into(), "*.Example.com".into())]),
            account: BTreeMap::new(),
        };
        apply_backup(&root, good).unwrap();
        assert!(Settings::load(&root).autostart);
        assert_eq!(Settings::load(&root).strategy, 1);
        assert_eq!(
            fs::read_to_string(root.join("app/config/lists/proxy-domains.txt")).unwrap(),
            "+.example.com\n"
        );
        assert_eq!(
            Settings::load(
                &fs::read_dir(root.join("data/import-backups"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path()
            )
            .strategy,
            old.strategy
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn best_profile_uses_reliability_and_only_current_successes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-profile-ranking");
        fs::create_dir_all(&root).unwrap();
        record(&root, 0, None).unwrap();
        record(&root, 0, Some(20)).unwrap();
        record(&root, 1, Some(100)).unwrap();
        record(&root, 2, Some(1)).unwrap();
        assert_eq!(best(&root, &[(0, 20), (1, 100)]), Some(1));
        assert_eq!(best(&root, &[]), None);
        fs::remove_dir_all(root).unwrap();
    }
}
