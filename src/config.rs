//! Config tuỳ chọn — vắng file thì mọi giá trị là default, tool vẫn chạy.

use anyhow::Context;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub macos: MacosConfig,
    pub windows: WindowsConfig,
    pub verify: VerifyConfig,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacosConfig {
    pub strategy: String,
    pub source_vi: String,
    pub source_zh: String,
    pub app_name: String,
}

impl Default for MacosConfig {
    fn default() -> Self {
        Self {
            strategy: "process".into(),
            source_vi: "com.apple.keylayout.ABC".into(),
            source_zh: "com.apple.inputmethod.SCIM.ITABC".into(),
            app_name: "GoNhanh".into(),
        }
    }
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowsConfig {
    pub vkey_path: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct VerifyConfig {
    pub timeout_ms: u64,
    pub poll_ms: u64,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self { timeout_ms: 1000, poll_ms: 50 }
    }
}

pub fn parse(text: &str) -> anyhow::Result<Config> {
    toml::from_str(text).context("config.toml không hợp lệ")
}

fn default_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // ~/.config chứ không phải ~/Library/Application Support — đồng bộ với
        // các dotfile khác của chủ repo.
        dirs::home_dir().map(|h| h.join(".config/tongue/config.toml"))
    }
    #[cfg(windows)]
    {
        dirs::config_dir().map(|d| d.join("tongue/config.toml"))
    }
}

pub fn load() -> anyhow::Result<Config> {
    match default_path() {
        Some(p) if p.exists() => {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("không đọc được {}", p.display()))?;
            parse(&text)
        }
        _ => Ok(Config::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn khong_co_gi_thi_ra_default() {
        let c = parse("").unwrap();
        assert_eq!(c.macos.strategy, "process");
        assert_eq!(c.macos.source_vi, "com.apple.keylayout.ABC");
        assert_eq!(c.macos.source_zh, "com.apple.inputmethod.SCIM.ITABC");
        assert_eq!(c.macos.app_name, "GoNhanh");
        assert_eq!(c.windows.vkey_path, "");
        assert_eq!(c.verify.timeout_ms, 1000);
        assert_eq!(c.verify.poll_ms, 50);
    }

    #[test]
    fn override_mot_phan_giu_default_phan_con_lai() {
        let c = parse("[verify]\ntimeout_ms = 3000\n").unwrap();
        assert_eq!(c.verify.timeout_ms, 3000);
        assert_eq!(c.verify.poll_ms, 50);
        assert_eq!(c.macos.strategy, "process");
    }

    #[test]
    fn toml_hong_thi_bao_loi() {
        assert!(parse("[macos\nstrategy=").is_err());
    }
}
