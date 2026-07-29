//! Bảng mode → trạng thái đích. Thuần, không chạm OS.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Vi,
    En,
    Zh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    // Chỉ được construct thật trong Platform::current() dưới cfg(target_os = "macos"),
    // hoặc trong test của chính file này (desired() vốn thuần, test cross-platform trên
    // mọi OS) — trên build windows đây là dead code hợp lệ.
    #[allow(dead_code)]
    MacOs,
    // Chỉ được construct thật trong Platform::current() dưới cfg(windows), hoặc trong
    // test của chính file này (desired() vốn thuần, test cross-platform trên mọi OS).
    #[allow(dead_code)]
    Windows,
}

/// Trạng thái đích của một mode: layout hệ thống (None = không đụng) + bộ gõ ngoài phải bật?
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desired {
    pub layout: Option<String>,
    pub ime_on: bool,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Vi => "vi",
            Mode::En => "en",
            Mode::Zh => "zh",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "vi" => Ok(Mode::Vi),
            "en" => Ok(Mode::En),
            "zh" => Ok(Mode::Zh),
            other => Err(format!("mode không hợp lệ: {other} (vi|en|zh)")),
        }
    }
}

impl Platform {
    #[cfg(target_os = "macos")]
    pub fn current() -> Platform {
        Platform::MacOs
    }
    #[cfg(windows)]
    pub fn current() -> Platform {
        Platform::Windows
    }
}

/// None = mode không tồn tại trên nền tảng này (zh trên Windows).
pub fn desired(
    mode: Mode,
    platform: Platform,
    source_vi: &str,
    source_zh: &str,
) -> Option<Desired> {
    match (platform, mode) {
        (Platform::MacOs, Mode::Vi) => Some(Desired {
            layout: Some(source_vi.into()),
            ime_on: true,
        }),
        (Platform::MacOs, Mode::En) => Some(Desired {
            layout: Some(source_vi.into()),
            ime_on: false,
        }),
        (Platform::MacOs, Mode::Zh) => Some(Desired {
            layout: Some(source_zh.into()),
            ime_on: false,
        }),
        (Platform::Windows, Mode::Vi) => Some(Desired {
            layout: None,
            ime_on: true,
        }),
        (Platform::Windows, Mode::En) => Some(Desired {
            layout: None,
            ime_on: false,
        }),
        (Platform::Windows, Mode::Zh) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "com.apple.keylayout.ABC";
    const PINYIN: &str = "com.apple.inputmethod.SCIM.ITABC";

    #[test]
    fn parse_mode() {
        assert_eq!("vi".parse::<Mode>().unwrap(), Mode::Vi);
        assert_eq!("zh".parse::<Mode>().unwrap(), Mode::Zh);
        assert!("xx".parse::<Mode>().is_err());
    }

    #[test]
    fn mac_vi_bat_ime_layout_abc() {
        let d = desired(Mode::Vi, Platform::MacOs, ABC, PINYIN).unwrap();
        assert_eq!(d.layout.as_deref(), Some(ABC));
        assert!(d.ime_on);
    }

    #[test]
    fn mac_en_tat_ime_layout_abc() {
        let d = desired(Mode::En, Platform::MacOs, ABC, PINYIN).unwrap();
        assert_eq!(d.layout.as_deref(), Some(ABC));
        assert!(!d.ime_on);
    }

    #[test]
    fn mac_zh_tat_ime_layout_pinyin() {
        let d = desired(Mode::Zh, Platform::MacOs, ABC, PINYIN).unwrap();
        assert_eq!(d.layout.as_deref(), Some(PINYIN));
        assert!(!d.ime_on);
    }

    #[test]
    fn win_khong_dung_layout() {
        let vi = desired(Mode::Vi, Platform::Windows, "", "").unwrap();
        assert!(vi.layout.is_none() && vi.ime_on);
        let en = desired(Mode::En, Platform::Windows, "", "").unwrap();
        assert!(en.layout.is_none() && !en.ime_on);
    }

    #[test]
    fn win_khong_co_zh() {
        assert!(desired(Mode::Zh, Platform::Windows, "", "").is_none());
    }
}
