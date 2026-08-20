pub mod layout;
pub mod pipe;
pub mod vkey;

use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

/// Session 0 là session của service. Từ Vista nó bị tách hẳn khỏi desktop tương tác
/// (Session 0 Isolation), và SSH của Windows chạy đúng ở đó.
///
/// Hai cơ chế tongue dùng đều THEO SESSION: window station (FindWindow) và namespace
/// `Local\` (OpenFileMapping, thực chất là `Session\<n>\`). Nên từ session 0, VKey của
/// người dùng vừa không đọc được vừa không điều khiển được — mà kiểu hỏng lại rất tệ:
/// read_state() thấy section trống nên kết luận "VKey chưa chạy", rồi set(true) đi
/// spawn một VKey THỨ HAI trong session 0. Nó không hook được desktop nào, chỉ ngồi đó
/// làm rác và khiến `tongue status` báo một trạng thái hoàn toàn tưởng tượng.
pub(crate) fn in_service_session() -> bool {
    let mut sid = 0u32;
    let ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) };
    ok != 0 && sid == 0
}

pub(crate) const SERVICE_SESSION_ERR: &str =
    "đang chạy trong session 0 (service hoặc SSH của Windows), \
không với tới được desktop tương tác — window station và namespace `Local\\` đều theo session, \
nên VKey của người dùng vừa không đọc được vừa không điều khiển được. \
Hãy chạy từ chính session của người dùng, ví dụ scheduled task với `-LogonType Interactive`.";
