# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## tongue là gì

CLI Rust chuyển chế độ gõ `vi | en | zh` bằng MỘT thao tác nguyên tử: đổi cả
layout hệ thống lẫn bộ gõ ngoài (GoNhanh trên macOS, VKey trên Windows), có
verify sau khi áp. Không daemon, không state file — nguồn chân lý luôn là hệ
thống. Thiết kế đầy đủ kèm bằng chứng file:line từ source hai bộ gõ:
`docs/superpowers/specs/2026-07-29-tongue-design.md`.

Exit code: `0` = đích đã verify khớp · `1` = verify trượt (`VerifyFailed`) ·
`2` = lỗi môi trường (kể cả `tongue zh` trên Windows).

## Lệnh kiểm tra — chạy đủ TRƯỚC khi push

```bash
cargo test                                                    # 63 unit test + 2 smoke ignored
cargo fmt --check
cargo clippy --all-targets -- -D warnings                     # macOS
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings   # BẮT BUỘC
nix build .#tongue && ./result/bin/tongue --help              # đúng cái CI làm
```

Lệnh clippy target Windows là bắt buộc vì đã có tiền lệ: code cfg(macos) từng
làm CI Windows đỏ mà máy mac không hề hay biết (dead-code khác nhau giữa hai
target). `cargo check --target x86_64-pc-windows-msvc` không cần linker MSVC —
chạy được ngay trên mac (`rustup target add x86_64-pc-windows-msvc` một lần).

CI (`.github/workflows/ci.yml`): matrix macos + windows, đủ 4 gate trên, upload
artifact `tongue-windows` (tongue.exe) — nguồn binary cho máy win không cài Rust.

### Smoke thật (thay cho integration test)

- **macOS (máy này):** `cargo run -q -- status|en|vi|zh` — ĐỔI chế độ gõ thật
  của máy. Luôn kết thúc bằng `cargo run -q -- vi` để khôi phục. `tongue doctor
  --fix` lần đầu trên máy mới.
- **Windows (a14-win):** phải chạy từ terminal NGỒI TRỰC TIẾP máy — qua ssh sẽ
  trượt vì phiên SSH khác window station, `FindWindow("VKeyTrayClass")` không
  thấy. Trình tự trong plan Task 8 Step 5 + Task 10 Step 4.

## Kiến trúc

Mỗi mode là một **trạng thái đích khai báo** `(layout, bit IME)`; chuyển mode =
reconcile: đọc trạng thái thật → áp phần lệch → poll verify tới timeout.

Bộ gõ nào lo tiếng Việt là **tuỳ chọn cấu hình**, không phải hằng số trong code:
`[macos] backend = "gonhanh" | "app" | "system"` (mặc định `gonhanh`).

| Mode | mac `gonhanh`/`app` | mac `system` | Windows: layout | Windows: VKey |
|------|---------------------|--------------|-----------------|---------------|
| `vi` | ABC + app bật       | Telex, không app | US (không đụng) | bật        |
| `en` | ABC + app tắt       | ABC, không app   | US (không đụng) | tắt        |
| `zh` | Pinyin + app tắt    | Pinyin           | —               | —          |

Hai cột mac là hai mô hình khác nhau: bộ gõ ngoài giữ nguyên layout và dùng **bit
IME** phân biệt vi/en (`source_vi == source_en`), còn bộ gõ hệ thống dùng chính
**layout** để phân biệt (`source_vi != source_en`, bit IME luôn tắt). `source_en`
mặc định trùng `source_vi` nên mô hình cũ là trường hợp riêng, không cần cấu hình.

Cột `gonhanh`/`app` ở trên gộp chung hai strategy khác nhau của backend
`gonhanh`: `process` (mặc định — vi = app chạy, en = app bị SIGTERM) và `hotkey`
(app luôn chạy, vi/en phân biệt bằng bit `gonhanh.enabled` đổi qua chord toggle).
Xem `[macos] strategy` và mục bất biến bên dưới.

```
src/mode.rs        # bảng mode → Desired {layout, ime_on} + struct Sources — thuần
src/reconcile.rs   # vòng áp + verify sau 2 trait; VerifyFailed → exit 1
src/backend/
  mod.rs           # trait Layout {current, select}, trait Ime {is_on, set, diagnose}; NoopLayout (win)
  vkey_shm.rs      # parser bytes shared memory VKey — thuần, test fixture mọi OS
  macos/tis.rs     # FFI TIS API (link framework Carbon) — đổi input source
  macos/app.rs     # helper pgrep/open/killall + AppIme generic (EVKey, OpenKey...)
  macos/gonhanh.rs # AppIme + defaults write; diagnose() ôm luôn bẫy perAppMode
  macos/chord.rs   # parser chord toggle GoNhanh (thuần) — test không cần máy thật
  macos/prefs.rs   # đọc defaults qua CFPreferences (chỉ đọc)
  macos/hotkey.rs  # strategy hotkey: giữ app sống, đổi mode bằng chord
  macos/system.rs  # SystemIme: không app ngoài, tiếng Việt từ input source macOS
  windows/vkey.rs  # FindWindow + PostMessage + shared memory; diagnose() ôm 3 check VKey
src/config.rs      # ~/.config/tongue/config.toml (mac) / %APPDATA%\tongue (win); vắng file = default
src/status.rs      # suy mode từ trạng thái thật; render human/json
src/doctor.rs      # in Finding; phần khám riêng nằm ở diagnose() của backend
src/main.rs        # clap; make_ime/make_layout/snapshot cfg-gate theo OS
```

## Bất biến — đổi là hỏng, đọc kỹ trước khi sửa

**Hằng số là API ngoại lai, chép verbatim từ source hai bộ gõ** (spec có
file:line): VKey magic `0x5945_4B4E`, structVersion ≤ 4, flags @offset 16 bit
`0x0001`, message `WM_USER+100`, class `VKeyTrayClass`, section
`Local\VKeySharedState`; GoNhanh domain `org.gonhanh.GoNhanh`, key
`gonhanh.enabled`/`gonhanh.perAppMode`; source `com.apple.keylayout.ABC` /
`com.apple.inputmethod.SCIM.ITABC`.

**GoNhanh không có kênh IPC lúc đang chạy** — defaults chỉ được đọc lúc khởi
động, nên bit bật/tắt = sự tồn tại của process (ghi `gonhanh.enabled=1` TRƯỚC
khi `open`). Bẫy: `perAppMode` mặc định bật làm key `enabled` thành đồ giả —
`doctor --fix` ghim nó về 0. Muốn "đổi không cần restart" thì đi đường nâng cấp
trong spec (giả lập chord hotkey / PR upstream), đừng chế kênh mới.

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

**VKey KHÔNG BAO GIỜ bị kill trong luồng chuyển thường** — kill/restart là xáo
hook `WH_KEYBOARD_LL`, hồi sinh đúng lỗi tranh hook với kanata mà tool này sinh
ra để diệt. Set mode qua `WM_VKEY_SET_MODE` (idempotent, sticky với
smart-switch vì đi đúng đường hotkey); ghi thẳng shared memory thì KHÔNG sticky
— focus change sẽ ghi đè. `VKeyClassic.exe` là peer bình đẳng (cùng window
class, cùng shm).

**reconcile re-áp phần lệch mỗi vòng poll** — đó chính là mẹo retry macism cho
ca TISSelectInputSource với CJK, không phải code thừa. Deadline verify reset
sau lượt áp đầu tiên (lượt đầu có thể block ~5s chờ VKey cold-start) — bỏ
reset là `tongue vi` đầu tiên trên win exit 1 giả.

**doctor --fix restart GoNhanh phải chờ process chết thật** giữa `set(false)`
và `set(true)` — killall trả về khi SIGTERM được GỬI, không phải khi process
chết; bỏ vòng chờ là "restart" thành "giết hẳn". Vòng chờ đó nay nằm ở
`GonhanhIme::restart`.

**`SystemIme::is_on()` phải LUÔN trả false**, kể cả khi có app bộ gõ khác đang
chạy. reconcile dùng nó làm đích: báo true là chờ một cần gạt vĩnh viễn không
nhúc nhích rồi trả `VerifyFailed` giả. Việc phát hiện "còn app chạy chồng" là
của `diagnose()` — chẩn đoán môi trường, không phải trạng thái đích. Cùng lý do,
`status` KHÔNG suy drift từ `ime_on` ở nhánh `source_vi != source_en`.

**Thêm bộ gõ mới = thêm một file + một nhánh `match` trong `make_ime`.** Ba
đường (switch, snapshot, doctor) đều đi qua đúng một cửa đó; phần khám riêng
nằm trong `diagnose()` của chính backend, không rải ra `doctor.rs`. Bộ gõ nào
chỉ cần bật/tắt bằng process thì KHÔNG cần code mới — đặt `backend = "app"` và
`app_name` là xong.

Shell-out dùng tên lệnh trần (`pgrep`, `defaults`, `open`, `killall`) — không
hardcode đường dẫn tuyệt đối.

**Trên Windows, session 0 là vùng chết — phải chặn, không được đoán.** Window
station (`FindWindow`) và namespace `Local\` (`OpenFileMapping`) đều theo session,
nên từ SSH/service (session 0) VKey của người dùng vừa không đọc được vừa không
điều khiển được. Kiểu hỏng rất tệ nếu không chặn: `read_state()` thấy section
trống → kết luận "VKey chưa chạy" → `set(true)` spawn một VKey THỨ HAI trong
session 0, không hook được gì, chỉ làm rác và khiến `status` báo trạng thái tưởng
tượng. Nay `in_service_session()` chặn ở cả `read_state()` lẫn `ensure_running()`.
Muốn test từ xa thì chạy qua scheduled task `-LogonType Interactive`.

**Spawn process con trên Windows phải qua `spawn_no_inherit` (CreateProcessW với
`bInheritHandles = FALSE`), KHÔNG dùng `std::process::Command`.** Trên Windows,
"cắt stdio" và "cắt thừa kế handle" là hai việc khác nhau: `Stdio::null()` chỉ đặt
ba std handle của con thành NUL, còn Rust vẫn gọi `CreateProcessW` với
`bInheritHandles = TRUE`, mà Windows thì thừa kế TẤT CẢ handle đang bật cờ
inheritable của cha. Nên pipe stdout do caller tạo vẫn đi sang VKey, VKey sống lâu
hơn tongue, và bên đọc treo vô hạn dù tongue đã thoát. Đã đo A/B trên a14-win,
cold-start thật: bản `Stdio::null()` → tongue thoát sau 184ms, EOF **không** tới
trong 12s; bản `bInheritHandles=FALSE` → EOF tới đúng lúc thoát, 193ms.

**`run_as_admin` KHÔNG làm shared memory đọc trượt** — đã nghiệm chứng 30/07/2026
trên a14-win: VKey chạy elevated (High IL), tongue chạy Medium, `OpenFileMappingW`
vẫn trả HANDLE HỢP LỆ (mandatory integrity chặn ghi-lên, không chặn đọc-lên), và
`FindWindow` vẫn thấy cửa sổ (UIPI chặn message, không chặn liệt kê). Nghĩa là
`Ok(None) = VKey không chạy` vẫn là suy luận đúng, không cần phân biệt
ERROR_FILE_NOT_FOUND với ERROR_ACCESS_DENIED. Chỗ duy nhất hỏng là `PostMessage`
bị UIPI nuốt → verify của reconcile trượt → exit 1 sau ~1s kèm gợi ý chạy doctor,
và doctor chỉ đúng thủ phạm. Đúng như thiết kế, không phải sửa gì.

## Git

- Commit message tiếng Việt, dạng `<phạm vi>: <nội dung>`.
- KHÔNG thêm `Co-Authored-By`.

## Ngoài phạm vi hiện tại (đã ghi trong spec, đừng tự làm)

Auto-switch theo app, daemon/hotkey, Linux, tích hợp `~/.nix` (flake input +
hotkey gọi CLI như beckon — chủ repo tự nối), strategy `notify` cho GoNhanh,
fallback ghi shm khi VKey chạy admin.
