//! Chuẩn hoá HKL của Windows thành định danh layout của tongue.
//!
//! Thuần, không chạm OS — cùng lý do như `vkey_shm`: phần quyết định phải test được
//! trên mọi nền tảng, chỉ để lại đúng lời gọi FFI cho `windows/layout.rs`.
//!
//! Định danh layout của tongue trên Windows CỐ Ý là **LANGID 4 chữ số hex** ("0409" US,
//! "0804" Trung giản thể, "042a" Việt), không phải KLID 8 chữ số. Lý do:
//! `GetKeyboardLayout` chỉ trả về HKL, mà **word cao của HKL là handle thiết bị** — nó
//! đổi giữa các phiên và giữa các bàn phím, nên so sánh nguyên HKL thì verify của
//! reconcile không bao giờ khớp. Word thấp là LANGID thì ổn định.
//!
//! Đánh đổi đã biết: các KLID không chuẩn (ví dụ `00010409` = US-Dvorak) không phân
//! biệt được với `00000409` vì cùng LANGID. tongue chỉ cần ba mode ngôn ngữ nên chấp
//! nhận được; ai cần ghim đúng một biến thể bàn phím thì đây là chỗ phải mở rộng.

/// Lấy LANGID (word thấp) từ HKL đã ép về usize.
pub fn langid_of(hkl: usize) -> u16 {
    (hkl as u32 & 0xFFFF) as u16
}

/// HKL -> định danh layout của tongue.
pub fn format_langid(hkl: usize) -> String {
    format!("{:04x}", langid_of(hkl))
}

/// Định danh của tongue -> KLID 8 chữ số mà `LoadKeyboardLayoutW` đòi.
///
/// Nhận cả chuỗi đã đủ 8 chữ số (trả về nguyên), để ai cần một KLID cụ thể vẫn khai
/// được trong config — dù lúc đó `format_langid` sẽ không round-trip, xem ghi chú
/// về đánh đổi ở đầu file.
pub fn klid_of(id: &str) -> String {
    format!("{id:0>8}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langid_bo_word_cao_vi_do_la_handle_thiet_bi() {
        // Ba giá trị dưới có dạng HKL thật: word cao là handle, word thấp là LANGID.
        assert_eq!(langid_of(0x0409_0409), 0x0409);
        assert_eq!(langid_of(0xF0C1_0804), 0x0804);
        assert_eq!(langid_of(0x0000_042A), 0x042A);
    }

    #[test]
    fn format_ra_4_chu_so_hex_thuong() {
        assert_eq!(format_langid(0x0409_0409), "0409");
        assert_eq!(format_langid(0xF0C1_0804), "0804");
        // chữ thường: config viết "042a", đọc về cũng phải "042a" mới khớp chuỗi
        assert_eq!(format_langid(0x0000_042A), "042a");
    }

    #[test]
    fn klid_dem_khong_len_8_chu_so() {
        assert_eq!(klid_of("0409"), "00000409");
        assert_eq!(klid_of("804"), "00000804");
    }

    #[test]
    fn klid_giu_nguyen_chuoi_da_du_8() {
        assert_eq!(klid_of("00010409"), "00010409");
    }

    /// Vòng round-trip mà reconcile dựa vào: đặt LANGID nào thì đọc lại đúng chuỗi đó.
    #[test]
    fn round_trip_cho_layout_chuan() {
        for id in ["0409", "0804", "042a"] {
            let klid = klid_of(id);
            let fake_hkl = usize::from_str_radix(&klid, 16).unwrap();
            assert_eq!(format_langid(fake_hkl), id);
        }
    }
}
