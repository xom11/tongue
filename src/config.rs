//! Config tuỳ chọn — vắng file thì mọi giá trị là default, tool vẫn chạy.

use anyhow::Context;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub macos: MacosConfig,
    pub windows: WindowsConfig,
    pub verify: VerifyConfig,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct MacosConfig {
    /// Bộ gõ nào lo tiếng Việt: "gonhanh" | "app" | "system". Xem backend::macos.
    pub backend: String,
    /// Cách điều khiển bộ gõ đó: "process" (kill/launch) | "hotkey" (giả lập
    /// chord toggle, giữ app sống — chỉ dùng được với backend "gonhanh").
    pub strategy: String,
    pub source_vi: String,
    /// Mặc định TRÙNG source_vi — đúng cho mọi bộ gõ ngoài (layout giữ ABC, bit IME
    /// phân biệt vi/en). Chỉ khác khi backend = "system".
    pub source_en: String,
    pub source_zh: String,
    pub app_name: String,
}

impl Default for MacosConfig {
    fn default() -> Self {
        Self {
            backend: "gonhanh".into(),
            strategy: "process".into(),
            source_vi: "com.apple.keylayout.ABC".into(),
            source_en: "com.apple.keylayout.ABC".into(),
            source_zh: "com.apple.inputmethod.SCIM.ITABC".into(),
            app_name: "GoNhanh".into(),
        }
    }
}

impl MacosConfig {
    // Đối xứng với WindowsConfig::sources: từ khi main.rs có `sources()` cfg-gate theo
    // nền tảng, hàm này chỉ còn call site dưới cfg(target_os = "macos"), nên trên build
    // Windows nó là dead code hợp lệ. (Trước đây switch() gọi cfg.macos.sources() vô
    // điều kiện nên cả hai nền tảng đều dùng.)
    #[allow(dead_code)]
    pub fn sources(&self) -> crate::mode::Sources {
        crate::mode::Sources {
            vi: self.source_vi.clone(),
            en: self.source_en.clone(),
            zh: self.source_zh.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowsConfig {
    pub vkey_path: String,
    /// LANGID 4 chu so hex, KHONG phai KLID 8 chu so — xem backend::hkl de biet vi sao.
    ///
    /// vi va en mac dinh TRUNG nhau ("0409" = US): VKey dung chinh layout US va bit
    /// bat/tat cua no phan biet vi voi en, y nhu bo go ngoai tren macOS. zh thi layout
    /// moi la thu phan biet, nen no phai khac.
    pub source_vi: String,
    pub source_en: String,
    pub source_zh: String,
    /// Ten scheduled task chay `tongue agent` trong session tuong tac. Client o
    /// session 0 goi `schtasks /run /tn <day>` khi khong thay agent nao.
    ///
    /// La CONFIG chu khong phai hang so vi task nay do lop tich hop (`~/.nix`,
    /// `windows/modules/services/`) tao ra, khong phai do tongue. Hardcode ten no la
    /// bat cong cu phu thuoc nguoc vao mot repo ma no vua duoc tach ra khoi.
    pub agent_task: String,
    /// Ngan sach cho agent tra loi, mili giay.
    ///
    /// San phai LON HON ca xau nhat cua agent, va ca xau nhat khong phai ~1s ma ~6s:
    /// `ensure_running` cho VKey cold-start toi 5000ms roi `reconcile` con an them
    /// `verify.timeout_ms` (mac dinh 1000), cong thoi gian dung tien trinh con va mot
    /// luot `schtasks /run` (376-401ms, do 5/5 tren a14). Dat 2000 nhu phan xa tu nhien
    /// la bao THAT BAI cho mot lan chuyen dang thanh cong bon giay sau do — va moi cache
    /// ma nguoi goi giu deu thanh sai.
    pub agent_timeout_ms: u64,
    /// Agent tu thoat sau ngan nay mili giay ranh.
    ///
    /// Khong co watchdog: client tu hoi sinh agent bang `schtasks /run`, nen "da chet"
    /// khong phai mot trang thai can ai canh. Doi lai phai co duong thoat nay — no cung
    /// la thu nha khoa file tren `tongue.exe` (Windows khong cho ghi de .exe dang chay),
    /// ma kenh cai dat tren a14 la chep tay.
    pub agent_idle_ms: u64,
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            vkey_path: String::new(),
            source_vi: "0409".into(),
            source_en: "0409".into(),
            source_zh: "0804".into(),
            agent_task: r"\TongueAgent".into(),
            agent_timeout_ms: 15_000,
            agent_idle_ms: 600_000,
        }
    }
}

impl WindowsConfig {
    // Call site thật chỉ có trong main.rs dưới cfg(windows) (sources() và snapshot());
    // trên build macOS đây là dead code hợp lệ, cùng lý do như backend::hkl.
    #[allow(dead_code)]
    pub fn sources(&self) -> crate::mode::Sources {
        crate::mode::Sources {
            vi: self.source_vi.clone(),
            en: self.source_en.clone(),
            zh: self.source_zh.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct VerifyConfig {
    pub timeout_ms: u64,
    pub poll_ms: u64,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 1000,
            poll_ms: 50,
        }
    }
}

pub fn parse(text: &str) -> anyhow::Result<Config> {
    toml::from_str(text).context("config.toml không hợp lệ")
}

fn default_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // ~/.config chứ không phải ~/Library/Application Support — đồng bộ với
        // các dotfile khác của chủ repo.
        dirs::home_dir().map(|h| h.join(".config/tongue/config.toml"))
    }
    #[cfg(windows)]
    {
        dirs::config_dir().map(|d| d.join("tongue/config.toml"))
    }
}

pub fn load() -> anyhow::Result<Config> {
    match default_path() {
        Some(p) if p.exists() => {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("không đọc được {}", p.display()))?;
            parse(&text)
        }
        _ => Ok(Config::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn khong_co_gi_thi_ra_default() {
        let c = parse("").unwrap();
        assert_eq!(c.macos.backend, "gonhanh");
        assert_eq!(c.macos.strategy, "process");
        assert_eq!(c.macos.source_vi, "com.apple.keylayout.ABC");
        assert_eq!(c.macos.source_zh, "com.apple.inputmethod.SCIM.ITABC");
        assert_eq!(c.macos.app_name, "GoNhanh");
        assert_eq!(c.windows.vkey_path, "");
        assert_eq!(c.windows.agent_task, r"\TongueAgent");
        assert_eq!(c.verify.timeout_ms, 1000);
        assert_eq!(c.verify.poll_ms, 50);
    }

    /// Bất biến giữ tương thích ngược: mặc định vi và en dùng CHUNG một layout,
    /// nên bit IME là thứ duy nhất phân biệt — đúng hành vi trước khi có `system`.
    #[test]
    fn mac_dinh_source_en_trung_source_vi() {
        let c = parse("").unwrap();
        assert_eq!(c.macos.source_en, c.macos.source_vi);
        let s = c.macos.sources();
        assert_eq!(s.vi, s.en);
    }

    #[test]
    fn backend_system_khai_source_vi_rieng() {
        let c = parse(
            "[macos]\nbackend = \"system\"\nsource_vi = \"com.apple.inputmethod.VietnameseIM.VietnameseTelex\"\n",
        )
        .unwrap();
        assert_eq!(c.macos.backend, "system");
        let s = c.macos.sources();
        assert_ne!(s.vi, s.en);
        assert_eq!(s.en, "com.apple.keylayout.ABC");
    }

    #[test]
    fn override_mot_phan_giu_default_phan_con_lai() {
        let c = parse("[verify]\ntimeout_ms = 3000\n").unwrap();
        assert_eq!(c.verify.timeout_ms, 3000);
        assert_eq!(c.verify.poll_ms, 50);
        assert_eq!(c.macos.strategy, "process");
    }

    #[test]
    fn toml_hong_thi_bao_loi() {
        assert!(parse("[macos\nstrategy=").is_err());
    }

    /// San cua ngan sach cho agent phai lon hon ca xau nhat cua no: 5000ms cold-start
    /// VKey + verify.timeout_ms. Ha xuong duoi la bao that bai cho mot lan chuyen dang
    /// thanh cong.
    #[test]
    fn ngan_sach_agent_lon_hon_ca_xau_nhat() {
        let c = Config::default();
        assert!(c.windows.agent_timeout_ms > 5_000 + c.verify.timeout_ms);
    }

    #[test]
    fn khai_agent_task_rieng() {
        let c = parse("[windows]\nagent_task = \"\\\\Tongue\"\n").unwrap();
        assert_eq!(c.windows.agent_task, r"\Tongue");
        assert_eq!(c.windows.agent_timeout_ms, 15_000);
    }

    #[test]
    fn khai_strategy_hotkey() {
        let c = parse("[macos]\nstrategy = \"hotkey\"\n").unwrap();
        assert_eq!(c.macos.strategy, "hotkey");
        assert_eq!(c.macos.backend, "gonhanh"); // vẫn là default
    }
}
