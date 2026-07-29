//! Parser trạng thái VKey từ section `Local\VKeySharedState`.
//! Thuần bytes — không FFI — nên test được trên mọi nền tảng.
//! Layout đóng băng bởi static_assert trong VKey (SharedState.h, structVersion 4).

/// "NKEY" đọc little-endian.
pub const VKEY_MAGIC: u32 = 0x5945_4B4E;
pub const MAX_STRUCT_VERSION: u32 = 4;
const FLAGS_OFFSET: usize = 16;
const FLAG_VIETNAMESE: u32 = 0x0001;

#[derive(Debug, PartialEq, Eq)]
pub enum ShmError {
    TooShort(usize),
    BadMagic(u32),
    UnsupportedVersion(u32),
}

impl std::fmt::Display for ShmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShmError::TooShort(n) => write!(f, "shared memory quá ngắn ({n} byte)"),
            ShmError::BadMagic(m) => write!(f, "magic không khớp (0x{m:08X}) — không phải VKey?"),
            ShmError::UnsupportedVersion(v) => {
                write!(f, "structVersion {v} mới hơn mức tongue hiểu ({MAX_STRUCT_VERSION}) — VKey vừa nâng cấp?")
            }
        }
    }
}

impl std::error::Error for ShmError {}

pub fn parse_vietnamese_flag(bytes: &[u8]) -> Result<bool, ShmError> {
    if bytes.len() < FLAGS_OFFSET + 4 {
        return Err(ShmError::TooShort(bytes.len()));
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let magic = u32_at(0);
    if magic != VKEY_MAGIC {
        return Err(ShmError::BadMagic(magic));
    }
    let version = u32_at(4);
    if version > MAX_STRUCT_VERSION {
        return Err(ShmError::UnsupportedVersion(version));
    }
    Ok(u32_at(FLAGS_OFFSET) & FLAG_VIETNAMESE != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dựng buffer 20 byte đúng layout section Local\VKeySharedState.
    fn shm(magic: u32, version: u32, flags: u32) -> Vec<u8> {
        let mut b = vec![0u8; 20];
        b[0..4].copy_from_slice(&magic.to_le_bytes());
        b[4..8].copy_from_slice(&version.to_le_bytes());
        b[16..20].copy_from_slice(&flags.to_le_bytes());
        b
    }

    #[test]
    fn doc_bit_vietnamese() {
        assert_eq!(parse_vietnamese_flag(&shm(VKEY_MAGIC, 4, 0x0001)), Ok(true));
        assert_eq!(parse_vietnamese_flag(&shm(VKEY_MAGIC, 4, 0x0000)), Ok(false));
        // bit khác bật không ảnh hưởng bit 0
        assert_eq!(parse_vietnamese_flag(&shm(VKEY_MAGIC, 2, 0x0102)), Ok(false));
    }

    #[test]
    fn magic_sai_thi_tu_choi() {
        assert_eq!(
            parse_vietnamese_flag(&shm(0xDEADBEEF, 4, 1)),
            Err(ShmError::BadMagic(0xDEADBEEF))
        );
    }

    #[test]
    fn version_moi_hon_thi_tu_choi() {
        assert_eq!(
            parse_vietnamese_flag(&shm(VKEY_MAGIC, 5, 1)),
            Err(ShmError::UnsupportedVersion(5))
        );
    }

    #[test]
    fn buffer_ngan_thi_tu_choi() {
        assert_eq!(parse_vietnamese_flag(&[0u8; 10]), Err(ShmError::TooShort(10)));
    }
}
