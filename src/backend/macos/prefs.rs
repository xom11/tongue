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
            // core-foundation-sys 0.8.7 khai CFNumberGetValue trả `bool` (không
            // phải `Boolean`/u8 như brief giả định) — so trực tiếp, không `!= 0`.
            if ok {
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
        eprintln!(
            "blob {} byte: {}",
            blob.len(),
            String::from_utf8_lossy(&blob)
        );
        let c = super::super::chord::parse(&blob).unwrap();
        eprintln!("chord: {}", super::super::chord::describe(&c));
        assert!(blob.len() > 10);
    }
}
