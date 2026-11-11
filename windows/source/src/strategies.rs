//! Каталог WARP: `app/config/strategies.json` — пул адресов и профили маскировки.
//! Профиль — мусорные пакеты AmneziaWG и, по желанию, приём winws (`zapret`: только ключи
//! `--dpi-desync…`). Адрес и порт профиль не задаёт: программа подбирает их из пула сама,
//! потому что провайдер режет связку «адрес + порт», а маскировку — отдельно. Обвязку winws
//! код добавляет сам: только UDP-порт выбранного адреса и только адреса WARP из `warp-ips.txt`.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, net::Ipv4Addr, path::Path};

/// Профиль маскировки из каталога.
#[derive(Clone, Deserialize, Serialize)]
pub struct Profile {
    pub name: String,
    pub jc: u8,
    pub jmin: u16,
    pub jmax: u16,
    /// Аргументы маскировки winws; `{zapret}` — папка `app/bin/zapret`. Нет поля — без winws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zapret: Option<Vec<String>>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Catalog {
    /// Пул адресов WARP «ip:порт» в порядке предпочтения.
    pub endpoints: Vec<String>,
    pub profiles: Vec<Profile>,
}

/// Профиль, собранный с конкретным адресом: так он пишется в warp.yaml и аргументы winws.
#[derive(Clone)]
pub struct Strategy {
    pub name: String,
    pub endpoint: String,
    pub port: u16,
    pub jc: u8,
    pub jmin: u16,
    pub jmax: u16,
    pub zapret: Option<Vec<String>>,
}

impl Catalog {
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Номер адреса в пуле; неизвестный или пустой — первый.
    pub fn endpoint_index(&self, endpoint: &str) -> usize {
        self.endpoints
            .iter()
            .position(|e| e == endpoint)
            .unwrap_or(0)
    }

    pub fn resolve(&self, profile: usize, endpoint: usize) -> Strategy {
        let p = &self.profiles[profile.min(self.profiles.len() - 1)];
        let address = &self.endpoints[endpoint % self.endpoints.len()];
        let (ip, port) = split(address).expect("адрес проверен при загрузке");
        Strategy {
            name: p.name.clone(),
            endpoint: ip.to_string(),
            port,
            jc: p.jc,
            jmin: p.jmin,
            jmax: p.jmax,
            zapret: p.zapret.clone(),
        }
    }
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

fn split(address: &str) -> Option<(Ipv4Addr, u16)> {
    let (ip, port) = address.split_once(':')?;
    let ip = ip.parse::<Ipv4Addr>().ok()?;
    let port = port.parse::<u16>().ok().filter(|p| *p != 0)?;
    WARP_NETS
        .contains(&[ip.octets()[0], ip.octets()[1], ip.octets()[2]])
        .then_some((ip, port))
}

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

pub fn load(root: &Path) -> Result<Catalog> {
    let json = match fs::read_to_string(root.join("app/config/strategies.json")) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            include_str!("../../app/config/strategies.json").to_owned()
        }
        Err(error) => return Err(error.into()),
    };
    parse(&json)
}

pub fn parse(json: &str) -> Result<Catalog> {
    let catalog: Catalog = serde_json::from_str(json)?;
    if catalog.profiles.is_empty() || catalog.profiles.len() > 64 {
        bail!("Каталог должен содержать от 1 до 64 профилей WARP");
    }
    if catalog.endpoints.is_empty() || catalog.endpoints.len() > 64 {
        bail!("Пул должен содержать от 1 до 64 адресов WARP");
    }
    for address in &catalog.endpoints {
        if split(address).is_none() {
            bail!("Недопустимый адрес WARP: {address}");
        }
    }
    for profile in &catalog.profiles {
        if profile.jc > 32
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
    Ok(catalog)
}

/// Отпечаток каталога: при смене профилей старая статистика и выбор по номеру сбрасываются.
pub fn fingerprint(catalog: &Catalog) -> String {
    let text = serde_json::to_string(catalog).unwrap_or_default();
    // FNV-1a: стабилен между версиями Rust, в отличие от DefaultHasher.
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// Порядок проверки при подборе: сначала все адреса текущего профиля, начиная с `endpoint`,
/// затем остальные профили. Провайдер обычно режет адрес — тогда хватает соседнего адреса;
/// если не прошёл ни один, значит узнали саму маскировку, и дальше идут другие профили.
pub fn search_order(catalog: &Catalog, profile: usize, endpoint: usize) -> Vec<(usize, usize)> {
    let (n, m) = (catalog.profiles.len(), catalog.endpoints.len());
    let profile = profile.min(n - 1);
    std::iter::once(profile)
        .chain((0..n).filter(|p| *p != profile))
        .flat_map(|p| (0..m).map(move |k| (p, (endpoint + k) % m)))
        .collect()
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
        let catalog = load(root).unwrap();
        assert!(catalog.profiles.len() >= 8);
        assert!(catalog.endpoints.len() >= 8);
        for p in 0..catalog.len() {
            for e in 0..catalog.endpoints.len() {
                let profile = catalog.resolve(p, e);
                let args = args(&profile, root);
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
    }
    #[test]
    fn catalog_has_no_duplicates_and_bundled_payloads_exist() {
        let catalog = load(Path::new("missing-root")).unwrap();
        let mut names = std::collections::BTreeSet::new();
        let mut params = std::collections::BTreeSet::new();
        let mut endpoints = std::collections::BTreeSet::new();
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/bin/zapret");
        for address in &catalog.endpoints {
            assert!(endpoints.insert(address.clone()), "дубль адреса: {address}");
        }
        for profile in &catalog.profiles {
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
    fn search_tries_the_current_profile_on_every_address_first() {
        let catalog = load(Path::new("missing-root")).unwrap();
        let m = catalog.endpoints.len();
        let order = search_order(&catalog, 2, 3);
        assert_eq!(order.len(), catalog.len() * m);
        assert_eq!(order[0], (2, 3));
        assert_eq!(order[m - 1], (2, 2));
        assert!(order[..m].iter().all(|(p, _)| *p == 2));
        assert_eq!(order[m], (0, 3));
    }
    #[test]
    fn rejects_bad_addresses_and_unsafe_zapret_arguments() {
        for bad in ["1.1.1.1:2408", "162.159.192.1:0", "162.159.192.1", "x:1"] {
            assert!(split(bad).is_none(), "{bad}");
        }
        assert!(split("188.114.97.1:4500").is_some());
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
