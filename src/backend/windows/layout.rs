//! Đổi input locale của Windows qua HKL — cần cho mode `zh`, thứ mà bit bật/tắt của
//! VKey một mình không diễn tả được.
//!
//! **Khác macOS ở một điểm khái niệm, không phải chi tiết cài đặt:** input locale trên
//! Windows là **theo thread**, không toàn cục như input source của macOS. Nên "layout
//! hiện tại" nghĩa là layout của thread sở hữu cửa sổ đang foreground, và đổi layout =
//! gửi `WM_INPUTLANGCHANGEREQUEST` tới đúng cửa sổ đó. Cả đọc và ghi đều quy về cửa sổ
//! foreground nên chúng nhất quán; hệ quả là nếu focus đổi giữa lúc reconcile đang
//! verify thì nó đọc sang thread khác và verify trượt — thà vậy còn hơn im lặng báo
//! thành công. Đây cũng chính là cơ chế `switch-language.ahk` của chủ repo vẫn dùng,
//! nên đường này đã được chứng minh trên a14-win.
//!
//! Phần chuẩn hoá HKL <-> định danh layout nằm ở `backend::hkl` để test được trên mọi OS.

use super::{in_service_session, SERVICE_SESSION_ERR};
use crate::backend::hkl;
use anyhow::{bail, Result};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, LoadKeyboardLayoutW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW,
};

const WM_INPUTLANGCHANGEREQUEST: u32 = 0x0050;
const KLF_ACTIVATE: u32 = 0x0001;

/// Các HKL ĐANG được nạp trong hệ thống.
///
/// Phải tìm trong danh sách này TRƯỚC khi nghĩ tới `LoadKeyboardLayoutW`: tiếng Trung
/// trên Windows là một **TSF text service**, không phải keyboard layout — trong
/// `Get-WinUserLanguageList` nó hiện ra dạng `0804:{81D4E9C9-...}{FA550B04-...}`, nên
/// `LoadKeyboardLayoutW("00000804")` đi tìm một KLID không tồn tại. Số đo trên a14-win
/// 30/07/2026: danh sách trả về đúng hai HKL, `0x04090409` và `0x08040804`, tức khớp
/// word thấp là ra ngay.
///
/// **CHƯA XONG, đừng tin là `zh` chạy được:** tìm đúng HKL rồi
/// `ActivateKeyboardLayout` + `PostMessage(WM_INPUTLANGCHANGEREQUEST)` vẫn KHÔNG đổi
/// được layout khi gọi từ tiến trình nền (scheduled task). Đã loại trừ khả năng lỗi ở
/// đây: tái hiện y nguyên thuật toán này bằng PowerShell từ cùng ngữ cảnh cũng thất bại
/// hệt như vậy — `PostMessage` trả về TRUE mà layout đứng im sau 2.8s. Nghĩa là cần
/// đường TSF (`ITfInputProcessorProfileMgr::ActivateProfile`) hoặc phải gọi từ tiến
/// trình đang sở hữu cửa sổ foreground. `vi`/`en` không phụ thuộc chỗ này nên vẫn tốt.
fn loaded_layouts() -> Vec<usize> {
    unsafe {
        let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
        if n <= 0 {
            return Vec::new();
        }
        let mut buf = vec![0usize; n as usize];
        let got = GetKeyboardLayoutList(n, buf.as_mut_ptr().cast());
        buf.truncate(got.max(0) as usize);
        buf
    }
}

fn foreground_thread() -> Result<u32> {
    // Chặn trước khi hỏi cửa sổ foreground: ở session 0 thì KHÔNG BAO GIỜ có cửa sổ nào,
    // nên báo "không có foreground" là đúng hiện tượng mà sai nguyên nhân — người đọc sẽ
    // đi tìm cửa sổ thay vì thấy mình đang ở sai session.
    if in_service_session() {
        bail!("{SERVICE_SESSION_ERR}");
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            bail!("không có cửa sổ foreground — không biết đọc layout của thread nào");
        }
        Ok(GetWindowThreadProcessId(hwnd, std::ptr::null_mut()))
    }
}

pub fn current_langid() -> Result<String> {
    let tid = foreground_thread()?;
    unsafe {
        let h = GetKeyboardLayout(tid);
        if h.is_null() {
            bail!("GetKeyboardLayout trả về null cho thread {tid}");
        }
        Ok(hkl::format_langid(h as usize))
    }
}

pub fn select_langid(id: &str) -> Result<()> {
    let want = hkl::langid_of(
        usize::from_str_radix(&hkl::klid_of(id), 16)
            .map_err(|_| anyhow::anyhow!("định danh layout không phải hex: {id}"))?,
    );

    // 1. Ưu tiên HKL đã nạp, khớp theo word thấp (LANGID).
    let mut found = loaded_layouts()
        .into_iter()
        .find(|h| hkl::langid_of(*h) == want);

    // 2. Chưa nạp thì mới thử nạp — chỉ đúng với keyboard layout thật, không với TSF IME.
    if found.is_none() {
        let klid: Vec<u16> = hkl::klid_of(id)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let h = unsafe { LoadKeyboardLayoutW(klid.as_ptr(), KLF_ACTIVATE) };
        if !h.is_null() {
            found = Some(h as usize);
        }
    }

    let Some(h) = found else {
        bail!(
            "không tìm thấy layout {id} — đã thêm nó trong Settings > Time & language > \
             Language & region chưa? (các layout đang nạp: {:?})",
            loaded_layouts()
                .iter()
                .map(|h| hkl::format_langid(*h))
                .collect::<Vec<_>>()
        );
    };

    unsafe {
        // ActivateKeyboardLayout là bước dễ bỏ sót nhất: chỉ PostMessage thôi thì với TSF
        // IME layout không đổi. Bản AHK đang chạy trên a14-win cũng gọi đúng cặp này.
        ActivateKeyboardLayout(h as _, 0);

        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            bail!("không có cửa sổ foreground để gửi WM_INPUTLANGCHANGEREQUEST");
        }
        // wParam = 0 + lParam = HKL: chọn ĐÚNG layout này. Dùng
        // INPUTLANGCHANGE_FORWARD/BACKWARD thì thành xoay vòng, mất tính idempotent mà
        // reconcile dựa vào.
        //
        // Không kiểm giá trị trả về: nếu cửa sổ foreground chạy elevated thì UIPI nuốt
        // message mà PostMessage vẫn có thể báo thành công. Phát hiện là việc của verify
        // trong reconcile — nó đọc lại layout thật nên bắt được cả ca đó.
        PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, h as isize);
    }
    Ok(())
}

pub struct WinLayout;

impl crate::backend::Layout for WinLayout {
    fn current(&self) -> Result<String> {
        current_langid()
    }
    fn select(&self, id: &str) -> Result<()> {
        select_langid(id)
    }
}
