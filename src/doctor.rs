//! Biến các "lỗi im lặng" (perAppMode, smart_switch, chạy admin, thiếu
//! source...) thành chẩn đoán nhìn thấy được. --fix chỉ sửa những gì an toàn.

use crate::config::Config;
use anyhow::Result;

pub enum Level {
    Ok,
    Warn,
    Fail,
}

pub struct Finding {
    pub level: Level,
    pub msg: String,
}

pub fn print_findings(fs: &[Finding]) -> bool {
    let mut failed = false;
    for f in fs {
        let icon = match f.level {
            Level::Ok => "✓",
            Level::Warn => "⚠",
            Level::Fail => {
                failed = true;
                "✗"
            }
        };
        println!("{icon} {}", f.msg);
    }
    failed
}

#[cfg(target_os = "macos")]
pub fn run(fix: bool, cfg: &Config, ime: &dyn crate::backend::Ime) -> Result<bool> {
    use crate::backend::macos::tis;

    let mut fs = vec![Finding {
        level: Level::Ok,
        msg: format!("backend = {}", cfg.macos.backend),
    }];

    // 1. phần khám riêng của bộ gõ đang chọn — doctor không cần biết đó là ai
    fs.extend(ime.diagnose(fix)?);

    // 2. các input source phải được bật trong System Settings.
    //    vi và en thường TRÙNG nhau (bộ gõ ngoài giữ nguyên layout) — khử trùng
    //    để khỏi in hai dòng y hệt.
    let mut seen = Vec::new();
    for (label, id) in [
        ("source_vi", &cfg.macos.source_vi),
        ("source_en", &cfg.macos.source_en),
        ("source_zh", &cfg.macos.source_zh),
    ] {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        if tis::source_exists(id)? {
            fs.push(Finding {
                level: Level::Ok,
                msg: format!("{label}: {id} có mặt"),
            });
        } else {
            fs.push(Finding {
                level: Level::Fail,
                msg: format!(
                    "{label}: {id} chưa bật trong System Settings > Keyboard > Input Sources"
                ),
            });
        }
    }

    // 3. backend `system` mà vi và en trùng layout thì chuyển mode thành no-op —
    //    lỗi cấu hình im lặng nhất có thể, phải bắt.
    if cfg.macos.backend == "system" && cfg.macos.source_vi == cfg.macos.source_en {
        fs.push(Finding {
            level: Level::Fail,
            msg: "backend = system nhưng source_vi trùng source_en — không có app ngoài thì layout là thứ duy nhất phân biệt vi/en, trùng nhau nghĩa là `tongue vi` và `tongue en` không làm gì cả".into(),
        });
    }

    // 4. strategy
    if matches!(cfg.macos.strategy.as_str(), "process" | "hotkey") {
        fs.push(Finding {
            level: Level::Ok,
            msg: format!("strategy = {}", cfg.macos.strategy),
        });
    } else {
        fs.push(Finding {
            level: Level::Fail,
            msg: format!(
                "strategy '{}' không hợp lệ (process|hotkey)",
                cfg.macos.strategy
            ),
        });
    }

    Ok(print_findings(&fs))
}

// Windows không có layout để khám (US cố định) nên toàn bộ phần khám nằm trong
// VkeyIme::diagnose — đối xứng với macOS, và doctor không cần biết VKey là gì.
#[cfg(windows)]
pub fn run(fix: bool, _cfg: &Config, ime: &dyn crate::backend::Ime) -> Result<bool> {
    Ok(print_findings(&ime.diagnose(fix)?))
}
