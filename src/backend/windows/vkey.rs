//! Điều khiển VKey (phatMT97/VKey ≥ 4.x) qua giao diện có sẵn của nó:
//! - set mode:  PostMessage(WM_USER+100, 1|0) tới cửa sổ ẩn "VKeyTrayClass"
//!   (idempotent, đi đúng đường hotkey nên smart-switch không ghi đè lại)
//! - đọc mode:  section "Local\VKeySharedState" (magic + version + flags)
//!   VKey KHÔNG bao giờ bị kill ở luồng thường — nhờ vậy không xáo hook
//!   WH_KEYBOARD_LL với kanata.

use super::{in_service_session, SERVICE_SESSION_ERR};
use crate::backend::{vkey_shm, Ime};
use crate::doctor::{Finding, Level};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW,
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
    // Ở session 0 thì "không thấy section" KHÔNG có nghĩa là VKey không chạy — nó chỉ có
    // nghĩa là ta đang nhìn sai chỗ. Trả None ở đây là bịa dữ liệu, nên báo lỗi thẳng.
    if in_service_session() {
        bail!("{SERVICE_SESSION_ERR}");
    }
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
        // Đọc 20 byte không check kích thước view: MapViewOfFile luôn map tròn lên
        // ít nhất 1 trang bộ nhớ (4KB trên x86/x64), nên section dù khai báo bé hơn
        // 20 byte vẫn không đọc ra ngoài trang được map. Nếu section thật sự chứa
        // rác/khác định dạng, parse_vietnamese_flag chặn lại qua kiểm tra magic +
        // version trước khi đọc flags.
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, 20);
        let parsed = vkey_shm::parse_vietnamese_flag(bytes);
        UnmapViewOfFile(view);
        CloseHandle(h);
        Ok(Some(parsed?))
    }
}

/// Chạy exe mà KHÔNG cho nó thừa kế handle nào của mình.
///
/// `std::process::Command` không làm được việc này, kể cả với `Stdio::null()` cả ba —
/// đã ĐO trên máy thật, không phải suy đoán. Lý do: `Stdio::null()` chỉ đặt ba std handle
/// của con thành NUL, còn Rust vẫn gọi `CreateProcessW` với `bInheritHandles = TRUE`, và
/// Windows thì thừa kế TẤT CẢ handle đang bật cờ inheritable của cha — không có danh sách
/// chọn lọc nào (muốn chọn lọc phải dùng `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
///
/// Hậu quả nếu để nguyên: bất cứ ai chạy tongue với stdout là pipe — `$(tongue vi)`,
/// `|`, ssh, CI, .NET `RedirectStandardOutput` — đều đưa cho VKey một bản handle của
/// đầu ghi pipe đó. VKey sống lâu hơn tongue, nên pipe KHÔNG BAO GIỜ tới EOF và bên đọc
/// treo vô hạn dù tongue đã thoát từ lâu. Số đo: tongue thoát sau 199ms, EOF không tới
/// trong 12s tiếp theo.
///
/// `bInheritHandles = FALSE` cắt gốc vấn đề. VKey không cần handle nào của ta cả.
fn spawn_no_inherit(exe: &std::path::Path) -> Result<()> {
    // CreateProcessW ghi vào lpCommandLine nên phải là buffer sửa được. Bọc ngoặc kép
    // để đường dẫn có khoảng trắng không bị tách thành nhiều tham số.
    let mut cmdline: Vec<u16> = format!("\"{}\"", exe.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // bInheritHandles = FALSE — đây là cả điểm của hàm này
            DETACHED_PROCESS,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("CreateProcessW thất bại cho {}", exe.display()));
    }
    // Không chờ VKey: nó là app GUI sống lâu hơn ta. Chỉ đóng hai handle vừa nhận.
    unsafe {
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
    }
    Ok(())
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

    /// VKey ưu tiên config cạnh exe; fallback %APPDATA%\VKey\config.toml.
    fn config_toml(&self) -> Option<Result<toml::Value>> {
        let mut candidates = Vec::new();
        if let Ok(exe) = self.discover_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("config.toml"));
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            candidates.push(std::path::Path::new(&appdata).join(r"VKey\config.toml"));
        }
        let path = candidates.into_iter().find(|p| p.exists())?;
        Some(
            std::fs::read_to_string(&path)
                .map_err(Into::into)
                .and_then(|t| t.parse::<toml::Value>().map_err(Into::into)),
        )
    }

    fn ensure_running(&self) -> Result<HWND> {
        if let Some(h) = find_window() {
            return Ok(h);
        }
        // Chặn TRƯỚC khi spawn: đây là chỗ session 0 gây hại thật, không chỉ đọc sai.
        if in_service_session() {
            bail!("{SERVICE_SESSION_ERR}");
        }
        let exe = self.discover_exe()?;
        spawn_no_inherit(&exe)?;
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

    fn diagnose(&self, _fix: bool) -> Result<Vec<Finding>> {
        let mut fs = Vec::new();

        // 1. VKey.exe tìm được?
        match self.discover_exe() {
            Ok(p) => fs.push(Finding {
                level: Level::Ok,
                msg: format!("VKey.exe: {}", p.display()),
            }),
            Err(e) => fs.push(Finding {
                level: Level::Fail,
                msg: format!("{e:#}"),
            }),
        }

        // 2. đang chạy? shared memory hợp lệ?
        match read_state() {
            Ok(Some(vi)) => fs.push(Finding {
                level: Level::Ok,
                msg: format!(
                    "VKey đang chạy, mode hiện tại = {}",
                    if vi { "vi" } else { "en" }
                ),
            }),
            Ok(None) => fs.push(Finding {
                level: Level::Warn,
                msg: "VKey chưa chạy — `tongue vi` sẽ tự bật".into(),
            }),
            Err(e) => fs.push(Finding {
                level: Level::Fail,
                msg: format!("shared memory không đọc được: {e:#}"),
            }),
        }

        // 3. config.toml của VKey: các cờ giành lái với tongue
        match self.config_toml() {
            Some(Ok(v)) => {
                let get = |t: &str, k: &str| {
                    v.get(t)
                        .and_then(|x| x.get(k))
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false)
                };
                fs.push(if get("features", "smart_switch") {
                    Finding {
                        level: Level::Warn,
                        msg: "smart_switch đang bật — VKey tự đổi mode theo app, giành lái với tongue; cân nhắc tắt trong Settings của VKey".into(),
                    }
                } else {
                    Finding {
                        level: Level::Ok,
                        msg: "smart_switch tắt".into(),
                    }
                });
                fs.push(if get("system", "run_as_admin") {
                    Finding {
                        level: Level::Warn,
                        msg: "run_as_admin đang bật — UIPI sẽ nuốt lệnh set mode của tongue; cân nhắc tắt".into(),
                    }
                } else {
                    Finding {
                        level: Level::Ok,
                        msg: "run_as_admin tắt".into(),
                    }
                });
            }
            Some(Err(e)) => fs.push(Finding {
                level: Level::Warn,
                msg: format!("config.toml của VKey không parse được: {e:#}"),
            }),
            None => fs.push(Finding {
                level: Level::Warn,
                msg: "không tìm thấy config.toml của VKey".into(),
            }),
        }

        Ok(fs)
    }
}
