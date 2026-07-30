//! Strategy `hotkey`: giữ GoNhanh sống, đổi vi/en bằng chính chord toggle mà app
//! đã đăng ký. Bỏ hẳn cold-start vì `en` không còn giết app.
//!
//! BẤT BIẾN CỦA FILE NÀY — chord là RELAY, không idempotent:
//! `reconcile` gọi lại `set()` mỗi vòng poll 50ms còn lệch, nhưng GoNhanh mất
//! 87–286ms (đo 4 lần, 30/07/2026) mới ghi `gonhanh.enabled`. Trả về sớm là bắn
//! trùng 2–6 chord và lật mode qua lại. Nên `set()` phải TỰ CHỜ XÁC NHẬN, và
//! bắn TỐI ĐA MỘT chord cho mỗi lần chạy tongue.
//!
//! Logic nằm sau ba trait để test được bằng fake; FFI CoreGraphics ở cuối file.

use anyhow::Result;
use std::cell::Cell;
use std::time::{Duration, Instant};

pub trait ChordSender {
    fn send(&self) -> Result<()>;
}

pub trait Launcher {
    /// Ghi enabled=1 rồi mở app — app đọc defaults lúc khởi động nên lên là đã bật.
    fn launch(&self) -> Result<()>;
}

pub trait StateSource {
    fn running(&self) -> Result<bool>;
    /// None = key chưa từng được ghi.
    fn enabled(&self) -> Result<Option<bool>>;
}

pub struct HotkeyCore<'a> {
    sender: &'a dyn ChordSender,
    launcher: &'a dyn Launcher,
    state: &'a dyn StateSource,
    timeout: Duration,
    poll: Duration,
    /// MƯỢN từ bên ngoài, không sở hữu: `reconcile` gọi `Ime::set()` nhiều lượt
    /// và mỗi lượt dựng một `HotkeyCore` mới, nên cờ "đã bắn" phải sống ở tầng
    /// `HotkeyIme` (tồn tại suốt lần chạy tongue). Để `Cell` ở đây là nó reset
    /// mỗi lượt và chốt "tối đa một chord" thành vô nghĩa.
    fired: &'a Cell<bool>,
}

impl<'a> HotkeyCore<'a> {
    pub fn new(
        sender: &'a dyn ChordSender,
        launcher: &'a dyn Launcher,
        state: &'a dyn StateSource,
        timeout: Duration,
        poll: Duration,
        fired: &'a Cell<bool>,
    ) -> Self {
        Self {
            sender,
            launcher,
            state,
            timeout,
            poll,
            fired,
        }
    }

    /// App đang chạy VÀ enabled — không phải một trong hai. App chết thì
    /// `enabled` còn sót lại trong defaults cũng chẳng gõ được gì.
    pub fn is_on(&self) -> Result<bool> {
        Ok(self.state.running()? && self.state.enabled()?.unwrap_or(false))
    }

    pub fn set(&self, on: bool) -> Result<()> {
        if !self.state.running()? {
            if !on {
                return Ok(()); // app chết = đã là `en`
            }
            // App đọc defaults lúc khởi động, nên lên là đã bật sẵn — bắn thêm
            // chord ở đây là tắt mất cái vừa bật.
            self.launcher.launch()?;
            return self.cho_toi(on);
        }
        if self.is_on()? == on {
            return Ok(());
        }
        // Tối đa MỘT chord mỗi lần chạy tongue. reconcile sẽ gọi lại set() sau
        // khi ta trả về; nếu cú đầu thật ra có ăn nhưng chậm bất thường thì cú
        // thứ hai lật ngược mode. Một lần chạy = một lần chuyển mode.
        if self.fired.get() {
            return Ok(());
        }
        self.sender.send()?;
        self.fired.set(true);
        self.cho_toi(on)
    }

    /// Chờ trạng thái thật khớp `want`, tối đa hết ngân sách. Hết giờ vẫn trả
    /// Ok: `reconcile` là chỗ DUY NHẤT quyết định VerifyFailed, không phải đây.
    fn cho_toi(&self, want: bool) -> Result<()> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if self.is_on()? == want {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(self.poll);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// enabled chỉ đổi sau `do_tre` lần đọc — mô phỏng 87–286ms trễ ghi defaults
    /// mà reconcile poll 50ms sẽ đâm phải.
    struct FakeState {
        running: Cell<bool>,
        enabled: Cell<bool>,
        cho_doi: Cell<u32>,
        moi_doi: Cell<Option<bool>>,
        so_lan_doc: Cell<u32>,
    }
    impl FakeState {
        fn new(running: bool, enabled: bool) -> Self {
            Self {
                running: Cell::new(running),
                enabled: Cell::new(enabled),
                cho_doi: Cell::new(0),
                moi_doi: Cell::new(None),
                so_lan_doc: Cell::new(0),
            }
        }
        /// Hẹn: sau `n` lần đọc nữa thì enabled thành `gia_tri`.
        fn hen_doi_sau(&self, n: u32, gia_tri: bool) {
            self.cho_doi.set(n);
            self.moi_doi.set(Some(gia_tri));
        }
    }
    impl StateSource for FakeState {
        fn running(&self) -> Result<bool> {
            Ok(self.running.get())
        }
        fn enabled(&self) -> Result<Option<bool>> {
            self.so_lan_doc.set(self.so_lan_doc.get() + 1);
            let con = self.cho_doi.get();
            if con > 0 {
                self.cho_doi.set(con - 1);
                if con == 1 {
                    if let Some(v) = self.moi_doi.get() {
                        self.enabled.set(v);
                    }
                }
            }
            Ok(Some(self.enabled.get()))
        }
    }

    struct FakeSender<'a> {
        so_lan: Cell<u32>,
        state: &'a FakeState,
        /// số lần đọc trước khi enabled đổi, sau mỗi cú bắn
        tre: u32,
    }
    impl<'a> ChordSender for FakeSender<'a> {
        fn send(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            let moi = !self.state.enabled.get();
            self.state.hen_doi_sau(self.tre, moi);
            Ok(())
        }
    }

    /// Chord bắn ra nhưng KHÔNG có tác dụng — Accessibility bị thu hồi, app treo.
    struct SenderTruot {
        so_lan: Cell<u32>,
    }
    impl ChordSender for SenderTruot {
        fn send(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            Ok(())
        }
    }

    struct FakeLauncher<'a> {
        so_lan: Cell<u32>,
        state: &'a FakeState,
    }
    impl<'a> Launcher for FakeLauncher<'a> {
        fn launch(&self) -> Result<()> {
            self.so_lan.set(self.so_lan.get() + 1);
            self.state.running.set(true);
            self.state.enabled.set(true);
            Ok(())
        }
    }

    struct LauncherKhongDuocGoi;
    impl Launcher for LauncherKhongDuocGoi {
        fn launch(&self) -> Result<()> {
            panic!("không được launch ở nhánh này");
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// HỒI QUY CHO VẤN ĐỀ CỐT LÕI: enabled chỉ đổi sau 6 lần đọc (mô phỏng 286ms
    /// / poll 50ms), nhưng chỉ được bắn ĐÚNG MỘT chord.
    #[test]
    fn bat_vi_chi_ban_mot_chord_du_readback_tre() {
        let st = FakeState::new(true, false);
        let sender = FakeSender {
            so_lan: Cell::new(0),
            state: &st,
            tre: 6,
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(1000), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 1, "phải bắn đúng 1 chord");
        assert!(core.is_on().unwrap(), "phải kết thúc ở trạng thái bật");
    }

    /// reconcile gọi set() thêm lượt nữa sau khi chord trượt — KHÔNG được bắn cú
    /// thứ hai, vì nếu cú đầu thật ra ăn chậm thì cú hai lật ngược mode.
    #[test]
    fn chord_truot_thi_khong_ban_cu_thu_hai() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(20), ms(1), &fired);
        core.set(true).unwrap();
        core.set(true).unwrap();
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 1, "tối đa một chord mỗi lần chạy");
    }

    /// Giống test trên nhưng dựng HotkeyCore MỚI mỗi lượt — đúng cách
    /// `HotkeyIme::set` chạy thật khi reconcile gọi nhiều lượt. Khoá việc cờ
    /// `fired` phải sống NGOÀI core; để nó bên trong là mỗi lượt reset về false
    /// và bắn thêm chord.
    #[test]
    fn core_moi_moi_luot_van_chi_ban_mot_chord() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        for _ in 0..3 {
            let core = HotkeyCore::new(&sender, &lc, &st, ms(10), ms(1), &fired);
            core.set(true).unwrap();
        }
        assert_eq!(sender.so_lan.get(), 1, "cờ fired phải sống ngoài core");
    }

    /// Chord trượt hẳn: set() phải TRẢ VỀ sau ngân sách chứ không treo, và trả
    /// Ok để reconcile là chỗ duy nhất quyết định VerifyFailed.
    #[test]
    fn chord_truot_thi_tra_ve_sau_ngan_sach_khong_treo() {
        let st = FakeState::new(true, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(30), ms(1), &fired);
        let t0 = Instant::now();
        core.set(true).unwrap();
        let mat = t0.elapsed();
        assert!(mat >= ms(30), "phải chờ hết ngân sách, mất {mat:?}");
        assert!(mat < ms(500), "không được treo, mất {mat:?}");
        assert!(!core.is_on().unwrap());
    }

    /// App chưa chạy + muốn bật: đi nhánh launch, KHÔNG bắn chord (app khởi động
    /// đã ở trạng thái bật sẵn — bắn thêm là tắt mất).
    #[test]
    fn app_chua_chay_thi_launch_chu_khong_chord() {
        let st = FakeState::new(false, false);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = FakeLauncher {
            so_lan: Cell::new(0),
            state: &st,
        };
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(lc.so_lan.get(), 1);
        assert_eq!(
            sender.so_lan.get(),
            0,
            "không được bắn chord ở nhánh launch"
        );
        assert!(core.is_on().unwrap());
    }

    /// App chưa chạy + muốn tắt: đã là `en` rồi, không làm gì hết.
    #[test]
    fn app_chua_chay_muon_tat_thi_no_op() {
        let st = FakeState::new(false, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
        core.set(false).unwrap();
        assert_eq!(sender.so_lan.get(), 0);
    }

    /// Đã đúng sẵn thì không đụng gì.
    #[test]
    fn dung_san_thi_khong_ban() {
        let st = FakeState::new(true, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
        core.set(true).unwrap();
        assert_eq!(sender.so_lan.get(), 0);
    }

    /// App chết thì dù defaults còn sót enabled=1 cũng KHÔNG gõ được tiếng Việt.
    #[test]
    fn app_chet_thi_is_on_false_du_enabled_con_sot() {
        let st = FakeState::new(false, true);
        let sender = SenderTruot {
            so_lan: Cell::new(0),
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(200), ms(1), &fired);
        assert!(!core.is_on().unwrap());
    }

    /// Tắt tiếng Việt khi app đang chạy: cũng chỉ một chord.
    #[test]
    fn tat_vi_ban_mot_chord() {
        let st = FakeState::new(true, true);
        let sender = FakeSender {
            so_lan: Cell::new(0),
            state: &st,
            tre: 4,
        };
        let lc = LauncherKhongDuocGoi;
        let fired = Cell::new(false);
        let core = HotkeyCore::new(&sender, &lc, &st, ms(1000), ms(1), &fired);
        core.set(false).unwrap();
        assert_eq!(sender.so_lan.get(), 1);
        assert!(!core.is_on().unwrap());
    }
}
