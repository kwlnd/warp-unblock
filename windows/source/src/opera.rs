use anyhow::{bail, Context, Result};

pub const PORT: u16 = 1371;
pub const UNDERLAY_PORT: u16 = 17892;

pub fn launch_args() -> Vec<String> {
    vec![
        "-country".into(),
        "EU".into(),
        "-bind-address".into(),
        format!("127.0.0.1:{PORT}"),
        "-server-selection".into(),
        "first".into(),
        // Includes both SurfEasy registration/discovery and TLS to Opera servers.
        "-proxy".into(),
        format!("http://127.0.0.1:{UNDERLAY_PORT}"),
    ]
}

pub fn country_from_trace(trace: &str) -> Result<String> {
    let country = trace
        .lines()
        .find_map(|line| line.strip_prefix("loc="))
        .context("Opera не вернула страну выхода")?
        .trim();
    if country.len() != 2 || !country.bytes().all(|c| c.is_ascii_uppercase()) {
        bail!("Opera вернула некорректную страну выхода");
    }
    if ["RU", "BY", "CN", "IR"].contains(&country) {
        bail!("Выход Opera находится в {country}; нужен зарубежный выход для AI");
    }
    Ok(country.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_malformed_or_region_blocked_exit() {
        assert_eq!(country_from_trace("ip=redacted\nloc=NL\n").unwrap(), "NL");
        assert!(country_from_trace("loc=RU\n").is_err());
        assert!(country_from_trace("<html>error</html>").is_err());
        assert!(country_from_trace("loc=Netherlands\n").is_err());
    }
}
