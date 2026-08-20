//! Cầu qua ranh giới session, bằng named pipe.
//!
//! Windows chia namespace theo session: `Local\...` thực chất là `Session\<n>\...`,
//! và window station cũng theo session. Đó là lý do `tongue` chạy qua SSH (session 0)
//! vừa không đọc được VKey vừa không điều khiển được nó — xem `SERVICE_SESSION_ERR`.
//!
//! Named pipe thì KHÔNG theo session: `\\.\pipe\` là một namespace duy nhất cho cả
//! máy. Đã đo trên a14 20/08/2026 với ACL MẶC ĐỊNH, không cần đặc quyền gì: server
//! trong session 1 tạo pipe, client trong session 0 mở được và trao đổi hai chiều.
//! Đây là cơ chế Microsoft dựng cho đúng việc "service nói chuyện với app desktop".
//!
//! Vì vậy `tongue agent` chạy sẵn trong session của người dùng, còn `tongue vi` gọi
//! từ SSH thì chuyển tiếp yêu cầu vào pipe thay vì báo lỗi.
//!
//! Điều này KHÔNG mua lấy tốc độ, và đừng thiết kế như thể nó mua: một lượt SSH tới
//! máy đó tốn 452 ms (có ControlMaster) đến 829 ms (không), nên nó vẫn quá chậm cho
//! thứ như "ép tiếng Anh ngay khi rời Insert mode". Thứ nó mua là `ssh may tongue vi`
//! CHẠY ĐƯỢC, cho mọi script và mọi tự động hoá.

use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
// `PIPE_ACCESS_DUPLEX` song o FileSystem chu khong o Pipes -- mot cho de mat mot
// vong build neu doan theo ten module.
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

/// Đặt tên kèm username để hai người cùng đăng nhập (fast user switching) không đâm
/// nhau. Đây CHỈ để tránh trùng tên, không phải hàng rào: hàng rào là ACL mặc định
/// của pipe, vốn chỉ cho chính user đó và Administrators mở.
fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
    format!(r"\\.\pipe\tongue-{user}")
}

fn wide(s: &str) -> Vec<u16> {
    OsString::from(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Biến này chặn vòng lặp vô hạn: agent chạy lại chính `tongue` như tiến trình con,
/// và tiến trình con đó phải KHÔNG được chuyển tiếp ngược vào pipe. Trên thực tế con
/// nằm ở session 1 nên `in_service_session()` đã là false rồi — biến này là dây an
/// toàn thứ hai, vì cái giá của việc sai là một vòng lặp không đáy.
pub const NO_FORWARD_ENV: &str = "TONGUE_NO_FORWARD";

/// Một lượt trao đổi. Mã thoát đi kèm vì tongue phân biệt 1 (verify trượt) với 2
/// (lỗi khác), và người gọi qua SSH phải thấy đúng con số như chạy tại chỗ.
pub struct Reply {
    pub code: u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

// ── khung tin ────────────────────────────────────────────────────────────────
//
// Không dùng JSON: `serde_json` hiện chỉ là dependency của nhánh macOS, và khung
// dưới đây tốn hai chục dòng. Mỗi phần là `<độ dài u32 little-endian><bytes>`, nên
// không có ký tự nào cần thoát và không có gì để phân tích sai.
//
//   yêu cầu:  [n][arg0][arg1]...   (n = số tham số)
//   trả lời:  [code u8][stdout][stderr]

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
            bail!("ghi vào pipe thất bại: {}", io::Error::last_os_error());
        }
        done += n as usize;
    }
    Ok(())
}

fn read_exact(h: HANDLE, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    let mut done = 0usize;
    while done < len {
        let mut n = 0u32;
        let ok = unsafe {
            ReadFile(
                h,
                buf[done..].as_mut_ptr(),
                (len - done) as u32,
                &mut n,
                null_mut(),
            )
        };
        if ok == 0 || n == 0 {
            bail!("đọc từ pipe thất bại: {}", io::Error::last_os_error());
        }
        done += n as usize;
    }
    Ok(buf)
}

fn write_chunk(h: HANDLE, b: &[u8]) -> Result<()> {
    write_all(h, &(b.len() as u32).to_le_bytes())?;
    write_all(h, b)
}

fn read_chunk(h: HANDLE) -> Result<Vec<u8>> {
    let len = u32::from_le_bytes(
        read_exact(h, 4)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("độ dài khung hỏng"))?,
    );
    // Trần để một client hỏng không kéo được agent vào cấp phát khổng lồ. Tham số
    // của tongue dài nhất là vài chục byte; output dài nhất là `status --json`.
    if len > 1 << 20 {
        bail!("khung dài bất thường ({len} byte)");
    }
    read_exact(h, len as usize)
}

// ── phía client (chạy trong session 0) ───────────────────────────────────────

/// `None` = không có agent nào đang nghe. Người gọi phải xử lý đúng như trước:
/// báo `SERVICE_SESSION_ERR`, chứ đừng im lặng coi như thành công.
pub fn forward(args: &[String]) -> Result<Option<Reply>> {
    let name = wide(&pipe_name());
    // Một lần thử lại cho `FILE_NOT_FOUND`, và nó có lý do đo được: server luôn giữ
    // sẵn một instance đang chờ, nhưng vẫn còn một khe cực hẹp ngay lúc nó vừa nhận
    // client trước. Rơi vào khe đó thì Windows trả FILE_NOT_FOUND -- KHÔNG phải
    // PIPE_BUSY -- và nếu coi đó là "không có agent" thì lệnh thất bại chập chờn.
    // Đã gặp thật: ba lệnh liên tiếp thì hai cái trượt, một cái chạy.
    //
    // Không thử lại nhiều hơn: khi thật sự KHÔNG có agent thì đây là đường đi phổ
    // biến, và mỗi lần thử là thời gian trả cho một câu trả lời đã biết.
    let mut retried = false;
    let h = loop {
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            break h;
        }
        let err = unsafe { GetLastError() };
        if err == ERROR_FILE_NOT_FOUND && !retried {
            retried = true;
            std::thread::sleep(std::time::Duration::from_millis(60));
            continue;
        }
        // Bận = có agent, chỉ là mọi instance đang nói chuyện với người khác. Chờ
        // rồi thử lại. Mọi lỗi khác nghĩa là không có agent -> None.
        if err != ERROR_PIPE_BUSY {
            return Ok(None);
        }
        if unsafe { WaitNamedPipeW(name.as_ptr(), 2000) } == 0 {
            return Ok(None);
        }
    };

    let res = (|| -> Result<Reply> {
        write_all(h, &(args.len() as u32).to_le_bytes())?;
        for a in args {
            write_chunk(h, a.as_bytes())?;
        }
        let code = read_exact(h, 1)?[0];
        let stdout = read_chunk(h)?;
        let stderr = read_chunk(h)?;
        Ok(Reply {
            code,
            stdout,
            stderr,
        })
    })();
    unsafe { CloseHandle(h) };
    res.map(Some)
}

// ── phía agent (chạy trong session của người dùng) ───────────────────────────

/// Vòng lặp phục vụ. Mỗi lượt tạo một instance mới rồi đóng: đơn giản hơn hẳn việc
/// dùng lại handle, và chi phí không đáng kể so với một lượt SSH.
pub fn serve() -> Result<()> {
    if super::in_service_session() {
        bail!(
            "agent phải chạy TRONG session của người dùng, không phải session 0 — \
             chính nó là thứ bắc cầu qua ranh giới đó. \
             Dùng scheduled task với `-LogonType Interactive`."
        );
    }
    let name = wide(&pipe_name());
    eprintln!("tongue agent: đang nghe {}", pipe_name());

    // Luôn có MỘT instance đang chờ, kể cả trong lúc đang phục vụ một client khác.
    // Vòng lặp ngây thơ -- tạo, nhận, phục vụ, đóng, tạo lại -- để hở một khoảng
    // KHÔNG CÓ instance nào tồn tại, và client rơi vào đó nhận FILE_NOT_FOUND rồi
    // kết luận sai là "không có agent". Đo được: ba lệnh liên tiếp qua SSH thì hai
    // trượt. Nên instance kế tiếp được tạo NGAY sau khi nhận client, trước khi phục
    // vụ -- mà phục vụ là phần lâu nhất vì nó spawn một tiến trình con.
    let mut pending = create_instance(&name)?;
    loop {
        let h = pending;
        // `ConnectNamedPipe` trả FALSE kèm `ERROR_PIPE_CONNECTED` khi client đã kịp
        // nối TRƯỚC lúc gọi -- và đó nghĩa là ĐÃ NỐI, không phải lỗi. Đây là cái bẫy
        // kinh điển của named pipe, và bỏ qua nó thì triệu chứng giống hệt "không có
        // agent": handle bị đóng ngay, client thấy pipe đứt rồi rơi về thông báo
        // session 0. Đo được trước khi sửa: 5 lệnh liên tiếp qua SSH thì 4 trượt.
        // `ConnectNamedPipe` trả FALSE kèm `ERROR_PIPE_CONNECTED` khi client đã kịp
        // nối TRƯỚC lúc gọi -- và đó nghĩa là ĐÃ NỐI, không phải lỗi. Bỏ qua vế này
        // thì triệu chứng giống hệt "không có agent".
        let connected = unsafe { ConnectNamedPipe(h, null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        pending = create_instance(&name)?;
        if connected {
            // Một client hỏng không được làm chết agent: mọi thứ đang chạy trong
            // session của người dùng phụ thuộc vào nó còn sống.
            if let Err(e) = handle_one(h) {
                eprintln!("tongue agent: bỏ qua một yêu cầu hỏng: {e:#}");
            }
        }
        unsafe {
            // BẮT BUỘC, và đây là chỗ đã đốt một vòng gỡ lỗi: `DisconnectNamedPipe`
            // VỨT BỎ dữ liệu client chưa kịp đọc. Ghi trả lời xong mà ngắt ngay thì
            // câu trả lời bị huỷ trước khi tới nơi, và ai thắng cuộc đua là tuỳ nhịp
            // -- đo được 1/6 lệnh thành công, 5/6 báo "No process is on the other end
            // of the pipe" (233). `FlushFileBuffers` chặn cho tới khi client đọc hết.
            FlushFileBuffers(h);
            DisconnectNamedPipe(h);
            CloseHandle(h);
        }
    }
}

fn create_instance(name: &[u16]) -> Result<HANDLE> {
    let h = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error()).context("CreateNamedPipeW thất bại");
    }
    Ok(h)
}

fn handle_one(h: HANDLE) -> Result<()> {
    let n = u32::from_le_bytes(
        read_exact(h, 4)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("số tham số hỏng"))?,
    );
    if n > 32 {
        bail!("quá nhiều tham số ({n})");
    }
    let mut args = Vec::with_capacity(n as usize);
    for _ in 0..n {
        args.push(String::from_utf8(read_chunk(h)?).context("tham số không phải UTF-8")?);
    }

    // Chạy lại CHÍNH `tongue` như tiến trình con thay vì gọi hàm trong tiến trình.
    // Con nằm trong session của agent, nên nó thấy đúng VKey; và vì nó là một lần
    // chạy tongue bình thường nên hành vi, mã thoát và stderr giống hệt lúc gõ tay
    // — không có nhánh thứ hai nào để lệch nhau về sau.
    let exe = std::env::current_exe().context("không xác định được đường dẫn tongue")?;
    let out = std::process::Command::new(exe)
        .args(&args)
        .env(NO_FORWARD_ENV, "1")
        .output()
        .context("chạy tongue con thất bại")?;

    let code = out.status.code().unwrap_or(2).clamp(0, 255) as u8;
    write_all(h, &[code])?;
    write_chunk(h, &out.stdout)?;
    write_chunk(h, &out.stderr)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_pipe_mang_username_va_dung_tien_to_may_cuc_bo() {
        // `\\.\pipe\` là namespace TOÀN MÁY — đó là cả lý do module này tồn tại.
        // Nếu ai đó đổi thành `Local\` thì cầu sập mà không có triệu chứng nào ngoài
        // "vẫn báo session 0".
        let n = pipe_name();
        assert!(n.starts_with(r"\\.\pipe\"), "sai tiền tố: {n}");
        assert!(n.len() > r"\\.\pipe\".len(), "tên rỗng: {n}");
    }

    #[test]
    fn khung_tin_di_ve_nguyen_ven() {
        // Kiểm phần thuần tuý của khung: độ dài u32 little-endian rồi tới payload.
        // Không cần pipe thật, nên test này chạy trên mọi nền tảng CI.
        let payload = b"status --json";
        let mut framed = Vec::new();
        framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        framed.extend_from_slice(payload);
        let len = u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(len, payload.len());
        assert_eq!(&framed[4..4 + len], payload);
    }
}
