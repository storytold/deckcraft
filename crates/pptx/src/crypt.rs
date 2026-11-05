//! ECMA-376 / MS-OFFCRYPTO presentation package decryption.
//!
//! Supports both:
//! - Standard Encryption (Office 2007, version 2..=4, minor 2; AES-128-ECB, SHA-1)
//! - Agile Encryption (Office 2010+, version 4.4; AES-128/256-CBC, SHA-1/256/384/512, spin count)
//!
//! Key material derived from the password (the UTF-16 password, every intermediate hash and the
//! AES keys) is held in `Zeroizing` buffers and wiped when it drops.

use aes::cipher::{BlockCipherDecrypt, KeyInit};
use sha1::Digest;
use zeroize::Zeroizing;

use crate::PptxError;
use crate::xml;

const MAX_INFO: u64 = 1 << 20; // 1 MiB
const MAX_SPIN_COUNT: u32 = 10_000_000;
const SEGMENT: usize = 4096;
const BLOCK: usize = 16;

const BK_VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const BK_VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const BK_KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const BK_HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const BK_HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// Decrypt the zip package inside an encrypted Office presentation.
///
/// A caller that knows the file is encrypted but holds no password is told so before the container
/// is parsed: an unreadable or hostile container is a question about the password first, and the
/// caller's answer is the text the user typed.
pub fn decrypt(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>, PptxError> {
    let Some(password) = password else {
        return Err(PptxError::PasswordRequired);
    };
    let comp = crate::cfb::Compound::open(bytes).map_err(|e| PptxError::Encrypted(format!("unreadable compound file ({e})")))?;
    let info = comp
        .root_stream_capped("EncryptionInfo", MAX_INFO)
        .map_err(PptxError::Encrypted)?
        .ok_or_else(|| PptxError::Encrypted("missing EncryptionInfo stream".into()))?;
    let package = comp
        .root_stream_capped("EncryptedPackage", bytes.len() as u64)
        .map_err(PptxError::Encrypted)?
        .ok_or_else(|| PptxError::Encrypted("missing EncryptedPackage stream".into()))?;

    let (major, minor) = match info.get(..4) {
        Some([a, b, c, d]) => (u16::from_le_bytes([*a, *b]), u16::from_le_bytes([*c, *d])),
        _ => return Err(PptxError::Encrypted("EncryptionInfo is truncated".into())),
    };

    match (major, minor) {
        (2..=4, 2) => decrypt_standard(&info, &package, password),
        (4, 4) => decrypt_agile(&info, &package, password),
        _ => Err(PptxError::Encrypted(format!("unsupported encryption version {major}.{minor}"))),
    }
}

// -------------------------------------------------------------------------------------------------
// Standard Encryption (MS-OFFCRYPTO §2.3.4.5 - §2.3.4.7)

/// MS-OFFCRYPTO §2.3.4.6 algorithm identifiers.
const ALG_ID_AES128: u32 = 0x0000_660E;
const ALG_ID_AES192: u32 = 0x0000_660F;
const ALG_ID_AES256: u32 = 0x0000_6610;
const ALG_ID_HASH_SHA1: u32 = 0x0000_8004;

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at.saturating_add(4)).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes)
}

fn decrypt_standard(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, PptxError> {
    let header_size = u32_at(info, 8).ok_or_else(|| PptxError::Encrypted("EncryptionInfo truncated in header".into()))? as usize;

    // EncryptionHeader (§2.3.4.5): Flags, SizeExtra, AlgID, AlgIDHash, KeySize, ProviderType,
    // Reserved1, Reserved2, CSPName. Reading the algorithm is what keeps an unsupported variant
    // from being mistaken for a wrong password.
    let header = info
        .get(12..12usize.saturating_add(header_size))
        .ok_or_else(|| PptxError::Encrypted("EncryptionInfo truncated in EncryptionHeader".into()))?;
    let truncated = || PptxError::Encrypted("EncryptionHeader truncated".into());
    let key_bytes = match u32_at(header, 8).ok_or_else(truncated)? {
        ALG_ID_AES128 => 16,
        ALG_ID_AES192 => 24,
        ALG_ID_AES256 => 32,
        other => return Err(PptxError::Encrypted(format!("unsupported standard cipher 0x{other:04x}"))),
    };
    match u32_at(header, 12).ok_or_else(truncated)? {
        ALG_ID_HASH_SHA1 => {}
        other => return Err(PptxError::Encrypted(format!("unsupported standard hash 0x{other:04x}"))),
    }

    let verifier_start = 12usize.saturating_add(header_size);
    let verifier = info.get(verifier_start..).ok_or_else(|| PptxError::Encrypted("EncryptionVerifier truncated".into()))?;

    let salt_size = verifier
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| PptxError::Encrypted("EncryptionVerifier missing salt size".into()))? as usize;

    let salt_end = 4usize.saturating_add(salt_size);
    let salt = verifier.get(4..salt_end).ok_or_else(|| PptxError::Encrypted("EncryptionVerifier truncated salt".into()))?;

    let enc_verifier = verifier
        .get(salt_end..salt_end.saturating_add(16))
        .ok_or_else(|| PptxError::Encrypted("EncryptionVerifier truncated encrypted verifier".into()))?;

    let vh_size_offset = salt_end.saturating_add(16);
    let verifier_hash_size =
        u32_at(verifier, vh_size_offset).ok_or_else(|| PptxError::Encrypted("EncryptionVerifier missing verifier hash size".into()))? as usize;

    // The hash is encrypted in whole AES blocks, so it is padded up to the next one.
    let enc_hash_len = verifier_hash_size.div_ceil(BLOCK).saturating_mul(BLOCK);
    let enc_vh_offset = vh_size_offset.saturating_add(4);
    let enc_verifier_hash = verifier
        .get(enc_vh_offset..enc_vh_offset.saturating_add(enc_hash_len))
        .ok_or_else(|| PptxError::Encrypted("EncryptionVerifier truncated encrypted verifier hash".into()))?;

    // Derive intermediate key: 50,000 SHA-1 iterations. Every password-derived buffer is wiped on
    // drop, here and below: the password and the key are secrets.
    let pw_utf16 = Zeroizing::new(password.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    let mut h = Zeroizing::new({
        let mut hasher = sha1::Sha1::new();
        hasher.update(salt);
        hasher.update(&pw_utf16[..]);
        hasher.finalize().to_vec()
    });

    for i in 0..50000u32 {
        let mut hasher = sha1::Sha1::new();
        hasher.update(i.to_le_bytes());
        hasher.update(&h[..]);
        *h = hasher.finalize().to_vec();
    }

    let mut hasher = sha1::Sha1::new();
    hasher.update(&h[..]);
    hasher.update(0u32.to_le_bytes()); // block 0
    let h_final = Zeroizing::new(hasher.finalize().to_vec());

    let mut b1 = Zeroizing::new([0x36u8; 64]);
    for (b, x) in b1.iter_mut().zip(h_final.iter()) {
        *b ^= *x;
    }
    let x1 = Zeroizing::new(sha1::Sha1::digest(&b1[..]).to_vec());

    let mut b2 = Zeroizing::new([0x5cu8; 64]);
    for (b, x) in b2.iter_mut().zip(h_final.iter()) {
        *b ^= *x;
    }
    let x2 = Zeroizing::new(sha1::Sha1::digest(&b2[..]).to_vec());

    // §2.3.4.7: the key is the first `keySize` bytes of x1 || x2 (40 bytes are available).
    let combined = Zeroizing::new([x1.as_slice(), x2.as_slice()].concat());
    let key = Zeroizing::new(combined.get(..key_bytes).ok_or_else(|| PptxError::Encrypted("derived key too short".into()))?.to_vec());

    // The verifier and the package are both encrypted with AES in ECB mode.
    let aes = Aes::new(&key)?;

    let mut verifier_dec = enc_verifier.to_vec();
    decrypt_ecb(&aes, &mut verifier_dec);
    let expected_hash = sha1::Sha1::digest(&verifier_dec);

    let mut vh_dec = enc_verifier_hash.to_vec();
    decrypt_ecb(&aes, &mut vh_dec);

    let actual_hash = vh_dec.get(..verifier_hash_size.min(20)).ok_or_else(|| PptxError::Encrypted("verifier hash too short".into()))?;
    if expected_hash.as_slice() != actual_hash {
        return Err(PptxError::WrongPassword);
    }

    // Decrypt EncryptedPackage
    let total_size = u32_at(package, 0).ok_or_else(|| PptxError::Encrypted("EncryptedPackage truncated".into()))? as usize;

    let payload = package.get(8..).ok_or_else(|| PptxError::Encrypted("EncryptedPackage missing payload".into()))?;
    let mut decrypted = payload.to_vec();
    decrypt_ecb(&aes, &mut decrypted);

    decrypted.truncate(total_size);
    Ok(decrypted)
}

/// Decrypt whole AES blocks in place (ECB), leaving any trailing partial block alone.
fn decrypt_ecb(aes: &Aes, data: &mut [u8]) {
    for chunk in data.chunks_mut(BLOCK) {
        if chunk.len() == BLOCK {
            let mut block = aes::Block::default();
            block.copy_from_slice(chunk);
            aes.decrypt(&mut block);
            chunk.copy_from_slice(&block);
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Agile Encryption (MS-OFFCRYPTO §2.3.4.10 - §2.3.4.15)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn parse(s: &str) -> Option<Hash> {
        match s.to_ascii_uppercase().as_str() {
            "SHA1" => Some(Hash::Sha1),
            "SHA256" => Some(Hash::Sha256),
            "SHA384" => Some(Hash::Sha384),
            "SHA512" => Some(Hash::Sha512),
            _ => None,
        }
    }
    fn size(self) -> usize {
        match self {
            Hash::Sha1 => 20,
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }
    fn hash(self, parts: &[&[u8]]) -> Vec<u8> {
        match self {
            Hash::Sha1 => {
                let mut h = sha1::Sha1::new();
                for p in parts {
                    h.update(p);
                }
                h.finalize().to_vec()
            }
            Hash::Sha256 => {
                let mut h = sha2::Sha256::new();
                for p in parts {
                    h.update(p);
                }
                h.finalize().to_vec()
            }
            Hash::Sha384 => {
                let mut h = sha2::Sha384::new();
                for p in parts {
                    h.update(p);
                }
                h.finalize().to_vec()
            }
            Hash::Sha512 => {
                let mut h = sha2::Sha512::new();
                for p in parts {
                    h.update(p);
                }
                h.finalize().to_vec()
            }
        }
    }
    fn block_size(self) -> usize {
        match self {
            Hash::Sha1 | Hash::Sha256 => 64,
            Hash::Sha384 | Hash::Sha512 => 128,
        }
    }
    /// HMAC (RFC 2104) over `data`: a key longer than the hash's block is hashed first, then the
    /// key is padded to the block and used for the inner and outer digests.
    fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        let bs = self.block_size();
        let mut k = Zeroizing::new(if key.len() > bs { self.hash(&[key]) } else { key.to_vec() });
        k.resize(bs, 0);
        let pad = |x: u8| Zeroizing::new(k.iter().map(|b| b ^ x).collect::<Vec<u8>>());
        let inner = self.hash(&[&pad(0x36)[..], data]);
        self.hash(&[&pad(0x5C)[..], &inner])
    }
}

enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> Result<Aes, PptxError> {
        let err = |_| PptxError::Encrypted("bad AES key".into());
        Ok(match key.len() {
            16 => Aes::A128(aes::Aes128::new_from_slice(key).map_err(err)?),
            24 => Aes::A192(aes::Aes192::new_from_slice(key).map_err(err)?),
            32 => Aes::A256(aes::Aes256::new_from_slice(key).map_err(err)?),
            n => return Err(PptxError::Encrypted(format!("unsupported AES key length {} bits", n * 8))),
        })
    }
    fn decrypt(&self, b: &mut aes::Block) {
        match self {
            Aes::A128(c) => c.decrypt_block(b),
            Aes::A192(c) => c.decrypt_block(b),
            Aes::A256(c) => c.decrypt_block(b),
        }
    }
}

fn cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, PptxError> {
    let aes = Aes::new(key)?;
    let (blocks, rest) = data.as_chunks::<BLOCK>();
    if !rest.is_empty() {
        return Err(PptxError::Encrypted("encrypted data is not a multiple of block size".into()));
    }
    let mut prev: [u8; BLOCK] = iv.try_into().map_err(|_| PptxError::Encrypted("bad initialization vector".into()))?;
    let mut out = Vec::with_capacity(data.len());
    for b in blocks {
        let mut x = aes::Block::default();
        x.copy_from_slice(b);
        aes.decrypt(&mut x);
        for (o, v) in x.iter_mut().zip(&prev) {
            *o ^= v;
        }
        prev = *b;
        out.extend_from_slice(&x);
    }
    Ok(out)
}

/// A key from the iterated password hash and a block key (§2.3.4.11): H(Hn + blockKey), padded
/// with 0x36 to `len` when the hash is shorter, truncated when it is longer.
fn derive_key(hash: Hash, hn: &[u8], block_key: &[u8], len: usize) -> Zeroizing<Vec<u8>> {
    let mut key = Zeroizing::new(hash.hash(&[hn, block_key]));
    key.resize(len, 0x36);
    key
}

fn make_iv(hash: Hash, salt: &[u8], block_key: Option<&[u8]>, block_size: usize) -> Vec<u8> {
    let mut v = match block_key {
        Some(k) => hash.hash(&[salt, k]),
        None => salt.to_vec(),
    };
    if v.len() < block_size {
        v.resize(block_size, 0x36);
    }
    v.truncate(block_size);
    v
}

fn iterate(hash: Hash, salt: &[u8], password: &str, spin_count: u32) -> Zeroizing<Vec<u8>> {
    let pw_utf16 = Zeroizing::new(password.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    let mut h = Zeroizing::new(hash.hash(&[salt, &pw_utf16[..]]));
    for i in 0..spin_count {
        *h = hash.hash(&[&i.to_le_bytes(), &h[..]]);
    }
    h
}

/// Decode standard base64 (padding optional, whitespace ignored). Strict: an unknown character is
/// an error rather than a silently shorter value.
fn b64(s: &str) -> Result<Vec<u8>, PptxError> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(PptxError::Encrypted("invalid base64".into())),
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn b64_attr(e: &xml::El, attr: &str) -> Result<Vec<u8>, PptxError> {
    let val = e.attr(attr).ok_or_else(|| PptxError::Encrypted(format!("missing {attr} attribute")))?;
    b64(val)
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct AgileInfo {
    hash: Hash,
    salt: Vec<u8>,
    block_size: usize,
    key_bytes: usize,
    integrity: Option<(Vec<u8>, Vec<u8>)>,
    pe: AgilePasswordEncryptor,
}

struct AgilePasswordEncryptor {
    hash: Hash,
    salt: Vec<u8>,
    salt_size: usize,
    block_size: usize,
    key_bytes: usize,
    spin_count: u32,
    verifier_input: Vec<u8>,
    verifier_hash: Vec<u8>,
    key_value: Vec<u8>,
}

fn parse_agile_info(info: &[u8]) -> Result<AgileInfo, PptxError> {
    let doc = xml::parse(info.get(8..).unwrap_or(&[])).map_err(|e| PptxError::Encrypted(format!("malformed EncryptionInfo XML: {e}")))?;
    let root = &doc.root;
    if root.local() != "encryption" {
        return Err(PptxError::Encrypted("EncryptionInfo has no <encryption> element".into()));
    }
    let key_data = root.elements().find(|e| e.local() == "keyData").ok_or_else(|| PptxError::Encrypted("missing keyData".into()))?;

    let kd_hash = Hash::parse(key_data.attr("hashAlgorithm").unwrap_or(""))
        .ok_or_else(|| PptxError::Encrypted("unsupported hash algorithm in keyData".into()))?;
    let kd_block_size: usize = key_data.attr("blockSize").and_then(|s| s.parse().ok()).unwrap_or(BLOCK);
    let kd_key_bits: usize = key_data.attr("keyBits").and_then(|s| s.parse().ok()).unwrap_or(256);
    let kd_key_bytes = kd_key_bits / 8;
    let kd_salt = b64_attr(key_data, "saltValue")?;

    let integrity = root
        .elements()
        .find(|e| e.local() == "dataIntegrity")
        .and_then(|di| Some((b64_attr(di, "encryptedHmacKey").ok()?, b64_attr(di, "encryptedHmacValue").ok()?)));

    let encryptors = root.elements().find(|e| e.local() == "keyEncryptors").ok_or_else(|| PptxError::Encrypted("missing keyEncryptors".into()))?;

    let ek = encryptors
        .elements()
        .filter(|e| e.local() == "keyEncryptor")
        .flat_map(|e| e.elements())
        .find(|e| e.local() == "encryptedKey" && e.attr("spinCount").is_some())
        .ok_or_else(|| PptxError::Encrypted("no password-based keyEncryptor found".into()))?;

    // The work factor is refused before anything else is read: lowering it would derive a
    // different key from the one the file was written with, and report a wrong password.
    let spin_count: u32 = match ek.attr("spinCount").and_then(|s| s.parse().ok()) {
        Some(n) if n <= MAX_SPIN_COUNT => n,
        Some(n) => {
            return Err(PptxError::Encrypted(format!("spin count {n} exceeds the supported maximum of {MAX_SPIN_COUNT}")));
        }
        None => 100_000,
    };

    let pe_hash =
        Hash::parse(ek.attr("hashAlgorithm").unwrap_or("")).ok_or_else(|| PptxError::Encrypted("unsupported hash in encryptedKey".into()))?;
    let pe_block_size: usize = ek.attr("blockSize").and_then(|s| s.parse().ok()).unwrap_or(BLOCK);
    let pe_key_bits: usize = ek.attr("keyBits").and_then(|s| s.parse().ok()).unwrap_or(256);
    let pe_key_bytes = pe_key_bits / 8;
    let pe_salt_size: usize = ek.attr("saltSize").and_then(|s| s.parse().ok()).unwrap_or(16);
    let pe_salt = b64_attr(ek, "saltValue")?;

    let verifier_input = b64_attr(ek, "encryptedVerifierHashInput")?;
    let verifier_hash = b64_attr(ek, "encryptedVerifierHashValue")?;
    let key_value = b64_attr(ek, "encryptedKeyValue")?;

    Ok(AgileInfo {
        hash: kd_hash,
        salt: kd_salt,
        block_size: kd_block_size,
        key_bytes: kd_key_bytes,
        integrity,
        pe: AgilePasswordEncryptor {
            hash: pe_hash,
            salt: pe_salt,
            salt_size: pe_salt_size,
            block_size: pe_block_size,
            key_bytes: pe_key_bytes,
            spin_count,
            verifier_input,
            verifier_hash,
            key_value,
        },
    })
}

fn decrypt_agile(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, PptxError> {
    let info = parse_agile_info(info)?;
    let pe = &info.pe;
    let hn = iterate(pe.hash, &pe.salt, password, pe.spin_count);

    let key_input = derive_key(pe.hash, &hn, &BK_VERIFIER_INPUT, pe.key_bytes);
    let key_value = derive_key(pe.hash, &hn, &BK_VERIFIER_VALUE, pe.key_bytes);
    let iv = make_iv(pe.hash, &pe.salt, None, pe.block_size);

    let input = Zeroizing::new(cbc_decrypt(&key_input, &iv, &pe.verifier_input)?);
    let input = input.get(..pe.salt_size).ok_or_else(|| PptxError::Encrypted("verifier input too short".into()))?;

    let value = Zeroizing::new(cbc_decrypt(&key_value, &iv, &pe.verifier_hash)?);
    let value = value.get(..pe.hash.size()).ok_or_else(|| PptxError::Encrypted("verifier hash too short".into()))?;

    if !same(&pe.hash.hash(&[input]), value) {
        return Err(PptxError::WrongPassword);
    }

    let key_key = derive_key(pe.hash, &hn, &BK_KEY_VALUE, pe.key_bytes);
    let key = Zeroizing::new(cbc_decrypt(&key_key, &iv, &pe.key_value)?);
    let key = Zeroizing::new(key.get(..info.key_bytes).ok_or_else(|| PptxError::Encrypted("encrypted key too short".into()))?.to_vec());

    if let Some((enc_key, enc_val)) = &info.integrity {
        let hmac_iv = make_iv(info.hash, &info.salt, Some(&BK_HMAC_KEY), info.block_size);
        let hmac_key = Zeroizing::new(cbc_decrypt(&key, &hmac_iv, enc_key)?);
        let hmac_key = hmac_key.get(..hmac_key.len().min(info.hash.size())).unwrap_or(&[]);

        let val_iv = make_iv(info.hash, &info.salt, Some(&BK_HMAC_VALUE), info.block_size);
        let expected = cbc_decrypt(&key, &val_iv, enc_val)?;
        let expected = expected.get(..info.hash.size()).ok_or_else(|| PptxError::Encrypted("integrity value too short".into()))?;

        if !same(&info.hash.hmac(hmac_key, package), expected) {
            return Err(PptxError::Encrypted("integrity verification failed".into()));
        }
    }

    let size = package
        .get(..8)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| PptxError::Encrypted("truncated encrypted package".into()))?;

    let data = package.get(8..).unwrap_or(&[]);
    let size = usize::try_from(size).ok().filter(|s| *s <= data.len()).ok_or_else(|| PptxError::Encrypted("truncated encrypted package".into()))?;
    let needed = size.div_ceil(BLOCK).saturating_mul(BLOCK);
    let data = data.get(..needed).ok_or_else(|| PptxError::Encrypted("truncated encrypted package".into()))?;

    let mut out = Vec::with_capacity(needed);
    for (i, seg) in data.chunks(SEGMENT).enumerate() {
        let i = u32::try_from(i).map_err(|_| PptxError::Encrypted("encrypted package too large".into()))?;
        let iv = make_iv(info.hash, &info.salt, Some(&i.to_le_bytes()), info.block_size);
        out.extend_from_slice(&cbc_decrypt(&key, &iv, seg)?);
    }
    out.truncate(size);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_key_derivation() {
        let salt = [0xe8, 0x82, 0x66, 0x49, 0x0c, 0x5b, 0xd1, 0xee, 0xbd, 0x2b, 0x43, 0x94, 0xe3, 0xf8, 0x30, 0xef];
        let password = "Password1234_";
        let pw_utf16: Vec<u8> = password.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();

        let mut h = {
            let mut hasher = sha1::Sha1::new();
            hasher.update(salt);
            hasher.update(&pw_utf16);
            hasher.finalize()
        };

        for i in 0..50000u32 {
            let mut hasher = sha1::Sha1::new();
            hasher.update(i.to_le_bytes());
            hasher.update(h);
            h = hasher.finalize();
        }

        let mut hasher = sha1::Sha1::new();
        hasher.update(h);
        hasher.update(0u32.to_le_bytes());
        let h_final = hasher.finalize();

        let mut b1 = [0x36u8; 64];
        for (b, x) in b1.iter_mut().zip(&h_final) {
            *b ^= *x;
        }
        let x1 = sha1::Sha1::digest(b1);

        let mut b2 = [0x5cu8; 64];
        for (b, x) in b2.iter_mut().zip(&h_final) {
            *b ^= *x;
        }
        let x2 = sha1::Sha1::digest(b2);

        let combined = [x1.as_slice(), x2.as_slice()].concat();
        let expected = [0x40, 0xb1, 0x3a, 0x71, 0xf9, 0x0b, 0x96, 0x6e, 0x37, 0x54, 0x08, 0xf2, 0xd1, 0x81, 0xa1, 0xaa];
        assert_eq!(&combined[..16], &expected);
    }

    #[test]
    fn a_derived_key_longer_than_the_hash_is_padded_with_0x36() {
        // §2.3.4.11: a 20-byte SHA-1 hash gives a 256-bit key by padding with 0x36, and a
        // 128-bit key by truncating the hash. msoffcrypto's stock helper only truncated, which is
        // why an agile SHA-1 container with AES-256 was never covered.
        let hn = [0x11u8; 20];
        let long = derive_key(Hash::Sha1, &hn, &BK_KEY_VALUE, 32);
        assert_eq!(long.len(), 32);
        assert_eq!(&long[20..], &[0x36u8; 12]);
        let short = derive_key(Hash::Sha1, &hn, &BK_KEY_VALUE, 16);
        assert_eq!(short.as_slice(), &Hash::Sha1.hash(&[&hn, &BK_KEY_VALUE])[..16]);
    }

    #[test]
    fn a_spin_count_above_the_cap_is_refused() {
        // Lowering the count silently would derive a different key from the one the file was
        // written with, and report a wrong password for a correct one.
        let mut info = vec![4u8, 4, 0, 0, 0, 0, 0, 0];
        info.extend_from_slice(
            format!(
                "<encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" \
                 xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\">\
                 <keyData saltValue=\"AAAA\" hashAlgorithm=\"SHA512\" blockSize=\"16\" keyBits=\"256\"/>\
                 <keyEncryptors><keyEncryptor><p:encryptedKey spinCount=\"{}\" hashAlgorithm=\"SHA512\"/>\
                 </keyEncryptor></keyEncryptors></encryption>",
                MAX_SPIN_COUNT + 1
            )
            .as_bytes(),
        );
        let e = parse_agile_info(&info).err().expect("a spin count above the cap must be refused");
        assert!(e.to_string().contains("spin count"), "{e}");
    }
}
