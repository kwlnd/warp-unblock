use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Deserialize, Serialize)]
pub struct Strategy {
    pub name: String,
    pub endpoint: String,
    pub port: u16,
    pub jc: u8,
    pub jmin: u16,
    pub jmax: u16,
    pub winws: bool,
    pub repeats: u16,
    pub ttl: u8,
    pub cutoff: u8,
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
        if !["162.159.192.1", "162.159.193.1"].contains(&profile.endpoint.as_str())
            || ![500, 1701, 2408, 4500].contains(&profile.port)
            || profile.jc > 32
            || profile.jmin > profile.jmax
            || profile.jmax > 1280
            || profile.repeats == 0
            || profile.repeats > 32
            || profile.cutoff == 0
        {
            bail!("Недопустимые параметры WARP: {}", profile.name);
        }
    }
    Ok(strategies)
}

pub fn args(profile: &Strategy, root: &Path) -> Vec<String> {
    if !profile.winws {
        return Vec::new();
    }
    let mut args = vec![
        format!("--wf-udp={}", profile.port),
        format!("--filter-udp={}", profile.port),
        format!(
            "--ipset={}",
            root.join("app/config/lists/warp-ips.txt").display()
        ),
        "--dpi-desync=fake".into(),
        "--dpi-desync-any-protocol".into(),
        format!("--dpi-desync-repeats={}", profile.repeats),
        format!("--dpi-desync-cutoff=n{}", profile.cutoff),
    ];
    if profile.ttl > 0 {
        args.push(format!("--dpi-desync-ttl={}", profile.ttl));
    }
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
        for profile in profiles {
            let args = args(&profile, root);
            assert!(!args.iter().any(|arg| arg.contains("--wf-tcp")
                || arg.contains("--hostlist")
                || arg.contains("--filter-tcp")));
            if profile.winws {
                assert!(args
                    .iter()
                    .any(|arg| arg == &format!("--wf-udp={}", profile.port)));
                assert!(args
                    .iter()
                    .any(|arg| arg.starts_with("--ipset=D:/Тест проекта")));
                assert!(args
                    .iter()
                    .any(|arg| arg.starts_with("--dpi-desync-cutoff=")));
            }
        }
    }
}
