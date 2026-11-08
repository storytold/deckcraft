//! The media channel: content-addressed picture bytes beside the operation stream.
//!
//! An operation never carries a picture. The client uploads the bytes once, names them by
//! `sha256` and sends a `picture.insert` that refers to them; the server stores one object per
//! digest and every replica fetches it from `GET /media/{sha256}`. This keeps an operation small
//! (the socket limit is 256 KiB), makes a repeated picture free, and lets a commit and a snapshot
//! reuse the same bytes.
//!
//! The cache is deliberately small and bounded: only images are accepted, one object is at most
//! [`MAX_MEDIA_BYTES`], and a session keeps at most [`MAX_MEDIA_OBJECTS`] /
//! [`MAX_MEDIA_TOTAL_BYTES`], so a participant cannot exhaust the server's memory with uploads.
//! The digest is always computed here and never trusted from a caller.

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// Largest single object the channel accepts.
pub const MAX_MEDIA_BYTES: usize = 8 * 1024 * 1024;
/// Most objects one session keeps.
pub const MAX_MEDIA_OBJECTS: usize = 512;
/// Most bytes one session keeps in total.
pub const MAX_MEDIA_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// Why bytes were refused by the media channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaError {
    pub message: String,
}

impl MediaError {
    fn new(message: impl Into<String>) -> Self {
        MediaError { message: message.into() }
    }
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MediaError {}

/// Lowercase hex digest of `bytes`, the object's name on the wire and on disk.
pub fn sha256_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Whether `sha` is a digest this channel could have issued: 64 lowercase hex characters.
pub fn valid_sha(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The image type of `bytes`, or `None` when they are not a picture this channel stores.
///
/// The allowlist is the set the `.pptx` writer and the renderer handle; anything else (video,
/// fonts, arbitrary files) belongs to a later channel with its own limits.
pub fn image_type(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("image/tiff")
    } else if lower.ends_with(".svg") || bytes.windows(4).take(512).any(|w| w == b"<svg") {
        Some("image/svg+xml")
    } else {
        None
    }
}

/// Picture bytes available to one adapter.
#[derive(Clone, Debug, Default)]
pub struct MediaCache {
    objects: HashMap<String, Arc<Vec<u8>>>,
    bytes: usize,
}

impl MediaCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check the bytes and store them under their digest. Returns the digest.
    pub fn put(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, MediaError> {
        if bytes.is_empty() {
            return Err(MediaError::new("the picture is empty"));
        }
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err(MediaError::new(format!("the picture is larger than {MAX_MEDIA_BYTES} bytes")));
        }
        if image_type(name, &bytes).is_none() {
            return Err(MediaError::new(format!("{name} is not a picture this channel stores")));
        }
        let sha = sha256_hex(&bytes);
        self.insert_checked(&sha, bytes)?;
        Ok(sha)
    }

    /// Store bytes that came from a trusted path (a journal replay or a local replica) under the
    /// digest they must have. The digest is recomputed: a mismatch is an error, not a rename.
    pub fn insert(&mut self, sha: &str, bytes: Vec<u8>) -> Result<(), MediaError> {
        if !valid_sha(sha) {
            return Err(MediaError::new("the media name is not a sha256 digest"));
        }
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err(MediaError::new(format!("the picture is larger than {MAX_MEDIA_BYTES} bytes")));
        }
        if sha256_hex(&bytes) != sha {
            return Err(MediaError::new("the media bytes do not match their digest"));
        }
        self.insert_checked(sha, bytes)
    }

    fn insert_checked(&mut self, sha: &str, bytes: Vec<u8>) -> Result<(), MediaError> {
        if self.objects.contains_key(sha) {
            return Ok(());
        }
        if self.objects.len() >= MAX_MEDIA_OBJECTS {
            return Err(MediaError::new("this session holds too many pictures"));
        }
        let total = self.bytes.saturating_add(bytes.len());
        if total > MAX_MEDIA_TOTAL_BYTES {
            return Err(MediaError::new("this session holds too many picture bytes"));
        }
        self.bytes = total;
        self.objects.insert(sha.to_string(), Arc::new(bytes));
        Ok(())
    }

    /// The bytes named by `sha`, if this replica has them.
    pub fn get(&self, sha: &str) -> Option<Arc<Vec<u8>>> {
        self.objects.get(sha).cloned()
    }

    pub fn contains(&self, sha: &str) -> bool {
        self.objects.contains_key(sha)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Total bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Every stored object, in digest order, for persistence and tests.
    pub fn entries(&self) -> Vec<(String, Arc<Vec<u8>>)> {
        let mut out: Vec<(String, Arc<Vec<u8>>)> = self.objects.iter().map(|(sha, bytes)| (sha.clone(), bytes.clone())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn a_digest_names_the_bytes_and_the_name_is_stable() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(valid_sha(&sha256_hex(PNG)));
        assert!(!valid_sha("9f2c"));
        assert!(!valid_sha("E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"));
    }

    #[test]
    fn only_images_are_stored_and_the_same_picture_is_one_object() {
        let mut media = MediaCache::new();
        let sha = media.put("a.png", PNG.to_vec()).expect("png");
        assert_eq!(media.put("b.png", PNG.to_vec()).expect("same bytes"), sha);
        assert_eq!(media.len(), 1);
        assert_eq!(media.get(&sha).expect("stored").as_slice(), PNG);
        assert!(media.put("a.txt", b"not a picture".to_vec()).is_err());
        assert!(media.put("empty.png", Vec::new()).is_err());
    }

    #[test]
    fn a_name_that_does_not_match_its_bytes_is_refused() {
        let mut media = MediaCache::new();
        assert!(media.insert(&"0".repeat(64), PNG.to_vec()).is_err());
        let sha = sha256_hex(PNG);
        media.insert(&sha, PNG.to_vec()).expect("matching");
        assert!(media.contains(&sha));
    }

    #[test]
    fn the_cache_is_bounded() {
        let mut media = MediaCache::new();
        let too_big = vec![0u8; MAX_MEDIA_BYTES + 1];
        assert!(media.put("big.png", too_big).is_err());
        let mut png = PNG.to_vec();
        png.extend(std::iter::repeat_n(0u8, 16));
        // Distinct bytes, still images by signature.
        for i in 0..MAX_MEDIA_OBJECTS {
            let mut bytes = png.clone();
            bytes.push(i as u8);
            bytes.push((i >> 8) as u8);
            media.put("a.png", bytes).expect("under the object cap");
        }
        let mut one_more = png.clone();
        one_more.extend([0xFF, 0xFE]);
        assert!(media.put("a.png", one_more).is_err());
    }
}
