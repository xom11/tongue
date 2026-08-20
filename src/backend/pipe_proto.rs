//! Phần thuần của cầu named pipe: tên pipe, khung tin, danh sách lệnh được chuyển
//! tiếp. Nằm ngoài `windows/` vì cùng lý do như `vkey_shm.rs` và `hkl.rs` — và ở đây
//! lý do đó nặng hơn: đây là mảnh DUY NHẤT của đường session-0 mà CI kiểm được trên
//! cả hai runner, phần còn lại cần một session tương tác thật.

/// Số giao thức, RIÊNG với `CARGO_PKG_VERSION`. Chỉ bump khi KHUNG tin đổi: ghim
/// theo version của crate thì một bản vá macOS không liên quan cũng làm chết mọi
/// agent đang chạy trên Windows.
///
/// Hình dạng của khung TRẢ LỜI là bất biến vĩnh viễn — `[proto u32][code u8]
/// [chunk stdout][chunk stderr]` — kể cả khi PROTO đổi. Nhờ vậy một client bản khác
/// vẫn đọc được câu "lệch phiên bản" thay vì nhận đứt kết nối trần.
pub const PROTO: u32 = 1;

/// Trần một chunk. Tham số của tongue dài nhất là vài chục byte, output dài nhất là
/// `doctor`; trần ở đây chỉ để một client hỏng không kéo agent vào cấp phát khổng lồ.
pub const MAX_CHUNK: u32 = 1 << 20;

/// `\\.\pipe\` là namespace TOÀN MÁY — đó là cả lý do module này tồn tại. Đổi sang
/// `Local\` là cầu sập, mà triệu chứng duy nhất là "vẫn báo session 0".
///
/// SID chứ không phải `%USERNAME%`: username là biến môi trường (người gọi tự đặt
/// được), trùng được giữa tài khoản local và domain, và đổi được. SID là thuộc tính
/// token, và client ở session 0 đọc SID của CHÍNH NÓ — không cần biết gì về session 1.
///
/// Session id cũng có trong tên vì namespace pipe KHÔNG chia theo session: hai phiên
/// cùng một user (console + RDP, fast user switching) sẽ tranh đúng một tên nếu thiếu.
///
/// Tên KHÔNG phải hàng rào — ai cũng tạo được pipe mang tên chứa SID của bạn. Hàng
/// rào là DACL tường minh + `FILE_FLAG_FIRST_PIPE_INSTANCE` ở server và
/// `SECURITY_IDENTIFICATION` ở client.
pub fn pipe_name(sid: &str, session: u32) -> String {
    format!(r"\\.\pipe\tongue.{sid}.{session}")
}

/// Tiền tố dùng để liệt kê mọi agent của CÙNG user. Namespace pipe liệt kê được, nên
/// "session nào đang phục vụ" là một phép TRA chứ không phải một phép đoán.
pub fn pipe_prefix(sid: &str) -> String {
    format!("tongue.{sid}.")
}

/// Tách session id ra khỏi một tên pipe trần (phần sau `\\.\pipe\`).
pub fn session_of(entry: &str, sid: &str) -> Option<u32> {
    entry.strip_prefix(&pipe_prefix(sid))?.parse().ok()
}

/// Lệnh được phép đi qua dây. Đầu kia của dây là một phiên đăng nhập từ xa, nên
/// đừng biến pipe thành shell: `argv` đi verbatim (thêm cờ mới không phải bump
/// giao thức) nhưng subcommand ĐẦU phải nằm trong danh sách này.
///
/// `agent` cố ý VẮNG MẶT: nó LÀ đầu kia của dây, chuyển tiếp nó là đệ quy không đáy.
/// `--version` và `--help` cũng vắng mặt, và đó là chủ đích — chúng phải nói về
/// binary bạn vừa gọi, vì đó chính là thứ cần biết khi đang gỡ một ca lệch phiên bản.
pub const FORWARDABLE: &[&str] = &["vi", "en", "zh", "status", "doctor"];

pub fn forwardable(args: &[String]) -> bool {
    matches!(args.first(), Some(a) if FORWARDABLE.contains(&a.as_str()))
}

/// Khung: `<u32 le độ dài><bytes>`. Byte-mode pipe + tiền tố độ dài, CỐ Ý không dùng
/// `PIPE_TYPE_MESSAGE`: message mode bắt client phải gọi thêm `SetNamedPipeHandleState`
/// và bắt vòng đọc phải hiểu `ERROR_MORE_DATA` (234) là "chưa hết" chứ không phải lỗi
/// — hai bề mặt mới để đổi lấy đúng cái mà tiền tố độ dài đã cho. Kiểu hỏng của message
/// mode cũng tệ hơn: chạy tốt cho tới khi bản tin dài ra (`doctor` chứ không phải `vi`).
pub fn put_chunk(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

pub fn chunk_len(head: [u8; 4]) -> anyhow::Result<usize> {
    let n = u32::from_le_bytes(head);
    if n > MAX_CHUNK {
        anyhow::bail!("khung dài bất thường ({n} byte)");
    }
    Ok(n as usize)
}

/// Yêu cầu: `[proto u32][n u32][chunk arg]*n`.
pub fn encode_request(args: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&PROTO.to_le_bytes());
    out.extend_from_slice(&(args.len() as u32).to_le_bytes());
    for a in args {
        put_chunk(&mut out, a.as_bytes());
    }
    out
}

/// Trả lời: `[proto u32][code u8][chunk stdout][chunk stderr]`.
pub fn encode_reply(code: u8, stdout: &[u8], stderr: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&PROTO.to_le_bytes());
    out.push(code);
    put_chunk(&mut out, stdout);
    put_chunk(&mut out, stderr);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_pipe_la_namespace_toan_may() {
        let n = pipe_name("S-1-5-21-1-2-3-1001", 1);
        assert!(n.starts_with(r"\\.\pipe\"), "sai tiền tố: {n}");
        assert!(
            !n.contains(r"Local\"),
            "Local\\ theo session — cầu sập: {n}"
        );
    }

    #[test]
    fn ten_pipe_phan_biet_session() {
        let sid = "S-1-5-21-1-2-3-1001";
        assert_ne!(pipe_name(sid, 1), pipe_name(sid, 2));
    }

    #[test]
    fn doc_nguoc_duoc_session_tu_ten() {
        let sid = "S-1-5-21-1-2-3-1001";
        let entry = pipe_name(sid, 7)
            .strip_prefix(r"\\.\pipe\")
            .unwrap()
            .to_string();
        assert_eq!(session_of(&entry, sid), Some(7));
        assert_eq!(session_of(&entry, "S-1-5-18"), None);
        assert_eq!(session_of("tongue.S-1-5-21-1-2-3-1001.x", sid), None);
    }

    /// Chốt chặn đệ quy. `agent` LÀ đầu kia của dây; nếu nó lọt vào danh sách thì một
    /// lời gọi từ session 0 sẽ đẻ ra vòng lặp không đáy.
    #[test]
    fn agent_khong_bao_gio_duoc_chuyen_tiep() {
        assert!(!FORWARDABLE.contains(&"agent"));
        assert!(!forwardable(&["agent".to_string()]));
    }

    /// `--version`/`--help` phải nói về binary VỪA GỌI, không phải của agent — đó
    /// chính là thứ người ta cần khi đang gỡ một ca lệch phiên bản.
    #[test]
    fn co_toan_cuc_khong_duoc_chuyen_tiep() {
        for a in ["--version", "-V", "--help", "-h"] {
            assert!(!forwardable(&[a.to_string()]), "{a} không được đi qua dây");
        }
    }

    #[test]
    fn lenh_that_thi_duoc_chuyen_tiep() {
        for a in ["vi", "en", "zh", "status", "doctor"] {
            assert!(forwardable(&[a.to_string()]), "{a} phải đi được");
        }
        assert!(forwardable(&["status".into(), "--json".into()]));
        assert!(!forwardable(&[]));
        assert!(!forwardable(&["khong-co-lenh-nay".into()]));
    }

    #[test]
    fn khung_di_ve_nguyen_ven() {
        let mut buf = Vec::new();
        put_chunk(&mut buf, b"");
        put_chunk(&mut buf, b"status --json");
        let n0 = chunk_len(buf[0..4].try_into().unwrap()).unwrap();
        assert_eq!(n0, 0);
        let off = 4 + n0;
        let n1 = chunk_len(buf[off..off + 4].try_into().unwrap()).unwrap();
        assert_eq!(&buf[off + 4..off + 4 + n1], b"status --json");
    }

    #[test]
    fn khung_qua_dai_thi_tu_choi() {
        assert!(chunk_len((MAX_CHUNK + 1).to_le_bytes()).is_err());
        assert!(chunk_len(MAX_CHUNK.to_le_bytes()).is_ok());
    }

    /// Hình dạng khung trả lời là bất biến vĩnh viễn: nhờ nó, client bản khác vẫn
    /// đọc được câu "lệch phiên bản" thay vì nhận một cú đứt kết nối trần.
    #[test]
    fn khung_tra_loi_bat_dau_bang_proto_roi_toi_code() {
        let r = encode_reply(1, b"out", b"err");
        assert_eq!(u32::from_le_bytes(r[0..4].try_into().unwrap()), PROTO);
        assert_eq!(r[4], 1);
    }

    #[test]
    fn khung_yeu_cau_mang_proto_va_so_tham_so() {
        let r = encode_request(&["status".into(), "--json".into()]);
        assert_eq!(u32::from_le_bytes(r[0..4].try_into().unwrap()), PROTO);
        assert_eq!(u32::from_le_bytes(r[4..8].try_into().unwrap()), 2);
    }
}
