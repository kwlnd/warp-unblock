//! Способы обхода DPI для регистрации аккаунта WARP: `app/config/registration.json`, общий
//! файл для Windows (winws) и Android (ByeDPI). У способа может быть вариант для одной
//! платформы. Обвязку winws код добавляет сам: только TCP 443 и только домены регистрации.
use anyhow::{bail, Result};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Clone, Deserialize)]
pub struct Method {
    pub name: String,
    /// Ключи `--dpi-desync…` для winws; `{zapret}` — папка `app/bin/zapret`.
    #[serde(default)]
    pub winws: Option<Vec<String>>,
    /// Аргументы ByeDPI для Android (здесь только читаются ради общей проверки файла).
    #[serde(default)]
    #[allow(dead_code)]
    pub byedpi: Option<Vec<String>>,
}

fn valid_winws(args: &[String]) -> bool {
    !args.is_empty()
        && args.len() <= 16
        && args.iter().all(|arg| {
            arg.starts_with("--dpi-desync")
                && !arg.contains("..")
                && arg.matches('{').count() == arg.matches("{zapret}").count()
        })
}

pub fn load(root: &Path) -> Result<Vec<Method>> {
    let json = match fs::read_to_string(root.join("app/config/registration.json")) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            include_str!("../../app/config/registration.json").to_owned()
        }
        Err(error) => return Err(error.into()),
    };
    let methods: Vec<Method> = serde_json::from_str(&json)?;
    if methods.len() > 50 {
        bail!("Слишком много способов регистрации");
    }
    for method in &methods {
        if method.winws.as_ref().is_some_and(|args| !valid_winws(args)) {
            bail!("Недопустимые аргументы регистрации: {}", method.name);
        }
    }
    Ok(methods)
}

/// Полные аргументы winws: только TCP 443 к доменам из `warp-hosts.txt`.
pub fn winws_args(method: &[String], root: &Path) -> Vec<String> {
    let bin = root.join("app/bin/zapret").display().to_string();
    let mut args = vec![
        "--wf-tcp=443".to_string(),
        "--filter-tcp=443".into(),
        format!(
            "--hostlist={}",
            root.join("app/config/lists/warp-hosts.txt").display()
        ),
    ];
    args.extend(method.iter().map(|arg| arg.replace("{zapret}", &bin)));
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_methods_are_valid_and_scoped_to_registration() {
        let root = Path::new("D:/Тест");
        let methods = load(Path::new("missing-root")).unwrap();
        assert!(methods.iter().filter(|m| m.winws.is_some()).count() >= 5);
        let mut names = std::collections::BTreeSet::new();
        for method in &methods {
            assert!(names.insert(&method.name), "дубль: {}", method.name);
            assert!(method.winws.is_some() || method.byedpi.is_some());
            if let Some(winws) = &method.winws {
                let args = winws_args(winws, root);
                assert_eq!(args[0], "--wf-tcp=443");
                assert!(args.iter().any(|a| a.starts_with("--hostlist=D:/Тест")));
                assert!(!args
                    .iter()
                    .any(|a| a.contains("{zapret}") || a.contains("udp")));
            }
        }
        assert!(!valid_winws(&["--wf-udp=1-65535".into()]));
    }
}
