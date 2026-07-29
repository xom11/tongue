//! Không có bộ gõ ngoài: tiếng Việt do chính input source của macOS lo
//! (`com.apple.inputmethod.VietnameseIM.VietnameseTelex`). Cần gạt IME luôn tắt,
//! việc chuyển mode hoàn toàn nằm ở tầng layout.
//!
//! `is_on()` PHẢI luôn trả false, kể cả khi có app bộ gõ khác đang chạy: reconcile
//! dùng nó làm đích, báo true sẽ khiến nó chờ một cần gạt vĩnh viễn không nhúc
//! nhích rồi trả VerifyFailed giả. Việc phát hiện "còn app ngoài chạy chồng lên"
//! là của diagnose() — đúng chỗ, vì đó là chẩn đoán môi trường chứ không phải
//! trạng thái đích.

use super::app;
use crate::backend::Ime;
use crate::doctor::{Finding, Level};
use anyhow::Result;

pub struct SystemIme {
    /// Chỉ dùng để CẢNH BÁO nếu app này còn chạy — không bao giờ bị bật/tắt.
    pub app_name: String,
}

/// Thuần, tách khỏi pgrep để test được trên mọi OS.
pub fn conflict_finding(app_running: bool, app_name: &str) -> Finding {
    if app_running {
        Finding {
            level: Level::Warn,
            msg: format!(
                "{app_name} vẫn đang chạy nhưng backend = system — hai bộ gõ chồng nhau sẽ ăn phím của nhau; tắt {app_name} (và bỏ nó khỏi Login Items) hoặc đổi backend"
            ),
        }
    } else {
        Finding {
            level: Level::Ok,
            msg: "không có bộ gõ ngoài nào chạy chồng".into(),
        }
    }
}

impl Ime for SystemIme {
    fn is_on(&self) -> Result<bool> {
        Ok(false)
    }

    fn set(&self, _on: bool) -> Result<()> {
        Ok(())
    }

    fn diagnose(&self, _fix: bool) -> Result<Vec<Finding>> {
        Ok(vec![conflict_finding(
            app::is_running(&self.app_name)?,
            &self.app_name,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ngoai_con_chay_thi_canh_bao() {
        let f = conflict_finding(true, "GoNhanh");
        assert!(matches!(f.level, Level::Warn));
        assert!(f.msg.contains("chồng nhau"));
        assert!(f.msg.contains("GoNhanh"));
    }

    #[test]
    fn khong_app_nao_chay_thi_ok() {
        let f = conflict_finding(false, "GoNhanh");
        assert!(matches!(f.level, Level::Ok));
    }
}
