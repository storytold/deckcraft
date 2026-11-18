//! The media channel: content-addressed picture, font, audio, video and 3-D model bytes beside
//! the operation stream.
//!
//! An operation never carries media. The client uploads the bytes once, names them by `sha256`
//! and sends an `insert.*` operation that refers to them; the server stores one object per digest
//! and every replica fetches it from `GET /media/{sha256}`. This keeps an operation small (the
//! socket limit is 256 KiB), makes a repeated picture free, and lets a commit and a snapshot reuse
//! the same bytes.
//!
//! The cache is deliberately small and bounded: only pictures, fonts, audio, video and 3-D models
//! are accepted, each object is capped by its kind, and a session keeps a separate object/byte
//! budget for pictures, fonts, audio/video and models, so a participant cannot exhaust the
//! server's memory with uploads. The digest is always computed here and never trusted from a
//! caller.

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// Largest single picture the channel accepts.
pub const MAX_MEDIA_BYTES: usize = 8 * 1024 * 1024;
/// Largest single audio or video object.
pub const MAX_AV_BYTES: usize = 32 * 1024 * 1024;
/// Most pictures one session keeps.
pub const MAX_MEDIA_OBJECTS: usize = 512;
/// Most picture bytes one session keeps in total.
pub const MAX_MEDIA_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Most audio/video objects one session keeps.
pub const MAX_AV_OBJECTS: usize = 32;
/// Most audio/video bytes one session keeps in total.
pub const MAX_AV_TOTAL_BYTES: usize = 192 * 1024 * 1024;
/// Largest single embedded-font face. A face is drawn whole, not subsetted by this channel.
pub const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
/// Most font objects one session keeps.
pub const MAX_FONT_OBJECTS: usize = 32;
/// Most font bytes one session keeps in total.
pub const MAX_FONT_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Largest single 3-D model part (a binary glTF, `.glb`).
pub const MAX_MODEL_BYTES: usize = 32 * 1024 * 1024;
/// Most 3-D model objects one session keeps.
pub const MAX_MODEL_OBJECTS: usize = 16;
/// Most 3-D model bytes one session keeps in total.
pub const MAX_MODEL_TOTAL_BYTES: usize = 128 * 1024 * 1024;

/// What the media channel stores. The allowlist is the set the `.pptx` writer and the media probe
/// handle; anything else belongs to a later channel with its own limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    Picture,
    Video,
    Audio,
    /// One face of a font the document embeds (`p:embeddedFontLst`).
    Font,
    /// One 3-D model part (`p:contentPart` with a `model3d` relationship, a binary glTF).
    Model,
}

impl MediaType {
    /// Content type for the HTTP response, from the signature.
    pub fn content_type(self, bytes: &[u8]) -> &'static str {
        match self {
            MediaType::Picture => image_type("", bytes).unwrap_or("application/octet-stream"),
            MediaType::Video => video_type(bytes).unwrap_or("application/octet-stream"),
            MediaType::Audio => audio_type(bytes).unwrap_or("application/octet-stream"),
            MediaType::Font => font_type(bytes).unwrap_or(FONT_CONTENT_TYPE),
            MediaType::Model => model_type(bytes).unwrap_or(MODEL_CONTENT_TYPE),
        }
    }

    /// Largest object of this kind.
    pub fn max_bytes(self) -> usize {
        match self {
            MediaType::Picture => MAX_MEDIA_BYTES,
            MediaType::Video | MediaType::Audio => MAX_AV_BYTES,
            MediaType::Font => MAX_FONT_BYTES,
            MediaType::Model => MAX_MODEL_BYTES,
        }
    }

    /// The noun used in refusals.
    pub fn noun(self) -> &'static str {
        match self {
            MediaType::Picture => "picture",
            MediaType::Video => "video",
            MediaType::Audio => "audio",
            MediaType::Font => "font",
            MediaType::Model => "3-D model",
        }
    }
}

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

/// The content type a face is stored under; the `.pptx` writer names the part the same way.
pub const FONT_CONTENT_TYPE: &str = "application/x-fontdata";

/// The font file `bytes` hold, or `None` when the signature is not one this channel stores.
///
/// The allowlist is the set the `.pptx` writer and the font database read: TrueType, OpenType and
/// collections by their sfnt tag, and Embedded OpenType by its header. MicroType Express subsets
/// keep their `fntdata` extension but are not a plain sfnt; they are refused here (the `.pptx`
/// reader still round-trips them when a file already carries one).
pub fn font_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12
        && (bytes.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || bytes.starts_with(b"OTTO")
            || bytes.starts_with(b"true")
            || bytes.starts_with(b"typ1")
            || bytes.starts_with(b"ttcf"))
    {
        return Some(FONT_CONTENT_TYPE);
    }
    if bytes.len() >= 82 && u16::from_le_bytes([bytes[34], bytes[35]]) == 0x504C {
        return Some("application/vnd.ms-fontobject");
    }
    None
}

/// The content type a 3-D model is stored under; the `.pptx` writer names the part the same way.
pub const MODEL_CONTENT_TYPE: &str = "model/gltf-binary";

/// The 3-D model `bytes` hold, or `None` when they are not a binary glTF this channel stores.
///
/// The allowlist is one signature: the `glTF` magic, version 2, and at least the 12-byte header.
/// Office's 3-D models are binary glTF parts (`.glb`, `RT_MODEL3D`); a JSON `.gltf` and its
/// separate buffers are not a part the package can carry, so they are refused. The declared
/// length and the chunk table are left to the reader: the `.pptx` writer copies the part through
/// unchanged, so a model this build cannot tessellate is still a model to the package.
pub fn model_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && bytes.starts_with(b"glTF") && u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) == 2 {
        return Some(MODEL_CONTENT_TYPE);
    }
    None
}

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
/// fonts, arbitrary files) has its own check in [`media_type`].
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

/// The video container of `bytes`, or `None`.
pub fn video_type(bytes: &[u8]) -> Option<&'static str> {
    if is_ftyp(bytes) {
        return match iso_brand(bytes) {
            Some(b"M4A " | b"M4B " | b"M4P ") => Some("audio/mp4"),
            _ => Some("video/mp4"),
        };
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ") {
        return Some("video/x-msvideo");
    }
    None
}

/// The audio container of `bytes`, or `None` when the signature is not one this channel stores.
pub fn audio_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return Some("audio/wav");
    }
    if bytes.starts_with(b"FORM") && bytes.get(8..12) == Some(b"AIFF") {
        return Some("audio/aiff");
    }
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.starts_with(b"ID3") || bytes.first() == Some(&0xFF) && bytes.get(1).is_some_and(|b| b & 0xE0 == 0xE0 && *b != 0xD8) {
        return Some("audio/mpeg");
    }
    if bytes.starts_with(b"MThd") {
        return Some("audio/midi");
    }
    if is_ftyp(bytes) {
        return Some(match iso_brand(bytes) {
            Some(b"M4A " | b"M4B " | b"M4P ") => "audio/mp4",
            _ => "audio/mp4",
        });
    }
    None
}

fn is_ftyp(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

fn iso_brand(bytes: &[u8]) -> Option<&[u8; 4]> {
    bytes.get(8..12).and_then(|b| b.try_into().ok())
}

/// What the `bytes` are, by signature (and, for containers that share one, by name). `None` is a
/// file this channel does not store.
pub fn media_type(name: &str, bytes: &[u8]) -> Option<MediaType> {
    if image_type(name, bytes).is_some() {
        return Some(MediaType::Picture);
    }
    if font_type(bytes).is_some() {
        return Some(MediaType::Font);
    }
    if model_type(bytes).is_some() {
        return Some(MediaType::Model);
    }
    let lower = name.to_ascii_lowercase();
    let avi = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ");
    let webm = bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]);
    let ogg = bytes.starts_with(b"OggS");
    let mp4 = is_ftyp(bytes);
    let wav = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE");
    let flac = bytes.starts_with(b"fLaC");
    let aiff = bytes.starts_with(b"FORM") && bytes.get(8..12) == Some(b"AIFF");
    let mp3 = bytes.starts_with(b"ID3")
        || bytes.first() == Some(&0xFF) && bytes.get(1).is_some_and(|b| b & 0xE0 == 0xE0 && *b != 0xD8)
        || lower.ends_with(".mp3");
    let midi = bytes.starts_with(b"MThd");
    if wav || flac || aiff || midi || mp3 || (ogg && !lower.ends_with(".ogv")) {
        return Some(MediaType::Audio);
    }
    if avi || (webm && !lower.ends_with(".mka") && !lower.ends_with(".weba")) || (ogg && lower.ends_with(".ogv")) {
        return Some(MediaType::Video);
    }
    if mp4 {
        // `ftyp` covers video and audio-only MP4; the brand decides, the name breaks ties.
        let brand = iso_brand(bytes).copied().unwrap_or_default();
        let audio_brand = matches!(&brand, b"M4A " | b"M4B " | b"M4P " | b"mp4a");
        let video_name = [".mp4", ".m4v", ".mov"].iter().any(|ext| lower.ends_with(ext));
        let audio_name = [".m4a", ".m4b"].iter().any(|ext| lower.ends_with(ext));
        return Some(if audio_brand || (audio_name && !video_name) { MediaType::Audio } else { MediaType::Video });
    }
    if lower.ends_with(".mp4") || lower.ends_with(".mov") || lower.ends_with(".m4v") || lower.ends_with(".webm") {
        return Some(MediaType::Video);
    }
    if lower.ends_with(".m4a") || lower.ends_with(".aac") || lower.ends_with(".opus") {
        return Some(MediaType::Audio);
    }
    None
}

/// Media bytes available to one adapter.
#[derive(Clone, Debug, Default)]
pub struct MediaCache {
    objects: HashMap<String, (Arc<Vec<u8>>, MediaType)>,
    picture_bytes: usize,
    av_bytes: usize,
    font_bytes: usize,
    model_bytes: usize,
    pictures: usize,
    av_objects: usize,
    fonts: usize,
    models: usize,
}

impl MediaCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check the bytes and store them under their digest. Returns the digest.
    pub fn put(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, MediaError> {
        if bytes.is_empty() {
            return Err(MediaError::new("the media object is empty"));
        }
        let kind = media_type(name, &bytes)
            .ok_or_else(|| MediaError::new(format!("{name} is not a picture, font, video, audio or 3-D model file this channel stores")))?;
        self.check_size(kind, &bytes)?;
        let sha = sha256_hex(&bytes);
        self.insert_checked(kind, &sha, bytes)?;
        Ok(sha)
    }

    /// Store bytes that came from a trusted path (a journal replay or a local replica) under the
    /// digest they must have. The digest is recomputed: a mismatch is an error, not a rename.
    pub fn insert(&mut self, sha: &str, bytes: Vec<u8>) -> Result<(), MediaError> {
        if !valid_sha(sha) {
            return Err(MediaError::new("the media name is not a sha256 digest"));
        }
        if bytes.is_empty() {
            return Err(MediaError::new("the media object is empty"));
        }
        let kind = media_type("", &bytes)
            .ok_or_else(|| MediaError::new("the media bytes are not a picture, font, video, audio or 3-D model file this channel stores"))?;
        self.check_size(kind, &bytes)?;
        if sha256_hex(&bytes) != sha {
            return Err(MediaError::new("the media bytes do not match their digest"));
        }
        self.insert_checked(kind, sha, bytes)
    }

    fn check_size(&self, kind: MediaType, bytes: &[u8]) -> Result<(), MediaError> {
        let max = kind.max_bytes();
        if bytes.len() > max {
            return Err(MediaError::new(format!("the {} is larger than {max} bytes", kind.noun())));
        }
        Ok(())
    }

    fn insert_checked(&mut self, kind: MediaType, sha: &str, bytes: Vec<u8>) -> Result<(), MediaError> {
        if self.objects.contains_key(sha) {
            return Ok(());
        }
        let len = bytes.len();
        match kind {
            MediaType::Picture => {
                if self.pictures >= MAX_MEDIA_OBJECTS {
                    return Err(MediaError::new("this session holds too many pictures"));
                }
                if self.picture_bytes.saturating_add(len) > MAX_MEDIA_TOTAL_BYTES {
                    return Err(MediaError::new("this session holds too many picture bytes"));
                }
                self.pictures += 1;
                self.picture_bytes = self.picture_bytes.saturating_add(len);
            }
            MediaType::Video | MediaType::Audio => {
                if self.av_objects >= MAX_AV_OBJECTS {
                    return Err(MediaError::new("this session holds too many audio and video objects"));
                }
                if self.av_bytes.saturating_add(len) > MAX_AV_TOTAL_BYTES {
                    return Err(MediaError::new("this session holds too many audio and video bytes"));
                }
                self.av_objects += 1;
                self.av_bytes = self.av_bytes.saturating_add(len);
            }
            MediaType::Font => {
                if self.fonts >= MAX_FONT_OBJECTS {
                    return Err(MediaError::new("this session holds too many fonts"));
                }
                if self.font_bytes.saturating_add(len) > MAX_FONT_TOTAL_BYTES {
                    return Err(MediaError::new("this session holds too many font bytes"));
                }
                self.fonts += 1;
                self.font_bytes = self.font_bytes.saturating_add(len);
            }
            MediaType::Model => {
                if self.models >= MAX_MODEL_OBJECTS {
                    return Err(MediaError::new("this session holds too many 3-D models"));
                }
                if self.model_bytes.saturating_add(len) > MAX_MODEL_TOTAL_BYTES {
                    return Err(MediaError::new("this session holds too many 3-D model bytes"));
                }
                self.models += 1;
                self.model_bytes = self.model_bytes.saturating_add(len);
            }
        }
        self.objects.insert(sha.to_string(), (Arc::new(bytes), kind));
        Ok(())
    }

    /// The kind of a stored object, if this replica has it.
    pub fn kind(&self, sha: &str) -> Option<MediaType> {
        self.objects.get(sha).map(|(_, kind)| *kind)
    }

    /// The bytes named by `sha`, if this replica has them.
    pub fn get(&self, sha: &str) -> Option<Arc<Vec<u8>>> {
        self.objects.get(sha).map(|(bytes, _)| bytes.clone())
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
        self.picture_bytes.saturating_add(self.av_bytes).saturating_add(self.font_bytes).saturating_add(self.model_bytes)
    }

    /// Every stored object, in digest order, for persistence and tests.
    pub fn entries(&self) -> Vec<(String, Arc<Vec<u8>>)> {
        let mut out: Vec<(String, Arc<Vec<u8>>)> = self.objects.iter().map(|(sha, (bytes, _))| (sha.clone(), bytes.clone())).collect();
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
    fn audio_and_video_are_recognised_by_signature() {
        let mut mp4 = vec![0, 0, 0, 0x18];
        mp4.extend(b"ftypisom\0\0\0\0");
        assert_eq!(media_type("clip.mp4", &mp4), Some(MediaType::Video));
        let mut m4a = vec![0, 0, 0, 0x18];
        m4a.extend(b"ftypM4A \0\0\0\0");
        assert_eq!(media_type("song.m4a", &m4a), Some(MediaType::Audio));
        assert_eq!(media_type("a.wav", b"RIFF\0\0\0\0WAVEfmt "), Some(MediaType::Audio));
        assert_eq!(media_type("a.flac", b"fLaC\0\0\0\0"), Some(MediaType::Audio));
        assert_eq!(media_type("v.webm", &[0x1A, 0x45, 0xDF, 0xA3, 0, 0, 0]), Some(MediaType::Video));
        assert_eq!(media_type("v.avi", b"RIFF\0\0\0\0AVI LIST"), Some(MediaType::Video));
        assert_eq!(media_type("a.mp3", b"ID3\x03\0\0"), Some(MediaType::Audio));
        assert_eq!(media_type("a.txt", b"not media"), None);
        assert_eq!(MediaType::Audio.content_type(b"OggS\0\0"), "audio/ogg");
        assert_eq!(MediaType::Video.content_type(&mp4), "video/mp4");
    }

    #[test]
    fn a_font_is_recognised_by_its_sfnt_tag_and_has_its_own_budget() {
        let ttf = include_bytes!("../../render/tests/fixtures/colr-squares.ttf");
        assert_eq!(media_type("face.ttf", ttf), Some(MediaType::Font));
        assert_eq!(media_type("face.otf", b"OTTO\0\0\0\0\0\0\0\0"), Some(MediaType::Font));
        // An sfnt tag alone is not a readable font, but the channel stores what the writer can
        // carry; the engine decides whether it can be embedded.
        assert_eq!(media_type("x", b"ttcf\0\0\0\0\0\0\0\0"), Some(MediaType::Font));
        assert_eq!(media_type("a.txt", b"not a font"), None);
        let mut media = MediaCache::new();
        let sha = media.put("face.ttf", ttf.to_vec()).expect("font");
        assert_eq!(media.kind(&sha), Some(MediaType::Font));
        assert_eq!(MediaType::Font.content_type(ttf), "application/x-fontdata");
        assert_eq!(MediaType::Font.noun(), "font");
        assert!(media.bytes() >= ttf.len(), "a font counts against the byte total");
        assert!(media.put("a.txt", b"not a picture or a font".to_vec()).is_err());
        assert_eq!(MediaType::Picture.max_bytes(), MAX_MEDIA_BYTES);
        assert_eq!(MediaType::Font.max_bytes(), MAX_FONT_BYTES);
    }

    /// One binary glTF header: the magic, version 2 and a declared length.
    fn glb() -> Vec<u8> {
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(20u32.to_le_bytes());
        bytes.extend([0u8; 8]);
        bytes
    }

    #[test]
    fn a_3d_model_is_one_glb_signature_with_its_own_budget() {
        let glb = glb();
        assert_eq!(media_type("block.glb", &glb), Some(MediaType::Model));
        assert_eq!(media_type("", &glb), Some(MediaType::Model), "a replay has no name: the signature alone decides");
        assert_eq!(media_type("block.glb", b"not a model"), None, "the name alone decides nothing");
        let mut v1 = glb.clone();
        v1[4] = 1;
        assert_eq!(media_type("block.glb", &v1), None, "only glTF 2 is a part Office writes");
        assert_eq!(media_type("block.glb", b"glTF"), None, "a header shorter than 12 bytes is not a model");

        let mut media = MediaCache::new();
        let sha = media.put("block.glb", glb.clone()).expect("model");
        assert_eq!(media.kind(&sha), Some(MediaType::Model));
        assert_eq!(MediaType::Model.content_type(&glb), "model/gltf-binary");
        assert_eq!(MediaType::Model.noun(), "3-D model");
        assert_eq!(MediaType::Model.max_bytes(), MAX_MODEL_BYTES);
        assert!(media.bytes() >= glb.len(), "a model counts against the byte total");

        // Bigger than the model cap, still a model by signature.
        let mut huge = glb.clone();
        huge.resize(MAX_MODEL_BYTES + 1, 0);
        assert!(media.put("huge.glb", huge).is_err(), "one model is bounded");

        for i in 0..MAX_MODEL_OBJECTS - 1 {
            let mut bytes = glb.clone();
            bytes.extend([i as u8, (i >> 8) as u8]);
            media.put("block.glb", bytes).expect("under the model object cap");
        }
        let mut one_more = glb.clone();
        one_more.extend([0xFF, 0xFE]);
        assert!(media.put("block.glb", one_more).is_err(), "the model budget is full");
        // Pictures keep their own budget: 16 models leave the picture channel untouched.
        media.put("a.png", PNG.to_vec()).expect("a picture is not a model object");
        assert_eq!(media.kind(&sha256_hex(PNG)), Some(MediaType::Picture));
    }

    #[test]
    fn the_same_audio_bytes_with_and_without_a_name_are_one_object() {
        let mut media = MediaCache::new();
        let wav = b"RIFF\x24\0\0\0WAVEfmt ".to_vec();
        let sha = media.put("hit.wav", wav.clone()).expect("wav");
        assert_eq!(media.kind(&sha), Some(MediaType::Audio));
        media.insert(&sha, wav).expect("a replay has no name");
        assert_eq!(media.len(), 1);
    }

    #[test]
    fn audio_and_video_have_their_own_budget() {
        let mut media = MediaCache::new();
        assert!(media.put("big.mp3", vec![0u8; MAX_AV_BYTES + 1]).is_err());
        let mut png = PNG.to_vec();
        png.extend(std::iter::repeat_n(0u8, 16));
        for i in 0..MAX_MEDIA_OBJECTS {
            let mut bytes = png.clone();
            bytes.push(i as u8);
            bytes.push((i >> 8) as u8);
            media.put("a.png", bytes).expect("a picture is not an audio object");
        }
        assert!(media.put("no_room_left.png", png).is_err(), "the picture budget is full");
        // Pictures and audio/video have separate budgets.
        let mut wav = b"RIFF\x0c\0\0\0WAVEfmt ".to_vec();
        wav.push(0);
        media.put("hit.wav", wav.clone()).expect("still room for audio");
        for i in 0..MAX_AV_OBJECTS {
            let mut bytes = b"RIFF\x0c\0\0\0WAVEfmt ".to_vec();
            bytes.extend([i as u8, (i >> 8) as u8]);
            let result = media.put("hit.wav", bytes);
            if i + 1 < MAX_AV_OBJECTS {
                result.expect("under the audio cap");
            } else {
                assert!(result.is_err(), "the audio budget is full");
            }
        }
    }

    #[test]
    fn the_picture_cache_is_bounded() {
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
