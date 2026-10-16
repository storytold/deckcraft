//! Slide keys (R4): ULIDs, 26 upper-case Crockford base-32 characters (48-bit millisecond time,
//! 80 random bits). Clock and randomness are injectable so tests get fixed keys.

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Makes slide keys.
pub struct KeyGen {
    clock: Box<dyn FnMut() -> u64>,
    rand: Box<dyn FnMut() -> u128>,
}

impl KeyGen {
    /// Keys from the given clock (milliseconds since 1970) and random source.
    pub fn new(clock: impl FnMut() -> u64 + 'static, rand: impl FnMut() -> u128 + 'static) -> Self {
        KeyGen { clock: Box::new(clock), rand: Box::new(rand) }
    }

    /// The system clock and randomness seeded by the standard library's hasher keys. On wasm
    /// (no clock in `std`) the time part is zero; the browser build doesn't save folders.
    pub fn system() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let state = std::collections::hash_map::RandomState::new();
        let mut counter: u64 = 0;
        KeyGen::new(now_ms, move || {
            let mut word = |salt: u64| {
                counter = counter.wrapping_add(1);
                let mut h = state.build_hasher();
                h.write_u64(counter);
                h.write_u64(salt);
                h.write_u64(now_ns());
                h.finish()
            };
            (u128::from(word(1)) << 64) | u128::from(word(2))
        })
    }

    /// Deterministic: a fixed time and a counter for the random part (tests).
    pub fn fixed(ms: u64) -> Self {
        let mut n: u128 = 0;
        KeyGen::new(
            move || ms,
            move || {
                n = n.wrapping_add(1);
                n
            },
        )
    }

    pub fn key(&mut self) -> String {
        let t = u128::from((self.clock)() & 0xFFFF_FFFF_FFFF);
        let r = (self.rand)() & ((1u128 << 80) - 1);
        encode((t << 80) | r)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
#[cfg(not(target_arch = "wasm32"))]
fn now_ns() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}
#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    0
}
#[cfg(target_arch = "wasm32")]
fn now_ns() -> u64 {
    0
}

fn encode(mut v: u128) -> String {
    let mut out = [b'0'; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD.get((v & 31) as usize).copied().unwrap_or(b'0');
        v >>= 5;
    }
    out.iter().map(|&b| char::from(b)).collect()
}

/// A valid slide key: 26 upper-case Crockford characters, the first at most `7`.
pub fn is_key(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| CROCKFORD.contains(&b)) && s.as_bytes().first().is_some_and(|b| *b <= b'7')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_is_deterministic_and_valid() {
        let mut g = KeyGen::fixed(1_760_000_000_000);
        let a = g.key();
        let b = g.key();
        assert_ne!(a, b);
        assert!(is_key(&a) && is_key(&b), "{a} {b}");
        assert_eq!(KeyGen::fixed(1_760_000_000_000).key(), a);
        assert!(a.starts_with("01K"), "{a}");
    }

    #[test]
    fn system_keys_differ() {
        let mut g = KeyGen::system();
        let keys: std::collections::HashSet<_> = (0..1000).map(|_| g.key()).collect();
        assert_eq!(keys.len(), 1000);
        assert!(keys.iter().all(|k| is_key(k)));
        // Two generators started at the same time don't repeat each other.
        assert_ne!(KeyGen::system().key(), KeyGen::system().key());
    }

    #[test]
    fn rejects_bad_keys() {
        for k in ["", "01JA7Q3M5K8X9Y2Z3A4B5C6D7", "01JA7Q3M5K8X9Y2Z3A4B5C6D7I", "81JA7Q3M5K8X9Y2Z3A4B5C6D7E", "01ja7q3m5k8x9y2z3a4b5c6d7e"] {
            assert!(!is_key(k), "{k}");
        }
        assert!(is_key("01JA7Q3M5K8X9Y2Z3A4B5C6D7E"));
    }
}
