//! Vòng reconcile: đọc trạng thái thật → áp phần lệch → poll verify.
//! Việc re-select layout mỗi vòng chính là mẹo retry kiểu macism cho
//! TISSelectInputSource với CJK source — không cần code riêng.

use crate::backend::{Ime, Layout};
use crate::mode::Desired;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct VerifyFailed {
    pub layout_expected: Option<String>,
    pub layout_actual: Option<String>,
    pub ime_expected: bool,
    pub ime_actual: bool,
}

impl std::fmt::Display for VerifyFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let (Some(want), Some(got)) = (&self.layout_expected, &self.layout_actual) {
            if want != got {
                parts.push(format!("layout muốn {want}, thực tế {got}"));
            }
        }
        if self.ime_expected != self.ime_actual {
            parts.push(format!(
                "bộ gõ ngoài muốn {}, thực tế {}",
                if self.ime_expected { "bật" } else { "tắt" },
                if self.ime_actual { "bật" } else { "tắt" }
            ));
        }
        write!(f, "verify trượt sau timeout: {}", parts.join("; "))
    }
}

impl std::error::Error for VerifyFailed {}

pub fn reconcile(
    layout: &dyn Layout,
    ime: &dyn Ime,
    desired: &Desired,
    timeout: Duration,
    poll: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut applied = false;
    loop {
        let cur_layout = match &desired.layout {
            Some(_) => Some(layout.current()?),
            None => None,
        };
        let cur_ime = ime.is_on()?;

        let layout_ok = match (&desired.layout, &cur_layout) {
            (Some(want), Some(got)) => want == got,
            _ => true,
        };
        let ime_ok = cur_ime == desired.ime_on;
        if layout_ok && ime_ok {
            return Ok(());
        }

        // luôn áp ít nhất một lần trước khi được phép fail
        if applied && Instant::now() >= deadline {
            return Err(VerifyFailed {
                layout_expected: desired.layout.clone(),
                layout_actual: cur_layout,
                ime_expected: desired.ime_on,
                ime_actual: cur_ime,
            }
            .into());
        }

        if !layout_ok {
            if let Some(want) = &desired.layout {
                layout.select(want)?;
            }
        }
        if !ime_ok {
            ime.set(desired.ime_on)?;
        }
        applied = true;
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::{reconcile, VerifyFailed};
    use crate::backend::{Ime, Layout};
    use crate::mode::Desired;
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    /// Layout chỉ "ăn" sau `applies_after` lần select — mô phỏng quirk
    /// TISSelectInputSource với CJK (lệnh nhận nhưng chưa đổi ngay).
    struct FakeLayout {
        current: RefCell<String>,
        applies_after: Cell<u32>,
    }
    impl FakeLayout {
        fn new(cur: &str, applies_after: u32) -> Self {
            Self {
                current: RefCell::new(cur.into()),
                applies_after: Cell::new(applies_after),
            }
        }
    }
    impl Layout for FakeLayout {
        fn current(&self) -> anyhow::Result<String> {
            Ok(self.current.borrow().clone())
        }
        fn select(&self, id: &str) -> anyhow::Result<()> {
            let n = self.applies_after.get();
            if n <= 1 {
                *self.current.borrow_mut() = id.into();
            } else {
                self.applies_after.set(n - 1);
            }
            Ok(())
        }
    }

    struct FakeIme {
        on: Cell<bool>,
        stuck: bool, // set() không có tác dụng — mô phỏng IME không phản hồi
    }
    impl Ime for FakeIme {
        fn is_on(&self) -> anyhow::Result<bool> {
            Ok(self.on.get())
        }
        fn set(&self, v: bool) -> anyhow::Result<()> {
            if !self.stuck {
                self.on.set(v);
            }
            Ok(())
        }
    }

    fn des(layout: Option<&str>, ime_on: bool) -> Desired {
        Desired {
            layout: layout.map(String::from),
            ime_on,
        }
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ap_ngay_thi_ok() {
        let l = FakeLayout::new("cu", 1);
        let i = FakeIme {
            on: Cell::new(false),
            stuck: false,
        };
        reconcile(&l, &i, &des(Some("moi"), true), ms(200), ms(1)).unwrap();
        assert_eq!(*l.current.borrow(), "moi");
        assert!(i.on.get());
    }

    #[test]
    fn da_khop_san_thi_khong_lam_gi_van_ok() {
        let l = FakeLayout::new("abc", 1);
        let i = FakeIme {
            on: Cell::new(true),
            stuck: true,
        }; // stuck nhưng đã đúng sẵn
        reconcile(&l, &i, &des(Some("abc"), true), ms(50), ms(1)).unwrap();
    }

    #[test]
    fn layout_cham_van_ok_nho_retry() {
        let l = FakeLayout::new("cu", 3); // chỉ ăn ở lần select thứ 3
        let i = FakeIme {
            on: Cell::new(false),
            stuck: false,
        };
        reconcile(&l, &i, &des(Some("moi"), false), ms(500), ms(1)).unwrap();
        assert_eq!(*l.current.borrow(), "moi");
    }

    #[test]
    fn ime_ket_thi_verify_failed() {
        let l = FakeLayout::new("abc", 1);
        let i = FakeIme {
            on: Cell::new(false),
            stuck: true,
        };
        let err = reconcile(&l, &i, &des(Some("abc"), true), ms(20), ms(1)).unwrap_err();
        let vf = err
            .downcast_ref::<VerifyFailed>()
            .expect("phải là VerifyFailed");
        assert!(vf.ime_expected);
        assert!(!vf.ime_actual);
    }

    #[test]
    fn khong_co_layout_thi_bo_qua_layout() {
        let l = FakeLayout::new("bat-ky", 1);
        let i = FakeIme {
            on: Cell::new(false),
            stuck: false,
        };
        reconcile(&l, &i, &des(None, true), ms(100), ms(1)).unwrap();
        assert_eq!(*l.current.borrow(), "bat-ky"); // không bị đổi
        assert!(i.on.get());
    }
}
