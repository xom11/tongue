//! Suy mode hiện tại từ trạng thái thật — không cache, không state file.

pub struct Snapshot {
    pub mode: String,
    pub layout: Option<String>,
    pub ime_on: bool,
    pub drift: Option<String>,
}

// Call site thật nằm trong main.rs dưới cfg(target_os = "macos"); test riêng chạy
// trên mọi OS — trên build windows đây là dead code hợp lệ.
//
// Thứ tự nhánh có ý nghĩa: khi sources.vi KHÁC sources.en (backend `system`),
// chính layout phân biệt vi/en nên phải xét trước. Khi chúng TRÙNG nhau (bộ gõ
// ngoài), nhánh đó tự động không khớp và rơi xuống nhánh phân biệt bằng bit IME.
#[allow(dead_code)]
pub fn infer_mac(
    ime_on: bool,
    layout: &str,
    sources: &crate::mode::Sources,
) -> (String, Option<String>) {
    if layout == sources.zh {
        let drift = ime_on.then(|| {
            "bộ gõ ngoài đang chạy cùng Pinyin — chạy `tongue zh` hoặc `tongue vi` để dọn".into()
        });
        return ("zh".into(), drift);
    }
    if layout == sources.vi && sources.vi != sources.en {
        // Tiếng Việt đến thẳng từ input source (backend `system`). KHÔNG xét ime_on
        // ở đây: backend đó luôn báo tắt theo thiết kế (xem backend::macos::system),
        // nên "có app ngoài chạy chồng" là việc của `tongue doctor`, không phải chỗ này.
        return ("vi".into(), None);
    }
    if layout == sources.en || layout == sources.vi {
        return (if ime_on { "vi" } else { "en" }.into(), None);
    }
    ("unknown".into(), Some(format!("layout lạ: {layout}")))
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

    use crate::mode::Sources;

    const ABC: &str = "com.apple.keylayout.ABC";
    const PINYIN: &str = "com.apple.inputmethod.SCIM.ITABC";
    const TELEX: &str = "com.apple.inputmethod.VietnameseIM.VietnameseTelex";

    fn app_sources() -> Sources {
        Sources {
            vi: ABC.into(),
            en: ABC.into(),
            zh: PINYIN.into(),
        }
    }

    fn system_sources() -> Sources {
        Sources {
            vi: TELEX.into(),
            en: ABC.into(),
            zh: PINYIN.into(),
        }
    }

    #[test]
    fn mac_suy_ra_du_bon_to_hop() {
        let s = app_sources();
        assert_eq!(infer_mac(true, ABC, &s), ("vi".into(), None));
        assert_eq!(infer_mac(false, ABC, &s), ("en".into(), None));
        assert_eq!(infer_mac(false, PINYIN, &s), ("zh".into(), None));
        // GoNhanh chạy cùng Pinyin = trạng thái lệch, phải cảnh báo
        let (mode, drift) = infer_mac(true, PINYIN, &s);
        assert_eq!(mode, "zh");
        assert!(drift.is_some());
    }

    #[test]
    fn system_layout_telex_la_vi_du_khong_co_app_nao_chay() {
        // Ca này chính là thứ code cũ suy sai: không có app ngoài => ime_on false,
        // mà layout lại không phải ABC nên bảng cũ trả "unknown".
        assert_eq!(
            infer_mac(false, TELEX, &system_sources()),
            ("vi".into(), None)
        );
    }

    #[test]
    fn system_layout_abc_la_en() {
        assert_eq!(
            infer_mac(false, ABC, &system_sources()),
            ("en".into(), None)
        );
    }

    #[test]
    fn mac_layout_la_thi_unknown() {
        let (mode, drift) = infer_mac(false, "com.apple.keylayout.US", &app_sources());
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
