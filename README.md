# tongue

CLI chuyển chế độ gõ `vi | en | zh` bằng **một thao tác duy nhất** — đổi cả layout
bàn phím hệ thống lẫn bộ gõ tiếng Việt, rồi *kiểm tra lại* xem máy có thật sự đổi
chưa mới báo thành công.

    tongue vi        # tiếng Việt
    tongue en        # tiếng Anh
    tongue zh        # tiếng Trung (chỉ macOS)
    tongue           # in mode hiện tại: "vi"
    tongue status    # chi tiết (--json cho script)
    tongue doctor    # khám môi trường (--fix sửa những gì an toàn)

macOS dùng [GoNhanh] (mặc định), bộ gõ ngoài bất kỳ, hoặc bộ gõ tiếng Việt có sẵn
của macOS. Windows dùng [VKey].

## Vì sao cần

Đổi chế độ gõ trên macOS thực ra là hai việc rời nhau: layout bàn phím của hệ
thống, và bật/tắt bộ gõ tiếng Việt. Hai thứ đó lệch nhau lúc nào không biết — và
bạn chỉ phát hiện sau khi đã gõ ra một dòng chữ hỏng.

`tongue` gộp chúng thành một trạng thái đích duy nhất, áp xong thì đọc lại trạng
thái thật của máy để xác nhận. Không daemon, không file state: nguồn chân lý luôn
là hệ thống, nên không bao giờ có chuyện "tool tưởng đang ở `vi`".

## Cài đặt

**Nix (flake)** — cách được hỗ trợ chính, chỉ macOS (`aarch64-darwin`,
`x86_64-darwin`):

```nix
{
  inputs.tongue.url = "github:xom11/tongue";
}
```

Rồi lấy package theo một trong hai cách:

```nix
# a) trực tiếp
home.packages = [ inputs.tongue.packages.${pkgs.system}.tongue ];

# b) qua overlay, để dùng được `pkgs.tongue` ở mọi nơi
nixpkgs.overlays = [ inputs.tongue.overlays.default ];
home.packages = [ pkgs.tongue ];
```

**Không dùng nix:**

```bash
cargo install --path .        # → ~/.cargo/bin/tongue
```

**Windows:** tải `tongue.exe` từ artifact `tongue-windows` của CI — không cần cài
Rust trên máy đó.

## Lần đầu chạy

```bash
tongue doctor --fix
```

Đọc kết quả theo icon: `✓` ổn · `⚠` nên xử lý nhưng vẫn chạy được · `✗` phải sửa
mới dùng được. Những gì `--fix` **không** tự làm được (như bật input source trong
System Settings) sẽ được nói rõ trong thông báo.

## Exit code

Đây là hợp đồng với script và hotkey gọi tới:

| Code | Nghĩa | Nên làm gì |
|------|-------|-----------|
| `0` | đích đã verify khớp | không cần làm gì |
| `1` | `VerifyFailed` — lệnh đã gửi nhưng máy không nhúc nhích | chạy `tongue doctor` |
| `2` | lỗi môi trường: thiếu app, input source chưa bật, config sai, `tongue zh` trên Windows | đọc thông báo, thường nói rõ thiếu gì |

Các lệnh đều **idempotent**: gọi `tongue vi` năm lần liên tiếp cho cùng một kết
quả và không sinh thêm process nào — bấm nhầm hotkey là vô hại.

## Cấu hình

Vắng file config thì mọi thứ chạy với mặc định (GoNhanh trên macOS, VKey trên
Windows). Muốn khác thì tạo:

- macOS: `~/.config/tongue/config.toml`
- Windows: `%APPDATA%\tongue\config.toml`

### Chọn bộ gõ (macOS)

Bộ gõ nào lo tiếng Việt là **tuỳ chọn cấu hình, không phải sửa code**:

```toml
# Bộ gõ ngoài bất kỳ chỉ cần bật/tắt bằng process — EVKey, OpenKey...
[macos]
backend = "app"
app_name = "EVKey"
```

```toml
# Bộ gõ tiếng Việt có sẵn của macOS: không cài gì thêm, không process chạy nền
[macos]
backend = "system"
source_vi = "com.apple.inputmethod.VietnameseIM.VietnameseTelex"
```

| `backend` | Cơ chế | Ghi chú |
|-----------|--------|---------|
| `gonhanh` (mặc định) | ghi defaults + `open` / `killall` | khám luôn bẫy `perAppMode` |
| `app` | `open` / `killall` / `pgrep` | chỉ cần `app_name`, không đụng defaults của ai |
| `system` | đổi input source, không có app ngoài | nhanh nhất, không có process nền |

Hai mô hình này khác nhau ở chỗ **cái gì phân biệt `vi` với `en`**: bộ gõ ngoài
giữ nguyên layout ABC và dùng bit bật/tắt của app, còn bộ gõ hệ thống thì chính
layout là thứ phân biệt. Vì vậy `backend = "system"` cần `source_vi` khác
`source_en`; `tongue doctor` sẽ báo `✗` nếu bạn để chúng trùng nhau (lúc đó
`tongue vi` và `tongue en` sẽ không làm gì cả).

`doctor` cũng cảnh báo nếu hai bộ gõ chạy chồng lên nhau — ví dụ đã chuyển sang
`backend = "system"` nhưng GoNhanh vẫn còn trong Login Items.

### Strategy hotkey (chỉ backend gonhanh)

Mặc định `strategy = "process"` kill/launch GoNhanh mỗi lần đổi mode. `strategy
= "hotkey"` giữ app sống liên tục và đổi vi/en bằng chính chord toggle
(mặc định Ctrl+Shift+Space) mà GoNhanh đã đăng ký — không kill/launch, không
cold-start.

Đòi quyền **Accessibility** — nhưng cấp cho TIẾN TRÌNH CHỦ gọi `tongue` (app
terminal, Hammerspoon...), không phải cho binary `tongue`. Thiếu quyền thì
chord không tới được GoNhanh; `tongue doctor` báo `✗` và nói rõ cấp quyền cho ai.

### Tất cả tuỳ chọn

```toml
[macos]
backend   = "gonhanh"                            # gonhanh | app | system
strategy  = "process"                            # process | hotkey (hotkey cần backend = "gonhanh")
app_name  = "GoNhanh"                            # tên app cho backend gonhanh/app
source_vi = "com.apple.keylayout.ABC"
source_en = "com.apple.keylayout.ABC"            # mặc định trùng source_vi
source_zh = "com.apple.inputmethod.SCIM.ITABC"

[windows]
vkey_path = ""                                   # rỗng = tự dò trong WinGet Packages

[verify]
timeout_ms = 1000                                # tối đa chờ máy đổi xong
poll_ms    = 50                                  # nhịp đọc lại trạng thái
```

Thiếu khoá nào thì khoá đó lấy mặc định — không cần chép cả khối.

Không chắc ID input source? Liệt kê những cái đang bật:

```bash
defaults read com.apple.HIToolbox AppleEnabledInputSources
```

## Nối vào phím tắt

`tongue` cố ý **không** có daemon hay hotkey — nó chỉ là CLI, việc bind phím dành
cho công cụ bạn đã dùng (kanata, skhd, Karabiner, Hammerspoon…). Ví dụ với skhd:

```
cmd - space        : tongue vi
cmd + shift - space: tongue en
```

Vì lệnh idempotent và không có state, bạn có thể gọi từ nhiều nguồn cùng lúc mà
không sợ lệch nhau.

## Cách nó hoạt động

Mỗi mode là một **trạng thái đích khai báo** `(layout, bit bộ gõ)`. Chuyển mode =
reconcile: đọc trạng thái thật → áp đúng phần đang lệch → đọc lại tới khi khớp
hoặc hết `timeout_ms`.

```
tongue vi
  ├─ đọc config (vắng file = mặc định)
  ├─ tra bảng mode → Desired { layout, ime_on }
  └─ vòng reconcile:
       đọc layout + bit bộ gõ thật
       khớp cả hai?  → exit 0
       quá hạn?      → VerifyFailed, exit 1
       áp phần lệch, chờ poll_ms, lặp lại
```

Việc áp lại mỗi vòng là có chủ ý: `TISSelectInputSource` với input source CJK đôi
khi nhận lệnh nhưng chưa đổi ngay, nên retry chính là cách xử lý.

Thời gian thực đo trên macOS: ~50ms khi trạng thái đã đúng sẵn (thoát ngay vòng
đầu), ~90ms với `backend = "system"`, ~150–240ms khi phải bật/tắt process thật.

## Chạy qua SSH trên Windows: `tongue agent`

SSH của Windows chạy như một service, nên shell nó sinh ra nằm ở **session 0**.
Hai cơ chế tongue dùng để lái VKey đều là tài nguyên **theo session**: window
station (`FindWindow`) và namespace `Local\` (`OpenFileMapping`, thực chất
`Session\<n>\`). Nên `ssh may tongue vi` vốn từ chối thẳng — nó gọi đúng tên
nhưng ra nhầm đối tượng.

`\\.\pipe\` thì **không** theo session: một namespace duy nhất cho cả máy, và là
cơ chế Microsoft dựng cho đúng việc "service nói chuyện với app desktop". Nên
`tongue agent` sống trong session của người dùng, còn mọi lần gọi từ session 0
tự chuyển tiếp vào đó — không đổi lệnh, không thêm cờ.

**Khởi động lười, không phải daemon.** Client ở session 0 tự chạy
`schtasks /run /tn <agent_task>` khi không thấy pipe, và agent tự thoát sau
`agent_idle_ms`. Một scheduled task, không watchdog: đây là request/response vài
lần một ngày do người gõ, không phải thứ phục vụ từng phím bấm. Agent cũng khoá
cứng `tongue.exe` khi chạy, nên tự thoát là thứ giữ cho việc nâng cấp khỏi vướng.

```toml
[windows]
agent_task       = "\\TongueAgent"   # task mà client sẽ gọi khi không thấy pipe
agent_timeout_ms = 15000            # phải LỚN hơn ca xấu nhất, xem dưới
agent_idle_ms    = 600000
```

`agent_timeout_ms` **không** phải 2000 như phản xạ tự nhiên: `ensure_running` chờ
VKey cold-start tới 5000 ms rồi `reconcile` còn ăn thêm `verify.timeout_ms`. Đặt
2000 là báo thất bại cho một lần chuyển thành công bốn giây sau đó.

Bảo mật, vì đây là một bề mặt IPC chứ không phải một lời gọi hàm: pipe dựng bằng
SDDL hẹp `D:P(A;;GA;;;<sid>)` chứ không dùng DACL mặc định, bật
`PIPE_REJECT_REMOTE_CLIENTS` (named pipe **với tới được qua SMB**),
`FILE_FLAG_FIRST_PIPE_INSTANCE` để không ai chiếm được tên trước, và phía client
khai `SECURITY_SQOS_PRESENT|SECURITY_IDENTIFICATION` rồi kiểm SID của server.

Màn hình khoá **không ảnh hưởng**, và đây là đo chứ không suy: khoá ≠ đăng xuất
nên session vẫn còn, task vẫn chạy, và `GetForegroundWindow()` vẫn trả handle hợp
lệ dù input desktop lúc đó là `WinSta0\Winlogon`. Đo trên a14: trước khoá handle
`2622524`, đang khoá `131664` — khác nhau nhưng đều không NULL; `tongue` đọc đúng,
`layout` vẫn `0409`, `en`/`vi` vẫn đổi thật; qua pipe từ xa 3/3 ở 453-528 ms.

Không có agent thì hành vi **y như cũ**: báo lỗi session 0 kèm hướng dẫn. Đó là
chủ đích — im lặng coi như thành công mới là thứ nguy hiểm.

**Nó không mua tốc độ.** Bước qua pipe gần như miễn phí, nhưng một lượt SSH tới
máy đó vẫn tốn 452 ms (có ControlMaster) đến 829 ms (không). Thứ nó mua là
`ssh may tongue vi` **chạy được**, cho script và tự động hoá.

## Bản đồ mã nguồn

```
src/mode.rs        bảng mode → trạng thái đích (thuần, không chạm OS)
src/reconcile.rs   vòng áp + verify; VerifyFailed → exit 1
src/status.rs      suy mode ngược từ trạng thái thật; render human/json
src/config.rs      đọc config.toml; vắng file = mặc định
src/doctor.rs      in kết quả khám; phần khám riêng nằm ở từng backend
src/backend/
  mod.rs           trait Layout {current, select} + trait Ime {is_on, set, diagnose}
  macos/tis.rs     FFI TIS API (framework Carbon) — đổi input source
  macos/app.rs     AppIme generic: pgrep / open / killall
  macos/gonhanh.rs AppIme + defaults; ôm luôn bẫy perAppMode
  macos/system.rs  SystemIme: không app ngoài, tiếng Việt từ input source macOS
  windows/vkey.rs  FindWindow + PostMessage + đọc shared memory
  vkey_shm.rs      parser bytes shared memory VKey (thuần, test được mọi OS)
```

Toàn bộ phần quyết định là code thuần và có test chạy trên mọi OS; chỉ bốn file
`tis.rs`, `gonhanh.rs`/`app.rs`, `vkey.rs`, `pipe.rs` là thật sự chạm hệ thống.

**Thêm bộ gõ mới:** nếu nó chỉ cần bật/tắt bằng process thì không cần code —
dùng `backend = "app"`. Nếu nó có kênh điều khiển riêng thì thêm một file trong
`src/backend/macos/` impl `trait Ime` và thêm một nhánh vào `make_ime` trong
`src/main.rs`. Không phải sửa chỗ nào khác.

## Phát triển

```bash
nix develop                                     # cargo, clippy, rustfmt, rust-analyzer
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
nix build .#tongue && ./result/bin/tongue --help
```

Lệnh clippy cho target Windows là **bắt buộc** dù bạn ngồi trên mac: hai target
có dead-code khác nhau, đã có tiền lệ CI Windows đỏ mà máy mac không hề hay biết.
Không cần linker MSVC, chỉ cần `rustup target add x86_64-pc-windows-msvc` một lần.

`nix build` lấy source từ git, nên file mới **phải `git add`** trước, không thì
nix không thấy và báo "file not found for module".

Test tự động không chạm hệ thống. Muốn thử thật thì dùng chính CLI và nhớ kết
thúc bằng `tongue vi` để khôi phục.

## Ngoài phạm vi hiện tại

Auto-switch theo app đang focus, daemon/hotkey tích hợp sẵn, Linux, và strategy
`notify` cho GoNhanh — đã ghi trong spec, chưa làm.

Chi tiết thiết kế kèm bằng chứng `file:line` từ source của cả hai bộ gõ:
`docs/superpowers/specs/2026-07-29-tongue-design.md`. Quy ước cho người sửa code
(và cho AI): `CLAUDE.md`.

[GoNhanh]: https://github.com/khaphanspace/gonhanh.org
[VKey]: https://github.com/phatMT97/VKey
