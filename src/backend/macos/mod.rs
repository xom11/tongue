pub mod app;

// Chỉ có call site thật ở task sau (bắn CGEvent qua chord) — parser thuần này
// đi trước để test được độc lập, nên tới lúc đó `parse`/`describe` chưa ai gọi
// ngoài #[cfg(test)] là dead code hợp lệ.
#[allow(dead_code)]
pub mod chord;

pub mod gonhanh;
pub mod system;
pub mod tis;
