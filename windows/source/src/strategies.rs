//! Каталог профилей доставки WireGuard до WARP: `app/config/strategies.json`.
//! Профиль — эндпоинт, порт, мусорные пакеты AmneziaWG и, по желанию, способ маскировки
//! рукопожатия через winws (`zapret`: только ключи `--dpi-desync…`). Обвязку winws код
//! добавляет сам: только UDP-порт профиля и только адреса WARP из `warp-ips.txt`.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, net::Ipv4Addr, path::Path};

#[derive(Clone, Deserialize, Serialize)]
pub struct Strategy {
    pub name: String,
    pub endpoint: String,
    pub port: u16,
    pub jc: u8,
    pub jmin: u16,
    pub jmax: u16,
    /// Аргументы маскировки winws; `{zapret}` — папка `app/bin/zapret`. Нет поля — без winws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapret: Option<Vec<String>>,
}

/// Подсети WireGuard-эндпоинтов WARP (как в `app/config/lists/warp-ips.txt`).
const WARP_NETS: [[u8; 3]; 8] = [
    [162, 159, 192],
    [162, 159, 193],
    [162, 159, 195],
    [162, 159, 204],
    [188, 114, 96],
    [188, 114, 97],
    [188, 114, 98],
    [188, 114, 99],
];

fn valid_zapret(args: &[String]) -> bool {
    !args.is_empty()
        && args.len() <= 16
        && args.iter().all(|arg| {
            arg.starts_with("--dpi-desync")
                && !arg.contains("tcp")
                && !arg.contains("..")
                && arg.matches('{').count() == arg.matches("{zapret}").count()
        })
}

pub fn load(root: &Path) -> Result<Vec<Strategy>> {
    let json = match fs::read_to_string(root.join("app/config/strategies.json")) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            include_str!("../../app/config/strategies.json").to_owned()
        }
        Err(error) => return Err(error.into()),
    };
    let strategies: Vec<Strategy> = serde_json::from_str(&json)?;
    if strategies.is_empty() || strategies.len() > 200 {
        bail!("Каталог должен содержать от 1 до 200 профилей WARP");
    }
    for profile in &strategies {
        let warp_ip = profile
            .endpoint
            .parse::<Ipv4Addr>()
            .is_ok_and(|ip| WARP_NETS.contains(&[ip.octets()[0], ip.octets()[1], ip.octets()[2]]));
        if !warp_ip
            || profile.port == 0
            || profile.jc > 32
            || profile.jmin > profile.jmax
            || profile.jmax > 1280
            || profile
                .zapret
                .as_ref()
                .is_some_and(|args| !valid_zapret(args))
        {
            bail!("Недопустимые параметры WARP: {}", profile.name);
        }
    }
    Ok(strategies)
}

/// Отпечаток каталога: при смене профилей старая статистика и выбор по номеру сбрасываются.
pub fn fingerprint(catalog: &[Strategy]) -> String {
    let text = serde_json::to_string(catalog).unwrap_or_default();
    // FNV-1a: стабилен между версиями Rust, в отличие от DefaultHasher.
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

pub fn args(profile: &Strategy, root: &Path) -> Vec<String> {
    let Some(zapret) = &profile.zapret else {
        return Vec::new();
    };
    let bin = root.join("app/bin/zapret").display().to_string();
    let mut args = vec![
        format!("--wf-udp={}", profile.port),
        format!("--filter-udp={}", profile.port),
        format!(
            "--ipset={}",
            root.join("app/config/lists/warp-ips.txt").display()
        ),
        "--dpi-desync-any-protocol".into(),
    ];
    args.extend(zapret.iter().map(|arg| arg.replace("{zapret}", &bin)));
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_targets_only_warp_udp() {
        let root = Path::new("D:/Тест проекта");
        let profiles = load(root).unwrap();
        assert!(profiles.len() >= 20);
        for profile in &profiles {
            let args = args(profile, root);
            assert!(!args.iter().any(|arg| arg.contains("--wf-tcp")
                || arg.contains("--hostlist")
                || arg.contains("--filter-tcp")));
            if profile.zapret.is_some() {
                assert!(args
                    .iter()
                    .any(|arg| arg == &format!("--wf-udp={}", profile.port)));
                assert!(args
                    .iter()
                    .any(|arg| arg.starts_with("--ipset=D:/Тест проекта")));
                assert!(args
                    .iter()
                    .any(|arg| arg.starts_with("--dpi-desync-cutoff=")));
                assert!(!args.iter().any(|arg| arg.contains("{zapret}")));
            }
        }
    }
    #[test]
    fn catalog_has_no_duplicates_and_bundled_payloads_exist() {
        let profiles = load(Path::new("missing-root")).unwrap();
        let mut names = std::collections::BTreeSet::new();
        let mut params = std::collections::BTreeSet::new();
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/bin/zapret");
        for profile in &profiles {
            assert!(names.insert(profile.name.clone()), "{}", profile.name);
            let mut without_name = profile.clone();
            without_name.name.clear();
            assert!(
                params.insert(serde_json::to_string(&without_name).unwrap()),
                "дубль параметров: {}",
                profile.name
            );
            for arg in profile.zapret.iter().flatten() {
                if let Some(file) = arg.split("{zapret}/").nth(1) {
                    assert!(bin.join(file).is_file(), "нет {file}");
                }
            }
        }
    }
    #[test]
    fn rejects_unsafe_zapret_arguments() {
        for bad in [
            vec!["--wf-tcp=443".to_string()],
            vec!["--dpi-desync-split-tcp=1".into()],
            vec!["--dpi-desync-fake-unknown-udp={zapret}/../../x.bin".into()],
            vec!["--dpi-desync-fake-unknown-udp={other}/x.bin".into()],
            vec![],
        ] {
            assert!(!valid_zapret(&bad), "{bad:?}");
        }
        assert!(valid_zapret(&["--dpi-desync=fake".into()]));
    }
}
