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
///
/// Danh sách này gác VỊ TRÍ 0 và chỉ vị trí 0. `tongue --help` không đi qua dây, nhưng
/// `tongue status --help` thì CÓ — clap gắn `-h/--help` vào mọi subcommand và CLI này
/// không đặt `DisableHelpFlag`, nên câu trả lời đến từ binary ở ĐẦU KIA. Đó là giới hạn
/// đã biết, không phải sơ suất: quét toàn argv sẽ giết đúng tính chất "argv đi verbatim,
/// thêm cờ mới không phải bump giao thức". Nó cũng không phải lỗ hổng — DACL owner-only
/// cộng `PIPE_REJECT_REMOTE_CLIENTS` nghĩa là người gửi duy nhất là chính user. Ca thật
/// sự cần "nói về binary vừa gọi" là lệch phiên bản, và ca đó đã được phục vụ ở chỗ
/// khác: `handle_one` trả lời kèm PID và version của agent TRƯỚC khi chạy tiến trình con.
pub const FORWARDABLE: &[&str] = &["vi", "en", "zh", "status", "doctor"];

/// Lệnh được phép đi qua CỬA TCP (tunnel ngược), hẹp hơn `FORWARDABLE` một cách cố ý.
///
/// `doctor` và `status` vắng mặt: `doctor` in tên pipe — có SID trong đó — và đường
/// dẫn VKey, còn tunnel chỉ cần đọc và đặt mode. Hai cửa vào khác nhau thì hai bề mặt
/// khác nhau; gộp chúng lại là cho cửa rộng hơn thừa hưởng thứ nó không cần.
pub const TCP_VERBS: &[&str] = &["vi", "en", "zh"];

/// Ở đây chứ không phải trong `windows/tcp.rs` vì cùng lý do như `forwardable`: đây là
/// mảnh kiểm được trên CẢ HAI runner, còn phần còn lại cần một session tương tác thật.
pub fn tcp_allowed(args: &[String]) -> bool {
    match args.first() {
        // Dòng rỗng = `tongue` trần = đọc mode, và đó là lệnh được gọi nhiều nhất. Bỏ
        // sót nó thì cửa trông như chạy (đặt được) mà vế đọc thì không.
        None => true,
        Some(a) => TCP_VERBS.contains(&a.as_str()),
    }
}

pub fn forwardable(args: &[String]) -> bool {
    match args.first() {
        // `tongue` trần là lệnh ĐỌC, và nó là lệnh được gọi nhiều nhất -- preset
        // `tongue` của tongue.nvim dùng đúng dạng này để lấy mode. Bỏ sót nó thì cầu
        // trông như chạy (`tongue vi` qua được) trong khi vế đọc thì không, và triệu
        // chứng là "set được mà không đọc được" -- rất khó lần ra.
        None => true,
        Some(a) => FORWARDABLE.contains(&a.as_str()),
    }
}

/// Kết cục một lượt tra agent, sau khi đã thăm dò từng tên.
///
/// Tách ra khỏi `windows/pipe.rs` vì đây đúng là chỗ bug sống: quyết định TỪ CHỐI từng
/// được nuôi bằng phép đếm TÊN, mà tên thì ai cũng tạo được. Ở đây nó thuần, nên kiểm
/// được trên cả hai runner — còn nếu để trong `forward` thì muốn kiểm phải có hai tài
/// khoản Windows và một desktop thật.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    /// Đúng một agent ĐÃ XÁC MINH.
    Use(u32),
    /// Nhiều hơn một agent đã xác minh — từ chối, không tự chọn, vì lái nhầm session
    /// nghĩa là đổi bộ gõ của một desktop khác.
    Ambiguous(Vec<u32>),
    /// Không cái nào xác minh được, mà có tên đang bị ai đó giữ.
    Foreign(Vec<u32>),
    Absent,
}

/// `ours` = session đã chứng minh chủ đúng là ta. `foreign` = tên có người nghe nhưng
/// không chứng minh được (lệch SID, hoặc không đọc nổi chủ).
pub fn choose(ours: Vec<u32>, foreign: Vec<u32>) -> Choice {
    match ours.len() {
        0 if foreign.is_empty() => Choice::Absent,
        0 => Choice::Foreign(foreign),
        1 => Choice::Use(ours[0]),
        _ => Choice::Ambiguous(ours),
    }
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
    #[test]
    fn cua_tcp_hep_hon_cua_pipe() {
        // Bất biến, không phải sở thích: cửa TCP mở ra cho một tunnel mà đầu kia là
        // một máy khác, nên nó không được thừa hưởng bề mặt của cửa pipe.
        for v in TCP_VERBS {
            assert!(FORWARDABLE.contains(v), "{v} phải nằm trong cả hai");
        }
        assert!(TCP_VERBS.len() < FORWARDABLE.len(), "TCP phải HẸP hơn");
        for v in ["doctor", "status"] {
            assert!(!TCP_VERBS.contains(&v), "{v} không được đi qua tunnel");
        }
    }

    #[test]
    fn tcp_cho_doc_mode_va_ba_lenh_dat() {
        let a = |s: &str| -> Vec<String> { s.split_whitespace().map(str::to_owned).collect() };
        // Dòng rỗng = đọc mode. Đây là lệnh được gọi nhiều nhất.
        assert!(tcp_allowed(&a("")));
        for v in ["vi", "en", "zh"] {
            assert!(tcp_allowed(&a(v)), "{v} phải qua được");
        }
        for v in ["doctor", "status", "agent", "vi; rm -rf /", "../tongue"] {
            assert!(!tcp_allowed(&a(v)), "{v} phải bị chặn");
        }
    }

    #[test]
    fn tcp_gac_vi_tri_0_va_chi_vi_tri_0() {
        // Cùng luật với `forwardable`: argv đi verbatim sau vị trí đầu, nên thêm một
        // cờ mới không phải bump giao thức.
        let args: Vec<String> = vec!["vi".into(), "--khong-ton-tai".into()];
        assert!(tcp_allowed(&args));
        let args: Vec<String> = vec!["doctor".into(), "vi".into()];
        assert!(!tcp_allowed(&args));
    }

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
    fn lenh_doc_tran_cung_duoc_chuyen_tiep() {
        // `tongue` không tham số = đọc mode, và preset `tongue` của tongue.nvim gọi
        // đúng dạng này. Quên nó thì `set` qua cầu mà `get` thì không.
        assert!(forwardable(&[]), "lệnh đọc trần phải qua được cầu");
    }

    #[test]
    fn lenh_that_thi_duoc_chuyen_tiep() {
        for a in ["vi", "en", "zh", "status", "doctor"] {
            assert!(forwardable(&[a.to_string()]), "{a} phải đi được");
        }
        assert!(forwardable(&["status".into(), "--json".into()]));
        assert!(!forwardable(&["khong-co-lenh-nay".into()]));
        // Lệnh trần thì CÓ đi được -- xem `lenh_doc_tran_cung_duoc_chuyen_tiep`.
        // Dòng này từng khẳng định ngược lại, và nó sai: `tongue` không tham số là
        // lệnh ĐỌC, không phải lệnh meta như `--version`.
    }

    /// Bài test chịu lực cho ca đã TÁI HIỆN trên a14 20/08/2026: một
    /// `NamedPipeServerStream` mang tên `tongue.<SID>.7` — không ConnectNamedPipe,
    /// không phục vụ gì, session 7 không tồn tại — làm mọi `ssh a14 tongue …` thoát 2
    /// kèm "có nhiều agent cùng lúc (session [1, 7])", trong khi agent thật vẫn khoẻ ở
    /// session 1. Kẻ tạo mồi không phải đua với ai và mồi sống qua mọi lần agent restart.
    ///
    /// Một cái tên KHÔNG phải một agent. Mồi phải bị bỏ qua, agent thật phải được dùng.
    #[test]
    fn moi_khong_lam_ket_agent_that() {
        assert_eq!(choose(vec![1], vec![7]), Choice::Use(1));
        assert_eq!(choose(vec![1], vec![5, 7, 9]), Choice::Use(1));
    }

    /// Hai agent THẬT thì vẫn phải từ chối — đó là hành vi đúng, không phải bug.
    #[test]
    fn hai_agent_da_xac_minh_thi_van_tu_choi() {
        assert_eq!(choose(vec![1, 2], vec![]), Choice::Ambiguous(vec![1, 2]));
        assert_eq!(choose(vec![1, 2], vec![7]), Choice::Ambiguous(vec![1, 2]));
    }

    /// Chỉ có tên của người khác: KHÔNG được rơi xuống `Absent`. Absent kéo theo
    /// `schtasks /run` rồi thử lại — đi đánh thức agent trong khi thứ chắn đường là
    /// pipe của người khác, và câu lỗi in ra sẽ chỉ sai chỗ.
    #[test]
    fn chi_co_ten_nguoi_khac_thi_khong_phai_absent() {
        assert_eq!(choose(vec![], vec![7]), Choice::Foreign(vec![7]));
        assert_eq!(choose(vec![], vec![]), Choice::Absent);
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
