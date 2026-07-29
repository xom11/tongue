//! Hai cần gạt của một mode. Impl thật nằm trong macos/ và windows/;
//! reconcile chỉ nhìn thấy trait nên test được bằng mock.

// Chỉ có call site thật trong windows/vkey.rs (cfg(windows)); giữ platform-độc lập
// để unit test chạy được trên mọi OS, nên trên build không phải windows/test đây là
// dead code hợp lệ.
#[allow(dead_code)]
pub mod vkey_shm;

pub trait Layout {
    fn current(&self) -> anyhow::Result<String>;
    fn select(&self, id: &str) -> anyhow::Result<()>;
}

pub trait Ime {
    fn is_on(&self) -> anyhow::Result<bool>;
    fn set(&self, on: bool) -> anyhow::Result<()>;
}

/// Windows không đụng layout (US cố định) — desired.layout luôn None ở đó.
// Chỉ được construct trong main.rs dưới cfg(windows); trên build không phải windows
// đây là dead code hợp lệ.
#[allow(dead_code)]
pub struct NoopLayout;

impl Layout for NoopLayout {
    fn current(&self) -> anyhow::Result<String> {
        Ok(String::new())
    }
    fn select(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(windows)]
pub mod windows;
