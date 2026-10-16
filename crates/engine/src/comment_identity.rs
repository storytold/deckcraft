//! Persistent comment wire identities use OS / Web Crypto entropy, never names or clocks.
use crate::{EngineError, Result};
pub(crate) fn fresh_guid() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| EngineError::Other(format!("cannot create comment identity: {error}")))?;
    if let Some(version) = bytes.get_mut(6) {
        *version = (*version & 0x0f) | 0x40;
    }
    if let Some(variant) = bytes.get_mut(8) {
        *variant = (*variant & 0x3f) | 0x80;
    }
    let mut result = String::with_capacity(38);
    result.push('{');
    use std::fmt::Write;
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            result.push('-');
        }
        write!(&mut result, "{byte:02X}").map_err(|_| EngineError::Other("cannot encode comment identity".into()))?;
    }
    result.push('}');
    Ok(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn identities_are_distinct_valid_v4_records() {
        let a = super::fresh_guid().unwrap();
        let b = super::fresh_guid().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 38);
        assert_eq!(a.get(15..16), Some("4"));
        assert!(matches!(a.get(20..21), Some("8" | "9" | "A" | "B")));
    }
}
