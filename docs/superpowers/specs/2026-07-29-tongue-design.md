# tongue — thiết kế v1

*2026-07-29. Trạng thái: đã duyệt qua brainstorming (3 phần), chờ implementation plan.*

## Vấn đề

Gõ tiếng Việt cần bộ gõ ngoài — **GoNhanh** trên macOS, **VKey** trên Windows. Chúng
không phải input method của hệ điều hành nên:

- không bật/tắt được bằng im-select/macism (mấy tool đó chỉ đổi input source hệ thống);
- không có giao diện CLI;
- sinh trạng thái lỗi "hai bộ gõ cùng bật": khi bộ gõ ngoài bật, layout hệ thống
  phải là bàn phím tiếng Anh trơn — nếu input method tiếng Việt/tiếng Trung của hệ
  thống cũng đang bật thì gõ ra rác.

Bản chất: trạng thái "đang gõ ngôn ngữ gì" phân tán ở **hai biến** — (1) input
source/layout của OS, (2) tiến trình + bit enabled của bộ gõ ngoài — và không có
thao tác nguyên tử nào đổi cả hai. Các hack hiện hành trong `~/.nix` (Unicode Hex
Input làm cờ "en" trên mac; layout US/NZ làm cờ vi/en trên win) đều là hệ quả của
việc trưng dụng layout hệ thống làm biến trạng thái.

## Mục tiêu

Một CLI duy nhất, `tongue`, là **nguồn chân lý cho thao tác chuyển chế độ gõ**:

```
tongue vi | en | zh      # chuyển chế độ (zh chỉ trên macOS)
tongue                   # in chế độ hiện tại, ngắn: "vi"
tongue status [--json]   # chi tiết: mode, layout, IME chạy?/bật?, sai lệch nếu có
tongue doctor            # khám môi trường: cài đặt, version, quyền, config xung đột
```

Hotkey (Hammerspoon/AHK) về sau chỉ là lớp mỏng gọi CLI — theo đúng khuôn `beckon`.

**Ngoài phạm vi v1:** auto-switch theo app, daemon, hotkey, Linux, GUI, tích hợp
`~/.nix` (chủ nhân repo tự nối sau khi tool chạy ổn).

## Mô hình trạng thái

Mỗi chế độ là một **trạng thái đích khai báo** `(layout hệ thống, bit IME ngoài)`:

| Mode | macOS: input source | macOS: GoNhanh | Windows: layout | Windows: VKey |
|------|--------------------|----------------|-----------------|---------------|
| `vi` | ABC                | **bật**        | US (không đổi)  | **bật**       |
| `en` | ABC                | **tắt**        | US (không đổi)  | **tắt**       |
| `zh` | Pinyin (SCIM.ITABC)| **tắt**        | —               | —             |

Chuyển mode = **reconcile**: đọc trạng thái thật từ OS/IME → áp phần lệch →
**verify** (poll, timeout ~1s) → exit 0 khi khớp. Không state file, không daemon —
nguồn chân lý luôn là hệ thống, `tongue status` không bao giờ nói dối.

Hệ quả thiết kế: các hack layout chết hết. Windows không cần đổi layout nữa (US cố
định, chỉ còn 1 bit VKey); macOS chỉ còn ABC ↔ Pinyin, không cần Unicode Hex Input.

## Backend macOS

### Layout: TIS API trực tiếp

FFI từ Rust vào `TISCreateInputSourceList` / `TISSelectInputSource` (cách
im-select/macism làm). Ca chuyển sang Pinyin từ process nền có quirk "lệnh nhận,
không đổi" — chép mẹo retry/verify của macism (mã nguồn mở, đã chứng minh).
Source ID override được trong config.

### Bit GoNhanh: chiến lược "process = bit" (v1)

Bằng chứng từ source ([khaphanspace/gonhanh.org](https://github.com/khaphanspace/gonhanh.org)):

- **Không có kênh ngoài nào nhận lệnh khi đang chạy.** Không DistributedNotification
  listener, không XPC/CFMessagePort, không URL scheme, không AppleScript
  (đã quét toàn repo). `defaults` chỉ được đọc lúc khởi động
  (`AppState.init` → `MainSettingsView.swift:226`).
- **Bẫy perAppMode:** mặc định `gonhanh.perAppMode = true` (`App.swift:38`). Khi đó
  toggle ghi vào `gonhanh.perAppModes[bundleId]`, KHÔNG ghi `gonhanh.enabled`
  (`MainSettingsView.swift:62-75`) — đọc `gonhanh.enabled` sẽ stale. Ghim
  `perAppMode=0` thì mọi toggle ghi `gonhanh.enabled` → key thành mirror tin được.
- Quit sạch: `applicationWillTerminate` gỡ event tap (`App.swift:27-32`); SIGKILL
  cũng không để rác (kernel thu hồi CFMachPort).

Hành vi:

| Lệnh | Bước |
|------|------|
| adopt (doctor, 1 lần) | `defaults write org.gonhanh.GoNhanh gonhanh.perAppMode -bool NO` |
| `vi` | layout → ABC; `defaults write … gonhanh.enabled -bool YES`; nếu chưa chạy → `open -ga GoNhanh`; verify pgrep |
| `en` | layout → ABC; SIGTERM GoNhanh nếu đang chạy; verify pgrep rỗng |
| `zh` | SIGTERM GoNhanh; layout → Pinyin; verify source ID |

Độ trễ: kill tức thời; launch ~200–400ms (chỉ trả khi bật `vi`). Chấp nhận cho v1.

### Lộ trình nâng cấp backend GoNhanh (không đổi CLI)

1. **v1 `process`** — như trên, không cần quyền gì.
2. **v1.1 `hotkey`** — giả lập đúng chord toggle đọc từ `gonhanh.shortcut.toggle`
   (JSON keyCode+modifiers trong defaults) bằng `CGEventPost(.cghidEventTap)` —
   tap của GoNhanh ở HID level nên thấy được (khác osascript, bị tap bỏ qua —
   `RustBridge.swift:855-859`). Đọc lại `gonhanh.enabled` làm readback (tin được
   khi perAppMode=0). Cần cấp Accessibility cho binary. Đổi trạng thái không
   restart app.
3. **endgame `notify`** — PR upstream một DistributedNotificationCenter listener
   cho GoNhanh (tác giả viết tool vì đau đúng nỗi đau gõ trong Claude Code — khả
   năng nhận cao).

Chọn qua `strategy` trong config; mặc định `process`.

## Backend Windows

Bằng chứng từ source ([phatMT97/VKey](https://github.com/phatMT97/VKey), v4.2.0):
VKey có sẵn giao diện điều khiển hoàn chỉnh, đạt "hướng B thuần" không cần sửa upstream.

- **Set mode:** `WM_VKEY_SET_MODE = WM_USER+100` (0x0464), `wParam` 1=VI 0=EN, gửi
  tới cửa sổ ẩn class `VKeyTrayClass` (`SharedConstants.h:16`, handler
  `TrayIcon.cpp:499-508`). Là lệnh **set idempotent**, đi đúng đường người dùng bấm
  hotkey → cập nhật cả smart-switch map nên **không bị focus-change ghi đè**.
- **Read state:** shared memory `Local\VKeySharedState` — validate magic
  `0x59454B4E` @0 + `structVersion ≤ 4` @4, đọc `flags` @16, bit 0 = VI
  (`SharedState.h:16,253,378`; layout đóng băng bằng static_assert). Đọc 32-bit
  aligned đơn, không cần seqlock. Section không tồn tại = VKey tắt.
- **Quit sạch (chỉ dùng khi cần):** `WM_CLOSE` tới cửa sổ đó — được whitelist UIPI
  chủ đích (`TrayIcon.cpp:100-103`), exit path báo watchdog đứng yên
  (`main.cpp:920-924`). Không bao giờ `TerminateProcess` trần (watchdog — nếu bật —
  sẽ hồi sinh; mặc định watchdog TẮT, `SystemConfig.h:64`).
- **Start:** `CreateProcessW` không tham số; mutex `Local\VKey_Main_Mutex` tự chống
  trùng. Không có tham số dòng lệnh nào set được mode (đã quét router
  `main.cpp:171-256`).

Hành vi:

| Lệnh | Bước |
|------|------|
| `vi` | VKey chưa chạy → start; `FindWindowW(L"VKeyTrayClass", NULL)` → `PostMessage(0x0464, 1, 0)`; poll shm bit 0 = 1 (timeout theo `[verify]`) |
| `en` | đang chạy → `PostMessage(0x0464, 0, 0)` + verify shm; không chạy → đã là `en`, no-op |
| `status` | đọc shm; vắng section = `en` |

**VKey không bao giờ bị kill trong luồng chuyển thường → không xáo hook
`WH_KEYBOARD_LL`, lỗi tranh hook với kanata (lý do tồn tại của evkey-monitor.ahk)
biến mất tận gốc.**

Caveat mã hoá vào doctor:

- VKey elevated (`run_as_admin=true`) → UIPI nuốt PostMessage (trừ WM_CLOSE):
  cảnh báo + hướng dẫn tắt; fallback ghi thẳng shm bằng `InterlockedOr/And`
  (chấp nhận: không sticky với smart-switch).
- `smart_switch=true` tự đổi mode theo app sau khi chuyển tay → doctor khuyến nghị tắt.
- `VKeyClassic.exe` là peer bình đẳng (cùng window class, mutex, shm) — xử lý như nhau.
- Bit shm là mode *hiệu lực* (app bị exclude ép 0) — status nên nói rõ khi lệch.

## Cấu trúc repo

```
tongue/
  Cargo.toml               # 1 crate, 1 binary, Rust
  src/
    main.rs                # clap: vi|en|zh|status|doctor (khuôn beckon)
    mode.rs                # bảng mode → trạng thái đích
    reconcile.rs           # đọc → diff → áp → verify; logic thuần
    backend/
      mod.rs               # trait Layout { current, select }, trait Ime { state, set(bool) }
      macos/  tis.rs gonhanh.rs
      windows/ vkey.rs
  flake.nix                # package + overlay theo khuôn beckon
  .github/workflows/ci.yml # build matrix macOS + Windows, clippy, test
  docs/superpowers/specs/  # file này
```

## Config (tuỳ chọn — không có vẫn chạy)

`~/.config/tongue/config.toml` (mac) / `%APPDATA%\tongue\config.toml` (win):

```toml
[macos]
strategy   = "process"                            # process | hotkey | notify
source_vi  = "com.apple.keylayout.ABC"
source_zh  = "com.apple.inputmethod.SCIM.ITABC"
app_name   = "GoNhanh"

[windows]
vkey_path  = ''                                   # rỗng = tự tìm (process đang chạy / winget path)

[verify]
timeout_ms = 1000
```

## Lỗi & exit code

- `0` — trạng thái đích đã verify khớp.
- `1` — áp xong nhưng verify trượt: in biến nào lệch, nghi phạm, trỏ `tongue doctor`.
- `2` — lỗi môi trường: IME chưa cài, không tìm thấy window/section, thiếu quyền,
  hoặc mode không tồn tại trên nền tảng này (`tongue zh` trên Windows).

Thông điệp lỗi nói thẳng nguyên nhân khả dĩ, ví dụ: *"VKey không nhận
WM_VKEY_SET_MODE — có thể đang chạy elevated; chạy `tongue doctor`."*
`status --json` cho hotkey/script tiêu thụ.

## Kiểm thử

- **Unit:** reconcile + bảng mode với mock 2 trait (không đụng OS, chạy trên CI);
  parser shared-memory VKey với fixture bytes (magic/version/flags, cả case hỏng).
- **Smoke thủ công:** `doctor` + `status` round-trip trên từng máy thật (cần
  desktop session, CI không làm được).
- **CI:** build matrix macOS + Windows + clippy + fmt — bắt vỡ compile sớm, đúng
  vai trò eval-check bên `~/.nix`.

## Rủi ro & đối sách

| Rủi ro | Đối sách |
|--------|----------|
| GoNhanh/VKey đổi nội bộ theo version (key defaults, layout shm) | doctor kiểm version + magic/structVersion trước khi tin; shm VKey đóng băng layout v4 bằng static_assert; endgame là kênh chính thức qua PR upstream |
| Chuyển sang Pinyin từ nền không ăn | mẹo retry của macism |
| VKey chạy admin nuốt message | doctor phát hiện, hướng dẫn tắt `run_as_admin`; fallback ghi shm |
| `smart_switch` / `perAppMode` giành lái | doctor cảnh báo + hướng dẫn tắt — đúng loại "lỗi im lặng" tool sinh ra để bắt |

## Quyết định đã chốt (lịch sử brainstorm)

1. Nền tảng v1: macOS + Windows. Tập mode: mac vi/en/zh, win vi/en.
2. CLI là nguồn chân lý; hotkey gọi CLI (khuôn beckon).
3. Windows chỉ cần điều khiển tại chỗ (không cần IPC xuyên SSH session).
4. Repo độc lập, chưa đụng `~/.nix`; tích hợp là việc sau, của chủ repo.
5. Rust, một codebase, theo khuôn flake/overlay của beckon.
6. Kiến trúc: B (điều khiển in-process) + fallback A (chạy/kill process) — thực tế
   ra: Windows đạt B thuần, macOS v1 dùng A có định hướng nâng cấp.
7. Auto per-app ngoài phạm vi; các lớp auto hiện có sẽ gọi CLI sau, tự chủ repo nối.
