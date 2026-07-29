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

/// Ba input source của ba mode trên macOS.
///
/// `vi` và `en` TRÙNG NHAU khi tiếng Việt do một app ngoài lo (GoNhanh, EVKey...):
/// layout giữ nguyên ABC, chỉ bit IME phân biệt. Chúng KHÁC NHAU khi tiếng Việt
/// đến thẳng từ input source của macOS (backend `system`) — lúc đó layout mới là
/// thứ phân biệt, và không có app ngoài nào để bật.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sources {
    pub vi: String,
    pub en: String,
    pub zh: String,
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
///
/// `has_external_ime = false` (backend `system`) nghĩa là không có app ngoài nào
/// để bật — tiếng Việt hoàn toàn do `sources.vi` lo, nên `ime_on` phải là false
/// cho MỌI mode, nếu không reconcile sẽ đi tìm một app không tồn tại và verify
/// trượt vĩnh viễn.
pub fn desired(
    mode: Mode,
    platform: Platform,
    sources: &Sources,
    has_external_ime: bool,
) -> Option<Desired> {
    match (platform, mode) {
        (Platform::MacOs, Mode::Vi) => Some(Desired {
            layout: Some(sources.vi.clone()),
            ime_on: has_external_ime,
        }),
        (Platform::MacOs, Mode::En) => Some(Desired {
            layout: Some(sources.en.clone()),
            ime_on: false,
        }),
        (Platform::MacOs, Mode::Zh) => Some(Desired {
            layout: Some(sources.zh.clone()),
            ime_on: false,
        }),
        (Platform::Windows, Mode::Vi) => Some(Desired {
            layout: None,
            ime_on: has_external_ime,
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
    const TELEX: &str = "com.apple.inputmethod.VietnameseIM.VietnameseTelex";

    /// Bộ gõ ngoài lo tiếng Việt: vi và en dùng chung layout ABC.
    fn app_sources() -> Sources {
        Sources {
            vi: ABC.into(),
            en: ABC.into(),
            zh: PINYIN.into(),
        }
    }

    /// Bộ gõ hệ thống của macOS: vi có layout riêng, không có app ngoài.
    fn system_sources() -> Sources {
        Sources {
            vi: TELEX.into(),
            en: ABC.into(),
            zh: PINYIN.into(),
        }
    }

    #[test]
    fn parse_mode() {
        assert_eq!("vi".parse::<Mode>().unwrap(), Mode::Vi);
        assert_eq!("zh".parse::<Mode>().unwrap(), Mode::Zh);
        assert!("xx".parse::<Mode>().is_err());
    }

    #[test]
    fn mac_vi_bat_ime_layout_abc() {
        let d = desired(Mode::Vi, Platform::MacOs, &app_sources(), true).unwrap();
        assert_eq!(d.layout.as_deref(), Some(ABC));
        assert!(d.ime_on);
    }

    #[test]
    fn mac_en_tat_ime_layout_abc() {
        let d = desired(Mode::En, Platform::MacOs, &app_sources(), true).unwrap();
        assert_eq!(d.layout.as_deref(), Some(ABC));
        assert!(!d.ime_on);
    }

    #[test]
    fn mac_zh_tat_ime_layout_pinyin() {
        let d = desired(Mode::Zh, Platform::MacOs, &app_sources(), true).unwrap();
        assert_eq!(d.layout.as_deref(), Some(PINYIN));
        assert!(!d.ime_on);
    }

    #[test]
    fn system_vi_doi_layout_va_khong_bat_app_nao() {
        let d = desired(Mode::Vi, Platform::MacOs, &system_sources(), false).unwrap();
        assert_eq!(d.layout.as_deref(), Some(TELEX));
        // không có app ngoài — đòi bật là đòi thứ không tồn tại
        assert!(!d.ime_on);
    }

    #[test]
    fn system_en_ve_layout_rieng_chu_khong_dung_layout_cua_vi() {
        let d = desired(Mode::En, Platform::MacOs, &system_sources(), false).unwrap();
        assert_eq!(d.layout.as_deref(), Some(ABC));
        assert!(!d.ime_on);
    }

    #[test]
    fn win_khong_dung_layout() {
        let s = app_sources();
        let vi = desired(Mode::Vi, Platform::Windows, &s, true).unwrap();
        assert!(vi.layout.is_none() && vi.ime_on);
        let en = desired(Mode::En, Platform::Windows, &s, true).unwrap();
        assert!(en.layout.is_none() && !en.ime_on);
    }

    #[test]
    fn win_khong_co_zh() {
        assert!(desired(Mode::Zh, Platform::Windows, &app_sources(), true).is_none());
    }
}
