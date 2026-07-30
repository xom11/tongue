//! GoNhanh không có kênh IPC nào lúc đang chạy (đã xác minh source
//! khaphanspace/gonhanh.org) — defaults chỉ được đọc lúc khởi động.
//! Vậy bit bật/tắt v1 = SỰ TỒN TẠI CỦA PROCESS: bật = ghi defaults + open,
//! tắt = SIGTERM. Phần khám perAppMode nằm ngay trong diagnose() của backend
//! này, không rải ra doctor.rs — thêm bộ gõ khác không phải sờ vào đây.

use super::app;
use crate::backend::Ime;
use crate::doctor::{Finding, Level};
use anyhow::{ensure, Result};
use std::process::Command;
use std::time::{Duration, Instant};

const DEFAULTS_DOMAIN: &str = "org.gonhanh.GoNhanh";
const KEY_ENABLED: &str = "gonhanh.enabled";
const KEY_PER_APP: &str = "gonhanh.perAppMode";

pub struct GonhanhIme {
    pub app_name: String,
}

impl GonhanhIme {
    /// defaults chỉ được đọc lúc khởi động → muốn giá trị mới có hiệu lực thì
    /// phải restart. killall chỉ GỬI SIGTERM rồi trả về ngay: nếu bật lại liền
    /// thì set(true) tự thấy "process còn sống" và bỏ qua `open`, biến restart
    /// thành giết hẳn. Phải đợi is_on() thật sự về false.
    ///
    /// Trả về `true` nếu đã thực sự kill+launch, `false` nếu app không chạy nên
    /// không có gì để restart — defaults vừa ghi tự có hiệu lực ở lần mở tới,
    /// không có instance nào đang giữ giá trị cũ trong bộ nhớ.
    fn restart(&self) -> Result<bool> {
        if !self.is_on()? {
            return Ok(false);
        }
        self.set(false)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.is_on()? {
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "{} không thoát sau killall trong 2s — tắt thủ công rồi chạy lại `tongue doctor --fix`",
                    self.app_name
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.set(true)?;
        Ok(true)
    }
}

impl Ime for GonhanhIme {
    fn is_on(&self) -> Result<bool> {
        app::is_running(&self.app_name)
    }

    fn set(&self, on: bool) -> Result<()> {
        if on {
            // ghi enabled=1 TRƯỚC khi launch — instance mới đọc nó lúc khởi động
            let st = Command::new("defaults")
                .args(["write", DEFAULTS_DOMAIN, KEY_ENABLED, "-bool", "YES"])
                .status()?;
            ensure!(st.success(), "defaults write {KEY_ENABLED} thất bại");
            if !self.is_on()? {
                app::launch(&self.app_name)?;
            }
        } else if self.is_on()? {
            // GoNhanh không có state cần dọn ngoài process
            app::terminate(&self.app_name)?;
        }
        Ok(())
    }

    fn diagnose(&self, fix: bool) -> Result<Vec<Finding>> {
        Ok(vec![
            app::diagnose_bundle(&self.app_name),
            diagnose_per_app_mode(fix, &self.app_name, &|| self.restart())?,
        ])
    }
}

/// Phần khám `perAppMode` dùng chung cho cả strategy `process` lẫn `hotkey`:
/// perAppMode bật = GoNhanh nhớ trạng thái theo từng app, khiến key
/// gonhanh.enabled thành đồ giả (bẫy đã xác minh trong source).
///
/// `restart` chỉ được gọi khi `fix`, giá trị đang sai, VÀ app đang chạy (không
/// gì để restart nếu app đã chết). Trả về đã-restart-thật-hay-chưa: strategy
/// `hotkey` truyền vào closure luôn trả `false` vì nó KHÔNG được phép giết app
/// — giá trị mới khi đó chỉ nằm trong defaults, chưa vào bộ nhớ của instance
/// đang chạy, nên phải nói rõ cho người dùng biết mà tự thoát/mở lại.
pub fn diagnose_per_app_mode(
    fix: bool,
    app_name: &str,
    restart: &dyn Fn() -> Result<bool>,
) -> Result<Finding> {
    let out = Command::new("defaults")
        .args(["read", DEFAULTS_DOMAIN, KEY_PER_APP])
        .output()?;
    if !out.status.success() {
        return Ok(per_app_mode_finding(
            app_name, false, false, fix, false, false,
        ));
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let per_app_on = val != "0";
    if !per_app_on || !fix {
        return Ok(per_app_mode_finding(
            app_name, true, per_app_on, fix, false, false,
        ));
    }
    let st = Command::new("defaults")
        .args(["write", DEFAULTS_DOMAIN, KEY_PER_APP, "-bool", "NO"])
        .status()?;
    ensure!(st.success(), "defaults write {KEY_PER_APP} thất bại");
    // App không chạy thì không có instance nào giữ giá trị cũ trong bộ nhớ —
    // không cần gọi restart(), defaults vừa ghi tự có hiệu lực ở lần mở tới.
    let running = app::is_running(app_name)?;
    let restarted = if running { restart()? } else { false };
    Ok(per_app_mode_finding(
        app_name, true, per_app_on, fix, running, restarted,
    ))
}

/// Quyết định Level + message thuần từ kết quả IO đã đọc/ghi ở trên — test
/// được không chạm hệ thống. `per_app_on` = giá trị đọc được khác "0".
/// `running`/`restarted` chỉ có ý nghĩa khi `fix && per_app_on`.
fn per_app_mode_finding(
    app_name: &str,
    read_ok: bool,
    per_app_on: bool,
    fix: bool,
    running: bool,
    restarted: bool,
) -> Finding {
    if !read_ok {
        return Finding {
            level: Level::Warn,
            msg: format!("chưa đọc được defaults của {app_name} — app đã chạy lần đầu chưa?"),
        };
    }
    if !per_app_on {
        return Finding {
            level: Level::Ok,
            msg: format!("{KEY_PER_APP} = 0"),
        };
    }
    if !fix {
        return Finding {
            level: Level::Warn,
            msg: format!("{KEY_PER_APP} đang bật — chạy `tongue doctor --fix` để ghim về 0 (không thì trạng thái enabled không tin được)"),
        };
    }
    if !running {
        return Finding {
            level: Level::Ok,
            msg: format!("đã ghim {KEY_PER_APP}=0 cho {app_name}"),
        };
    }
    if restarted {
        return Finding {
            level: Level::Ok,
            msg: format!("đã ghim {KEY_PER_APP}=0 và restart {app_name}"),
        };
    }
    // strategy hotkey: closure restart() cố ý không làm gì. Giá trị mới đã nằm
    // trong defaults nhưng instance đang chạy vẫn giữ perAppMode=1 trong bộ nhớ
    // cho tới khi tự thoát/mở lại — im lặng ở đây là đúng chuỗi hỏng CLAUDE.md
    // đã cảnh báo (enabled thành "đồ giả").
    Finding {
        level: Level::Warn,
        msg: format!(
            "đã ghim {KEY_PER_APP}=0 — cần thoát và mở lại {app_name} để có hiệu lực (strategy hotkey không tự giết app)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_defaults_that_bai_thi_warn() {
        let f = per_app_mode_finding("GoNhanh", false, false, false, false, false);
        assert!(matches!(f.level, Level::Warn));
        assert!(f.msg.contains("chưa đọc được"));
    }

    #[test]
    fn per_app_mode_da_la_0_thi_ok() {
        let f = per_app_mode_finding("GoNhanh", true, false, false, false, false);
        assert!(matches!(f.level, Level::Ok));
        assert_eq!(f.msg, format!("{KEY_PER_APP} = 0"));
    }

    #[test]
    fn bat_nhung_khong_fix_thi_warn_goi_y_fix() {
        let f = per_app_mode_finding("GoNhanh", true, true, false, false, false);
        assert!(matches!(f.level, Level::Warn));
        assert!(f.msg.contains("doctor --fix"));
    }

    /// fix + app không chạy: không có instance nào giữ giá trị cũ, không cần
    /// nói tới restart.
    #[test]
    fn fix_app_khong_chay_thi_ok_khong_nhac_restart() {
        let f = per_app_mode_finding("GoNhanh", true, true, true, false, false);
        assert!(matches!(f.level, Level::Ok));
        assert!(!f.msg.contains("restart"));
    }

    /// fix + app đang chạy + restart thật: khôi phục đúng chữ plan gốc yêu cầu.
    #[test]
    fn fix_da_restart_thi_ok_va_noi_ro_da_restart() {
        let f = per_app_mode_finding("GoNhanh", true, true, true, true, true);
        assert!(matches!(f.level, Level::Ok));
        assert!(f.msg.contains("và restart GoNhanh"));
    }

    /// HỒI QUY CHO FINDING #2: fix dưới strategy hotkey không restart được (app
    /// đang chạy) — phải Warn, không được Ok, và phải nói rõ cần tự thoát/mở lại.
    #[test]
    fn fix_khong_restart_duoc_thi_warn_khong_phai_ok() {
        let f = per_app_mode_finding("GoNhanh", true, true, true, true, false);
        assert!(
            matches!(f.level, Level::Warn),
            "chưa restart được thì việc CHƯA xong — không được báo Ok"
        );
        assert!(f.msg.contains("thoát và mở lại GoNhanh"));
    }
}
