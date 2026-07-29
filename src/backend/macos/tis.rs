//! Đổi input source hệ thống qua TIS API (HIToolbox/Carbon) — cách
//! im-select/macism làm, không phụ thuộc binary ngoài.
//! Quirk CJK (lệnh nhận, chưa đổi ngay) được xử lý ở tầng reconcile:
//! re-select mỗi vòng poll tới khi current() khớp.

use anyhow::{bail, Result};
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{Boolean, CFRelease, CFRetain};
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::string::CFStringRef;
use std::ffi::c_void;

#[repr(C)]
struct __TISInputSource(c_void);
type TISInputSourceRef = *mut __TISInputSource;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyInputSourceID: CFStringRef;
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCreateInputSourceList(
        properties: CFDictionaryRef,
        include_all_installed: Boolean,
    ) -> CFArrayRef;
    fn TISSelectInputSource(source: TISInputSourceRef) -> i32;
    fn TISGetInputSourceProperty(source: TISInputSourceRef, key: CFStringRef) -> *mut c_void;
}

pub fn current_source_id() -> Result<String> {
    unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            bail!("TISCopyCurrentKeyboardInputSource trả về null");
        }
        let id_ptr = TISGetInputSourceProperty(src, kTISPropertyInputSourceID);
        if id_ptr.is_null() {
            CFRelease(src as _);
            bail!("input source hiện tại không có InputSourceID");
        }
        let id = CFString::wrap_under_get_rule(id_ptr as CFStringRef).to_string();
        CFRelease(src as _);
        Ok(id)
    }
}

/// Trả về source đã retain (caller phải CFRelease), None nếu id chưa bật.
/// include_all_installed = 0: chỉ tìm trong các source ĐANG BẬT ở System Settings.
unsafe fn copy_source_by_id(id: &str) -> Option<TISInputSourceRef> {
    let key = CFString::wrap_under_get_rule(kTISPropertyInputSourceID);
    let val = CFString::new(id);
    let filter = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
    let list = TISCreateInputSourceList(filter.as_concrete_TypeRef() as CFDictionaryRef, 0);
    if list.is_null() {
        return None;
    }
    if CFArrayGetCount(list) == 0 {
        CFRelease(list as _);
        return None;
    }
    let src = CFArrayGetValueAtIndex(list, 0) as TISInputSourceRef;
    CFRetain(src as _); // giữ src sống qua release của list
    CFRelease(list as _);
    Some(src)
}

pub fn select_source(id: &str) -> Result<()> {
    unsafe {
        let Some(src) = copy_source_by_id(id) else {
            bail!(
                "không tìm thấy input source {id} — đã bật trong System Settings > Keyboard chưa?"
            );
        };
        let status = TISSelectInputSource(src);
        CFRelease(src as _);
        if status != 0 {
            bail!("TISSelectInputSource trả về OSStatus {status}");
        }
        Ok(())
    }
}

pub fn source_exists(id: &str) -> Result<bool> {
    unsafe {
        match copy_source_by_id(id) {
            Some(src) => {
                CFRelease(src as _);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

pub struct TisLayout;

impl crate::backend::Layout for TisLayout {
    fn current(&self) -> Result<String> {
        current_source_id()
    }
    fn select(&self, id: &str) -> Result<()> {
        select_source(id)
    }
}

#[cfg(test)]
mod smoke {
    #[test]
    #[ignore = "chạm hệ thống thật — chạy tay: cargo test -- --ignored"]
    fn doc_source_hien_tai() {
        let id = super::current_source_id().unwrap();
        eprintln!("current source: {id}");
        assert!(!id.is_empty());
    }
}
