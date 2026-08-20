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
/// `None` = không chuyển tiếp, cứ chạy như thường (và rồi báo lỗi session 0 như cũ,
/// vì đó vẫn là sự thật khi không có agent).
#[cfg(windows)]
fn maybe_forward() -> Option<std::process::ExitCode> {
    use std::io::Write;
    if std::env::var_os(backend::windows::pipe::NO_FORWARD_ENV).is_some() {
        return None;
    }
    if !backend::windows::in_service_session() {
        return None;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `agent` không bao giờ được chuyển tiếp: nó LÀ đầu kia.
    if args.first().map(String::as_str) == Some("agent") {
        return None;
    }
    // Phân biệt "không có agent" với "có agent mà đường truyền hỏng". Nuốt vế thứ
    // hai thành vế thứ nhất là đúng kiểu lỗi câm: người dùng nhận lời khuyên đi dựng
    // agent, trong khi agent đang chạy và thứ hỏng nằm chỗ khác.
    let reply = match backend::windows::pipe::forward(&args) {
        Ok(Some(r)) => r,
        Ok(None) => return None,
        Err(e) => {
            eprintln!("tongue: agent có mặt nhưng trao đổi thất bại: {e:#}");
            return Some(std::process::ExitCode::from(2));
        }
    };
    let _ = std::io::stdout().write_all(&reply.stdout);
    let _ = std::io::stderr().write_all(&reply.stderr);
    Some(std::process::ExitCode::from(reply.code))
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
        Some(Cmd::Agent) => backend::windows::pipe::serve(),
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

#[cfg(windows)]
fn snapshot(cfg: &config::Config) -> anyhow::Result<status::Snapshot> {
    let layout = backend::windows::layout::current_langid()?;
    let ime_on = make_ime(cfg)?.is_on()?;
    let (mode, drift) = status::infer_win(ime_on, &layout, &cfg.windows.sources());
    Ok(status::Snapshot {
        mode,
        layout: Some(layout),
        ime_on,
        drift,
    })
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
