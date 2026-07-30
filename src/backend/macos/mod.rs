pub mod app;

// Chỉ có call site thật ở task sau (bắn CGEvent qua chord) — parser thuần này
// đi trước để test được độc lập, nên tới lúc đó `parse`/`describe` chưa ai gọi
// ngoài #[cfg(test)] là dead code hợp lệ.
#[allow(dead_code)]
pub mod chord;

pub mod gonhanh;

// Call site thật (đọc chord GoNhanh trong tiến trình) nằm ở Task 4 — tới lúc
// đó `read_bool`/`read_data` chưa ai gọi ngoài #[cfg(test)] là dead code hợp lệ.
#[allow(dead_code)]
pub mod prefs;

pub mod system;
pub mod tis;
