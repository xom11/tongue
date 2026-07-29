//! Suy mode hiện tại từ trạng thái thật — không cache, không state file.

pub struct Snapshot {
    pub mode: String,
    pub layout: Option<String>,
    pub ime_on: bool,
    pub drift: Option<String>,
}

pub fn infer_mac(
    ime_on: bool,
    layout: &str,
    source_vi: &str,
    source_zh: &str,
) -> (String, Option<String>) {
    if layout == source_vi {
        if ime_on {
            ("vi".into(), None)
        } else {
            ("en".into(), None)
        }
    } else if layout == source_zh {
        if ime_on {
            (
                "zh".into(),
                Some(
                    "GoNhanh đang chạy cùng Pinyin — chạy `tongue zh` hoặc `tongue vi` để dọn"
                        .into(),
                ),
            )
        } else {
            ("zh".into(), None)
        }
    } else {
        ("unknown".into(), Some(format!("layout lạ: {layout}")))
    }
}

// Call site thật nằm trong main.rs dưới cfg(windows); test riêng chạy trên mọi OS.
#[allow(dead_code)]
pub fn infer_win(ime_on: bool) -> String {
    if ime_on {
        "vi".into()
    } else {
        "en".into()
    }
}

pub fn render_human(s: &Snapshot) -> String {
    let mut out = format!("mode:   {}\n", s.mode);
    if let Some(l) = &s.layout {
        out.push_str(&format!("layout: {l}\n"));
    }
    out.push_str(&format!(
        "ime:    {}\n",
        if s.ime_on { "bật" } else { "tắt" }
    ));
    if let Some(d) = &s.drift {
        out.push_str(&format!("lệch:   {d}\n"));
    }
    out
}

pub fn render_json(s: &Snapshot) -> String {
    // Các giá trị đều là token ASCII (source ID, mode) — không cần escape phức tạp
    let quote = |v: &Option<String>| match v {
        Some(x) => format!("\"{}\"", x.replace('"', "\\\"")),
        None => "null".into(),
    };
    format!(
        "{{\"mode\":\"{}\",\"layout\":{},\"ime_on\":{},\"drift\":{}}}\n",
        s.mode,
        quote(&s.layout),
        s.ime_on,
        quote(&s.drift)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "com.apple.keylayout.ABC";
    const PINYIN: &str = "com.apple.inputmethod.SCIM.ITABC";

    #[test]
    fn mac_suy_ra_du_bon_to_hop() {
        assert_eq!(infer_mac(true, ABC, ABC, PINYIN), ("vi".into(), None));
        assert_eq!(infer_mac(false, ABC, ABC, PINYIN), ("en".into(), None));
        assert_eq!(infer_mac(false, PINYIN, ABC, PINYIN), ("zh".into(), None));
        // GoNhanh chạy cùng Pinyin = trạng thái lệch, phải cảnh báo
        let (mode, drift) = infer_mac(true, PINYIN, ABC, PINYIN);
        assert_eq!(mode, "zh");
        assert!(drift.is_some());
    }

    #[test]
    fn mac_layout_la_thi_unknown() {
        let (mode, drift) = infer_mac(false, "com.apple.keylayout.US", ABC, PINYIN);
        assert_eq!(mode, "unknown");
        assert!(drift.unwrap().contains("com.apple.keylayout.US"));
    }

    #[test]
    fn win_suy_ra_tu_bit() {
        assert_eq!(infer_win(true), "vi");
        assert_eq!(infer_win(false), "en");
    }

    #[test]
    fn render_json_dung_dang() {
        let s = Snapshot {
            mode: "vi".into(),
            layout: Some("abc".into()),
            ime_on: true,
            drift: None,
        };
        assert_eq!(
            render_json(&s),
            "{\"mode\":\"vi\",\"layout\":\"abc\",\"ime_on\":true,\"drift\":null}\n"
        );
    }
}
