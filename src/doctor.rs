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

/// Khoá lạ trong config.toml. Đứng riêng vì cả hai nền tảng đều cần, và vì đây là
/// đúng chỗ để báo: `doctor` là thứ người ta chạy khi "tôi đặt rồi mà không ăn".
fn config_findings(cfg: &Config) -> Vec<Finding> {
    let unknown = cfg.unknown_keys();
    if unknown.is_empty() {
        return Vec::new();
    }
    vec![Finding {
        level: Level::Warn,
        msg: format!(
            "config.toml có khoá không nhận ra: {} — chúng bị BỎ QUA im lặng, giá trị đang \
             dùng là mặc định. Kiểm lại chính tả (ví dụ `agent_idle_ms`, không phải \
             `idle_exit_ms`).",
            unknown.join(", ")
        ),
    }]
}

#[cfg(target_os = "macos")]
pub fn run(fix: bool, cfg: &Config, ime: &dyn crate::backend::Ime) -> Result<bool> {
    use crate::backend::macos::{app, prefs, tis};

    let mut fs = config_findings(cfg);
    fs.push(Finding {
        level: Level::Ok,
        msg: format!("backend = {}", cfg.macos.backend),
    });

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

    // 5. hotkey → process: strategy hotkey thường xuyên để GoNhanh sống nhưng
    //    tắt tiếng Việt (`en`/`zh` đều kết thúc ở đó, đó là cả điểm của nó).
    //    Đổi cấu hình sang process ngay sau đó thì GonhanhIme::set(true) thấy
    //    app đã sống nên KHÔNG launch lại — defaults ghi enabled=1 nhưng
    //    instance đang chạy vẫn tắt tiếng Việt (chỉ đọc defaults lúc khởi
    //    động). is_on() = is_running() = true nên reconcile verify khớp: `tongue
    //    vi` báo exit 0 và `status` báo `vi` trong khi gõ tiếng Việt không hoạt
    //    động. Chỉ đúng dưới process — hotkey tự biết trạng thái này.
    if cfg.macos.backend == "gonhanh" && cfg.macos.strategy == "process" {
        let dang_chay = app::is_running(&cfg.macos.app_name)?;
        let enabled = prefs::read_bool("org.gonhanh.GoNhanh", "gonhanh.enabled");
        if dang_chay && enabled == Some(false) {
            fs.push(Finding {
                level: Level::Warn,
                msg: format!(
                    "{app} đang chạy nhưng đang tắt tiếng Việt (gonhanh.enabled=false) — \
`tongue vi` dưới strategy process sẽ là no-op và vẫn exit 0 vì chỉ kiểm tra app có sống; \
thoát {app} rồi mở lại, hoặc đổi sang strategy \"hotkey\"",
                    app = cfg.macos.app_name
                ),
            });
        }
    }

    Ok(print_findings(&fs))
}

// Windows không có layout để khám (US cố định) nên toàn bộ phần khám nằm trong
// VkeyIme::diagnose — đối xứng với macOS, và doctor không cần biết VKey là gì.
#[cfg(windows)]
pub fn run(fix: bool, cfg: &Config, ime: &dyn crate::backend::Ime) -> Result<bool> {
    // Cầu qua session là trạng thái hỏng MỚI mà máy này chưa từng có, nên nó phải nhìn
    // thấy được — và phải đứng TRƯỚC phần khám VKey, vì khi nó hỏng thì mọi dòng phía
    // sau đang nói về sai session.
    let mut fs = config_findings(cfg);
    fs.extend(crate::backend::windows::pipe::diagnose_bridge(
        &cfg.windows.agent_task,
    ));
    fs.extend(ime.diagnose(fix)?);
    Ok(print_findings(&fs))
}
