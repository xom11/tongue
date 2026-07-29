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
pub fn run(fix: bool, cfg: &Config) -> Result<bool> {
    use crate::backend::macos::{gonhanh::GonhanhIme, tis};
    use crate::backend::Ime as _;
    use std::process::Command;

    let mut fs = Vec::new();

    // 1. GoNhanh.app có mặt?
    let home = std::env::var("HOME").unwrap_or_default();
    let app_paths = ["/Applications/GoNhanh.app".to_string(), format!("{home}/Applications/GoNhanh.app")];
    if app_paths.iter().any(|p| std::path::Path::new(p).exists()) {
        fs.push(Finding { level: Level::Ok, msg: "GoNhanh.app có mặt".into() });
    } else {
        fs.push(Finding {
            level: Level::Fail,
            msg: "không thấy GoNhanh.app trong /Applications hoặc ~/Applications".into(),
        });
    }

    // 2. perAppMode phải = 0 — nếu bật, GoNhanh ghi trạng thái theo từng app
    //    và key gonhanh.enabled thành đồ giả (bẫy đã xác minh trong source)
    let out = Command::new("defaults")
        .args(["read", "org.gonhanh.GoNhanh", "gonhanh.perAppMode"])
        .output()?;
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        fs.push(Finding {
            level: Level::Warn,
            msg: "chưa đọc được defaults của GoNhanh — app đã chạy lần đầu chưa?".into(),
        });
    } else if val == "0" {
        fs.push(Finding { level: Level::Ok, msg: "gonhanh.perAppMode = 0".into() });
    } else if fix {
        let st = Command::new("defaults")
            .args(["write", "org.gonhanh.GoNhanh", "gonhanh.perAppMode", "-bool", "NO"])
            .status()?;
        anyhow::ensure!(st.success(), "defaults write gonhanh.perAppMode thất bại");
        // defaults chỉ được đọc lúc khởi động → restart để nạp
        let g = GonhanhIme { app_name: cfg.macos.app_name.clone() };
        if g.is_on()? {
            g.set(false)?;
            g.set(true)?;
        }
        fs.push(Finding { level: Level::Ok, msg: "đã ghim gonhanh.perAppMode=0 và restart GoNhanh".into() });
    } else {
        fs.push(Finding {
            level: Level::Warn,
            msg: "gonhanh.perAppMode đang bật — chạy `tongue doctor --fix` để ghim về 0 (không thì trạng thái enabled không tin được)".into(),
        });
    }

    // 3. hai input source phải được bật trong System Settings
    for (label, id) in [("source_vi", &cfg.macos.source_vi), ("source_zh", &cfg.macos.source_zh)] {
        if tis::source_exists(id)? {
            fs.push(Finding { level: Level::Ok, msg: format!("{label}: {id} có mặt") });
        } else {
            fs.push(Finding {
                level: Level::Fail,
                msg: format!("{label}: {id} chưa bật trong System Settings > Keyboard > Input Sources"),
            });
        }
    }

    // 4. strategy
    if cfg.macos.strategy == "process" {
        fs.push(Finding { level: Level::Ok, msg: "strategy = process".into() });
    } else {
        fs.push(Finding { level: Level::Fail, msg: format!("strategy '{}' chưa hỗ trợ", cfg.macos.strategy) });
    }

    Ok(print_findings(&fs))
}

#[cfg(windows)]
pub fn run(_fix: bool, _cfg: &Config) -> Result<bool> {
    // Task 10 thay stub này bằng bản khám VKey thật.
    Ok(print_findings(&[Finding {
        level: Level::Warn,
        msg: "doctor chưa hỗ trợ trên Windows ở bản này".into(),
    }]))
}
