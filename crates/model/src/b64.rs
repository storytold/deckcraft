//! Minimal base64 for serde: the opaque byte blobs the model keeps in JSON (VBA projects).

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serializer};

const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3).saturating_mul(4));
    for chunk in data.chunks(3) {
        let b0 = chunk.first().copied().unwrap_or(0) as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        for i in 0..4usize {
            if i <= chunk.len() {
                let k = ((n >> (18 - 6 * i as u32)) & 63) as usize;
                out.push(A.get(k).copied().unwrap_or(b'A') as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `None` for anything that isn't base64 (whitespace and padding are ignored).
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = A.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Base64 for a shared byte blob.
pub mod arc {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Arc<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<u8>>, D::Error> {
        let s = String::deserialize(d)?;
        decode(&s).map(Arc::new).ok_or_else(|| serde::de::Error::custom("invalid base64"))
    }
}

/// Base64 for an optional shared byte blob: `null`/missing stays `None`.
pub mod opt_arc {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Option<Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(data) => s.serialize_str(&encode(data)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Vec<u8>>>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(s) => decode(&s).map(|b| Some(Arc::new(b))).ok_or_else(|| serde::de::Error::custom("invalid base64")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"hello world", &[0, 255, 128, 7]] {
            assert_eq!(super::decode(&super::encode(data)).unwrap(), data);
        }
        assert_eq!(super::encode(b"Man"), "TWFu");
        assert_eq!(super::decode("not base64!"), None);
    }
}
