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

Thiết kế + bằng chứng source: `docs/superpowers/specs/2026-07-29-tongue-design.md`.

Lần đầu dùng trên máy mới: chạy `tongue doctor --fix`.

[GoNhanh]: https://github.com/khaphanspace/gonhanh.org
[VKey]: https://github.com/phatMT97/VKey
