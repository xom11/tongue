mod backend;
mod config;
mod doctor;
mod mode;
mod reconcile;
mod status;

use clap::{Parser, Subcommand};
use mode::{desired, Mode, Platform};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "tongue",
    version,
    about = "Chuyển chế độ gõ vi/en/zh — một lệnh cho cả layout hệ thống lẫn bộ gõ ngoài"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Tiếng Việt (bộ gõ ngoài bật)
    Vi,
    /// Tiếng Anh (bộ gõ ngoài tắt)
    En,
    /// Tiếng Trung — chỉ macOS
    Zh,
    /// Trạng thái chi tiết
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Khám môi trường; --fix sửa những gì an toàn (ghim perAppMode=0...)
    Doctor {
        #[arg(long)]
        fix: bool,
    },
    /// Nghe named pipe để phục vụ những lần gọi từ session 0 (SSH của Windows).
    ///
    /// Phải chạy TRONG session của người dùng — chính nó là cây cầu qua ranh giới
    /// đó. Xem `src/backend/windows/pipe.rs`.
    #[cfg(windows)]
    Agent,
}

/// Ở session 0 (SSH của Windows) thì mọi thứ tongue cần đều nằm sai phía một bức
/// tường: window station và `Local\` đều theo session. Nếu có agent đang nghe trong
/// session của người dùng thì chuyển tiếp cả lượt chạy sang đó và tái hiện y nguyên
/// mã thoát cùng hai luồng ra — người gọi không phân biệt được với chạy tại chỗ.
///
/// **STDOUT phải là stdout của agent, không thêm một byte.** Bên tiêu thụ ràng buộc
/// chặt hơn người ta tưởng: `sanitize` của tongue.nvim loại thẳng output có khoảng
/// trắng bên trong, và `set_async` coi BẤT KỲ stdout nào trên một lần set là thất bại.
/// Mọi thứ client tự nói — định tuyến, phiên bản, chẩn đoán transport — đi ra STDERR.
///
/// `None` = không chuyển tiếp, cứ chạy như thường.
#[cfg(windows)]
fn maybe_forward() -> Option<std::process::ExitCode> {
    use backend::pipe_proto as proto;
    use backend::windows::pipe::{self, Bridge};
    use std::io::Write;
    use std::process::ExitCode;

    if std::env::var_os(pipe::NO_FORWARD_ENV).is_some() {
        return None;
    }
    // Đường CỤC BỘ không bao giờ đi qua pipe. `switch-language.ahk` trên a14 gọi tongue
    // mỗi lần đổi cửa sổ và nó ở session 1; cho nó đi qua agent là biến một công cụ hôm
    // nay không có single point of failure thành một công cụ có.
    if !backend::windows::in_service_session() {
        return None;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `agent` vắng mặt khỏi danh sách (nó LÀ đầu kia), `--version`/`--help` cũng vậy —
    // chúng phải nói về binary vừa gọi, vì đó chính là thứ cần khi gỡ ca lệch phiên bản.
    if !proto::forwardable(&args) {
        return None;
    }
    // Config hỏng không được chặn `--help`, nên nuốt lỗi ở đây là cố ý; run() sẽ đọc
    // lại và báo tử tế.
    let cfg = config::load().unwrap_or_default();
    let task = cfg.windows.agent_task.clone();
    let budget = std::time::Duration::from_millis(cfg.windows.agent_timeout_ms);

    let mut bridge = pipe::forward(&args, budget);
    // Không tự ĐĂNG KÝ task (bán kính ảnh hưởng lớn hơn hẳn "đổi chế độ gõ"), chỉ CHẠY
    // một task đã tồn tại. Client ở session 0 vốn không spawn được vào session 1 — đó là
    // cả tiền đề của việc này — nên Task Scheduler là cửa duy nhất.
    if matches!(bridge, Ok(Bridge::Absent)) && !task.is_empty() {
        // `schtasks` là tiến trình ngắn và `.output()` cấp cho nó pipe riêng; tiến trình
        // task thì do Task Scheduler sinh ra chứ không phải do ta, nên không thừa kế gì
        // của ta. Đây là lý do chỗ này KHÔNG cần `spawn_no_inherit` như vkey.rs.
        let _ = std::process::Command::new("schtasks")
            .args(["/run", "/tn", &task])
            .output();
        // `schtasks /run` trả về NGAY, còn agent thì chưa dựng xong pipe: bước nhảy
        // session tốn 376-401 ms (đo 5/5 trên a14) rồi tongue.exe còn phải khởi động
        // và tạo instance. Thử lại đúng MỘT lần là luôn hụt -- đo được: lần gọi đầu
        // báo Absent trong khi agent lên thật ngay sau đó, nên người dùng thấy lỗi
        // "chưa có agent" ở đúng lần đã khởi động thành công nó.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            bridge = pipe::forward(&args, budget);
            if !matches!(bridge, Ok(Bridge::Absent)) || std::time::Instant::now() >= until {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
    }

    match bridge {
        Ok(Bridge::Reply(r)) => {
            let _ = std::io::stdout().write_all(&r.stdout);
            let _ = std::io::stderr().write_all(&r.stderr);
            Some(ExitCode::from(r.code))
        }
        Ok(Bridge::Absent) => {
            eprintln!("tongue: {}", pipe::service_session_hint(&task));
            Some(ExitCode::from(2))
        }
        Ok(Bridge::Ambiguous(sessions)) => {
            eprintln!(
                "tongue: có nhiều agent cùng lúc (session {sessions:?}) — KHÔNG tự chọn, \
                 vì lái nhầm session nghĩa là đổi bộ gõ của một desktop khác. Dừng bớt đi."
            );
            Some(ExitCode::from(2))
        }
        // "Có agent nhưng không tới được" KHÁC HẲN "chưa có agent": rơi xuống đường cũ
        // ở đây là in ra một lời khuyên sai (đi dựng scheduled task) cho một agent đang
        // treo, và tệ hơn — nó hồi sinh đúng cái bug session 0 mà cả module này sinh ra
        // để chặn (read_state thấy rỗng -> spawn một VKey THỨ HAI trong session 0).
        Err(e) => {
            eprintln!("tongue: cầu tới agent hỏng: {e:#}");
            Some(ExitCode::from(2))
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    if let Some(code) = maybe_forward() {
        return code;
    }
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tongue: {e:#}");
            if e.downcast_ref::<reconcile::VerifyFailed>().is_some() {
                eprintln!("tongue: chạy `tongue doctor` để khám nguyên nhân");
                std::process::ExitCode::from(1)
            } else {
                std::process::ExitCode::from(2)
            }
        }
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = config::load()?;
    let platform = Platform::current();
    match cli.cmd {
        Some(Cmd::Vi) => switch(Mode::Vi, platform, &cfg),
        Some(Cmd::En) => switch(Mode::En, platform, &cfg),
        Some(Cmd::Zh) => switch(Mode::Zh, platform, &cfg),
        Some(Cmd::Status { json }) => {
            let s = snapshot(&cfg)?;
            print!(
                "{}",
                if json {
                    status::render_json(&s)
                } else {
                    status::render_human(&s)
                }
            );
            Ok(())
        }
        #[cfg(windows)]
        Some(Cmd::Agent) => backend::windows::pipe::serve(std::time::Duration::from_millis(
            cfg.windows.agent_idle_ms,
        )),
        Some(Cmd::Doctor { fix }) => {
            if doctor::run(fix, &cfg, make_ime(&cfg)?.as_ref())? {
                std::process::exit(2);
            }
            Ok(())
        }
        None => {
            let s = snapshot(&cfg)?;
            println!("{}", s.mode);
            Ok(())
        }
    }
}

fn switch(mode: Mode, platform: Platform, cfg: &config::Config) -> anyhow::Result<()> {
    let Some(want) = desired(mode, platform, &sources(cfg), has_external_ime(cfg)) else {
        anyhow::bail!("mode {} không có trên nền tảng này", mode.as_str());
    };
    let layout = make_layout();
    let ime = make_ime(cfg)?;
    reconcile::reconcile(
        layout.as_ref(),
        ime.as_ref(),
        &want,
        Duration::from_millis(cfg.verify.timeout_ms),
        Duration::from_millis(cfg.verify.poll_ms),
    )
}

// --- một cửa duy nhất dựng backend ---------------------------------------
// switch, snapshot và doctor đều đi qua đây. Thêm bộ gõ mới = thêm một nhánh
// match, không phải lùng ba chỗ khác nhau.

#[cfg(target_os = "macos")]
fn make_layout() -> Box<dyn backend::Layout> {
    Box::new(backend::macos::tis::TisLayout)
}

#[cfg(windows)]
fn make_layout() -> Box<dyn backend::Layout> {
    Box::new(backend::windows::layout::WinLayout)
}

/// Bảng source của nền tảng đang chạy. Cùng khuôn với make_layout/has_external_ime:
/// cfg-gate đúng một chỗ thay vì rải `#[cfg]` vào giữa logic.
#[cfg(target_os = "macos")]
fn sources(cfg: &config::Config) -> mode::Sources {
    cfg.macos.sources()
}

#[cfg(windows)]
fn sources(cfg: &config::Config) -> mode::Sources {
    cfg.windows.sources()
}

/// Có app ngoài lo tiếng Việt không? false = macOS tự lo qua input source.
#[cfg(target_os = "macos")]
fn has_external_ime(cfg: &config::Config) -> bool {
    cfg.macos.backend != "system"
}

#[cfg(windows)]
fn has_external_ime(_cfg: &config::Config) -> bool {
    true // Windows luôn qua VKey
}

#[cfg(target_os = "macos")]
fn make_ime(cfg: &config::Config) -> anyhow::Result<Box<dyn backend::Ime>> {
    use backend::macos::{app::AppIme, gonhanh::GonhanhIme, hotkey::HotkeyIme, system::SystemIme};
    let name = cfg.macos.app_name.clone();
    Ok(
        match (cfg.macos.backend.as_str(), cfg.macos.strategy.as_str()) {
            ("gonhanh", "process") => Box::new(GonhanhIme { app_name: name }),
            ("gonhanh", "hotkey") => Box::new(HotkeyIme::new(
                name,
                cfg.verify.timeout_ms,
                cfg.verify.poll_ms,
            )),
            ("app", "process") => Box::new(AppIme { app_name: name }),
            ("system", "process") => Box::new(SystemIme { app_name: name }),
            (b @ ("app" | "system"), "hotkey") => anyhow::bail!(
                "strategy 'hotkey' chỉ dùng được với backend 'gonhanh' — backend '{b}' không có chord toggle để giả lập"
            ),
            ("gonhanh" | "app" | "system", s) => {
                anyhow::bail!("strategy '{s}' không hợp lệ (process|hotkey)")
            }
            (b, _) => anyhow::bail!("backend '{b}' không hợp lệ (gonhanh|app|system)"),
        },
    )
}

#[cfg(windows)]
fn make_ime(cfg: &config::Config) -> anyhow::Result<Box<dyn backend::Ime>> {
    Ok(Box::new(backend::windows::vkey::VkeyIme {
        exe_path_override: cfg.windows.vkey_path.clone(),
    }))
}

#[cfg(target_os = "macos")]
fn snapshot(cfg: &config::Config) -> anyhow::Result<status::Snapshot> {
    let layout = backend::macos::tis::current_source_id()?;
    let ime_on = make_ime(cfg)?.is_on()?;
    let (mode, drift) = status::infer_mac(ime_on, &layout, &cfg.macos.sources());
    Ok(status::Snapshot {
        mode,
        layout: Some(layout),
        ime_on,
        drift,
    })
}

/// `status` là CHẨN ĐOÁN, không phải một đích cần verify — nên thiếu cửa sổ foreground
/// KHÔNG được làm nó chết.
///
/// Trên Windows input locale là thuộc tính CỦA THREAD, nên "không có foreground" nghĩa
/// là không có thread nào để hỏi; đó là sự thật về cái máy, không phải lỗi của tongue.
/// Mà ca đó xảy ra đúng lúc người ta hay ssh vào nhất (máy khoá, hoặc không app nào
/// focus), nên bail ở đây là làm hỏng lệnh ngay tại ca dùng chính.
///
/// Session 0 vẫn báo lỗi như cũ: `make_ime(..).is_on()` chạy TRƯỚC và `read_state()` đã
/// bail ở đó, nên nhánh degrade này chỉ ăn đúng ca "thiếu foreground".
#[cfg(windows)]
fn snapshot(cfg: &config::Config) -> anyhow::Result<status::Snapshot> {
    let ime_on = make_ime(cfg)?.is_on()?;
    match backend::windows::layout::current_langid() {
        Ok(layout) => {
            let (mode, drift) = status::infer_win(ime_on, &layout, &cfg.windows.sources());
            Ok(status::Snapshot {
                mode,
                layout: Some(layout),
                ime_on,
                drift,
            })
        }
        Err(e) => Ok(status::Snapshot {
            // Bit VKey một mình phân biệt được vi với en (source_vi trùng source_en),
            // nhưng KHÔNG loại trừ được zh — nên sự thiếu chắc chắn đó phải nằm trong
            // `drift`, không được im.
            mode: if ime_on { "vi" } else { "en" }.into(),
            layout: None,
            ime_on,
            drift: Some(format!(
                "không đọc được layout ({e:#}) — mode suy từ bit VKey, chưa loại trừ được zh"
            )),
        }),
    }
}

// Chỉ chạy trên macOS: make_ime bản Windows bỏ qua cfg.macos hoàn toàn nên
// những ca lỗi dưới đây không có ý nghĩa gì bên đó (luôn Ok bất kể backend/
// strategy). make_ime chỉ DỰNG struct (không mở app, không đọc defaults,
// không chạm FFI) nên gọi thẳng trong test là an toàn — bail! xảy ra trước
// khi có bất kỳ side effect nào.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn cfg_voi(backend: &str, strategy: &str) -> config::Config {
        let mut c = config::Config::default();
        c.macos.backend = backend.into();
        c.macos.strategy = strategy.into();
        c
    }

    #[test]
    fn hotkey_voi_backend_system_thi_loi() {
        let cfg = cfg_voi("system", "hotkey");
        // Box<dyn Ime> không impl Debug nên unwrap_err() thẳng không được —
        // map Ok về () (có Debug) trước khi unwrap_err.
        let err = make_ime(&cfg).map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("chord toggle"));
    }

    #[test]
    fn hotkey_voi_backend_app_thi_loi() {
        let cfg = cfg_voi("app", "hotkey");
        let err = make_ime(&cfg).map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("chord toggle"));
    }

    #[test]
    fn gonhanh_hotkey_thi_ok() {
        let cfg = cfg_voi("gonhanh", "hotkey");
        assert!(make_ime(&cfg).is_ok());
    }

    #[test]
    fn gonhanh_process_mac_dinh_thi_ok() {
        // Mặc định của config: backend = gonhanh, strategy = process.
        let cfg = config::Config::default();
        assert!(make_ime(&cfg).is_ok());
    }
}
