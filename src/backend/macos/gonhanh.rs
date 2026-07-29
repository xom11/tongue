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
    fn restart(&self) -> Result<()> {
        if !self.is_on()? {
            return Ok(());
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
        self.set(true)
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
        let mut fs = vec![app::diagnose_bundle(&self.app_name)];

        // perAppMode bật = GoNhanh nhớ trạng thái theo từng app, khiến key
        // gonhanh.enabled thành đồ giả (bẫy đã xác minh trong source).
        let out = Command::new("defaults")
            .args(["read", DEFAULTS_DOMAIN, KEY_PER_APP])
            .output()?;
        let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() {
            fs.push(Finding {
                level: Level::Warn,
                msg: format!(
                    "chưa đọc được defaults của {} — app đã chạy lần đầu chưa?",
                    self.app_name
                ),
            });
        } else if val == "0" {
            fs.push(Finding {
                level: Level::Ok,
                msg: format!("{KEY_PER_APP} = 0"),
            });
        } else if fix {
            let st = Command::new("defaults")
                .args(["write", DEFAULTS_DOMAIN, KEY_PER_APP, "-bool", "NO"])
                .status()?;
            ensure!(st.success(), "defaults write {KEY_PER_APP} thất bại");
            self.restart()?;
            fs.push(Finding {
                level: Level::Ok,
                msg: format!("đã ghim {KEY_PER_APP}=0 và restart {}", self.app_name),
            });
        } else {
            fs.push(Finding {
                level: Level::Warn,
                msg: format!("{KEY_PER_APP} đang bật — chạy `tongue doctor --fix` để ghim về 0 (không thì trạng thái enabled không tin được)"),
            });
        }
        Ok(fs)
    }
}
