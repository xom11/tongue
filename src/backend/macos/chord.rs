//! Chord toggle của GoNhanh: blob JSON trong defaults -> keyCode + flags.
//! Thuần, không FFI — test chạy được mà không cần GoNhanh trên máy.

use anyhow::{Context, Result};
use serde::Deserialize;

/// NSEvent modifier flags và CGEventFlags TRÙNG bit nhau:
/// CapsLock 1<<16, Shift 1<<17, Control 1<<18, Option 1<<19, Command 1<<20.
/// Nên chuyển đổi là identity — không có bảng ánh xạ nào cả. Mask này chỉ để
/// bỏ 16 bit device-dependent thấp (trái/phải của phím bổ trợ) và các bit lạ.
const MODIFIER_MASK: u64 = 0x001F_0000;

const FLAG_CAPS: u64 = 1 << 16;
const FLAG_SHIFT: u64 = 1 << 17;
const FLAG_CONTROL: u64 = 1 << 18;
const FLAG_OPTION: u64 = 1 << 19;
const FLAG_COMMAND: u64 = 1 << 20;

#[derive(Debug, PartialEq, Eq)]
pub struct Chord {
    pub key_code: u16,
    pub flags: u64,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "keyCode")]
    key_code: u16,
    modifiers: u64,
}

pub fn parse(blob: &[u8]) -> Result<Chord> {
    let raw: Raw = serde_json::from_slice(blob)
        .context("gonhanh.shortcut.toggle không phải JSON dạng {\"keyCode\":N,\"modifiers\":N}")?;
    Ok(Chord {
        key_code: raw.key_code,
        flags: raw.modifiers & MODIFIER_MASK,
    })
}

/// Chỉ phục vụ `doctor` — không nằm trên đường switch, nên chỉ cần đủ tên phím
/// thông dụng; phím lạ in thẳng mã số thay vì đoán bừa.
pub fn describe(c: &Chord) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, ten) in [
        (FLAG_CONTROL, "Ctrl"),
        (FLAG_OPTION, "Option"),
        (FLAG_SHIFT, "Shift"),
        (FLAG_COMMAND, "Cmd"),
        (FLAG_CAPS, "CapsLock"),
    ] {
        if c.flags & bit != 0 {
            parts.push(ten.into());
        }
    }
    parts.push(match c.key_code {
        36 => "Return".into(),
        48 => "Tab".into(),
        49 => "Space".into(),
        53 => "Esc".into(),
        n => format!("keyCode {n}"),
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chord thật đọc từ máy 30/07/2026: Ctrl+Shift+Space.
    const THAT: &[u8] = br#"{"keyCode":49,"modifiers":393216}"#;

    #[test]
    fn parse_chord_that_tu_may() {
        let c = parse(THAT).unwrap();
        assert_eq!(
            c,
            Chord {
                key_code: 49,
                flags: 0x0006_0000
            }
        );
    }

    /// GoNhanh có thể ghi kèm bit device-dependent (trái/phải) ở 16 bit thấp —
    /// CGEventSetFlags không cần chúng, và giữ lại dễ làm chord không khớp.
    #[test]
    fn mask_bo_bit_device_dependent_va_bit_la() {
        let blob = br#"{"keyCode":49,"modifiers":4294967295}"#;
        let c = parse(blob).unwrap();
        assert_eq!(c.flags, 0x001F_0000);
    }

    #[test]
    fn json_hong_thi_bao_loi() {
        assert!(parse(b"khong-phai-json").is_err());
    }

    #[test]
    fn thieu_field_modifiers_thi_bao_loi() {
        assert!(parse(br#"{"keyCode":49}"#).is_err());
    }

    #[test]
    fn describe_chord_that() {
        let c = Chord {
            key_code: 49,
            flags: 0x0006_0000,
        };
        assert_eq!(describe(&c), "Ctrl+Shift+Space");
    }

    #[test]
    fn describe_phim_la_in_thang_ma_so() {
        let c = Chord {
            key_code: 200,
            flags: 0,
        };
        assert_eq!(describe(&c), "keyCode 200");
    }

    #[test]
    fn describe_du_bon_phim_bo_tro() {
        let c = Chord {
            key_code: 36,
            flags: FLAG_CONTROL | FLAG_OPTION | FLAG_SHIFT | FLAG_COMMAND,
        };
        assert_eq!(describe(&c), "Ctrl+Option+Shift+Cmd+Return");
    }
}
