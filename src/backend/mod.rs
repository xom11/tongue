//! Hai cần gạt của một mode. Impl thật nằm trong macos/ và windows/;
//! reconcile chỉ nhìn thấy trait nên test được bằng mock.

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
pub struct NoopLayout;

impl Layout for NoopLayout {
    fn current(&self) -> anyhow::Result<String> {
        Ok(String::new())
    }
    fn select(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
