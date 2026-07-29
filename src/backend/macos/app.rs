//! Bộ gõ ngoài kiểu "process là bit": app chạy = bật, app chết = tắt.
//! Đúng cho phần lớn bộ gõ macOS không có kênh IPC (EVKey, OpenKey...).
//! GoNhanh là ca riêng của kiểu này, thêm một bước ghi defaults — xem gonhanh.rs.

use crate::backend::Ime;
use crate::doctor::{Finding, Level};
use anyhow::{ensure, Result};
use std::process::Command;

/// pgrep -x: khớp ĐÚNG tên process, exit 0 nếu có.
pub fn is_running(app_name: &str) -> Result<bool> {
    Ok(Command::new("pgrep")
        .args(["-x", app_name])
        .output()?
        .status
        .success())
}

/// -g: không kéo app ra foreground. Gọi lặp khi app đang khởi động (reconcile
/// poll) là vô hại — LaunchServices no-op với app đã chạy.
pub fn launch(app_name: &str) -> Result<()> {
    let st = Command::new("open").args(["-ga", app_name]).status()?;
    ensure!(st.success(), "không mở được {app_name} — đã cài chưa?");
    Ok(())
}

/// killall gửi SIGTERM. Lưu ý: nó trả về khi tín hiệu được GỬI, không phải khi
/// process đã chết — nơi nào cần "chết hẳn rồi mới làm tiếp" phải tự chờ is_running.
pub fn terminate(app_name: &str) -> Result<()> {
    let st = Command::new("killall").arg(app_name).status()?;
    ensure!(st.success(), "killall {app_name} thất bại");
    Ok(())
}

/// Có .app trong /Applications hoặc ~/Applications không.
pub fn app_bundle_exists(app_name: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    [
        format!("/Applications/{app_name}.app"),
        format!("{home}/Applications/{app_name}.app"),
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

pub fn diagnose_bundle(app_name: &str) -> Finding {
    if app_bundle_exists(app_name) {
        Finding {
            level: Level::Ok,
            msg: format!("{app_name}.app có mặt"),
        }
    } else {
        Finding {
            level: Level::Fail,
            msg: format!("không thấy {app_name}.app trong /Applications hoặc ~/Applications"),
        }
    }
}

/// Bộ gõ ngoài chung, chỉ cần tên app. Không đụng defaults của bất kỳ ai.
pub struct AppIme {
    pub app_name: String,
}

impl Ime for AppIme {
    fn is_on(&self) -> Result<bool> {
        is_running(&self.app_name)
    }

    fn set(&self, on: bool) -> Result<()> {
        if on {
            if !self.is_on()? {
                launch(&self.app_name)?;
            }
        } else if self.is_on()? {
            terminate(&self.app_name)?;
        }
        Ok(())
    }

    fn diagnose(&self, _fix: bool) -> Result<Vec<Finding>> {
        Ok(vec![diagnose_bundle(&self.app_name)])
    }
}
