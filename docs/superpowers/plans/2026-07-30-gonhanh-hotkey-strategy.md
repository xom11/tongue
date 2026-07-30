# Strategy `hotkey` cho GoNhanh — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Thêm `[macos] strategy = "hotkey"` để `tongue vi`/`en` đổi mode bằng chord toggle của GoNhanh thay vì kill/launch app, bỏ hẳn cold-start 200–400ms.

**Architecture:** Ba file mới `cfg(target_os = "macos")`: `chord.rs` (parser thuần cho blob JSON trong defaults), `prefs.rs` (đọc CFPreferences trong tiến trình), `hotkey.rs` (`HotkeyIme` — logic tách sau ba trait nhỏ để test bằng fake, FFI CoreGraphics nằm riêng). Nối vào qua đúng một cửa `make_ime` như bất biến của repo.

**Tech Stack:** Rust 2021, anyhow, serde/serde_json, core-foundation 0.10 + core-foundation-sys 0.8 (CFPreferences), FFI tay tới CoreGraphics + ApplicationServices (cùng kiểu `tis.rs` link Carbon).

**Spec:** `docs/superpowers/specs/2026-07-30-gonhanh-hotkey-strategy-design.md`

## Global Constraints

- Mặc định giữ nguyên `strategy = "process"`. `hotkey` là opt-in.
- Không đổi giao diện CLI, không đổi exit code: `0` verify khớp · `1` `VerifyFailed` · `2` lỗi môi trường.
- Không sửa trait `Ime` trong `src/backend/mod.rs` — nó dùng chung với Windows và `system`.
- Không refactor `gonhanh.rs` sang CFPreferences cho đường GHI; chỉ đường ĐỌC của `hotkey` dùng CFPreferences.
- Hằng số ngoại lai chép verbatim: domain `org.gonhanh.GoNhanh`, key `gonhanh.enabled`, `gonhanh.perAppMode`, `gonhanh.shortcut.toggle`.
- Ngân sách chờ dùng lại `verify.timeout_ms` / `verify.poll_ms` — KHÔNG thêm hằng số cấu hình mới.
- Comment và commit message tiếng Việt, dạng `<phạm vi>: <nội dung>`. Không `Co-Authored-By`.
- Gate bắt buộc trước push (cả 4, theo CLAUDE.md):
  `cargo test` · `cargo fmt --check` · `cargo clippy --all-targets -- -D warnings` ·
  `cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings`

---

### Task 1: `chord.rs` — parser thuần cho chord toggle

**Files:**
- Create: `src/backend/macos/chord.rs`
- Modify: `src/backend/macos/mod.rs:1-4` (thêm `pub mod chord;`)
- Modify: `Cargo.toml:13-15` (thêm `serde_json` vào khối macOS)

**Interfaces:**
- Consumes: không có (task đầu)
- Produces: `pub struct Chord { pub key_code: u16, pub flags: u64 }`, `pub fn parse(blob: &[u8]) -> anyhow::Result<Chord>`, `pub fn describe(c: &Chord) -> String`

- [ ] **Step 1: Thêm dependency**

Trong `Cargo.toml`, khối `[target.'cfg(target_os = "macos")'.dependencies]` thành:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
core-foundation = "0.10"
core-foundation-sys = "0.8"
serde_json = "1"
```

- [ ] **Step 2: Đăng ký module**

`src/backend/macos/mod.rs`:

```rust
pub mod app;
pub mod chord;
pub mod gonhanh;
pub mod system;
pub mod tis;
```

- [ ] **Step 3: Viết test trước (file mới, chỉ phần test)**

Tạo `src/backend/macos/chord.rs` với đúng nội dung này (test trước, impl rỗng để nó fail biên dịch có chủ đích ở bước sau):

```rust
//! Chord toggle của GoNhanh: blob JSON trong defaults -> keyCode + flags.
//! Thuần, không FFI — test chạy được mà không cần GoNhanh trên máy.

use anyhow::{Context, Result};
use serde::Deserialize;

/// NSEvent modifier flags và CGEventFlags TRÙNG bit nhau:
/// CapsLock 1<<16, Shift 1<<17, Control 1<<18, Option 1<<19, Command 1<<20.
/// Nên chuyển đổi là identity — không có bảng ánh xạ nào cả. Mask này chỉ để
/// bỏ 16 bit device-dependent thấp (trái/phải của phím bổ trợ) và các bit lạ.
const MODIFIER_MASK: u64 = 0x001F_0000;

const FLAG_CAPS: u64 = 1 << 16;
const FLAG_SHIFT: u64 = 1 << 17;
const FLAG_CONTROL: u64 = 1 << 18;
const FLAG_OPTION: u64 = 1 << 19;
const FLAG_COMMAND: u64 = 1 << 20;

#[derive(Debug, PartialEq, Eq)]
pub struct Chord {
    pub key_code: u16,
    pub flags: u64,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "keyCode")]
    key_code: u16,
    modifiers: u64,
}

pub fn parse(_blob: &[u8]) -> Result<Chord> {
    unimplemented!()
}

pub fn describe(_c: &Chord) -> String {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chord thật đọc từ máy 30/07/2026: Ctrl+Shift+Space.
    const THAT: &[u8] = br#"{"keyCode":49,"modifiers":393216}"#;

    #[test]
    fn parse_chord_that_tu_may() {
        let c = parse(THAT).unwrap();
        assert_eq!(
            c,
            Chord {
                key_code: 49,
                flags: 0x0006_0000
            }
        );
    }

    /// GoNhanh có thể ghi kèm bit device-dependent (trái/phải) ở 16 bit thấp —
    /// CGEventSetFlags không cần chúng, và giữ lại dễ làm chord không khớp.
    #[test]
    fn mask_bo_bit_device_dependent_va_bit_la() {
        let blob = br#"{"keyCode":49,"modifiers":4294967295}"#;
        let c = parse(blob).unwrap();
        assert_eq!(c.flags, 0x001F_0000);
    }

    #[test]
    fn json_hong_thi_bao_loi() {
        assert!(parse(b"khong-phai-json").is_err());
    }

    #[test]
    fn thieu_field_modifiers_thi_bao_loi() {
        assert!(parse(br#"{"keyCode":49}"#).is_err());
    }

    #[test]
    fn describe_chord_that() {
        let c = Chord {
            key_code: 49,
            flags: 0x0006_0000,
        };
        assert_eq!(describe(&c), "Ctrl+Shift+Space");
    }

    #[test]
    fn describe_phim_la_in_thang_ma_so() {
        let c = Chord {
            key_code: 200,
            flags: 0,
        };
        assert_eq!(describe(&c), "keyCode 200");
    }

    #[test]
    fn describe_du_bon_phim_bo_tro() {
        let c = Chord {
            key_code: 36,
            flags: FLAG_CONTROL | FLAG_OPTION | FLAG_SHIFT | FLAG_COMMAND,
        };
        assert_eq!(describe(&c), "Ctrl+Option+Shift+Cmd+Return");
    }
}
```

- [ ] **Step 4: Chạy test để chắc nó fail**

Run: `cargo test --lib chord`
Expected: FAIL — panic `not implemented` ở cả 7 test.

- [ ] **Step 5: Cài đặt `parse` và `describe`**

Thay hai hàm `unimplemented!()` bằng:

```rust
pub fn parse(blob: &[u8]) -> Result<Chord> {
    let raw: Raw = serde_json::from_slice(blob).context(
        "gonhanh.shortcut.toggle không phải JSON dạng {\"keyCode\":N,\"modifiers\":N}",
    )?;
    Ok(Chord {
        key_code: raw.key_code,
        flags: raw.modifiers & MODIFIER_MASK,
    })
}

/// Chỉ phục vụ `doctor` — không nằm trên đường switch, nên chỉ cần đủ tên phím
/// thông dụng; phím lạ in thẳng mã số thay vì đoán bừa.
pub fn describe(c: &Chord) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, ten) in [
        (FLAG_CONTROL, "Ctrl"),
        (FLAG_OPTION, "Option"),
        (FLAG_SHIFT, "Shift"),
        (FLAG_COMMAND, "Cmd"),
        (FLAG_CAPS, "CapsLock"),
    ] {
        if c.flags & bit != 0 {
            parts.push(ten.into());
        }
    }
    parts.push(match c.key_code {
        36 => "Return".into(),
        48 => "Tab".into(),
        49 => "Space".into(),
        53 => "Esc".into(),
        n => format!("keyCode {n}"),
    });
    parts.join("+")
}
```

- [ ] **Step 6: Chạy lại test**

Run: `cargo test --lib chord`
Expected: PASS, 7 test.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/backend/macos/mod.rs src/backend/macos/chord.rs
git commit -m "macos: parser chord toggle của GoNhanh (thuần, test không cần máy thật)"
```

---

### Task 2: `prefs.rs` — đọc defaults trong tiến trình qua CFPreferences

**Files:**
- Create: `src/backend/macos/prefs.rs`
- Modify: `src/backend/macos/mod.rs` (thêm `pub mod prefs;`)

**Interfaces:**
- Consumes: không có
- Produces: `pub fn read_bool(domain: &str, key: &str) -> Option<bool>`, `pub fn read_data(domain: &str, key: &str) -> Option<Vec<u8>>`

- [ ] **Step 1: Đăng ký module**

`src/backend/macos/mod.rs` — chèn `pub mod prefs;` giữ thứ tự chữ cái (sau `gonhanh`, trước `system`).

Kèm `#[allow(dead_code)]` và một comment tiếng Việt giải thích, y như `chord`
ngay phía trên và như `vkey_shm`/`hkl` trong `src/backend/mod.rs:6-12`: call site
thật của `prefs` nằm ở Task 4, nên tới lúc đó nó là dead code hợp lệ. Thiếu
attribute này là `cargo build` ra warning và cả hai lệnh `cargo clippy -D warnings`
đỏ — đã xảy ra thật ở Task 1.

- [ ] **Step 2: Viết file kèm test**

Tạo `src/backend/macos/prefs.rs`:

```rust
//! Đọc defaults của app khác NGAY TRONG tiến trình, qua CFPreferences.
//!
//! Vì sao không shell-out `defaults` như phần còn lại của repo — đã đo trên máy
//! thật 30/07/2026, hai lý do độc lập, mỗi lý do đủ để loại:
//!   1. `defaults read <domain> <key>` CẮT NGẮN blob data thành
//!      `{length = 33, bytes = 0x7b22... ... 7d}` → chord không parse được.
//!      CFPreferencesCopyAppValue trả trọn 33 byte.
//!   2. Chi phí: CFPreferences 0.01ms/lần, shell-out 66.5ms/lần. reconcile poll
//!      mỗi 50ms, nên shell-out còn tốn hơn cả một chu kỳ poll.
//!
//! CHỈ ĐỌC. Đường ghi (`defaults write`) vẫn nằm ở gonhanh.rs, không đụng tới.

use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFGetTypeID, CFRelease, CFTypeRef};
use core_foundation_sys::data::{CFDataGetBytePtr, CFDataGetLength, CFDataGetTypeID, CFDataRef};
use core_foundation_sys::number::{
    kCFNumberSInt64Type, CFBooleanGetTypeID, CFBooleanGetValue, CFBooleanRef, CFNumberGetTypeID,
    CFNumberGetValue, CFNumberRef,
};
use core_foundation_sys::preferences::{CFPreferencesAppSynchronize, CFPreferencesCopyAppValue};

/// Bắt buộc synchronize trước mỗi lần đọc: GoNhanh là tiến trình KHÁC vừa ghi
/// xuống, không sync thì đọc phải bản cache trong tiến trình mình. Đã nghiệm
/// chứng là sau khi sync thì thấy thay đổi sau 87–225ms.
///
/// Trả về giá trị đã +1 retain (Copy rule) — caller phải CFRelease.
unsafe fn copy_value(domain: &str, key: &str) -> Option<CFTypeRef> {
    let d = CFString::new(domain);
    let k = CFString::new(key);
    CFPreferencesAppSynchronize(d.as_concrete_TypeRef());
    let v = CFPreferencesCopyAppValue(k.as_concrete_TypeRef(), d.as_concrete_TypeRef());
    if v.is_null() {
        None
    } else {
        Some(v as CFTypeRef)
    }
}

/// None = key không có, hoặc có nhưng không phải bool/number.
pub fn read_bool(domain: &str, key: &str) -> Option<bool> {
    unsafe {
        let v = copy_value(domain, key)?;
        let tid = CFGetTypeID(v);
        // `defaults write -bool` tạo CFBoolean, còn app ghi qua UserDefaults có
        // thể ra CFNumber — nhận cả hai để khỏi phụ thuộc ai ghi.
        let out = if tid == CFBooleanGetTypeID() {
            Some(CFBooleanGetValue(v as CFBooleanRef))
        } else if tid == CFNumberGetTypeID() {
            let mut n: i64 = 0;
            let ok = CFNumberGetValue(
                v as CFNumberRef,
                kCFNumberSInt64Type,
                &mut n as *mut i64 as *mut std::ffi::c_void,
            );
            if ok != 0 {
                Some(n != 0)
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(v);
        out
    }
}

/// None = key không có, hoặc có nhưng không phải data.
pub fn read_data(domain: &str, key: &str) -> Option<Vec<u8>> {
    unsafe {
        let v = copy_value(domain, key)?;
        let out = if CFGetTypeID(v) == CFDataGetTypeID() {
            let d = v as CFDataRef;
            let len = CFDataGetLength(d) as usize;
            let ptr = CFDataGetBytePtr(d);
            if ptr.is_null() {
                None
            } else {
                Some(std::slice::from_raw_parts(ptr, len).to_vec())
            }
        } else {
            None
        };
        CFRelease(v);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_khong_ton_tai_thi_none() {
        assert_eq!(
            read_bool("com.example.khong-he-ton-tai.tongue-test", "bat-ky"),
            None
        );
        assert_eq!(
            read_data("com.example.khong-he-ton-tai.tongue-test", "bat-ky"),
            None
        );
    }

    /// Đọc GoNhanh thật — chỉ chạy tay vì phụ thuộc máy đã cài GoNhanh.
    #[test]
    #[ignore = "chạm hệ thống thật — chạy tay: cargo test -- --ignored"]
    fn doc_chord_that_cua_gonhanh() {
        let blob = read_data("org.gonhanh.GoNhanh", "gonhanh.shortcut.toggle")
            .expect("không đọc được gonhanh.shortcut.toggle");
        eprintln!("blob {} byte: {}", blob.len(), String::from_utf8_lossy(&blob));
        let c = super::super::chord::parse(&blob).unwrap();
        eprintln!("chord: {}", super::super::chord::describe(&c));
        assert!(blob.len() > 10);
    }
}
```

- [ ] **Step 3: Chạy test**

Run: `cargo test prefs`
Expected: PASS (1 test chạy, 1 ignored).

(Crate này là binary-only — không có `src/lib.rs` — nên `cargo test --lib` báo
`no library targets found`. Lọc theo tên như trên.)

- [ ] **Step 4: Chạy test thật để nghiệm chứng FFI đúng**

Run: `cargo test prefs -- --ignored --nocapture`
Expected: PASS, in ra `blob 33 byte: {"keyCode":49,"modifiers":393216}` và `chord: Ctrl+Shift+Space`.

Nếu blob rỗng hoặc test panic: FFI sai — dừng lại sửa trước khi đi tiếp, các task sau đều dựa lên đây.

- [ ] **Step 5: Chạy đủ gate trước khi commit**

```bash
cargo test
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
Expected: PASS cả 5, `cargo build` 0 warning. Nếu đỏ vì `dead_code` thì Step 1
chưa làm đúng.

- [ ] **Step 6: Commit**

```bash
git add src/backend/macos/mod.rs src/backend/macos/prefs.rs
git commit -m "macos: đọc defaults qua CFPreferences (defaults read cắt blob, lại tốn 66ms/lần)"
```

---

### Task 3: Logic `hotkey` sau trait — phần quan trọng nhất, test bằng fake

**Files:**
- Create: `src/backend/macos/hotkey.rs`
- Modify: `src/backend/macos/mod.rs` (thêm `pub mod hotkey;`)

**Interfaces:**
- Consumes: không có (logic thuần, chưa đụng FFI)
- Produces: `pub trait ChordSender { fn send(&self) -> Result<()> }`, `pub trait Launcher { fn launch(&self) -> Result<()> }`, `pub trait StateSource { fn running(&self) -> Result<bool>; fn enabled(&self) -> Result<Option<bool>> }`, `pub struct HotkeyCore<'a>` với `new(sender: &'a dyn ChordSender, launcher: &'a dyn Launcher, state: &'a dyn StateSource, timeout: Duration, poll: Duration, fired: &'a Cell<bool>)`, `is_on() -> Result<bool>`, `set(on: bool) -> Result<()>`

Đây là task khoá đúng lỗi mà cả thiết kế sinh ra để tránh: chord là **relay**, còn `reconcile` gọi lại `set()` mỗi 50ms trong khi `enabled` mất 87–286ms mới phản ánh.

- [ ] **Step 1: Đăng ký module**

`src/backend/macos/mod.rs` — chèn `pub mod hotkey;` sau `pub mod gonhanh;`.

Kèm `#[allow(dead_code)]` và comment tiếng Việt như `chord`/`prefs` phía trên: cả
`HotkeyCore` lẫn ba trait chỉ có call site thật ở Task 4 (`HotkeyIme`), nên tới lúc
đó chúng là dead code hợp lệ. Thiếu attribute là `cargo build` warning và cả hai
lệnh `cargo clippy -D warnings` đỏ — đã xảy ra thật ở Task 1.

- [ ] **Step 2: Viết test trước, cùng khung trait và impl rỗng**

Tạo `src/backend/macos/hotkey.rs`:

```rust
//! Strategy `hotkey`: giữ GoNhanh sống, đổi vi/en bằng chính chord toggle mà app
//! đã đăng ký. Bỏ hẳn cold-start vì `en` không còn giết app.
//!
//! BẤT BIẾN CỦA FILE NÀY — chord là RELAY, không idempotent:
//! `reconcile` gọi lại `set()` mỗi vòng poll 50ms còn lệch, nhưng GoNhanh mất
//! 87–286ms (đo 4 lần, 30/07/2026) mới ghi `gonhanh.enabled`. Trả về sớm là bắn
//! trùng 2–6 chord và lật mode qua lại. Nên `set()` phải TỰ CHỜ XÁC NHẬN, và
//! bắn TỐI ĐA MỘT chord cho mỗi lần chạy tongue.
//!
//! Logic nằm sau ba trait để test được bằng fake; FFI CoreGraphics ở cuối file.

use anyhow::Result;
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

pub struct HotkeyCore<'a> {
    sender: &'a dyn ChordSender,
    launcher: &'a dyn Launcher,
    state: &'a dyn StateSource,
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
        timeout: Duration,
        poll: Duration,
        fired: &'a Cell<bool>,
    ) -> Self {
        Self {
            sender,
            launcher,
            state,
            timeout,
            poll,
            fired,
        }
    }

    pub fn is_on(&self) -> Result<bool> {
        unimplemented!()
    }

    pub fn set(&self, _on: bool) -> Result<()> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(1000), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(20), ms(1), &fired);
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
            let core = HotkeyCore::new(&sender, &lc, &st, ms(10), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(30), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(lc.so_lan.get(), 1);
        assert_eq!(sender.so_lan.get(), 0, "không được bắn chord ở nhánh launch");
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
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
        let core = HotkeyCore::new(&sender, &lc, &st, ms(1000), ms(1), &fired);
        core.set(false).unwrap();
        assert_eq!(sender.so_lan.get(), 1);
        assert!(!core.is_on().unwrap());
    }
}
```

- [ ] **Step 3: Chạy test để chắc nó fail**

Run: `cargo test hotkey`
Expected: FAIL — panic `not implemented` ở cả 9 test.

(Crate binary-only, không có `src/lib.rs` — `cargo test --lib` sẽ báo
`no library targets found`. Lọc theo tên như trên.)

- [ ] **Step 4: Cài đặt `is_on` và `set`**

Thay hai hàm `unimplemented!()`:

```rust
    /// App đang chạy VÀ enabled — không phải một trong hai. App chết thì
    /// `enabled` còn sót lại trong defaults cũng chẳng gõ được gì.
    pub fn is_on(&self) -> Result<bool> {
        Ok(self.state.running()? && self.state.enabled()?.unwrap_or(false))
    }

    pub fn set(&self, on: bool) -> Result<()> {
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
```

- [ ] **Step 5: Chạy lại test**

Run: `cargo test hotkey`
Expected: PASS, 9 test.

- [ ] **Step 6: Chạy đủ gate trước khi commit**

```bash
cargo test
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
Expected: PASS cả 5, `cargo build` 0 warning. Nếu đỏ vì `dead_code` thì Step 1
chưa làm đúng.

- [ ] **Step 7: Commit**

```bash
git add src/backend/macos/mod.rs src/backend/macos/hotkey.rs
git commit -m "macos: logic strategy hotkey — chord là relay nên set() tự chờ xác nhận"
```

---

### Task 4: FFI CoreGraphics + `HotkeyIme` thật

**Files:**
- Modify: `src/backend/macos/hotkey.rs` (thêm phần FFI và `HotkeyIme` vào cuối, trước `mod tests`)
- Modify: `src/backend/macos/gonhanh.rs:68-107` (tách phần khám `perAppMode` thành hàm dùng chung)

**Interfaces:**
- Consumes: `chord::{Chord, parse, describe}` (Task 1), `prefs::{read_bool, read_data}` (Task 2), `HotkeyCore` (Task 3), `app::{is_running, launch, diagnose_bundle}` (có sẵn)
- Produces: `pub struct HotkeyIme` với `pub fn new(app_name: String, timeout_ms: u64, poll_ms: u64) -> Self` (field `fired: Cell<bool>` là private — Task 5 phải dựng qua `new`, không dùng struct literal), implement `crate::backend::Ime`; `gonhanh::diagnose_per_app_mode(fix: bool, app_name: &str, restart: &dyn Fn() -> Result<()>) -> Result<Finding>`

- [ ] **Step 1: Tách phần khám `perAppMode` khỏi `GonhanhIme::diagnose`**

Trong `src/backend/macos/gonhanh.rs`, đổi `diagnose` thành gọi hàm dùng chung, và thêm hàm đó ở tầng module:

```rust
/// Phần khám `perAppMode` dùng chung cho cả strategy `process` lẫn `hotkey`:
/// perAppMode bật = GoNhanh nhớ trạng thái theo từng app, khiến key
/// gonhanh.enabled thành đồ giả (bẫy đã xác minh trong source).
///
/// `restart` chỉ được gọi khi `fix` và giá trị đang sai — strategy `hotkey`
/// truyền vào closure không làm gì, vì nó KHÔNG được phép giết app.
pub fn diagnose_per_app_mode(
    fix: bool,
    app_name: &str,
    restart: &dyn Fn() -> Result<()>,
) -> Result<Finding> {
    let out = Command::new("defaults")
        .args(["read", DEFAULTS_DOMAIN, KEY_PER_APP])
        .output()?;
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        return Ok(Finding {
            level: Level::Warn,
            msg: format!("chưa đọc được defaults của {app_name} — app đã chạy lần đầu chưa?"),
        });
    }
    if val == "0" {
        return Ok(Finding {
            level: Level::Ok,
            msg: format!("{KEY_PER_APP} = 0"),
        });
    }
    if fix {
        let st = Command::new("defaults")
            .args(["write", DEFAULTS_DOMAIN, KEY_PER_APP, "-bool", "NO"])
            .status()?;
        ensure!(st.success(), "defaults write {KEY_PER_APP} thất bại");
        restart()?;
        return Ok(Finding {
            level: Level::Ok,
            msg: format!("đã ghim {KEY_PER_APP}=0 cho {app_name}"),
        });
    }
    Ok(Finding {
        level: Level::Warn,
        msg: format!("{KEY_PER_APP} đang bật — chạy `tongue doctor --fix` để ghim về 0 (không thì trạng thái enabled không tin được)"),
    })
}
```

Rồi `GonhanhIme::diagnose` rút gọn còn:

```rust
    fn diagnose(&self, fix: bool) -> Result<Vec<Finding>> {
        Ok(vec![
            app::diagnose_bundle(&self.app_name),
            diagnose_per_app_mode(fix, &self.app_name, &|| self.restart())?,
        ])
    }
```

- [ ] **Step 2: Chạy test để chắc không vỡ gì**

Run: `cargo test`
Expected: PASS toàn bộ (31 test cũ + 17 test mới từ Task 1–3: 7 chord, 1 prefs, 9 hotkey).

- [ ] **Step 3: Thêm FFI và `HotkeyIme` vào `hotkey.rs`**

Trước hết sửa dòng import đầu file — phần dưới cần thêm `bail` và `Context`:

```rust
use anyhow::{bail, Context, Result};
```

Rồi chèn khối sau vào `src/backend/macos/hotkey.rs`, phía trên `#[cfg(test)] mod tests`:

```rust
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

struct CgChordSender {
    chord: chord::Chord,
}

impl ChordSender for CgChordSender {
    fn send(&self) -> Result<()> {
        unsafe {
            let src = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID);
            let down = CGEventCreateKeyboardEvent(src, self.chord.key_code, true);
            let up = CGEventCreateKeyboardEvent(src, self.chord.key_code, false);
            if down.is_null() || up.is_null() {
                if !src.is_null() {
                    CFRelease(src as _);
                }
                bail!("CGEventCreateKeyboardEvent trả về null");
            }
            CGEventSetFlags(down, self.chord.flags);
            CGEventSetFlags(up, self.chord.flags);
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
    /// Không đi qua HotkeyCore: `is_on` phải trả lời được cả khi chord chưa đọc
    /// được (GoNhanh chưa chạy lần đầu), mà dựng core thì cần chord.
    fn is_on(&self) -> Result<bool> {
        let state = self.state();
        Ok(state.running()? && state.enabled()?.unwrap_or(false))
    }

    fn set(&self, on: bool) -> Result<()> {
        let launcher = self.launcher();
        let state = self.state();
        let sender = CgChordSender {
            chord: doc_chord()?,
        };
        let core = HotkeyCore::new(
            &sender,
            &launcher,
            &state,
            Duration::from_millis(self.timeout_ms),
            Duration::from_millis(self.poll_ms),
            &self.fired,
        );
        core.set(on)
    }

    fn diagnose(&self, fix: bool) -> Result<Vec<Finding>> {
        let mut fs = vec![app::diagnose_bundle(&self.app_name)];

        // `hotkey` KHÔNG được phép giết app — đó là cả điểm của strategy này.
        // Nên --fix ở đây ghim perAppMode mà không restart; giá trị mới có hiệu
        // lực ở lần khởi động sau của GoNhanh.
        fs.push(super::gonhanh::diagnose_per_app_mode(
            fix,
            &self.app_name,
            &|| Ok(()),
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
```

- [ ] **Step 4: Chạy test và clippy cả hai target**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
Expected: PASS cả ba. Nếu clippy Windows đỏ vì dead-code, thêm `#[allow(dead_code)]` đúng chỗ chứ đừng bỏ `cfg`.

- [ ] **Step 5: Commit**

```bash
git add src/backend/macos/hotkey.rs src/backend/macos/gonhanh.rs
git commit -m "macos: HotkeyIme — bắn chord qua CGEventPost tầng HID, doctor khám Accessibility"
```

---

### Task 5: Nối vào `make_ime`, `doctor`, và doc config

**Files:**
- Modify: `src/main.rs:132-147` (`make_ime`)
- Modify: `src/doctor.rs:83-94` (mục 4 — strategy)
- Modify: `src/config.rs:20-21` (doc comment của `strategy`)

**Interfaces:**
- Consumes: `HotkeyIme::new(app_name, timeout_ms, poll_ms)` (Task 4)
- Produces: không có (đây là tầng nối)

- [ ] **Step 1: Sửa `make_ime`**

`src/main.rs`, thay toàn bộ hàm `make_ime` bản macOS:

```rust
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
```

- [ ] **Step 2: Sửa mục 4 của `doctor`**

`src/doctor.rs`, thay khối `// 4. strategy`:

```rust
    // 4. strategy
    if matches!(cfg.macos.strategy.as_str(), "process" | "hotkey") {
        fs.push(Finding {
            level: Level::Ok,
            msg: format!("strategy = {}", cfg.macos.strategy),
        });
    } else {
        fs.push(Finding {
            level: Level::Fail,
            msg: format!("strategy '{}' không hợp lệ (process|hotkey)", cfg.macos.strategy),
        });
    }
```

- [ ] **Step 3: Sửa doc comment trong `config.rs`**

```rust
    /// Cách điều khiển bộ gõ đó: "process" (kill/launch) | "hotkey" (giả lập
    /// chord toggle, giữ app sống — chỉ dùng được với backend "gonhanh").
    pub strategy: String,
```

- [ ] **Step 4: Thêm test cho config**

Chèn vào `mod tests` trong `src/config.rs`:

```rust
    #[test]
    fn khai_strategy_hotkey() {
        let c = parse("[macos]\nstrategy = \"hotkey\"\n").unwrap();
        assert_eq!(c.macos.strategy, "hotkey");
        assert_eq!(c.macos.backend, "gonhanh"); // vẫn là default
    }
```

- [ ] **Step 5: Chạy đủ 4 gate**

Run:
```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
Expected: PASS cả 4.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/doctor.rs src/config.rs
git commit -m "macos: nối strategy hotkey vào make_ime và doctor"
```

---

### Task 6: Smoke thật trên máy, rồi ghi bất biến vào CLAUDE.md

**Files:**
- Modify: `CLAUDE.md` (mục kiến trúc + mục bất biến)
- Không sửa code trừ khi smoke lộ ra lỗi.

**Interfaces:**
- Consumes: toàn bộ Task 1–5
- Produces: không có

Đây là task duy nhất chạm hệ thống thật. Nó ĐỔI chế độ gõ của máy — luôn kết thúc bằng `tongue vi`.

- [ ] **Step 1: Bật strategy hotkey trong config**

```bash
mkdir -p ~/.config/tongue
cp ~/.config/tongue/config.toml ~/.config/tongue/config.toml.bak 2>/dev/null || true
printf '[macos]\nstrategy = "hotkey"\n' >> ~/.config/tongue/config.toml
cat ~/.config/tongue/config.toml
```

Nếu file đã có `[macos]` từ trước thì sửa tay cho khỏi trùng section.

- [ ] **Step 2: Khám trước khi chuyển**

Run: `cargo run -q -- doctor`
Expected: `✓ strategy = hotkey`, `✓ Accessibility: tiến trình này được tin cậy`, `✓ chord toggle: Ctrl+Shift+Space`, `✓ gonhanh.perAppMode = 0`.

Nếu Accessibility báo ✗: cấp quyền cho chính app terminal đang chạy rồi mở lại terminal.

- [ ] **Step 3: Đo — đây là điều cả plan sinh ra để đạt**

```bash
pgrep -x GoNhanh
time cargo run -q -- en
pgrep -x GoNhanh
time cargo run -q -- vi
pgrep -x GoNhanh
```

Expected: PID **giống hệt nhau ở cả ba lần** (app không hề bị giết), `tongue vi` không còn cold-start, exit 0 cả hai lệnh.

Nếu PID đổi: có đường nào đó vẫn đi vào `GonhanhIme` — kiểm lại `make_ime`.

- [ ] **Step 4: Kiểm `status` và `zh`**

```bash
cargo run -q -- status
cargo run -q -- zh
cargo run -q -- status
cargo run -q -- vi
cargo run -q -- status
```

Expected: `status` báo đúng mode mỗi lần; `zh` đổi layout sang Pinyin và tắt bit mà **không** giết GoNhanh; lệnh cuối đưa máy về `vi`.

- [ ] **Step 5: Ghi lại kết quả đo vào phần bằng chứng của spec**

Mở `docs/superpowers/specs/2026-07-30-gonhanh-hotkey-strategy-design.md`, thêm vào cuối §"Bằng chứng đo trên máy thật" một dòng ghi kết quả smoke thực tế (PID trước/sau, thời gian `vi`). Số liệu thật, không phỏng đoán — nếu kết quả khác kỳ vọng thì ghi đúng cái đo được và dừng lại xử lý.

- [ ] **Step 6: Cập nhật CLAUDE.md**

Trong mục kiến trúc, thêm vào cây file:

```
  macos/chord.rs   # parser chord toggle GoNhanh (thuần) — test không cần máy thật
  macos/prefs.rs   # đọc defaults qua CFPreferences (chỉ đọc)
  macos/hotkey.rs  # strategy hotkey: giữ app sống, đổi mode bằng chord
```

Trong mục "Bất biến", thêm bốn đoạn:

```markdown
**Chord toggle của GoNhanh là RELAY, không idempotent — `set()` phải tự chờ xác
nhận rồi mới trả về, và bắn TỐI ĐA MỘT chord mỗi lần chạy.** reconcile gọi lại
`set()` mỗi vòng poll 50ms, mà GoNhanh mất 87–286ms (đo 4 lần, 30/07/2026) mới
ghi `gonhanh.enabled`. Trả về sớm là bắn trùng 2–6 chord và lật mode qua lại;
bỏ chốt "một chord" là cú thứ hai lật ngược cú đầu ăn chậm.

**Đọc defaults của GoNhanh trên đường nóng phải qua CFPreferences
(`macos/prefs.rs`), không shell-out.** Hai lý do độc lập: `defaults read` CẮT
NGẮN blob data nên `gonhanh.shortcut.toggle` không parse được, và nó tốn
66.5ms/lần — nhiều hơn cả chu kỳ poll 50ms (CFPreferences: 0.01ms). Nhớ
`CFPreferencesAppSynchronize` trước mỗi lần đọc, không thì đọc phải cache cũ.

**Quyền Accessibility cấp cho TIẾN TRÌNH CHỦ, không cho binary `tongue`.** Gõ
tay trong terminal thì cấp cho app terminal đó; gọi từ Hammerspoon thì cấp cho
Hammerspoon. Đây là kiểu hỏng khó đoán nhất của strategy `hotkey`, nên
`diagnose()` nói thẳng điều này thay vì chỉ báo "thiếu quyền".

**`is_on()` của strategy `hotkey` là `pgrep` VÀ `enabled`**, không phải một
trong hai — khác `process` (chỉ `pgrep`). App chết thì `enabled` còn sót lại
trong defaults cũng chẳng gõ được gì.
```

Trong bảng mode, thêm chú thích rằng cột `gonhanh` có hai strategy: `process`
(vi = app chạy, en = app bị giết) và `hotkey` (app luôn chạy, vi/en phân biệt
bằng bit `gonhanh.enabled`).

- [ ] **Step 7: Chạy lại đủ 4 gate rồi commit**

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
nix build .#tongue && ./result/bin/tongue --help
git add CLAUDE.md docs/superpowers/specs/2026-07-30-gonhanh-hotkey-strategy-design.md
git commit -m "docs: bất biến của strategy hotkey và kết quả smoke trên máy thật"
```

- [ ] **Step 8: Trả máy về trạng thái gõ tiếng Việt**

Run: `cargo run -q -- vi && cargo run -q -- status`
Expected: mode `vi`.
