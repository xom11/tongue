//! Điều khiển VKey (phatMT97/VKey ≥ 4.x) qua giao diện có sẵn của nó:
//! - set mode:  PostMessage(WM_USER+100, 1|0) tới cửa sổ ẩn "VKeyTrayClass"
//!   (idempotent, đi đúng đường hotkey nên smart-switch không ghi đè lại)
//! - đọc mode:  section "Local\VKeySharedState" (magic + version + flags)
//! VKey KHÔNG bao giờ bị kill ở luồng thường — nhờ vậy không xáo hook
//! WH_KEYBOARD_LL với kanata.

use crate::backend::{vkey_shm, Ime};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

const WM_VKEY_SET_MODE: u32 = 0x0400 + 100; // WM_USER+100 — VKey SharedConstants.h
const WINDOW_CLASS: &str = "VKeyTrayClass";
const SECTION: &str = "Local\\VKeySharedState";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn find_window() -> Option<HWND> {
    let cls = wide(WINDOW_CLASS);
    let h = unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) };
    if h.is_null() {
        None
    } else {
        Some(h)
    }
}

/// None = section không tồn tại = VKey không chạy (= mode en).
pub fn read_state() -> Result<Option<bool>> {
    unsafe {
        let name = wide(SECTION);
        let h = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
        if h.is_null() {
            return Ok(None);
        }
        let view = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        if view.Value.is_null() {
            CloseHandle(h);
            bail!("MapViewOfFile thất bại");
        }
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, 20);
        let parsed = vkey_shm::parse_vietnamese_flag(bytes);
        UnmapViewOfFile(view);
        CloseHandle(h);
        Ok(Some(parsed?))
    }
}

pub struct VkeyIme {
    pub exe_path_override: String,
}

impl VkeyIme {
    pub fn discover_exe(&self) -> Result<PathBuf> {
        if !self.exe_path_override.is_empty() {
            let p = PathBuf::from(&self.exe_path_override);
            if p.exists() {
                return Ok(p);
            }
            bail!("vkey_path trong config không tồn tại: {}", p.display());
        }
        let base = std::env::var("LOCALAPPDATA").context("thiếu %LOCALAPPDATA%")?;
        let pkgs = std::path::Path::new(&base).join(r"Microsoft\WinGet\Packages");
        if let Ok(entries) = std::fs::read_dir(&pkgs) {
            for entry in entries.flatten() {
                let dir = entry.path();
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("PhatMT97.VKey_") {
                    let exe = dir.join("VKey.exe");
                    if exe.exists() {
                        return Ok(exe);
                    }
                }
            }
        }
        bail!("không tìm thấy VKey.exe — cài bằng `winget install PhatMT97.VKey` hoặc khai [windows].vkey_path trong config")
    }

    fn ensure_running(&self) -> Result<HWND> {
        if let Some(h) = find_window() {
            return Ok(h);
        }
        let exe = self.discover_exe()?;
        std::process::Command::new(&exe)
            .spawn()
            .with_context(|| format!("không chạy được {}", exe.display()))?;
        // mutex nội bộ của VKey tự chống chạy trùng; chờ cửa sổ xuất hiện
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(h) = find_window() {
                return Ok(h);
            }
            if Instant::now() >= deadline {
                bail!("VKey đã được chạy nhưng không thấy cửa sổ VKeyTrayClass sau 5s");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Ime for VkeyIme {
    fn is_on(&self) -> Result<bool> {
        Ok(read_state()?.unwrap_or(false))
    }

    fn set(&self, on: bool) -> Result<()> {
        if on {
            let hwnd = self.ensure_running()?;
            unsafe { PostMessageW(hwnd, WM_VKEY_SET_MODE, 1, 0) };
        } else if let Some(hwnd) = find_window() {
            unsafe { PostMessageW(hwnd, WM_VKEY_SET_MODE, 0, 0) };
        }
        // VKey không chạy + muốn tắt = đã là en, không làm gì.
        // Nếu PostMessage bị UIPI nuốt (VKey chạy admin), verify của reconcile
        // sẽ trượt → exit 1 kèm gợi ý chạy doctor.
        Ok(())
    }
}
