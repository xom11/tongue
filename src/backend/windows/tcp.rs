//! Cửa TCP loopback: đường vào THỨ HAI cho agent, dành cho một tunnel ngược `ssh -R`.
//!
//! Vì sao thêm một cửa khi đã có named pipe: pipe phục vụ lời gọi đến từ SESSION 0
//! (phiên ssh của Windows), và mọi lời gọi ấy trả giá một lần khởi động PowerShell
//! trước khi làm bất cứ việc gì — đo 20/08/2026 trên a14: 293 ms trên tổng 656 ms
//! một chặng. Với thứ chạy mỗi lần rời Insert mode, đó là cả ngân sách.
//!
//! Tunnel thì không sinh shell nào. Người ngồi ở máy này mở `ssh -R` sang máy chạy
//! nvim, và lời gọi đi NGƯỢC kênh của phiên đó. Quan trọng hơn tốc độ: đầu này của
//! ống đã nằm trong session của người dùng — nơi VKey sống — nên không còn ranh
//! giới session nào để bắc cầu. Đo được 11.6 ms round trip so với 656 ms.
//!
//! Pipe KHÔNG bị thay thế. Nó vẫn là đường duy nhất cho `ssh a14 tongue vi` trong
//! script và tự động hoá, nơi không có phiên nào của người dùng để đi nhờ.
//!
//! Giao thức là MỘT DÒNG văn bản, cố ý khác khung nhị phân của pipe: client ở đây là
//! một script bash, và viết `u32` little-endian trong bash là thứ không nên tồn tại.
//!
//!     yêu cầu:  "<argv, phân tách bằng khoảng trắng>\n"   (dòng rỗng = đọc mode)
//!     trả lời:  "<code> <stdout một dòng>\n"
//!
//! Hàng rào, theo thứ tự:
//!
//!   * chỉ bind loopback, và từ chối thẳng nếu được yêu cầu bind ra ngoài. Một cổng
//!     nghe trên interface thật là chuyện khác hẳn về mức độ phơi bày;
//!   * allowlist RIÊNG và hẹp hơn `FORWARDABLE` của pipe. `doctor` và `status` cố ý
//!     vắng mặt: `doctor` in tên pipe (chứa SID) và đường dẫn VKey, mà tunnel không
//!     cần chúng. Bề mặt ở đây đúng bằng "đọc mode" và "đặt mode";
//!   * trần độ dài dòng và deadline hai chiều, nên một client chết nửa chừng không
//!     giữ được slot `inflight` — thứ mà reaper đọc để quyết định idle-exit.
//!
//! Không có xác thực ở tầng ứng dụng, và đó là một quyết định. Một token phải nằm ở
//! đâu đó đọc được, tức nó xác thực NGƯỜI MANG chứ không phải chủ thể, và tập đọc
//! được nó gần đúng bằng tập đã nối được tới loopback. Trường hợp xấu nhất ở đây là
//! ai đó cùng máy đổi bộ gõ của bạn.

use super::pipe::{Handler, InFlight};
use crate::backend::pipe_proto as proto;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tham số dài nhất ở đây là ba ký tự. Trần này chỉ để một client hỏng không kéo
/// agent vào một lần cấp phát vô hạn.
const MAX_LINE: u64 = 256;

const DEADLINE: Duration = Duration::from_secs(5);

/// Mở cửa TCP và phục vụ trong một thread nền.
///
/// Bind ĐỒNG BỘ rồi mới trả về: một cửa vắng mặt trong im lặng nghĩa là client treo ở
/// `connect` mà không ai biết vì sao, và lỗi bind (cổng đã có người) là thứ người
/// dùng cần đọc ngay lúc khởi động agent chứ không phải suy ra sau.
pub fn spawn(
    addr: SocketAddr,
    handler: Handler,
    serial: Arc<Mutex<()>>,
    inflight: Arc<AtomicUsize>,
    last: Arc<Mutex<Instant>>,
) -> Result<()> {
    if !addr.ip().is_loopback() {
        bail!("--listen chỉ nhận địa chỉ loopback, nhận được `{addr}`");
    }
    let l = TcpListener::bind(addr)
        .with_context(|| format!("không bind được `{addr}` — có thể đã có ai đó giữ cổng đó"))?;
    eprintln!("tongue agent: đang nghe {addr}");

    std::thread::spawn(move || {
        for conn in l.incoming() {
            let mut s = match conn {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("tongue agent: bỏ qua một kết nối TCP hỏng: {e}");
                    continue;
                }
            };
            let (handler, serial, last) = (handler.clone(), serial.clone(), last.clone());
            // Đếm TRƯỚC khi spawn, cùng lý do như bên pipe: giữa lúc `accept` trả về
            // và lúc thread kịp chạy, reaper thức dậy đúng khe đó sẽ `exit(0)` trên
            // một kết nối đang sống.
            let busy = InFlight::new(inflight.clone());
            std::thread::spawn(move || {
                let _busy = busy;
                if let Err(e) = handle(&mut s, &serial, &handler) {
                    eprintln!("tongue agent: bỏ qua một yêu cầu TCP hỏng: {e:#}");
                }
                *last.lock().unwrap() = Instant::now();
            });
        }
    });
    Ok(())
}

fn handle(s: &mut TcpStream, serial: &Mutex<()>, handler: &Handler) -> Result<()> {
    s.set_read_timeout(Some(DEADLINE))?;
    s.set_write_timeout(Some(DEADLINE))?;

    let mut line = String::new();
    BufReader::new(s.try_clone()?.take(MAX_LINE))
        .read_line(&mut line)
        .context("đọc yêu cầu thất bại")?;

    let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    if !proto::tcp_allowed(&args) {
        let msg = format!("2 từ chối lệnh ngoài danh sách {:?}\n", proto::TCP_VERBS);
        s.write_all(msg.as_bytes())?;
        return Ok(());
    }

    // Cùng `serial` với đường pipe, không phải một khoá riêng: hai `select_langid`
    // chạy chồng sẽ giằng nhau trên cùng một cửa sổ foreground, và hai cửa vào khác
    // nhau không làm điều đó bớt đúng.
    let (code, stdout, _stderr) = {
        let _g = serial.lock().unwrap_or_else(|e| e.into_inner());
        handler(&args)
    };

    // stdout của agent đi ra NGUYÊN VẸN trên một dòng. Bên tiêu thụ (`ime-route` rồi
    // `tongue.nvim`) loại thẳng output có khoảng trắng bên trong, nên gộp dòng ở đây
    // là để một backend nhiều dòng hỏng NGAY và ồn ào, thay vì đẻ ra một token trông
    // hợp lệ.
    let body = String::from_utf8_lossy(&stdout);
    let body = body.trim();
    writeln!(s, "{code} {body}")?;
    s.flush()?;
    Ok(())
}
