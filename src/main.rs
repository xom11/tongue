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
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tongue: {e:#}");
            if e.downcast_ref::<reconcile::VerifyFailed>().is_some() {
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
            print!("{}", if json { status::render_json(&s) } else { status::render_human(&s) });
            Ok(())
        }
        Some(Cmd::Doctor { fix }) => {
            if doctor::run(fix, &cfg)? {
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
    let Some(want) = desired(mode, platform, &cfg.macos.source_vi, &cfg.macos.source_zh) else {
        anyhow::bail!("mode {} không có trên nền tảng này", mode.as_str());
    };
    let (layout, ime) = build_backends(cfg)?;
    reconcile::reconcile(
        layout.as_ref(),
        ime.as_ref(),
        &want,
        Duration::from_millis(cfg.verify.timeout_ms),
        Duration::from_millis(cfg.verify.poll_ms),
    )
}

#[cfg(target_os = "macos")]
fn build_backends(
    cfg: &config::Config,
) -> anyhow::Result<(Box<dyn backend::Layout>, Box<dyn backend::Ime>)> {
    anyhow::ensure!(
        cfg.macos.strategy == "process",
        "strategy '{}' chưa hỗ trợ (v1 chỉ có 'process')",
        cfg.macos.strategy
    );
    Ok((
        Box::new(backend::macos::tis::TisLayout),
        Box::new(backend::macos::gonhanh::GonhanhIme { app_name: cfg.macos.app_name.clone() }),
    ))
}

#[cfg(target_os = "macos")]
fn snapshot(cfg: &config::Config) -> anyhow::Result<status::Snapshot> {
    use backend::Ime as _;
    let layout = backend::macos::tis::current_source_id()?;
    let gonhanh = backend::macos::gonhanh::GonhanhIme { app_name: cfg.macos.app_name.clone() };
    let ime_on = gonhanh.is_on()?;
    let (mode, drift) = status::infer_mac(ime_on, &layout, &cfg.macos.source_vi, &cfg.macos.source_zh);
    Ok(status::Snapshot { mode, layout: Some(layout), ime_on, drift })
}

#[cfg(windows)]
fn build_backends(
    cfg: &config::Config,
) -> anyhow::Result<(Box<dyn backend::Layout>, Box<dyn backend::Ime>)> {
    Ok((
        Box::new(backend::NoopLayout),
        Box::new(backend::windows::vkey::VkeyIme { exe_path_override: cfg.windows.vkey_path.clone() }),
    ))
}

#[cfg(windows)]
fn snapshot(cfg: &config::Config) -> anyhow::Result<status::Snapshot> {
    use backend::Ime as _;
    let vkey = backend::windows::vkey::VkeyIme { exe_path_override: cfg.windows.vkey_path.clone() };
    let ime_on = vkey.is_on()?;
    Ok(status::Snapshot { mode: status::infer_win(ime_on), layout: None, ime_on, drift: None })
}
