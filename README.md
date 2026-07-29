# tongue

CLI chuyển chế độ gõ `vi | en | zh` — một lệnh nguyên tử đổi cả layout hệ
thống lẫn bộ gõ ngoài, có verify. macOS điều khiển [GoNhanh], Windows điều
khiển [VKey].

    tongue vi        # tiếng Việt (bộ gõ ngoài bật)
    tongue en        # tiếng Anh  (bộ gõ ngoài tắt)
    tongue zh        # tiếng Trung (chỉ macOS: layout Pinyin)
    tongue           # in mode hiện tại
    tongue status    # chi tiết (--json cho script)
    tongue doctor    # khám môi trường; --fix sửa những gì an toàn

Exit code: `0` đích đã verify khớp · `1` verify trượt · `2` lỗi môi trường.

Lần đầu dùng trên máy mới: chạy `tongue doctor --fix`.

## Chọn bộ gõ (macOS)

Vắng config thì dùng GoNhanh — không cần làm gì. Muốn bộ gõ khác thì sửa
`~/.config/tongue/config.toml`, không phải sửa code:

```toml
# Bộ gõ ngoài bất kỳ chỉ cần bật/tắt bằng process (EVKey, OpenKey...)
[macos]
backend = "app"
app_name = "EVKey"
```

```toml
# Bộ gõ tiếng Việt có sẵn của macOS — không cài gì thêm, không process nào chạy nền
[macos]
backend = "system"
source_vi = "com.apple.inputmethod.VietnameseIM.VietnameseTelex"
```

Với `backend = "system"` thì `vi` và `en` là hai input source khác nhau
(`source_en` mặc định là ABC), còn bộ gõ ngoài thì giữ nguyên layout và chỉ
bật/tắt app. `tongue doctor` cảnh báo nếu hai bộ gõ chạy chồng lên nhau.

Bộ gõ có kênh điều khiển riêng (không quy về bật/tắt process) thì cần một impl
`Ime` mới — xem `src/backend/macos/` và `CLAUDE.md`.

Thiết kế + bằng chứng source: `docs/superpowers/specs/2026-07-29-tongue-design.md`.

[GoNhanh]: https://github.com/khaphanspace/gonhanh.org
[VKey]: https://github.com/phatMT97/VKey
