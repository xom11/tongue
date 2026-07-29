//! GoNhanh không có kênh IPC nào lúc đang chạy (đã xác minh source
//! khaphanspace/gonhanh.org) — defaults chỉ được đọc lúc khởi động.
//! Vậy bit bật/tắt v1 = SỰ TỒN TẠI CỦA PROCESS: bật = ghi defaults + open,
//! tắt = SIGTERM. `doctor --fix` ghim gonhanh.perAppMode=0 (Task 9) để
//! key gonhanh.enabled không bị per-app mode ghi đè.

use crate::backend::Ime;
use anyhow::{ensure, Result};
use std::process::Command;

const DEFAULTS_DOMAIN: &str = "org.gonhanh.GoNhanh";

pub struct GonhanhIme {
    pub app_name: String,
}

impl Ime for GonhanhIme {
    fn is_on(&self) -> Result<bool> {
        // pgrep -x: khớp đúng tên process, exit 0 nếu có
        Ok(Command::new("pgrep")
            .args(["-x", &self.app_name])
            .output()?
            .status
            .success())
    }

    fn set(&self, on: bool) -> Result<()> {
        if on {
            // ghi enabled=1 TRƯỚC khi launch — instance mới đọc nó lúc khởi động
            let st = Command::new("defaults")
                .args(["write", DEFAULTS_DOMAIN, "gonhanh.enabled", "-bool", "YES"])
                .status()?;
            ensure!(st.success(), "defaults write gonhanh.enabled thất bại");
            if !self.is_on()? {
                // -g: không kéo app ra foreground. Gọi lặp khi app đang khởi động
                // (reconcile poll) là vô hại — LaunchServices no-op với app đã chạy.
                let st = Command::new("open")
                    .args(["-ga", &self.app_name])
                    .status()?;
                ensure!(
                    st.success(),
                    "không mở được {} — đã cài chưa?",
                    self.app_name
                );
            }
        } else if self.is_on()? {
            // killall gửi SIGTERM; GoNhanh không có state cần dọn ngoài process
            let st = Command::new("killall").arg(&self.app_name).status()?;
            ensure!(st.success(), "killall {} thất bại", self.app_name);
        }
        Ok(())
    }
}
