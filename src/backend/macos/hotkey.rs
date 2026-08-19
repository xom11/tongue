//! Strategy `hotkey`: giữ GoNhanh sống, đổi vi/en bằng chính chord toggle mà app
//! đã đăng ký. Bỏ hẳn cold-start vì `en` không còn giết app.
//!
//! BẤT BIẾN CỦA FILE NÀY — chord là RELAY, không idempotent:
//! `reconcile` gọi lại `set()` mỗi vòng poll 50ms còn lệch, nhưng GoNhanh mất
//! 87–286ms (đo 4 lần, 30/07/2026) mới ghi `gonhanh.enabled`. Trả về sớm là bắn
//! trùng 2–6 chord và lật mode qua lại. Nên `set()` phải TỰ CHỜ XÁC NHẬN, và
//! bắn TỐI ĐA MỘT chord cho mỗi lần chạy tongue.
//!
//! VẾ THỨ HAI, và nó KHÔNG suy ra được từ vế trên: chốt "một chord mỗi lần
//! chạy" là một `Cell` nên chỉ có tác dụng trong MỘT tiến trình. Cửa sổ
//! 87–286ms đó cũng chính là cửa sổ để một tiến trình tongue THỨ HAI đọc phải
//! trạng thái cũ. Đo trên máy thật 19/08/2026: rời terminal sang trình duyệt
//! làm Hammerspoon (khôi phục chế độ theo app) và tongue.nvim (`restore_on_
//! unfocus`) cùng phát `tongue vi`, cách nhau 495ms; khi hai lời gọi rơi vào
//! trong cửa sổ trên thì cả hai đọc "đang tắt", mỗi bên bắn một chord, GoNhanh
//! bật rồi tắt, và CẢ HAI thoát 1 với `VerifyFailed`. Nên `set()` còn phải chạy
//! trong một khoá LIÊN TIẾN TRÌNH (`Gate`), và lần đọc trạng thái quyết định có
//! bắn hay không phải nằm TRONG khoá đó.
//!
//! Logic nằm sau bốn trait để test được bằng fake; FFI CoreGraphics ở cuối file.

use anyhow::{bail, Context, Result};
use std::cell::Cell;
use std::time::{Duration, Instant};

pub trait ChordSender {
    fn send(&self) -> Result<()>;
}

pub trait Launcher {
    /// Ghi enabled=1 rồi mở app — app đọc defaults lúc khởi động nên lên là đã bật.
    fn launch(&self) -> Result<()>;
}

pub trait StateSource {
    fn running(&self) -> Result<bool>;
    /// None = key chưa từng được ghi.
    fn enabled(&self) -> Result<Option<bool>>;
}

/// Vật giữ khoá: việc duy nhất của nó là sống. Nhả khi bị drop.
pub trait GateGuard {}

/// Khoá liên TIẾN TRÌNH bọc quanh cụm đọc trạng thái → bắn chord → chờ xác nhận.
///
/// Cờ `fired` chốt "một chord mỗi lần chạy" trong phạm vi MỘT tiến trình; cái
/// này chốt phần còn lại. Xem bất biến ở đầu file để biết vì sao chốt trong
/// một tiến trình là chưa đủ.
pub trait Gate {
    /// Chặn tới khi giành được quyền độc chiếm. `Ok(None)` = hết hạn chờ mà
    /// tiến trình tongue khác vẫn đang giữ.
    fn enter(&self) -> Result<Option<Box<dyn GateGuard>>>;
}

pub struct HotkeyCore<'a> {
    sender: &'a dyn ChordSender,
    launcher: &'a dyn Launcher,
    state: &'a dyn StateSource,
    gate: &'a dyn Gate,
    timeout: Duration,
    poll: Duration,
    /// MƯỢN từ bên ngoài, không sở hữu: `reconcile` gọi `Ime::set()` nhiều lượt
    /// và mỗi lượt dựng một `HotkeyCore` mới, nên cờ "đã bắn" phải sống ở tầng
    /// `HotkeyIme` (tồn tại suốt lần chạy tongue). Để `Cell` ở đây là nó reset
    /// mỗi lượt và chốt "tối đa một chord" thành vô nghĩa.
    fired: &'a Cell<bool>,
}

impl<'a> HotkeyCore<'a> {
    pub fn new(
        sender: &'a dyn ChordSender,
        launcher: &'a dyn Launcher,
        state: &'a dyn StateSource,
        gate: &'a dyn Gate,
        timeout: Duration,
        poll: Duration,
        fired: &'a Cell<bool>,
    ) -> Self {
        Self {
            sender,
            launcher,
            state,
            gate,
            timeout,
            poll,
            fired,
        }
    }

    /// App đang chạy VÀ enabled — không phải một trong hai. App chết thì
    /// `enabled` còn sót lại trong defaults cũng chẳng gõ được gì.
    pub fn is_on(&self) -> Result<bool> {
        Ok(self.state.running()? && self.state.enabled()?.unwrap_or(false))
    }

    pub fn set(&self, on: bool) -> Result<()> {
        // MỌI thứ dưới đây — đọc trạng thái, bắn chord, chờ xác nhận — phải nằm
        // TRỌN trong khoá. Đọc trước khi vào khoá là đọc phải trạng thái mà một
        // tiến trình tongue khác đang dở tay đổi: cả hai thấy "đang tắt", cả
        // hai bắn chord, GoNhanh bật rồi tắt.
        let Some(_khoa) = self.gate.enter()? else {
            // Hết hạn chờ mà bên kia vẫn giữ. Bắn chord lúc này là tái lập đúng
            // cái lỗi vừa chặn, nên không bắn — trả về để reconcile đọc lại
            // trạng thái thật rồi tự quyết định VerifyFailed.
            return Ok(());
        };
        if !self.state.running()? {
            if !on {
                return Ok(()); // app chết = đã là `en`
            }
            // App đọc defaults lúc khởi động, nên lên là đã bật sẵn — bắn thêm
            // chord ở đây là tắt mất cái vừa bật.
            self.launcher.launch()?;
            return self.cho_toi(on);
        }
        if self.is_on()? == on {
            return Ok(());
        }
        // Tối đa MỘT chord mỗi lần chạy tongue. reconcile sẽ gọi lại set() sau
        // khi ta trả về; nếu cú đầu thật ra có ăn nhưng chậm bất thường thì cú
        // thứ hai lật ngược mode. Một lần chạy = một lần chuyển mode.
        if self.fired.get() {
            return Ok(());
        }
        self.sender.send()?;
        self.fired.set(true);
        self.cho_toi(on)
    }

    /// Chờ trạng thái thật khớp `want`, tối đa hết ngân sách. Hết giờ vẫn trả
    /// Ok: `reconcile` là chỗ DUY NHẤT quyết định VerifyFailed, không phải đây.
    fn cho_toi(&self, want: bool) -> Result<()> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if self.is_on()? == want {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(self.poll);
        }
    }
}

// --- FFI: bắn chord ở tầng HID ------------------------------------------
// Tap của GoNhanh nằm ở HID level nên thấy được sự kiện từ CGEventPost;
// osascript thì KHÔNG (tap bỏ qua) — xem spec gốc, RustBridge.swift:855-859.

use super::{app, chord, prefs};
use crate::backend::Ime;
use crate::doctor::{Finding, Level};
use core_foundation_sys::base::{Boolean, CFRelease};
use std::ffi::c_void;

const DEFAULTS_DOMAIN: &str = "org.gonhanh.GoNhanh";
const KEY_ENABLED: &str = "gonhanh.enabled";
const KEY_SHORTCUT: &str = "gonhanh.shortcut.toggle";

#[repr(C)]
struct __CGEvent(c_void);
type CGEventRef = *mut __CGEvent;
#[repr(C)]
struct __CGEventSource(c_void);
type CGEventSourceRef = *mut __CGEventSource;

const K_CG_EVENT_SOURCE_STATE_HID: i32 = 1; // kCGEventSourceStateHIDSystemState
const K_CG_HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
}

// Không giữ `chord` đã parse sẵn: HotkeyCore::set() chỉ gọi sender.send() ở
// ĐÚNG MỘT trong bốn nhánh của nó (app đang chạy, is_on() != on, chưa fired).
// Ba nhánh còn lại (đã khớp sẵn, app chết + muốn tắt, app chết + muốn bật rồi
// launch) không bao giờ cần chord. Đọc chord ở đây — bên trong send(), không
// phải lúc dựng sender — để ba nhánh kia không bị chặn bởi lỗi đọc chord (ca
// điển hình: GoNhanh chưa từng chạy lần đầu nên defaults chưa có
// gonhanh.shortcut.toggle, nhưng `tongue en` khi app đã tắt sẵn phải là no-op,
// không phải lỗi môi trường).
struct CgChordSender;

impl ChordSender for CgChordSender {
    fn send(&self) -> Result<()> {
        let chord = doc_chord()?;
        unsafe {
            let src = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID);
            let down = CGEventCreateKeyboardEvent(src, chord.key_code, true);
            let up = CGEventCreateKeyboardEvent(src, chord.key_code, false);
            if down.is_null() || up.is_null() {
                // Release đúng những gì THẬT SỰ được tạo ra — down và up có thể
                // thành bại độc lập nhau (cùng nguồn src nhưng lời gọi riêng).
                if !down.is_null() {
                    CFRelease(down as _);
                }
                if !up.is_null() {
                    CFRelease(up as _);
                }
                if !src.is_null() {
                    CFRelease(src as _);
                }
                bail!("CGEventCreateKeyboardEvent trả về null");
            }
            CGEventSetFlags(down, chord.flags);
            CGEventSetFlags(up, chord.flags);
            CGEventPost(K_CG_HID_EVENT_TAP, down);
            // 30ms giữa down và up — đúng khoảng đã nghiệm chứng là GoNhanh ăn.
            std::thread::sleep(Duration::from_millis(30));
            CGEventPost(K_CG_HID_EVENT_TAP, up);
            CFRelease(down as _);
            CFRelease(up as _);
            if !src.is_null() {
                CFRelease(src as _);
            }
        }
        Ok(())
    }
}

struct GonhanhLauncher {
    app_name: String,
}

impl Launcher for GonhanhLauncher {
    fn launch(&self) -> Result<()> {
        // Ghi enabled=1 TRƯỚC khi open — app đọc defaults lúc khởi động.
        let st = std::process::Command::new("defaults")
            .args(["write", DEFAULTS_DOMAIN, KEY_ENABLED, "-bool", "YES"])
            .status()?;
        anyhow::ensure!(st.success(), "defaults write {KEY_ENABLED} thất bại");
        app::launch(&self.app_name)
    }
}

struct GonhanhState {
    app_name: String,
}

impl StateSource for GonhanhState {
    fn running(&self) -> Result<bool> {
        app::is_running(&self.app_name)
    }
    fn enabled(&self) -> Result<Option<bool>> {
        Ok(prefs::read_bool(DEFAULTS_DOMAIN, KEY_ENABLED))
    }
}

/// Khoá liên tiến trình dựa trên một file trống dùng chung.
///
/// File chỉ mang vai khoá, KHÔNG giữ trạng thái — nguồn chân lý vẫn là hệ
/// thống, đúng như bất biến của repo. Khoá là advisory và gắn với file
/// description, nên kernel tự nhả khi tiến trình chết: một tongue bị kill giữa
/// chừng không để lại khoá chết.
pub struct FileGate {
    path: std::path::PathBuf,
    cho: Duration,
}

/// Nơi đặt khoá — cùng một đường dẫn cho mọi tiến trình tongue của CÙNG một
/// người dùng. `~/Library/Caches` chứ không phải `~/.config`: đây là thứ dựng
/// lại được bất cứ lúc nào, không phải cấu hình.
fn duong_khoa() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("tongue/switch.lock")
}

impl FileGate {
    pub fn new(path: &std::path::Path, cho: Duration) -> Result<Self> {
        if let Some(cha) = path.parent() {
            std::fs::create_dir_all(cha)
                .with_context(|| format!("không tạo được {}", cha.display()))?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            cho,
        })
    }
}

/// Giữ khoá bằng cách giữ file mở. Sở hữu `File` chứ không mượn: nhờ vậy nó
/// không dính lifetime của gate và cất đi đâu cũng được.
struct FileGuard {
    file: std::fs::File,
}

impl GateGuard for FileGuard {}

impl Drop for FileGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Nhịp hỏi lại khi khoá đang bận. Nhỏ hơn hẳn 87ms — cận dưới của thời gian
/// GoNhanh ghi `gonhanh.enabled` — để lần chạy đang chờ vào được gần như ngay
/// khi bên kia nhả, thay vì ngủ qua mất cả cửa sổ đó.
const NHIP_CHO_KHOA: Duration = Duration::from_millis(5);

impl Gate for FileGate {
    fn enter(&self) -> Result<Option<Box<dyn GateGuard>>> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)
            .with_context(|| format!("không mở được khoá {}", self.path.display()))?;
        // MỖI lời gọi mở file RIÊNG: khoá gắn với file description chứ không
        // với đường dẫn, nên hai lần mở cùng một file vẫn tranh nhau thật — kể
        // cả khi chúng nằm trong cùng một tiến trình.
        let han = Instant::now() + self.cho;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(Box::new(FileGuard { file }))),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= han {
                        return Ok(None);
                    }
                    std::thread::sleep(NHIP_CHO_KHOA);
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(e)
                        .with_context(|| format!("không khoá được {}", self.path.display()));
                }
            }
        }
    }
}

fn doc_chord() -> Result<chord::Chord> {
    let blob = prefs::read_data(DEFAULTS_DOMAIN, KEY_SHORTCUT).with_context(|| {
        format!("không đọc được {KEY_SHORTCUT} — GoNhanh đã chạy lần đầu chưa?")
    })?;
    chord::parse(&blob)
}

pub struct HotkeyIme {
    pub app_name: String,
    pub timeout_ms: u64,
    pub poll_ms: u64,
    /// Sống suốt lần chạy tongue, KHÔNG nằm trong HotkeyCore: reconcile gọi
    /// `set()` nhiều lượt và mỗi lượt dựng core mới, nên cờ phải ở đây thì chốt
    /// "tối đa một chord" mới có tác dụng.
    fired: Cell<bool>,
}

impl HotkeyIme {
    pub fn new(app_name: String, timeout_ms: u64, poll_ms: u64) -> Self {
        Self {
            app_name,
            timeout_ms,
            poll_ms,
            fired: Cell::new(false),
        }
    }

    fn launcher(&self) -> GonhanhLauncher {
        GonhanhLauncher {
            app_name: self.app_name.clone(),
        }
    }

    fn state(&self) -> GonhanhState {
        GonhanhState {
            app_name: self.app_name.clone(),
        }
    }
}

impl Ime for HotkeyIme {
    /// Không đi qua HotkeyCore: đây là truy vấn đọc thuần, chỉ cần state chứ
    /// không cần dựng launcher/sender đầy đủ. Vẫn phải trả lời được cả khi
    /// chord chưa đọc được (GoNhanh chưa chạy lần đầu) — is_on() không đụng
    /// tới chord nên không phụ thuộc gì vào việc parse nó có thành hay không.
    fn is_on(&self) -> Result<bool> {
        let state = self.state();
        Ok(state.running()? && state.enabled()?.unwrap_or(false))
    }

    fn set(&self, on: bool) -> Result<()> {
        let launcher = self.launcher();
        let state = self.state();
        // CgChordSender KHÔNG cầm chord đã parse sẵn — nó tự đọc bên trong
        // send(), vì HotkeyCore::set() chỉ gọi tới sender ở đúng một nhánh (xem
        // comment tại impl ChordSender). Ba nhánh còn lại (đã khớp sẵn, app
        // chết + muốn tắt, app chết + muốn bật) không được phép bị chặn chỉ vì
        // GoNhanh chưa từng chạy lần đầu nên chưa có chord trong defaults.
        let sender = CgChordSender;
        // Ngân sách chờ khoá = ngân sách verify. Bên đang giữ khoá cũng bị chính
        // `cho_toi` chặn trên ở đúng con số đó, nên chờ lâu hơn là chờ một thứ
        // đã bỏ cuộc.
        let gate = FileGate::new(&duong_khoa(), Duration::from_millis(self.timeout_ms))?;
        let core = HotkeyCore::new(
            &sender,
            &launcher,
            &state,
            &gate,
            Duration::from_millis(self.timeout_ms),
            Duration::from_millis(self.poll_ms),
            &self.fired,
        );
        core.set(on)
    }

    fn diagnose(&self, fix: bool) -> Result<Vec<Finding>> {
        let mut fs = vec![app::diagnose_bundle(&self.app_name)];

        // hotkey đòi app sống LIÊN TỤC — khác `process`, nơi "chết" chính là
        // trạng thái `en` bình thường. Ở đây app chết là bất thường: lần
        // `tongue vi`/`en` tới sẽ phải cold-start (launch) trước khi bắn chord
        // được. Vẫn tự phục hồi được (xem HotkeyCore::set nhánh launch) nên
        // Warn chứ không Fail.
        let running = app::is_running(&self.app_name)?;
        fs.push(if running {
            Finding {
                level: Level::Ok,
                msg: format!("{}: đang chạy", self.app_name),
            }
        } else {
            Finding {
                level: Level::Warn,
                msg: format!(
                    "{} không chạy — strategy hotkey cần app sống liên tục; lần \
`tongue vi`/`en` tới sẽ phải launch (cold-start) trước khi bắn chord được",
                    self.app_name
                ),
            }
        });

        // Ca hiếm nhưng nguy hiểm: app đang chạy mà `gonhanh.enabled` đọc ra
        // None (chưa từng ghi, hoặc bị xoá) — is_on() dùng unwrap_or(false) nên
        // coi như đang tắt. `tongue vi` khi đó sẽ bắn chord MÙ, mà chord là
        // TOGGLE chứ không set thẳng: nếu người dùng đang thật sự ở `vi` thì cú
        // bắn đó TẮT MẤT tiếng Việt họ đang gõ, rồi không xác nhận lại được
        // (enabled vẫn None) nên reconcile hết ngân sách → VerifyFailed.
        if running && prefs::read_bool(DEFAULTS_DOMAIN, KEY_ENABLED).is_none() {
            fs.push(Finding {
                level: Level::Warn,
                msg: format!(
                    "{} đang chạy nhưng chưa đọc được {KEY_ENABLED} — is_on() sẽ coi như \
tắt, `tongue vi` có thể bắn chord mù và TẮT MẤT tiếng Việt đang gõ (chord là toggle, \
không phải set thẳng)",
                    self.app_name
                ),
            });
        }

        // `hotkey` KHÔNG được phép giết app — đó là cả điểm của strategy này.
        // Nên --fix ở đây ghim perAppMode mà không restart; giá trị mới có hiệu
        // lực ở lần khởi động sau của GoNhanh. Trả `false` (chưa restart) để
        // diagnose_per_app_mode biết mà báo Warn thay vì Ok — việc chưa xong.
        fs.push(super::gonhanh::diagnose_per_app_mode(
            fix,
            &self.app_name,
            &|| Ok(false),
        )?);

        // Kiểu hỏng khó đoán nhất: quyền cấp cho TIẾN TRÌNH CHỦ, không cho binary
        // tongue. Thông điệp phải nói thẳng điều đó.
        fs.push(if unsafe { AXIsProcessTrusted() } != 0 {
            Finding {
                level: Level::Ok,
                msg: "Accessibility: tiến trình này được tin cậy".into(),
            }
        } else {
            Finding {
                level: Level::Fail,
                msg: "thiếu quyền Accessibility — chord sẽ không tới được GoNhanh. \
Quyền cấp cho TIẾN TRÌNH CHỦ chứ không cho binary tongue: nếu gõ tay trong terminal \
thì cấp cho chính app terminal đó, nếu gọi từ Hammerspoon thì cấp cho Hammerspoon \
(System Settings > Privacy & Security > Accessibility)"
                    .into(),
            }
        });

        fs.push(match doc_chord() {
            Ok(c) => Finding {
                level: Level::Ok,
                msg: format!("chord toggle: {}", chord::describe(&c)),
            },
            Err(e) => Finding {
                level: Level::Fail,
                msg: format!("{e:#}"),
            },
        });

        Ok(fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Barrier, Mutex};

    /// Không khoá gì cả — cho các test một-lần-chạy, nơi tuần tự hoá không phải
    /// thứ đang được kiểm.
    struct KhongKhoa;
    struct GuardRong;
    impl GateGuard for GuardRong {}
    impl Gate for KhongKhoa {
        fn enter(&self) -> Result<Option<Box<dyn GateGuard>>> {
            Ok(Some(Box::new(GuardRong)))
        }
    }

    /// enabled chỉ đổi sau `do_tre` lần đọc — mô phỏng 87–286ms trễ ghi defaults
    /// mà reconcile poll 50ms sẽ đâm phải.
    struct FakeState {
        running: Cell<bool>,
        enabled: Cell<bool>,
        cho_doi: Cell<u32>,
        moi_doi: Cell<Option<bool>>,
        so_lan_doc: Cell<u32>,
    }
    impl FakeState {
        fn new(running: bool, enabled: bool) -> Self {
            Self {
                running: Cell::new(running),
                enabled: Cell::new(enabled),
                cho_doi: Cell::new(0),
                moi_doi: Cell::new(None),
                so_lan_doc: Cell::new(0),
            }
        }
        /// Hẹn: sau `n` lần đọc nữa thì enabled thành `gia_tri`.
        fn hen_doi_sau(&self, n: u32, gia_tri: bool) {
            self.cho_doi.set(n);
            self.moi_doi.set(Some(gia_tri));
        }
    }
    impl StateSource for FakeState {
        fn running(&self) -> Result<bool> {
            Ok(self.running.get())
        }
        fn enabled(&self) -> Result<Option<bool>> {
            self.so_lan_doc.set(self.so_lan_doc.get() + 1);
            let con = self.cho_doi.get();
            if con > 0 {
                self.cho_doi.set(con - 1);
                if con == 1 {
                    if let Some(v) = self.moi_doi.get() {
                        self.enabled.set(v);
                    }
                }
            }
            Ok(Some(self.enabled.get()))
        }
    }

    struct FakeSender<'a> {
        so_lan: Cell<u32>,
        state: &'a FakeState,
        /// số lần đọc trước khi enabled đổi, sau mỗi cú bắn
        tre: u32,
    }
    impl<'a> ChordSender for FakeSender<'a> {
        fn send(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            let moi = !self.state.enabled.get();
            self.state.hen_doi_sau(self.tre, moi);
            Ok(())
        }
    }

    /// Chord bắn ra nhưng KHÔNG có tác dụng — Accessibility bị thu hồi, app treo.
    struct SenderTruot {
        so_lan: Cell<u32>,
    }
    impl ChordSender for SenderTruot {
        fn send(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            Ok(())
        }
    }

    struct FakeLauncher<'a> {
        so_lan: Cell<u32>,
        state: &'a FakeState,
    }
    impl<'a> Launcher for FakeLauncher<'a> {
        fn launch(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            self.state.running.set(true);
            self.state.enabled.set(true);
            Ok(())
        }
    }

    struct LauncherKhongDuocGoi;
    impl Launcher for LauncherKhongDuocGoi {
        fn launch(&self) -> Result<()> {
            panic!("không được launch ở nhánh này");
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// HỒI QUY CHO VẤN ĐỀ CỐT LÕI: enabled chỉ đổi sau 6 lần đọc (mô phỏng 286ms
    /// / poll 50ms), nhưng chỉ được bắn ĐÚNG MỘT chord.
    #[test]
    fn bat_vi_chi_ban_mot_chord_du_readback_tre() {
        let st = FakeState::new(true, false);
        let sender = FakeSender {
            so_lan: Cell::new(0),
            state: &st,
            tre: 6,
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(1000), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 1, "phải bắn đúng 1 chord");
        assert!(core.is_on().unwrap(), "phải kết thúc ở trạng thái bật");
    }

    /// reconcile gọi set() thêm lượt nữa sau khi chord trượt — KHÔNG được bắn cú
    /// thứ hai, vì nếu cú đầu thật ra ăn chậm thì cú hai lật ngược mode.
    #[test]
    fn chord_truot_thi_khong_ban_cu_thu_hai() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(20), ms(1), &fired);
        core.set(true).unwrap();
        core.set(true).unwrap();
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 1, "tối đa một chord mỗi lần chạy");
    }

    /// Giống test trên nhưng dựng HotkeyCore MỚI mỗi lượt — đúng cách
    /// `HotkeyIme::set` chạy thật khi reconcile gọi nhiều lượt. Khoá việc cờ
    /// `fired` phải sống NGOÀI core; để nó bên trong là mỗi lượt reset về false
    /// và bắn thêm chord.
    #[test]
    fn core_moi_moi_luot_van_chi_ban_mot_chord() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        for _ in 0..3 {
            let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(10), ms(1), &fired);
            core.set(true).unwrap();
        }
        assert_eq!(sender.so_lan.get(), 1, "cờ fired phải sống ngoài core");
    }

    /// Chord trượt hẳn: set() phải TRẢ VỀ sau ngân sách chứ không treo, và trả
    /// Ok để reconcile là chỗ duy nhất quyết định VerifyFailed.
    #[test]
    fn chord_truot_thi_tra_ve_sau_ngan_sach_khong_treo() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(30), ms(1), &fired);
        let t0 = Instant::now();
        core.set(true).unwrap();
        let mat = t0.elapsed();
        assert!(mat >= ms(30), "phải chờ hết ngân sách, mất {mat:?}");
        assert!(mat < ms(500), "không được treo, mất {mat:?}");
        assert!(!core.is_on().unwrap());
    }

    /// App chưa chạy + muốn bật: đi nhánh launch, KHÔNG bắn chord (app khởi động
    /// đã ở trạng thái bật sẵn — bắn thêm là tắt mất).
    #[test]
    fn app_chua_chay_thi_launch_chu_khong_chord() {
        let st = FakeState::new(false, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = FakeLauncher {
            so_lan: Cell::new(0),
            state: &st,
        };
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(200), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(lc.so_lan.get(), 1);
        assert_eq!(
            sender.so_lan.get(),
            0,
            "không được bắn chord ở nhánh launch"
        );
        assert!(core.is_on().unwrap());
    }

    /// App chưa chạy + muốn tắt: đã là `en` rồi, không làm gì hết.
    #[test]
    fn app_chua_chay_muon_tat_thi_no_op() {
        let st = FakeState::new(false, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(200), ms(1), &fired);
        core.set(false).unwrap();
        assert_eq!(sender.so_lan.get(), 0);
    }

    /// Đã đúng sẵn thì không đụng gì.
    #[test]
    fn dung_san_thi_khong_ban() {
        let st = FakeState::new(true, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(200), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 0);
    }

    /// App chết thì dù defaults còn sót enabled=1 cũng KHÔNG gõ được tiếng Việt.
    #[test]
    fn app_chet_thi_is_on_false_du_enabled_con_sot() {
        let st = FakeState::new(false, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(200), ms(1), &fired);
        assert!(!core.is_on().unwrap());
    }

    /// Tắt tiếng Việt khi app đang chạy: cũng chỉ một chord.
    #[test]
    fn tat_vi_ban_mot_chord() {
        let st = FakeState::new(true, true);
        let sender = FakeSender {
            so_lan: Cell::new(0),
            state: &st,
            tre: 4,
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &KhongKhoa, ms(1000), ms(1), &fired);
        core.set(false).unwrap();
        assert_eq!(sender.so_lan.get(), 1);
        assert!(!core.is_on().unwrap());
    }

    // --- Đua giữa HAI TIẾN TRÌNH tongue ------------------------------------
    //
    // Cờ `fired` chốt "một chord mỗi lần chạy" chỉ bên trong MỘT tiến trình.
    // Trên máy thật có hai bên cùng phát `tongue vi` khi rời terminal sang
    // trình duyệt — Hammerspoon khôi phục theo app, và tongue.nvim trả bộ gõ
    // lúc nvim mất focus. Đo được 495ms giữa hai lần spawn; rơi vào trong cửa
    // sổ 87–286ms mà GoNhanh chưa kịp ghi `gonhanh.enabled` thì cả hai đọc
    // "đang tắt", mỗi bên bắn một chord, GoNhanh bật rồi tắt, cả hai
    // VerifyFailed.

    /// GoNhanh dùng chung giữa nhiều luồng. Mỗi chord xếp một cú LẬT ăn sau
    /// `tre` — lật chứ không phải gán, vì chord là toggle: hai cú thì về chỗ cũ.
    struct GoNhanhChung {
        inner: Mutex<TrangThai>,
        so_chord: AtomicU32,
        tre: Duration,
    }
    struct TrangThai {
        enabled: bool,
        cho_lat: Vec<Instant>,
    }
    impl GoNhanhChung {
        fn new(enabled: bool, tre: Duration) -> Self {
            Self {
                inner: Mutex::new(TrangThai {
                    enabled,
                    cho_lat: Vec::new(),
                }),
                so_chord: AtomicU32::new(0),
                tre,
            }
        }
        /// Đọc bit, áp trước mọi cú lật đã tới hạn.
        fn doc(&self) -> bool {
            let mut g = self.inner.lock().unwrap();
            let now = Instant::now();
            let mut i = 0;
            while i < g.cho_lat.len() {
                if now >= g.cho_lat[i] {
                    g.cho_lat.remove(i);
                    g.enabled = !g.enabled;
                } else {
                    i += 1;
                }
            }
            g.enabled
        }
        fn nhan_chord(&self) {
            self.so_chord.fetch_add(1, Ordering::SeqCst);
            let han = Instant::now() + self.tre;
            self.inner.lock().unwrap().cho_lat.push(han);
        }
    }
    impl StateSource for GoNhanhChung {
        fn running(&self) -> Result<bool> {
            Ok(true)
        }
        fn enabled(&self) -> Result<Option<bool>> {
            Ok(Some(self.doc()))
        }
    }

    struct SenderChung<'a> {
        app: &'a GoNhanhChung,
    }
    impl ChordSender for SenderChung<'_> {
        fn send(&self) -> Result<()> {
            self.app.nhan_chord();
            Ok(())
        }
    }

    fn thu_muc_tam(ten: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tongue-test-{}-{}", ten, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// HỒI QUY CHO LỖI ĐÃ ĐO NGOÀI ĐỜI: hai lần chạy tongue chồng nhau, mỗi lần
    /// có cờ `fired` riêng, nhưng TỔNG CỘNG chỉ được một chord.
    #[test]
    fn hai_lan_chay_chong_nhau_chi_ban_mot_chord() {
        let dir = thu_muc_tam("dua");
        let khoa = dir.join("switch.lock");
        let app = GoNhanhChung::new(false, ms(150));
        let cong = Barrier::new(2);

        std::thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| {
                    let sender = SenderChung { app: &app };
                    let lc = LauncherKhongDuocGoi;
                    let gate = FileGate::new(&khoa, ms(3000)).unwrap();
                    let fired = Cell::new(false);
                    let core = HotkeyCore::new(&sender, &lc, &app, &gate, ms(1000), ms(5), &fired);
                    cong.wait();
                    core.set(true).unwrap();
                });
            }
        });

        assert_eq!(
            app.so_chord.load(Ordering::SeqCst),
            1,
            "hai lần chạy chồng nhau chỉ được bắn TỔNG CỘNG một chord"
        );
        assert!(app.doc(), "phải kết thúc ở trạng thái bật");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Bên kia giữ khoá quá ngân sách: KHÔNG được bắn chord — bắn lúc này là
    /// tái lập đúng lỗi vừa chặn. Và phải trả về chứ không treo, để reconcile
    /// vẫn là chỗ duy nhất quyết định VerifyFailed.
    #[test]
    fn khoa_bi_giu_qua_han_thi_khong_ban_chord() {
        let dir = thu_muc_tam("qua-han");
        let khoa = dir.join("switch.lock");

        let ben_kia = FileGate::new(&khoa, ms(1000)).unwrap();
        let _cam = ben_kia
            .enter()
            .unwrap()
            .expect("lần đầu phải giành được khoá");

        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let gate = FileGate::new(&khoa, ms(60)).unwrap();
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, &gate, ms(1000), ms(1), &fired);

        let t0 = Instant::now();
        core.set(true).unwrap();
        let mat = t0.elapsed();

        assert_eq!(
            sender.so_lan.get(),
            0,
            "không giành được khoá thì không được bắn chord"
        );
        assert!(mat >= ms(60), "phải chờ hết ngân sách, mất {mat:?}");
        assert!(mat < ms(600), "không được treo, mất {mat:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
