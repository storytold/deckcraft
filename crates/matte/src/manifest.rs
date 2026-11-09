//! The installed model manifest and its verification.
//!
//! The weights are never in the repository. `MODEL_ID` names the directory under the model root;
//! [`MANIFEST`] records the exact repository revision, file, byte size, SHA-256 and licence so an
//! install can be verified before use and the download prompt can tell the user what it gets.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Where inference runs. The default is CPU everywhere; an optional backend can add an
/// accelerator behind the same seam without changing the feature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Accelerator {
    /// Pure-Rust CPU inference, on every platform this app builds for.
    #[default]
    Cpu,
    /// Intel OpenVINO (an optional future backend, not in this build).
    OpenVino,
}

impl Accelerator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Accelerator::Cpu => "cpu",
            Accelerator::OpenVino => "openvino",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Accelerator::Cpu => "CPU",
            Accelerator::OpenVino => "OpenVINO (Intel NPU/GPU)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(Accelerator::Cpu),
            "openvino" | "npu" | "gpu" => Some(Accelerator::OpenVino),
            _ => None,
        }
    }
}

/// What this build can run. OpenVINO is deliberately absent: it would need a C/C++ runtime, and
/// this PR must not add one.
pub const SUPPORTED_ACCELERATORS: &[Accelerator] = &[Accelerator::Cpu];

/// The message a request for an unavailable accelerator gets.
pub const ACCELERATOR_UNAVAILABLE: &str = "runs on CPU; OpenVINO NPU/GPU not available";

/// Directory name under the model root.
pub const MODEL_ID: &str = "birefnet-lite-512";
/// Hugging Face repository.
pub const MODEL_REPO: &str = "studioludens/birefnet-lite-512";
/// Exact revision the size and hash were measured at.
pub const MODEL_REVISION: &str = "962e67969ec6acec9c19fab76acee4f5e22e990c";
/// The ONNX file inside the repository.
pub const MODEL_FILE: &str = "onnx/model.onnx";
/// The model's licence, as its repository states it.
pub const MODEL_LICENSE: &str = "MIT";
/// The input side the model takes and returns.
pub const INPUT_SIZE: u32 = 512;
/// Bytes the file must be.
pub const MODEL_BYTES: u64 = 191_877_254;
/// SHA-256 of the file, lowercase hexadecimal.
pub const MODEL_SHA256: &str = "1cb0fb360dadd15af77c639085d77a9df67db0c64315560c3de005f676345ac2";

/// One model a build knows how to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub id: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    /// Path inside the model directory.
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    /// Side the model runs at.
    pub input_size: u32,
    /// The runtime this build uses for the model (`rten`).
    pub runtime: &'static str,
    /// The accelerator a request uses when none is named.
    pub default_accelerator: Accelerator,
    /// Every accelerator this build implements.
    pub accelerators: &'static [Accelerator],
}

/// The camera-background model.
pub const MANIFEST: Manifest = Manifest {
    id: MODEL_ID,
    repo: MODEL_REPO,
    revision: MODEL_REVISION,
    file: MODEL_FILE,
    bytes: MODEL_BYTES,
    sha256: MODEL_SHA256,
    license: MODEL_LICENSE,
    input_size: INPUT_SIZE,
    runtime: "rten",
    default_accelerator: Accelerator::Cpu,
    accelerators: SUPPORTED_ACCELERATORS,
};

impl Manifest {
    /// The HTTPS URL a consenting user downloads from, pinned to the recorded revision.
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{}", self.repo, self.revision, self.file)
    }

    /// Where the file lives under `root`.
    pub fn path_in(&self, root: &Path) -> PathBuf {
        root.join(self.id).join(self.file)
    }

    /// Check a downloaded file's size and hash before anything runs it. `sha256_hex` is compared
    /// case-insensitively; a wrong length is reported before the hash is looked at.
    pub fn verify(&self, bytes: u64, sha256_hex: &str) -> Result<(), ManifestError> {
        if bytes != self.bytes {
            return Err(ManifestError::Size { expected: self.bytes, found: bytes });
        }
        if !is_hex_sha256(sha256_hex) {
            return Err(ManifestError::BadHash);
        }
        if !sha256_hex.trim().eq_ignore_ascii_case(self.sha256) {
            return Err(ManifestError::Hash { expected: self.sha256.to_string(), found: sha256_hex.trim().to_string() });
        }
        Ok(())
    }
}

/// Why an install was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("model has {found} bytes, expected {expected}")]
    Size { expected: u64, found: u64 },
    #[error("model SHA-256 is {found}, expected {expected}")]
    Hash { expected: String, found: String },
    #[error("model SHA-256 is not 64 hexadecimal characters")]
    BadHash,
    #[error("cannot read model: {0}")]
    Io(String),
}

/// Does this string look like a SHA-256 digest (64 hex characters, no prefix)?
pub fn is_hex_sha256(s: &str) -> bool {
    let s = s.trim();
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Verify an installed file: its size and SHA-256 against [`MANIFEST`]. The size is checked first,
/// so an oversized file (a video the user picked by mistake, a partly written copy) is refused
/// without reading all of it. Reads in bounded chunks, so a 192 MB model never sits in memory
/// twice.
pub fn verify_file(path: &Path) -> Result<(), ManifestError> {
    let meta = std::fs::metadata(path).map_err(|e| ManifestError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(ManifestError::Io(format!("{} is not a file", path.display())));
    }
    if meta.len() != MANIFEST.bytes {
        return Err(ManifestError::Size { expected: MANIFEST.bytes, found: meta.len() });
    }
    MANIFEST.verify(meta.len(), &sha256_file(path)?)
}

/// Read a file entirely, checking its size before reading it and hashing the bytes that were read.
/// The caller parses the returned bytes, so the file is opened once: a replacement between a hash
/// and a reopen cannot slip unverified weights past the check.
pub fn read_verified(path: &Path) -> Result<Vec<u8>, ManifestError> {
    let mut f = std::fs::File::open(path).map_err(|e| ManifestError::Io(e.to_string()))?;
    let meta = f.metadata().map_err(|e| ManifestError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(ManifestError::Io(format!("{} is not a file", path.display())));
    }
    if meta.len() != MANIFEST.bytes {
        return Err(ManifestError::Size { expected: MANIFEST.bytes, found: meta.len() });
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(MANIFEST.bytes as usize).map_err(|_| ManifestError::Io("not enough memory for the model".into()))?;
    f.read_to_end(&mut bytes).map_err(|e| ManifestError::Io(e.to_string()))?;
    if bytes.len() as u64 != MANIFEST.bytes {
        return Err(ManifestError::Size { expected: MANIFEST.bytes, found: bytes.len() as u64 });
    }
    MANIFEST.verify(bytes.len() as u64, &sha256_hex(&bytes))?;
    Ok(bytes)
}

/// SHA-256 of a file, read in 1 MiB chunks.
pub fn sha256_file(path: &Path) -> Result<String, ManifestError> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| ManifestError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| ManifestError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    let out = hasher.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
}

/// SHA-256 of a byte slice, lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut s = String::with_capacity(64);
    for b in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_matches_the_measured_install() {
        assert_eq!(MANIFEST.id, "birefnet-lite-512");
        assert_eq!(MANIFEST.revision, "962e67969ec6acec9c19fab76acee4f5e22e990c");
        assert_eq!(MANIFEST.bytes, 191_877_254);
        assert_eq!(MANIFEST.input_size, 512);
        assert_eq!(MANIFEST.license, "MIT");
        assert_eq!(MANIFEST.runtime, "rten");
        assert_eq!(MANIFEST.default_accelerator, Accelerator::Cpu);
        assert_eq!(MANIFEST.accelerators, [Accelerator::Cpu]);
        assert_eq!(Accelerator::parse("OPENVINO"), Some(Accelerator::OpenVino));
        assert_eq!(Accelerator::parse("npu"), Some(Accelerator::OpenVino));
        assert_eq!(Accelerator::parse("cuda"), None);
        assert!(MANIFEST.url().ends_with("/962e67969ec6acec9c19fab76acee4f5e22e990c/onnx/model.onnx"));
        assert!(is_hex_sha256(MANIFEST.sha256));
    }

    #[test]
    fn a_wrong_size_or_hash_is_rejected_before_use() {
        assert_eq!(MANIFEST.verify(MODEL_BYTES - 1, MODEL_SHA256), Err(ManifestError::Size { expected: MODEL_BYTES, found: MODEL_BYTES - 1 }));
        let wrong = "0".repeat(64);
        assert!(matches!(MANIFEST.verify(MODEL_BYTES, &wrong), Err(ManifestError::Hash { .. })));
        assert_eq!(MANIFEST.verify(MODEL_BYTES, "not-a-hash"), Err(ManifestError::BadHash));
        assert!(MANIFEST.verify(MODEL_BYTES, &MODEL_SHA256.to_uppercase()).is_ok());
    }

    #[test]
    fn the_hash_of_known_bytes_is_known() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    /// The size is checked from the metadata first: an oversized file is refused without being
    /// read, and a short file is a size error rather than a hash of bytes that could never match.
    #[test]
    fn verification_checks_the_size_before_hashing() {
        let dir = std::env::temp_dir();
        let short = dir.join(format!("deckcraft-matte-verify-short-{}.onnx", std::process::id()));
        std::fs::write(&short, b"short").expect("file");
        assert!(matches!(verify_file(&short), Err(ManifestError::Size { .. })), "{:?}", verify_file(&short));
        assert!(matches!(read_verified(&short), Err(ManifestError::Size { .. })), "read_verified refuses before reading");
        let _ = std::fs::remove_file(&short);

        // A sparse file one byte over the manifest's size: no data is read to make it.
        let over = dir.join(format!("deckcraft-matte-verify-over-{}.onnx", std::process::id()));
        std::fs::File::create(&over).and_then(|f| f.set_len(MODEL_BYTES + 1)).expect("sparse file");
        assert!(matches!(verify_file(&over), Err(ManifestError::Size { .. })), "an oversized file is not hashed");
        let _ = std::fs::remove_file(&over);
    }
}
