//! Cầu qua ranh giới session, bằng named pipe.
//!
//! Windows chia namespace theo session: `Local\...` thực chất là `Session\<n>\...`,
//! và window station cũng theo session. Đó là lý do `tongue` chạy qua SSH (session 0)
//! vừa không đọc được VKey vừa không điều khiển được nó — xem `SERVICE_SESSION_ERR`.
//!
//! Named pipe thì KHÔNG theo session: pipe nằm ở `\Device\NamedPipe`, tới được qua
//! symlink `PIPE` trong `\GLOBAL??`, và tên pipe không bị qualify theo session. Session 0
//! Isolation cắt window station/desktop và BaseNamedObjects, không cắt pipe. Đã đo trên
//! a14 20/08/2026: server trong session 1 tạo pipe, client trong session 0 mở được và
//! trao đổi hai chiều. Đây là cơ chế Microsoft dựng cho đúng việc "service nói chuyện
//! với app desktop".
//!
//! Điều này KHÔNG mua lấy tốc độ, và đừng thiết kế như thể nó mua: một lượt SSH tới máy
//! đó tốn 452 ms (có ControlMaster) đến 829 ms (không), nên nó vẫn quá chậm cho thứ như
//! "ép tiếng Anh ngay khi rời Insert mode". Thứ nó mua là `ssh may tongue vi` CHẠY ĐƯỢC,
//! cho mọi script và mọi tự động hoá.
//!
//! Ưu thế DUY NHẤT của pipe so với việc chỉ nhảy session bằng `schtasks /run`
//! (376-401 ms, đo 5/5 trên a14) là KÊNH TRẢ KẾT QUẢ: `schtasks /run` trả về ngay và
//! không mang theo stdout lẫn mã thoát của action, mà hợp đồng 0/1/2 và stdout của
//! tongue đang có người dùng. Nếu câu đó rơi ra khỏi tài liệu thì sáu tháng nữa sẽ có
//! người nhìn thấy "một daemon để tiết kiệm 400 ms", kết luận là thừa, và thay bằng
//! schtasks — rồi phát hiện `tongue status` qua ssh trả về rỗng.
//!
//! ## Bất biến của agent
//!
//! **Agent là NGƯỜI ĐƯA THƯ, không phải cái kho.** Nó không giữ mode, không cache
//! HANDLE của section, không cache HWND, không giữ `Config`. Mỗi yêu cầu là một lần
//! CHẠY LẠI chính `tongue` như tiến trình con — nên hành vi, mã thoát, stderr và cả
//! thời điểm đọc config giống hệt lúc gõ tay, và không có nhánh thứ hai nào để lệch
//! về sau. Cache HANDLE của section là dựng lại đúng cái bug mà `in_service_session()`
//! sinh ra để chặn: section object sống chừng nào còn một handle mở, nên `read_state()`
//! sẽ trả trạng thái của một VKey đã chết.
//!
//! **Đường CỤC BỘ không bao giờ đi qua pipe.** `maybe_forward` chỉ chạy khi
//! `in_service_session()` là true. `switch-language.ahk` trên a14 gọi `tongue` mỗi lần
//! đổi cửa sổ và nó ở session 1 — cho nó đi qua agent là biến một công cụ hôm nay không
//! có single point of failure thành một công cụ có.

use super::{in_service_session, SERVICE_SESSION_ERR};
use crate::backend::pipe_proto as proto;
use crate::doctor::{Finding, Level};
use anyhow::{bail, Context, Result};
use std::io;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FindClose, FindFirstFileW, FindNextFileW, FlushFileBuffers, ReadFile, WriteFile,
    FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WIN32_FIND_DATAW,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PeekNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTSGetActiveConsoleSessionId,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_INFORMATION,
};

/// Biến này chặn vòng lặp vô hạn: agent chạy lại chính `tongue` như tiến trình con, và
/// tiến trình con đó phải KHÔNG được chuyển tiếp ngược vào pipe. Trên thực tế con nằm ở
/// session 1 nên `in_service_session()` đã là false rồi — biến này là dây an toàn thứ
/// hai, vì cái giá của việc sai là một vòng lặp không đáy.
pub const NO_FORWARD_ENV: &str = "TONGUE_NO_FORWARD";

pub struct Reply {
    pub code: u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Ba kết cục phân biệt được, vì ba câu lỗi khác nhau. Gộp chúng lại là đẩy người
/// dùng đi dựng scheduled task trong khi thứ hỏng là một agent đang treo.
pub enum Bridge {
    /// Không thấy agent nào của user này.
    Absent,
    /// Nhiều hơn một agent (hai phiên cùng user). KHÔNG tự chọn — lái nhầm session
    /// nghĩa là đổi bộ gõ của một desktop khác, im lặng.
    Ambiguous(Vec<u32>),
    /// Có pipe mang tên ta mà không cái nào chứng minh được là agent của ta.
    ///
    /// KHÁC `Absent`, và khác biệt đó là lối thoát khỏi một DoS vĩnh viễn: tên mồi
    /// không chiếm tên THẬT của agent, nên agent vẫn khởi động được. Gộp vào một lỗi
    /// cứng là để một mồi biết tự nối lại khoá luôn cả đường hồi sinh — người gọi phải
    /// vẫn thử `schtasks /run` rồi hỏi lại. Chỉ khi hỏi lại vẫn thế mới là lỗi.
    Foreign(Vec<u32>),
    Reply(Reply),
}

// ── helper nhỏ ───────────────────────────────────────────────────────────────

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn from_wide(p: PWSTR) -> String {
    let mut n = 0usize;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

fn last_err() -> io::Error {
    io::Error::last_os_error()
}

/// SID của token ĐANG chạy, dạng chuỗi. Client ở session 0 đọc được cái này dễ dàng
/// vì nó là thuộc tính token của CHÍNH NÓ — hoàn toàn không liên quan tới session.
fn current_user_sid() -> Result<String> {
    unsafe { sid_of_process_handle(GetCurrentProcess()) }
}

unsafe fn sid_of_process_handle(proc: HANDLE) -> Result<String> {
    let mut token: HANDLE = null_mut();
    if OpenProcessToken(proc, TOKEN_QUERY, &mut token) == 0 {
        return Err(last_err()).context("OpenProcessToken");
    }
    // Lượt gọi đầu CỐ Ý trượt — nó chỉ để hỏi kích thước, và GetLastError lúc đó là
    // ERROR_INSUFFICIENT_BUFFER (122), không phải lỗi.
    let mut need = 0u32;
    GetTokenInformation(token, TokenUser, null_mut(), 0, &mut need);
    let mut buf = vec![0u8; need.max(1) as usize];
    let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), need, &mut need);
    CloseHandle(token);
    if ok == 0 {
        return Err(last_err()).context("GetTokenInformation(TokenUser)");
    }
    let tu = &*(buf.as_ptr() as *const TOKEN_USER);
    let mut pw: PWSTR = null_mut();
    if ConvertSidToStringSidW(tu.User.Sid, &mut pw) == 0 {
        return Err(last_err()).context("ConvertSidToStringSidW");
    }
    let s = from_wide(pw);
    LocalFree(pw as HLOCAL);
    Ok(s)
}

fn current_session() -> Result<u32> {
    let mut sid = 0u32;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) } == 0 {
        return Err(last_err()).context("ProcessIdToSessionId");
    }
    Ok(sid)
}

/// `0xFFFFFFFF` = không có session console nào (chưa ai đăng nhập sau reboot). Đây là
/// câu trả lời quan trọng nhất khi client không thấy agent: sshd vẫn chạy (nó là
/// service, session 0) nên `ssh` vẫn vào được, nhưng session tương tác chưa tồn tại và
/// scheduled task `-LogonType Interactive` sẽ kẹt `State=Queued` với `LastTaskResult=0`.
fn console_session() -> Option<u32> {
    let s = unsafe { WTSGetActiveConsoleSessionId() };
    (s != u32::MAX).then_some(s)
}

/// Bọc SD do `ConvertStringSecurityDescriptorToSecurityDescriptorW` cấp phát bằng
/// LocalAlloc — phải LocalFree, không phải free của Rust.
struct SecDesc(PSECURITY_DESCRIPTOR);

impl Drop for SecDesc {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0 as HLOCAL) };
    }
}

/// DACL chỉ cho đúng SID của người tạo.
///
/// Đây là bản HẸP HƠN mặc định chứ không rộng hơn: DACL mặc định của named pipe cấp
/// full control cho creator-owner (nên về CHỨC NĂNG không cần gì thêm — client cùng
/// user ở session 0 mở được), nhưng nó CÒN cấp quyền đọc cho Everyone và cho tài khoản
/// anonymous. Cộng thêm một vế dễ quên: `\\.\pipe\X` cũng chính là `\\<máy>\pipe\X`
/// qua IPC$ của SMB, và máy này nằm trên tailnet. `PIPE_REJECT_REMOTE_CLIENTS` bịt vế đó.
///
/// `D:` = DACL, `P` = protected (không nhận ACE thừa kế), `A` = allow, `GA` = GENERIC_ALL.
fn owner_only_sd(sid: &str) -> Result<SecDesc> {
    let s = wide(&format!("D:P(A;;GA;;;{sid})"));
    let mut psd: PSECURITY_DESCRIPTOR = null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            s.as_ptr(),
            SDDL_REVISION_1,
            &mut psd,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_err()).context("SDDL không dựng được");
    }
    Ok(SecDesc(psd))
}

// ── I/O có hạn định ──────────────────────────────────────────────────────────

fn write_all(h: HANDLE, buf: &[u8]) -> Result<()> {
    let mut done = 0usize;
    while done < buf.len() {
        let mut n = 0u32;
        let ok = unsafe {
            WriteFile(
                h,
                buf[done..].as_ptr(),
                (buf.len() - done) as u32,
                &mut n,
                null_mut(),
            )
        };
        if ok == 0 || n == 0 {
            bail!("ghi vào pipe thất bại: {}", last_err());
        }
        done += n as usize;
    }
    Ok(())
}

/// `ReadFile` trên một handle pipe ĐỒNG BỘ không có timeout nào cả —
/// `nDefaultTimeOut` của `CreateNamedPipeW` chỉ chi phối `WaitNamedPipeW`. Không có
/// vòng `PeekNamedPipe` này thì một agent treo = một phiên ssh treo cứng, và deadline
/// 2000 ms của tongue.nvim không cứu được: nó chỉ giết tiến trình con TRỰC TIẾP (ssh),
/// không với tới agent ở đầu kia.
///
/// Poll bằng `PeekNamedPipe` chứ không dựng overlapped I/O: cùng idiom với `reconcile`,
/// và không phải mang thêm một OVERLAPPED cùng một event handle qua mọi nhánh lỗi.
fn read_exact_by(h: HANDLE, len: usize, deadline: Instant) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    let mut done = 0usize;
    while done < len {
        let mut avail = 0u32;
        if unsafe { PeekNamedPipe(h, null_mut(), 0, null_mut(), &mut avail, null_mut()) } == 0 {
            bail!("mất kết nối tới agent: {}", last_err());
        }
        if avail == 0 {
            if Instant::now() >= deadline {
                bail!("agent có mặt nhưng không trả lời kịp hạn");
            }
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let want = (len - done).min(avail as usize);
        let mut n = 0u32;
        let ok = unsafe { ReadFile(h, buf[done..].as_mut_ptr(), want as u32, &mut n, null_mut()) };
        if ok == 0 || n == 0 {
            bail!("đọc từ pipe thất bại: {}", last_err());
        }
        done += n as usize;
    }
    Ok(buf)
}

fn read_chunk_by(h: HANDLE, deadline: Instant) -> Result<Vec<u8>> {
    let head: [u8; 4] = read_exact_by(h, 4, deadline)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("độ dài khung hỏng"))?;
    read_exact_by(h, proto::chunk_len(head)?, deadline)
}

// ── phía client (chạy trong session 0) ───────────────────────────────────────

/// Liệt kê session của mọi agent thuộc CÙNG user. Namespace pipe liệt kê được bằng
/// `FindFirstFileW`, nên "session nào đang phục vụ" là một phép TRA chứ không phải một
/// phép đoán — cần đúng khi người dùng đang ngồi RDP (lúc đó session console lại là
/// một phiên đã ngắt).
///
/// Best-effort: đây là hành vi được dùng rộng rãi chứ không phải hợp đồng có tài liệu,
/// nên mọi lỗi ở đây trả về danh sách rỗng và người gọi lùi về ứng viên từ
/// `WTSGetActiveConsoleSessionId`.
fn list_agent_sessions(sid: &str) -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let pat = wide(&format!(r"\\.\pipe\{}*", proto::pipe_prefix(sid)));
        let mut data: WIN32_FIND_DATAW = std::mem::zeroed();
        let h = FindFirstFileW(pat.as_ptr(), &mut data);
        if h == INVALID_HANDLE_VALUE {
            return out;
        }
        loop {
            let n = data.cFileName.iter().position(|c| *c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&data.cFileName[..n]);
            if let Some(s) = proto::session_of(&name, sid) {
                out.push(s);
            }
            if FindNextFileW(h, &mut data) == 0 {
                break;
            }
        }
        FindClose(h);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Một lượt `CreateFileW` DUY NHẤT, không chờ. `Err` mang GetLastError để người gọi
/// phân biệt được `ERROR_PIPE_BUSY` với "không có ai nghe".
fn try_open(name: &str) -> std::result::Result<HANDLE, u32> {
    let w = wide(name);
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            null_mut(),
            OPEN_EXISTING,
            // SECURITY_SQOS_PRESENT|SECURITY_IDENTIFICATION: nếu ai đó CHIẾM được
            // tên pipe này thì họ chỉ nhận identification token, không
            // `ImpersonateNamedPipeClient` để mượn danh tính ta được. Phiên SSH của
            // một admin trên Win32-OpenSSH thường mang token elevated, nên vế đó
            // không phải giả thuyết — đây đúng họ lỗi "Potato". Cũng chính vì thế
            // KHÔNG dùng `CallNamedPipeW`/`TransactNamedPipe` dù chúng gọn hơn: hai
            // hàm đó không cho khai SQOS.
            FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    };
    if h != INVALID_HANDLE_VALUE {
        Ok(h)
    } else {
        Err(unsafe { GetLastError() })
    }
}

/// Mở pipe, CHỜ khi mọi instance đang bận. `None` = tên đó không có ai nghe.
fn open_pipe(name: &str, deadline: Instant) -> Option<HANDLE> {
    let w = wide(name);
    loop {
        match try_open(name) {
            Ok(h) => return Some(h),
            // BUSY = có agent, chỉ là mọi instance đang bận. Đây KHÔNG phải "không có
            // agent": agent phục vụ tuần tự, mà một lượt có thể tốn tới ~6s (VKey
            // cold-start 5s + verify 1s), và tongue.nvim bắn hai lời gọi tuần tự mỗi
            // lần rời Insert. Trả "không có agent" ở đây là câu lỗi sai hoàn toàn.
            Err(e) if e != ERROR_PIPE_BUSY => return None,
            Err(_) => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        if unsafe { WaitNamedPipeW(w.as_ptr(), left.as_millis().min(30_000) as u32) } == 0 {
            return None;
        }
    }
}

/// Trần số tên được thăm dò trong một lượt. Tên pipe là chuỗi tự do nên ai cũng tạo
/// được hàng nghìn cái mang SID của người khác; không có trần thì vòng thăm dò dài
/// bằng thứ kẻ tấn công muốn.
const MAX_CANDIDATES: usize = 8;

/// Kết cục thăm dò MỘT tên. Phân biệt được bốn thứ mà bản trước gộp làm hai.
enum Probe {
    /// Mở được, và chủ đầu server đúng là ta. Handle đi kèm để lượt trao đổi dùng
    /// luôn — mở lại lần nữa là tự tạo một khe TOCTOU không cần thiết.
    Ours(HANDLE),
    /// Có người nghe nhưng KHÔNG chứng minh được là ta: chủ lệch SID, hoặc không đọc
    /// nổi chủ. Hai ca đó gộp làm một là CỐ Ý — xem `probe`.
    Foreign,
    /// Mọi instance đang bận nên chưa cầm được handle để hỏi chủ.
    Busy,
    /// Không ai nghe tên này.
    Vacant,
}

/// Thăm dò một tên: có agent CỦA TA đứng sau nó không? Không gửi byte nào.
///
/// **`server_sid()` trả `None` là KHÔNG TIN, không phải là tin.** Bản trước viết
/// `if let Some(owner) = ... { if owner != sid { bail } }`, nên nhánh `None` — đúng
/// nhánh của kẻ chiếm pipe mà client không đọc nổi tiến trình của họ — rơi thẳng
/// xuống `exchange()`. Hàng rào chỉ bắn được với server cùng user, nơi nó là hằng đúng.
///
/// Danh tính lấy từ TIẾN TRÌNH giữ đầu server (`GetNamedPipeServerProcessId` +
/// token), KHÔNG phải từ chủ sở hữu của pipe object. Đã đo trên a14 20/08/2026: token
/// elevated có `Owner = S-1-5-32-544` (BUILTIN\Administrators), nên pipe do nó tạo
/// mang owner Administrators chứ không mang SID người dùng — so owner với SID người
/// dùng là so trượt ở mọi tiến trình elevated.
fn probe(sid: &str, session: u32) -> Probe {
    match try_open(&proto::pipe_name(sid, session)) {
        Ok(h) => verify(h, sid, session),
        Err(e) if e == ERROR_PIPE_BUSY => Probe::Busy,
        Err(_) => Probe::Vacant,
    }
}

/// Phần chung của `probe` và nhánh chờ-BUSY: đã cầm handle rồi thì hỏi danh tính.
///
/// Kiểm HAI vế, và vế thứ hai là thứ bịt được cả kẻ chiếm CÙNG user: cái tên
/// `tongue.<sid>.<n>` tuyên bố một session, agent thật luôn nằm đúng trong session nó
/// khai. Mồi trong PoC trên a14 do một tiến trình ở session 0 tạo nhưng đặt tên `.7`,
/// nên chỉ cần đối chiếu là rụng. Chỉ so SID thì mồi cùng user vẫn lọt.
fn verify(h: HANDLE, sid: &str, session: u32) -> Probe {
    match server_identity(h) {
        Some((owner, sess)) if owner == sid && sess == session => Probe::Ours(h),
        _ => {
            unsafe { CloseHandle(h) };
            Probe::Foreign
        }
    }
}

/// SID chủ VÀ session của tiến trình đang giữ đầu server. `None` = không xác minh được.
///
/// Trả về cả session vì cái TÊN pipe đã tuyên bố một session (`tongue.<sid>.<n>`) và
/// tuyên bố đó kiểm được: agent thật luôn nằm trong đúng session nó tự khai. Không đối
/// chiếu thì một tên khai session không tồn tại vẫn được tính là agent — xem `probe`.
fn server_identity(h: HANDLE) -> Option<(String, u32)> {
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeServerProcessId(h, &mut pid) == 0 {
            return None;
        }
        let p = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if p.is_null() {
            return None;
        }
        let sid = sid_of_process_handle(p).ok();
        CloseHandle(p);
        let mut sess = 0u32;
        if ProcessIdToSessionId(pid, &mut sess) == 0 {
            return None;
        }
        sid.map(|s| (s, sess))
    }
}

pub fn forward(args: &[String], budget: Duration) -> Result<Bridge> {
    let sid = current_user_sid()?;
    let deadline = Instant::now() + budget;

    // Ưu tiên session console đang hoạt động, rồi mới tới danh sách liệt kê được.
    let mut cands: Vec<u32> = Vec::new();
    if let Some(s) = console_session().filter(|s| *s != 0) {
        cands.push(s);
    }
    for s in list_agent_sessions(&sid) {
        if !cands.contains(&s) {
            cands.push(s);
        }
    }
    cands.truncate(MAX_CANDIDATES);

    // Một TÊN không phải một AGENT, và quyết định `Ambiguous` là quyết định TỪ CHỐI —
    // nuôi nó bằng phép đếm chưa xác thực là để người ngoài tắt cầu. Đã tái hiện trên
    // a14 20/08/2026: một `NamedPipeServerStream` mang tên `tongue.<SID>.7`, không
    // ConnectNamedPipe, không phục vụ gì, làm mọi `ssh a14 tongue …` thoát 2 kèm
    // "có nhiều agent cùng lúc (session [1, 7])" trong khi agent thật vẫn khoẻ ở
    // session 1. Kẻ tạo mồi KHÔNG phải đua với ai: session 7 không tồn tại nên agent
    // thật chẳng bao giờ tranh cái tên đó, và mồi sống qua mọi lần agent restart.
    //
    // Nên đếm AGENT ĐÃ XÁC MINH, không đếm tên. Thăm dò không chờ trước, để một mồi
    // giữ BUSY không ăn hết ngân sách của agent thật.
    let mut ours: Vec<(u32, HANDLE)> = Vec::new();
    let mut foreign: Vec<u32> = Vec::new();
    let mut busy: Vec<u32> = Vec::new();
    for s in &cands {
        match probe(&sid, *s) {
            Probe::Ours(h) => ours.push((*s, h)),
            Probe::Foreign => foreign.push(*s),
            Probe::Busy => busy.push(*s),
            Probe::Vacant => {}
        }
    }

    // Bận là trạng thái BÌNH THƯỜNG của agent thật (nó phục vụ tuần tự, một lượt tới
    // ~6s), nên chỉ khi không có ứng viên rảnh nào mới bỏ tiền chờ.
    if ours.is_empty() {
        for s in busy {
            let name = proto::pipe_name(&sid, s);
            if let Some(h) = open_pipe(&name, deadline) {
                match verify(h, &sid, s) {
                    Probe::Ours(h) => ours.push((s, h)),
                    _ => foreign.push(s),
                }
            }
        }
    }

    let choice = proto::choose(ours.iter().map(|(s, _)| *s).collect(), foreign);
    // `choose` chỉ trả `Use` khi có ĐÚNG một agent xác minh, nên `pop` ở đây là cái duy
    // nhất. Mọi nhánh còn lại không dùng handle nào — đóng hết.
    if let proto::Choice::Use(_) = choice {
        let (_, h) = ours.pop().expect("choose trả Use thì ours không rỗng");
        let res = exchange(h, args, deadline);
        unsafe { CloseHandle(h) };
        return res.map(Bridge::Reply);
    }
    for (_, h) in &ours {
        unsafe { CloseHandle(*h) };
    }
    match choice {
        proto::Choice::Use(_) => unreachable!("đã xử ở trên"),
        proto::Choice::Ambiguous(v) => Ok(Bridge::Ambiguous(v)),
        proto::Choice::Foreign(v) => Ok(Bridge::Foreign(v)),
        proto::Choice::Absent => Ok(Bridge::Absent),
    }
}

fn exchange(h: HANDLE, args: &[String], deadline: Instant) -> Result<Reply> {
    write_all(h, &proto::encode_request(args))?;
    let head: [u8; 4] = read_exact_by(h, 4, deadline)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("khung trả lời hỏng"))?;
    let their = u32::from_le_bytes(head);
    // Hình dạng khung trả lời là bất biến vĩnh viễn, nên vẫn đọc tiếp được kể cả khi
    // lệch — và stderr trong đó chính là câu giải thích lệch ở đâu.
    let code = read_exact_by(h, 1, deadline)?[0];
    let stdout = read_chunk_by(h, deadline)?;
    let mut stderr = read_chunk_by(h, deadline)?;
    if their != proto::PROTO {
        stderr.extend_from_slice(
            format!(
                "tongue: agent nói giao thức {their}, client nói {} — dừng agent rồi chép \
                 lại tongue.exe (Windows không cho ghi đè .exe đang chạy)\n",
                proto::PROTO
            )
            .as_bytes(),
        );
    }
    Ok(Reply {
        code,
        stdout,
        stderr,
    })
}

// ── phía agent (chạy trong session của người dùng) ───────────────────────────

/// HANDLE là `*mut c_void` nên không `Send`. Bọc lại để chuyển quyền sở hữu sang
/// luồng phục vụ: handle được DI CHUYỂN, không chia sẻ — sau khi move không luồng nào
/// khác chạm vào nó nữa, kể cả vòng accept.
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

/// Một slot "đang phục vụ", trả lại khi luồng kết thúc — kể cả khi nó panic.
pub(crate) struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    pub(crate) fn new(c: Arc<AtomicUsize>) -> Self {
        c.fetch_add(1, Ordering::SeqCst);
        Self(c)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn create_instance(name: &str, sd: &SecDesc, first: bool) -> Result<HANDLE> {
    let w = wide(name);
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        // FALSE, và không phải chuyện thẩm mỹ: agent chạy tiến trình con bằng
        // `std::process::Command`, mà Rust gọi CreateProcessW với
        // bInheritHandles = TRUE và Windows thừa kế MỌI handle đang bật cờ inheritable.
        // Đặt TRUE ở đây là đưa cho mỗi tongue con một bản handle server của pipe —
        // đúng họ lỗi mà `spawn_no_inherit` trong vkey.rs được viết ra để chặn.
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        // Chỉ ở instance ĐẦU: đặt ở instance sau thì lời gọi trượt. Nó là thứ biến
        // "có kẻ chiếm tên" từ im lặng (lặng lẽ nối thêm một instance vào pipe của
        // người khác rồi phục vụ client của họ) thành một lỗi nói to.
        | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
    let h = unsafe {
        CreateNamedPipeW(
            w.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            &sa,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(last_err()).context("CreateNamedPipeW");
    }
    Ok(h)
}

fn accept(h: HANDLE) -> Result<()> {
    if unsafe { ConnectNamedPipe(h, null_mut()) } != 0 {
        return Ok(());
    }
    // 535 = client đã nối vào TRONG khe giữa CreateNamedPipeW và ConnectNamedPipe.
    // Đây là THÀNH CÔNG, không phải lỗi. Coi nó là lỗi thì agent rớt kết nối ngẫu
    // nhiên dưới tải, mà tần suất phụ thuộc thời điểm nên không tái hiện được.
    if unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
        return Ok(());
    }
    Err(last_err()).context("ConnectNamedPipe")
}

/// Thứ tự BẮT BUỘC, và cả ba bước đều im lặng khi làm sai.
///
/// `DisconnectNamedPipe` HUỶ mọi dữ liệu client chưa đọc, nên gọi nó ngay sau WriteFile
/// là reply bốc hơi — với hợp đồng "tongue.nvim đọc STDOUT", một reply mất thành stdout
/// RỖNG, tức tầng trên hiểu là mode rỗng chứ không hiểu là lỗi. `FlushFileBuffers` thì
/// BLOCK tới khi client đọc hết (nó trả về ngay khi pipe đứt, nên client chết không làm
/// treo), và đó là lý do cả chuỗi này nằm trên luồng phục vụ riêng.
fn finish(h: HANDLE) {
    unsafe {
        FlushFileBuffers(h);
        DisconnectNamedPipe(h);
        CloseHandle(h);
    }
}

pub(crate) type Handler = Arc<dyn Fn(&[String]) -> (u8, Vec<u8>, Vec<u8>) + Send + Sync>;

/// Chạy lại CHÍNH `tongue` như tiến trình con thay vì gọi hàm trong tiến trình. Con nằm
/// trong session của agent nên nó thấy đúng VKey; và vì nó là một lần chạy tongue bình
/// thường nên hành vi, mã thoát và stderr giống hệt lúc gõ tay — không có nhánh thứ hai
/// nào để lệch nhau về sau.
///
/// Ba thứ được cho không nhờ hình dạng này, và cả ba đều là bug nếu gọi hàm trong tiến
/// trình: `std::process::exit(2)` ở nhánh doctor không giết agent; `ActivateKeyboardLayout`
/// (tác động lên LUỒNG GỌI) vẫn chạy trên main thread của một tiến trình mới, y như hôm
/// nay; và config được đọc lại mỗi lần nên sửa `config.toml` ăn ngay, không phải khởi
/// động lại agent.
fn run_child(exe: &std::path::Path, args: &[String]) -> (u8, Vec<u8>, Vec<u8>) {
    match std::process::Command::new(exe)
        .args(args)
        .env(NO_FORWARD_ENV, "1")
        .output()
    {
        Ok(out) => (
            out.status.code().unwrap_or(2).clamp(0, 255) as u8,
            out.stdout,
            out.stderr,
        ),
        Err(e) => (
            2,
            Vec::new(),
            format!("tongue agent: chạy tongue con thất bại: {e}\n").into_bytes(),
        ),
    }
}

pub fn serve(idle: Duration, listen: Option<std::net::SocketAddr>) -> Result<()> {
    // Giải đường dẫn MỘT lần lúc khởi động, không mỗi request. `current_exe()` trên
    // Windows là chuỗi ghim trong PEB lúc load, nhưng `Command::new` thì mở lại đường
    // dẫn đó ở mỗi lượt — mà Windows cấm ghi đè .exe đang chạy lại CHO PHÉP rename
    // (đúng thủ thuật updater), nên đường dẫn tráo được dưới chân một agent đang sống.
    // Ghim ở đây không đóng được đường "thay exe rồi để client `schtasks /run`" — cùng
    // quyền ghi ấy đã đủ làm việc đó — nhưng nó bỏ cửa sổ tráo-giữa-đời, gần như miễn phí.
    let exe = std::env::current_exe().context("không xác định được đường dẫn tongue")?;
    serve_with(
        idle,
        Arc::new(move |args: &[String]| run_child(&exe, args)),
        listen,
    )
}

fn serve_with(
    idle: Duration,
    handler: Handler,
    listen: Option<std::net::SocketAddr>,
) -> Result<()> {
    if in_service_session() {
        bail!(
            "agent phải chạy TRONG session của người dùng, không phải session 0 — chính nó \
             là thứ bắc cầu qua ranh giới đó. Dùng scheduled task với `-LogonType Interactive` \
             (và nhớ `-MultipleInstances Parallel` + `-AllowStartIfOnBatteries`)."
        );
    }
    let sid = current_user_sid()?;
    let session = current_session()?;
    let name = proto::pipe_name(&sid, session);
    let sd = owner_only_sd(&sid)?;

    let last = Arc::new(Mutex::new(Instant::now()));
    let inflight = Arc::new(AtomicUsize::new(0));
    spawn_reaper(idle, last.clone(), inflight.clone());

    // Tuần tự hoá phần THỰC THI (không phải phần nhận kết nối). Đây là bản Windows của
    // đúng cái `Gate` mà macOS đã phải thêm: hai `select_langid` chạy chồng sẽ giằng
    // nhau trên cùng một cửa sổ foreground, và trên macOS đã đo được hai tiến trình độc
    // lập cách nhau 495 ms cùng bắn chord rồi cả hai thoát 1.
    let serial = Arc::new(Mutex::new(()));

    // Cửa thứ hai, cho tunnel ngược. Bind ĐỒNG BỘ và lỗi được propagate: một cửa
    // vắng mặt trong im lặng nghĩa là client treo ở `connect` mà không ai biết vì
    // sao. Dùng CHUNG `serial`, `inflight` và `last` với đường pipe — một reaper,
    // một khoá thực thi, một vòng đời.
    if let Some(addr) = listen {
        super::tcp::spawn(
            addr,
            handler.clone(),
            serial.clone(),
            inflight.clone(),
            last.clone(),
        )?;
    }

    eprintln!("tongue agent: đang nghe {name}");
    let mut first = true;
    loop {
        let h = create_instance(&name, &sd, first).with_context(|| {
            if first {
                format!(
                    "không tạo được instance ĐẦU của `{name}` — nhiều khả năng đã có agent \
                     khác trong cùng session này, hoặc ai đó chiếm tên đó trước"
                )
            } else {
                format!("không tạo được instance mới của `{name}`")
            }
        })?;
        first = false;
        if let Err(e) = accept(h) {
            unsafe { CloseHandle(h) };
            eprintln!("tongue agent: bỏ qua một kết nối hỏng: {e:#}");
            continue;
        }
        // Tạo instance kế tiếp NGAY sau khi có client, chứ không sau khi phục vụ xong:
        // instance hiện tại đã CONNECTED nên nếu đợi, tên pipe vẫn tồn tại nhưng mọi
        // client mới đều rơi vào ERROR_PIPE_BUSY suốt cả lượt phục vụ (tới ~6s).
        let (h, last, serial, handler) =
            (SendHandle(h), last.clone(), serial.clone(), handler.clone());
        // Đếm NGAY, trước `spawn`. Để `fetch_add` bên trong closure thì giữa lúc
        // `accept()` trả về (client ĐÃ nối) và lúc closure kịp chạy, `inflight` vẫn là
        // 0 — reaper thức dậy đúng khe đó sẽ `exit(0)` trên một kết nối đang sống, và
        // client nhận lỗi ghi rồi thoát 2.
        let busy = InFlight::new(inflight.clone());
        std::thread::spawn(move || {
            let h = h;
            // Khai SAU `h` nên drop TRƯỚC `h`, và quan trọng hơn: drop sau khi `last`
            // đã cập nhật. Là guard chứ không phải `fetch_sub` trần vì `fetch_add` nay
            // nằm ngoài closure — một panic trong `handle_one` mà không có Drop sẽ rò
            // slot vĩnh viễn, và agent không bao giờ idle-exit nữa.
            let _busy = busy;
            if let Err(e) = handle_one(h.0, &serial, &handler) {
                eprintln!("tongue agent: bỏ qua một yêu cầu hỏng: {e:#}");
            }
            finish(h.0);
            *last.lock().unwrap() = Instant::now();
        });
    }
}

/// Không watchdog, và đó là một quyết định chứ không phải một thiếu sót: client ở
/// session 0 tự hồi sinh agent bằng `schtasks /run` khi không thấy pipe, nên "agent đã
/// chết" không phải một trạng thái cần ai canh. Đổi lại phải có đường thoát này, nếu
/// không thì một thứ vài lần một ngày mới gọi lại nuôi một tiến trình sống mãi — và
/// tiện thể KHOÁ CỨNG `tongue.exe`, thứ mà kênh cài đặt trên a14 (chép tay vào
/// `%USERPROFILE%\.local\bin`) cần ghi đè được.
fn spawn_reaper(idle: Duration, last: Arc<Mutex<Instant>>, inflight: Arc<AtomicUsize>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        if inflight.load(Ordering::SeqCst) > 0 {
            continue;
        }
        if last.lock().unwrap().elapsed() >= idle {
            std::process::exit(0);
        }
    });
}

fn handle_one(h: HANDLE, serial: &Mutex<()>, handler: &Handler) -> Result<()> {
    // Hạn định cho phía agent nữa: một client chết nửa chừng (ssh đứt giữa lúc gửi)
    // không được giữ một luồng phục vụ ở đó mãi mãi.
    let deadline = Instant::now() + Duration::from_secs(30);

    let their = u32::from_le_bytes(
        read_exact_by(h, 4, deadline)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("khung yêu cầu hỏng"))?,
    );
    if their != proto::PROTO {
        // Trả lời bằng một khung BÌNH THƯỜNG, tuyệt đối không ngắt kết nối trần: đứt
        // kết nối làm client in ERROR_BROKEN_PIPE rồi người dùng đi tìm nhầm chỗ.
        let msg = format!(
            "tongue agent: giao thức {their} không khớp {} (agent PID {} bản {}) — dừng agent \
             rồi chép lại tongue.exe\n",
            proto::PROTO,
            std::process::id(),
            env!("CARGO_PKG_VERSION"),
        );
        return write_all(h, &proto::encode_reply(2, b"", msg.as_bytes()));
    }

    let n = u32::from_le_bytes(
        read_exact_by(h, 4, deadline)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("số tham số hỏng"))?,
    );
    if n > 32 {
        bail!("quá nhiều tham số ({n})");
    }
    let mut args = Vec::with_capacity(n as usize);
    for _ in 0..n {
        args.push(String::from_utf8(read_chunk_by(h, deadline)?).context("tham số không UTF-8")?);
    }

    // Chốt chặn thứ hai cho danh sách lệnh. Client đã lọc rồi, nhưng đầu kia của dây là
    // một phiên đăng nhập từ xa nên đừng tin nó: không nhận đường dẫn, không nhận
    // `vkey_path` qua dây, không biến pipe thành shell.
    if !proto::forwardable(&args) {
        let msg = format!(
            "tongue agent: từ chối lệnh không nằm trong danh sách ({:?})\n",
            proto::FORWARDABLE
        );
        return write_all(h, &proto::encode_reply(2, b"", msg.as_bytes()));
    }

    let (code, stdout, stderr) = {
        let _g = serial.lock().unwrap_or_else(|e| e.into_inner());
        handler(&args)
    };
    write_all(h, &proto::encode_reply(code, &stdout, &stderr))
}

// ── chẩn đoán ────────────────────────────────────────────────────────────────

/// Trạng thái hỏng MỚI mà máy này chưa từng có, nên nó phải nhìn thấy được: có pipe
/// không, của session nào, và có cửa sổ foreground không.
pub fn diagnose_bridge(task: &str) -> Vec<Finding> {
    let mut fs = Vec::new();
    let sid = match current_user_sid() {
        Ok(s) => s,
        Err(e) => {
            fs.push(Finding {
                level: Level::Fail,
                msg: format!("không đọc được SID của chính mình: {e:#}"),
            });
            return fs;
        }
    };
    let me = current_session().unwrap_or(u32::MAX);
    fs.push(Finding {
        level: if me == 0 { Level::Warn } else { Level::Ok },
        msg: format!(
            "tongue này đang ở session {me}{}",
            if me == 0 {
                " (service hoặc SSH) — mọi lệnh phải đi qua agent"
            } else {
                ""
            }
        ),
    });
    match console_session() {
        Some(s) => fs.push(Finding {
            level: Level::Ok,
            msg: format!("session console đang hoạt động: {s}"),
        }),
        None => fs.push(Finding {
            level: Level::Warn,
            msg: "không có session console — chưa ai đăng nhập, nên VKey cũng chưa chạy và \
                  scheduled task `-LogonType Interactive` sẽ kẹt State=Queued"
                .into(),
        }),
    }
    // Bức tường THỨ HAI, và nó không nằm ở tầng vận chuyển nên pipe không gỡ được:
    // input locale trên Windows là thuộc tính CỦA THREAD, nên `zh` và phần layout của
    // `status` cần một cửa sổ foreground là app thật. Gửi WM_INPUTLANGCHANGEREQUEST vào
    // desktop/shell thì message bị bỏ qua, PostMessage vẫn trả TRUE, và verify trượt.
    match crate::backend::windows::layout::current_langid() {
        Ok(l) => fs.push(Finding {
            level: Level::Ok,
            msg: format!("có cửa sổ foreground, layout của nó = {l}"),
        }),
        Err(e) => fs.push(Finding {
            level: Level::Warn,
            msg: format!(
                "không đọc được layout của cửa sổ foreground ({e:#}) — `vi`/`en` vẫn chạy \
                 (VKey là PostMessage tới cửa sổ tray), nhưng `zh` thì không, và `status` \
                 sẽ báo layout = null"
            ),
        }),
    }

    // Chia đúng cách client chia, nếu không thì doctor và client nói hai chuyện khác
    // nhau về cùng một máy: trước đây một tên bị người khác chiếm được doctor báo
    // `Ok "agent đang nghe"`, trong khi client thì từ chối.
    let mut ours: Vec<u32> = Vec::new();
    let mut foreign: Vec<u32> = Vec::new();
    for s in list_agent_sessions(&sid).into_iter().take(MAX_CANDIDATES) {
        match probe(&sid, s) {
            Probe::Ours(h) => {
                unsafe { CloseHandle(h) };
                ours.push(s);
            }
            // Bận = agent thật đang phục vụ. Với chẩn đoán thì đó vẫn là "có agent".
            Probe::Busy => ours.push(s),
            Probe::Foreign => foreign.push(s),
            Probe::Vacant => {}
        }
    }
    match ours.len() {
        0 => fs.push(Finding {
            level: Level::Warn,
            msg: format!("không thấy agent nào — khởi động: schtasks /run /tn \"{task}\""),
        }),
        1 => fs.push(Finding {
            level: Level::Ok,
            msg: format!(
                "agent đang nghe ở session {} ({})",
                ours[0],
                proto::pipe_name(&sid, ours[0])
            ),
        }),
        n => fs.push(Finding {
            level: Level::Fail,
            msg: format!(
                "có {n} agent cùng lúc (session {ours:?}) — client sẽ TỪ CHỐI chứ không tự \
                 chọn; dừng bớt đi"
            ),
        }),
    }
    if !foreign.is_empty() {
        fs.push(Finding {
            level: Level::Fail,
            msg: format!(
                "có pipe mang tên của bạn ở session {foreign:?} mà chủ KHÔNG phải bạn (hoặc \
                 không đọc nổi chủ) — ai đó đã chiếm tên. Client sẽ từ chối gửi lệnh vào đó."
            ),
        });
    }
    fs
}

pub fn service_session_hint(task: &str) -> String {
    let mut s = String::from(SERVICE_SESSION_ERR);
    if console_session().is_none() {
        s.push_str(
            "\n  Và chưa ai đăng nhập trên máy đó: session tương tác chưa tồn tại, nên \
             agent không thể khởi động.",
        );
    } else {
        s.push_str(&format!(
            "\n  Không thấy agent nào đang nghe. Khởi động nó trong session tương tác:\n    \
             schtasks /run /tn \"{task}\"",
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip THẬT qua một pipe thật: SDDL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    /// ERROR_PIPE_CONNECTED, FlushFileBuffers, khung tin, hạn định — tất cả chạy trên
    /// windows-latest của CI mà không cần desktop nào.
    ///
    /// Bài test này CÒN là bằng chứng cho chính luận điểm của module: runner của GitHub
    /// Actions chạy trong session 0, nên nếu nó xanh thì pipe thật sự không theo session.
    #[test]
    fn round_trip_that_qua_pipe() {
        let sid = current_user_sid().unwrap();
        // Tên riêng cho test: không được đụng vào agent thật của người đang chạy test.
        let name = format!(r"\\.\pipe\tongue.test.{}.{}", sid, std::process::id());

        let srv = {
            let name = name.clone();
            std::thread::spawn(move || {
                let sd = owner_only_sd(&current_user_sid().unwrap()).unwrap();
                let h = create_instance(&name, &sd, true).unwrap();
                accept(h).unwrap();
                let serial = Mutex::new(());
                let handler: Handler = Arc::new(|args: &[String]| {
                    assert_eq!(args, ["status".to_string(), "--json".to_string()]);
                    (1, b"{\"mode\":\"vi\"}\n".to_vec(), b"canh bao\n".to_vec())
                });
                handle_one(h, &serial, &handler).unwrap();
                finish(h);
            })
        };

        let deadline = Instant::now() + Duration::from_secs(10);
        // Server có thể chưa kịp tạo instance — đúng cái khe mà client thật cũng gặp.
        let h = loop {
            if let Some(h) = open_pipe(&name, deadline) {
                break h;
            }
            assert!(Instant::now() < deadline, "server không bao giờ xuất hiện");
            std::thread::sleep(Duration::from_millis(20));
        };
        let r = exchange(h, &["status".to_string(), "--json".to_string()], deadline).unwrap();
        unsafe { CloseHandle(h) };
        srv.join().unwrap();

        assert_eq!(r.code, 1, "mã thoát phải đi nguyên vẹn qua dây");
        assert_eq!(r.stdout, b"{\"mode\":\"vi\"}\n");
        assert_eq!(r.stderr, b"canh bao\n");
    }

    /// FILE_FLAG_FIRST_PIPE_INSTANCE: instance đầu thứ hai cùng tên PHẢI trượt, chứ
    /// không lặng lẽ trở thành một instance nữa của pipe người khác.
    #[test]
    fn khong_the_chiem_ten_da_co_chu() {
        let sid = current_user_sid().unwrap();
        let name = format!(r"\\.\pipe\tongue.test.squat.{}", std::process::id());
        let sd = owner_only_sd(&sid).unwrap();
        let h = create_instance(&name, &sd, true).unwrap();
        assert!(
            create_instance(&name, &sd, true).is_err(),
            "instance ĐẦU thứ hai phải trượt"
        );
        unsafe { CloseHandle(h) };
    }

    /// `fetch_add` nay nam NGOAI closure de bit khe reaper-giet-client-dang-noi, nen
    /// duong tra slot phai chiu duoc ca panic. Khong co Drop thi mot panic trong
    /// `handle_one` ro slot vinh vien va agent khong bao gio idle-exit nua.
    #[test]
    fn slot_duoc_tra_lai_ke_ca_khi_luong_panic() {
        let c = Arc::new(AtomicUsize::new(0));
        {
            let _g = InFlight::new(c.clone());
            assert_eq!(c.load(Ordering::SeqCst), 1, "new() phai dem len");
        }
        assert_eq!(c.load(Ordering::SeqCst), 0, "drop thuong phai tra slot");

        let c2 = c.clone();
        let r = std::panic::catch_unwind(move || {
            let _g = InFlight::new(c2);
            panic!("mo phong handle_one panic");
        });
        assert!(r.is_err(), "closure phai that su panic");
        assert_eq!(
            c.load(Ordering::SeqCst),
            0,
            "panic van phai tra slot -- neu khong, agent song mai"
        );
    }

    #[test]
    fn sid_va_session_doc_duoc_o_moi_session() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "SID lạ: {sid}");
        // In ra để đọc trong log CI: runner ở session 0 thì bài test trên là bằng chứng
        // pipe xuyên session.
        eprintln!("session của tiến trình test = {:?}", current_session());
    }
}
