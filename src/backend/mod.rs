//! Hai cần gạt của một mode. Impl thật nằm trong macos/ và windows/;
//! reconcile chỉ nhìn thấy trait nên test được bằng mock.

// Chỉ có call site thật trong windows/vkey.rs (cfg(windows)); giữ platform-độc lập
// để unit test chạy được trên mọi OS, nên trên build không phải windows/test đây là
// dead code hợp lệ.
#[allow(dead_code)]
pub mod vkey_shm;

// Cùng lý do như vkey_shm: chuẩn hoá HKL là code thuần nên để ngoài windows/ cho test
// chạy được trên mọi OS; call site thật chỉ có trong windows/layout.rs.
#[allow(dead_code)]
pub mod hkl;

pub trait Layout {
    fn current(&self) -> anyhow::Result<String>;
    fn select(&self, id: &str) -> anyhow::Result<()>;
}

pub trait Ime {
    fn is_on(&self) -> anyhow::Result<bool>;
    fn set(&self, on: bool) -> anyhow::Result<()>;

    /// Phần khám của riêng backend này cho `tongue doctor` — nhờ nó doctor không
    /// cần biết tên app nào tồn tại. `fix` = được phép sửa thứ an toàn.
    /// Mặc định: backend không có gì riêng để khám.
    fn diagnose(&self, _fix: bool) -> anyhow::Result<Vec<crate::doctor::Finding>> {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(windows)]
pub mod windows;
