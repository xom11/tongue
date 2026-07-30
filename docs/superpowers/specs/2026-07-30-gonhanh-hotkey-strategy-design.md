# Strategy `hotkey` cho backend GoNhanh — bỏ cold-start khi bật tiếng Việt

Ngày: 2026-07-30 · Trạng thái: thiết kế đã chốt, chưa cài đặt
Tiền đề: `2026-07-29-tongue-design.md` §"Lộ trình nâng cấp backend GoNhanh", mục 2.

## Vấn đề

`strategy = "process"` dùng SỰ TỒN TẠI CỦA PROCESS làm bit bật/tắt: `tongue en`
gửi SIGTERM cho GoNhanh, `tongue vi` `open -ga` lại. Vì `en` **luôn** giết app nên
`vi` kế tiếp **luôn** là cold-start — 200–400ms không phải trường hợp hiếm mà là
mọi lần quay lại tiếng Việt. Kèm theo đó app mất hết trạng thái nội bộ mỗi vòng.

Mục tiêu: giữ GoNhanh sống liên tục, đổi vi/en bằng chính chord toggle mà app đã
đăng ký. Không đổi giao diện CLI, không đổi exit code, không đụng backend khác.

## Bằng chứng đo trên máy thật (macOS, GoNhanh 1.0.157, 30/07/2026)

Bốn kết luận dưới đây là **đo được**, không phải suy đoán. Script còn trong
scratchpad phiên thiết kế; số liệu chép lại đây vì chúng định hình cả thiết kế.

1. **Chord thật** đọc từ `defaults`: `gonhanh.shortcut.toggle` =
   `{"keyCode":49,"modifiers":393216}` → keyCode 49 = Space, 393216 = 0x60000 =
   Shift|Control → **Ctrl+Shift+Space**.
2. **`CGEventPost(.cghidEventTap)` tới được GoNhanh.** Bắn chord → app đổi mode.
   `AXIsProcessTrusted` = true khi chạy dưới kitty đã được cấp Accessibility.
   (`osascript` thì không — tap của GoNhanh ở HID level bỏ qua nó, xem spec gốc.)
3. **GoNhanh tự ghi `gonhanh.enabled` mỗi lần toggle** → readback khả thi, mô hình
   reconcile giữ nguyên. PID **không** đổi (37506 trước và sau) → app không restart.
4. **Độ trễ readback: 87 / 152 / 225 / 286 ms** qua 4 lần đo. Đây là con số quan
   trọng nhất của cả thiết kế — xem §"Bất biến mới".

Hai phát hiện phụ, đều buộc phải dùng CFPreferences thay vì shell-out:

- `defaults read <domain> <key>` **cắt ngắn** blob data thành
  `{length = 33, bytes = 0x7b22... ... 7d}` → không parse được chord.
  `CFPreferencesCopyAppValue` trả về trọn 33 byte.
- Chi phí đọc: CFPreferences **0.01ms**, shell-out `defaults` **66.5ms**. Với
  `poll_ms = 50`, shell-out tốn nhiều thời gian hơn cả một chu kỳ poll.

### Smoke thật sau khi cài đặt xong (Task 6, 30/07/2026)

Chạy `tongue` build từ worktree `sdd/gonhanh-hotkey` (HEAD `823a7e5`), config
`~/.config/tongue/config.toml` có `strategy = "hotkey"`. GoNhanh PID trước khi
bắt đầu: **37506**.

| Lệnh | PID GoNhanh sau | thời gian (binary trực tiếp) | exit |
|------|------------------|-------------------------------|------|
| `tongue en`     | 37506 (không đổi) | 237ms | 0 |
| `tongue vi`     | 37506 (không đổi) | 237ms | 0 |
| `tongue zh`     | 37506 (không đổi) | 273ms | 0 |
| `tongue vi` (2) | 37506 (không đổi) | 267ms | 0 |

PID **giống hệt 37506 xuyên suốt cả bốn lệnh** — app không hề bị kill/relaunch,
đúng mục tiêu của thiết kế. `status` báo đúng mode ở mọi bước (`vi` → `zh` →
`vi`), layout đổi đúng `com.apple.keylayout.ABC` ↔ `com.apple.inputmethod.SCIM.ITABC`.

237–273ms **không phải cold-start theo nghĩa cũ** (kill+relaunch process, việc
này không còn xảy ra — PID đứng yên) mà là thời gian `set()` tự chờ xác nhận
`gonhanh.enabled` qua chord relay, đúng tầm 87–286ms đã đo ở trên. Mục tiêu "bỏ
cold-start 200–400ms do relaunch app" đã đạt; độ trễ còn lại là chi phí relay
cố hữu của cơ chế chord, không phải chi phí khởi động lại tiến trình.

## Vấn đề cốt lõi: chord là relay, `reconcile` giả định `set()` idempotent

`reconcile` poll mỗi 50ms và gọi lại `ime.set()` mỗi vòng còn lệch
(`reconcile.rs:82-84`). Với `process` điều đó vô hại — `launch`/`terminate` đều
idempotent. Với chord thì không: sau khi bắn, `enabled` mất 87–286ms mới phản ánh,
nên `reconcile` sẽ thấy trạng thái cũ trong 2–6 vòng poll và **bắn thêm 2–6 chord
nữa**, lật mode qua lại. Đây là hỏng chắc chắn, không phải rủi ro xa.

**Cách giải đã chọn: `set()` tự chờ xác nhận rồi mới trả về.** Bắn chord → poll
`enabled` tới khi đổi hoặc hết ngân sách → mới return. `reconcile` chỉ nhìn thấy
một lời gọi, không còn cửa sổ nào để bắn trùng.

Vì sao không debounce bằng hằng số thời gian trong backend: phải tự bịa ra một
hằng số mới, và chord trượt thật thì mất trọn cửa sổ đó mới thử lại. Vì sao không
sửa trait `Ime` để phân biệt relay: trait đó dùng chung cho cả Windows lẫn
`system`, sửa nó phá bất biến "thêm bộ gõ mới = thêm một file + một nhánh `match`".

Cách đã chọn khớp với tiền lệ sẵn có: `VkeyIme::ensure_running` cũng block tới 5s
ngay trong `set()`, và cơ chế reset deadline sau lượt apply đầu
(`reconcile.rs:85-90`) sinh ra chính là để đỡ ca đó. **Ngân sách chờ dùng lại
`verify.timeout_ms`**, không đẻ thêm hằng số cấu hình mới.

## Kiến trúc

Ba file mới, đều `cfg(target_os = "macos")`, cộng vài chỗ nối:

```
src/backend/macos/
  chord.rs    # THUẦN: blob JSON -> Chord{key_code, flags}; render tên người đọc
  prefs.rs    # bọc CFPreferences: read_bool / read_data (có synchronize)
  hotkey.rs   # HotkeyIme: is_on / set / diagnose — logic tách khỏi FFI để test
```

### `chord.rs` — thuần, test được không cần máy thật

```rust
pub struct Chord { pub key_code: u16, pub flags: u64 }
pub fn parse(blob: &[u8]) -> Result<Chord>   // {"keyCode":49,"modifiers":393216}
pub fn describe(c: &Chord) -> String          // "Ctrl+Shift+Space"
```

Điểm dễ sai, ghi rõ để khỏi tra lại: **NSEvent modifier flags và `CGEventFlags`
trùng bit nhau** — CapsLock 1<<16, Shift 1<<17, Control 1<<18, Option 1<<19,
Command 1<<20. Nên chuyển đổi là identity; chỉ cần **mask `0x001F_0000`** để bỏ
16 bit device-dependent thấp và các bit lạ. Không có bảng ánh xạ nào cả.

`describe` chỉ cần map một nhúm keyCode thông dụng (Space, các chữ cái, F1–F12);
phím lạ in thẳng `keyCode N`. Nó chỉ phục vụ `doctor`, không nằm trên đường switch.

### `prefs.rs` — đọc defaults trong tiến trình

```rust
pub fn read_bool(domain: &str, key: &str) -> Option<bool>
pub fn read_data(domain: &str, key: &str) -> Option<Vec<u8>>
```

Mỗi lần đọc gọi `CFPreferencesAppSynchronize` trước `CFPreferencesCopyAppValue` —
đã nghiệm chứng là thấy được thay đổi do tiến trình khác ghi. Chỉ ĐỌC; việc ghi
`enabled` vẫn để `defaults write` như `gonhanh.rs` đang làm, không refactor lan man.

### `hotkey.rs` — `HotkeyIme`

`is_on()` = **app đang chạy VÀ `enabled == 1`**. Khác `process` (chỉ `pgrep`), vì
giờ app sống không còn đồng nghĩa đang gõ tiếng Việt. Vế `pgrep` không bỏ được:
app chết thì `enabled` còn sót `1` trong defaults cũng chẳng gõ được gì.

`set(on)` theo ba nhánh:

| Tình huống | Hành động |
|---|---|
| `on=true`, app chưa chạy | `defaults write enabled=1` + `open -ga` + chờ `is_on()` trong `verify.timeout_ms` — **không** chord (app khởi động đã ở trạng thái bật) |
| app đang chạy, lệch | bắn chord → poll `enabled` mỗi `poll_ms` tới khi đổi hoặc hết `verify.timeout_ms` |
| `on=false`, app chưa chạy | no-op (đã là `en`) |

Nhánh đầu chính là chỗ cold-start còn sót lại — chỉ một lần sau mỗi lần boot.

**Bắn tối đa MỘT chord cho mỗi lần chạy `tongue`.** `set()` nhớ trong
`Cell<bool>` rằng đã bắn; nếu đã bắn và đã chờ hết ngân sách mà không xác nhận
được thì các lời gọi `set()` sau chỉ trả `Ok(())` chứ không bắn nữa. Không có
chốt này thì `reconcile` — vốn reset deadline sau lượt apply đầu — sẽ gọi `set()`
thêm lượt nữa và bắn chord thứ hai; nếu chord thứ nhất thật ra có ăn nhưng chậm
bất thường (quá 1000ms, tức hơn 3,5× mức tệ nhất đã đo là 286ms) thì cú thứ hai
lật ngược mode. `tongue` là CLI một phát, một lần chạy = một lần chuyển mode, nên
"tối đa một chord" là đúng ngữ nghĩa chứ không phải chống chế.

Để logic trên test được, tách FFI ra sau hai trait nội bộ:

```rust
trait ChordSender { fn send(&self) -> Result<()>; }
trait StateSource { fn running(&self) -> Result<bool>; fn enabled(&self) -> Result<Option<bool>>; }
```

Impl thật dùng `CGEvent` + `prefs.rs`; test dùng fake mô phỏng đúng độ trễ đã đo.

`diagnose()` gồm: bundle tồn tại (dùng lại `app::diagnose_bundle`), `perAppMode`
(tách phần này khỏi `GonhanhIme::diagnose` thành hàm `pub(crate)` dùng chung),
`AXIsProcessTrusted()`, và chord đọc/parse được — in ra dạng người đọc.

Findings quan trọng nhất là Accessibility, vì đó là kiểu hỏng khó đoán nhất:
quyền cấp cho **tiến trình chủ**, không cho binary `tongue`. Chạy từ kitty thì
kitty phải có quyền; gọi từ Hammerspoon thì Hammerspoon phải có. Thông điệp phải
nói thẳng điều đó, không chỉ "thiếu quyền".

### Chỗ nối

- `Cargo.toml`: thêm `serde_json` và `core-graphics` vào khối
  `cfg(target_os = "macos")`. `AXIsProcessTrusted` khai FFI tay, link framework
  `ApplicationServices` (cùng kiểu `tis.rs` link `Carbon`).
- `main.rs::make_ime`: bỏ `ensure!(strategy == "process")`, match trên cặp
  `(backend, strategy)`. `hotkey` chỉ hợp lệ với `backend = "gonhanh"`; cặp khác
  báo lỗi nói rõ vì sao. `HotkeyIme` nhận `verify.timeout_ms` và `poll_ms` lúc dựng.
- `doctor.rs` mục 4: chấp nhận `hotkey` bên cạnh `process`.
- `config.rs`: không đổi struct — field `strategy` đã có sẵn, chỉ đổi doc comment.

Mặc định vẫn là `process`. `hotkey` là opt-in vì nó đòi quyền Accessibility, còn
`process` không đòi gì.

## Xử lý lỗi

Chord bắn xong mà `enabled` không đổi trong ngân sách → `set()` trả `Ok(())`, để
`reconcile` phát hiện lệch và trả `VerifyFailed` → **exit 1** kèm gợi ý chạy
`doctor`, đúng bất biến hiện có. Không tự fallback sang kill/launch: nó sẽ giết
app bất ngờ và che mất lỗi cấu hình thật (Accessibility bị thu hồi, chord bị đổi
trong Settings, app treo). `doctor` mới đủ sức chỉ đúng thủ phạm trong cả ba ca.

## Test

**Unit (`cargo test`, không chạm hệ thống):**

- `chord.rs`: parse JSON hợp lệ; JSON hỏng; thiếu field; `modifiers` có bit lạ →
  mask đúng; `describe` cho Space và cho keyCode lạ.
- `hotkey.rs` qua fake — đây là nhóm test quan trọng nhất, nó khoá đúng lỗi mà
  thiết kế này sinh ra để tránh:
  - **bắn đúng MỘT chord** dù `enabled` chỉ đổi sau nhiều vòng poll (mô phỏng
    286ms) — hồi quy trực tiếp cho vấn đề cốt lõi;
  - chord trượt hẳn → `set()` trả về sau đúng ngân sách, không treo;
  - chord trượt rồi `reconcile` gọi `set()` lượt nữa → **không** bắn chord thứ
    hai (khoá chốt "tối đa một chord mỗi lần chạy");
  - `on=true` + app chưa chạy → đi nhánh launch, **không** bắn chord;
  - `on=false` + app chưa chạy → no-op;
  - `is_on()` = false khi app chết dù `enabled == 1`.

**Smoke thật (tay, trên máy này):** `cargo run -- en` rồi `vi` với
`strategy = "hotkey"`; kiểm PID GoNhanh **không đổi** qua cả hai lệnh, và đo thời
gian `vi` — phải không còn cold-start. Kết thúc bằng `tongue vi`.

**Gate bắt buộc trước push** giữ nguyên 4 lệnh trong CLAUDE.md, gồm cả
`cargo clippy --target x86_64-pc-windows-msvc` — file mới đều cfg-gate macOS nên
đây đúng là loại thay đổi từng làm CI Windows đỏ.

## Bất biến mới (sẽ ghi vào CLAUDE.md khi cài đặt xong)

1. **Chord toggle là RELAY, không idempotent — `set()` phải tự chờ xác nhận rồi
   mới trả về.** `reconcile` gọi lại `set()` mỗi vòng poll 50ms, mà `enabled` mất
   87–286ms mới phản ánh; trả về sớm là bắn trùng 2–6 chord và lật mode qua lại.
2. **Đọc defaults của GoNhanh trên đường nóng phải qua CFPreferences, không
   shell-out.** `defaults read` cắt ngắn blob data (chord không parse được) và tốn
   66.5ms/lần — nhiều hơn cả một chu kỳ poll.
3. **Accessibility cấp cho tiến trình CHỦ, không cho `tongue`.** Mỗi nơi gọi
   tongue (kitty, Hammerspoon) là một app phải cấp riêng.
4. **`is_on()` của `hotkey` là `pgrep` VÀ `enabled`**, không phải một trong hai.

## Ngoài phạm vi

Giữ nguyên các mục đã ghi trong spec gốc. Riêng với việc này: không làm strategy
`notify` (PR upstream), không đổi mặc định sang `hotkey`, không refactor
`gonhanh.rs` sang CFPreferences cho đường ghi, không đụng backend `app`/`system`.

Một ẩn số cố ý để lại: defaults có key `gonhanh.enable` (không có "d") luôn `= 1`
và **không** đổi khi toggle — vai trò chưa rõ. `gonhanh.enabled` là nguồn chân lý,
đúng như code hiện tại đang dùng. Ghi lại đây để lần sau khỏi tưởng là gõ nhầm.
