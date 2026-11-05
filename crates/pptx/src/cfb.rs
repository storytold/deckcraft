//! Just enough of the Compound File Binary (CFB/OLE2) container to tell a password-protected
//! presentation apart: PowerPoint wraps an encrypted `.pptx` in a CFB holding `EncryptionInfo`
//! and `EncryptedPackage` streams (ECMA-376 / [MS-OFFCRYPTO]) instead of a zip. Every read is
//! bounds-checked and every loop bounded. Copied from GridCraft's `crates/xlsx/src/cfb.rs`.

/// The CFB signature at the start of the file.
const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// Sector ids at or above this are markers (free, end of chain, FAT/DIFAT sector), not sectors.
const MAX_REG_SECT: u32 = 0xFFFF_FFFA;
/// Directory sectors followed at most (4096 entries with 512-byte sectors).
const MAX_DIR_SECTORS: usize = 1024;
/// DIFAT sectors followed at most.
const MAX_DIFAT_SECTORS: usize = 1024;

/// Stream names that mark an encrypted (password-protected) OOXML package.
const ENCRYPTED_STREAMS: [&str; 2] = ["EncryptedPackage", "EncryptionInfo"];

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// Whether `bytes` is a compound file holding an encryption stream. Walks the directory through
/// the FAT; when the structure can't be followed (truncated or malformed), falls back to scanning
/// the bytes for the UTF-16LE stream names.
pub(crate) fn is_encrypted(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&SIGNATURE) {
        return false;
    }
    match directory_names(bytes) {
        Some(names) => names.iter().any(|n| ENCRYPTED_STREAMS.iter().any(|e| n.eq_ignore_ascii_case(e))),
        None => ENCRYPTED_STREAMS.iter().any(|e| contains_utf16(bytes, e)),
    }
}

fn contains_utf16(hay: &[u8], needle: &str) -> bool {
    let n: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    !n.is_empty() && hay.windows(n.len()).any(|w| w == n.as_slice())
}

/// Names of every directory entry, or `None` when the directory can't be read.
fn directory_names(b: &[u8]) -> Option<Vec<String>> {
    if !b.starts_with(&SIGNATURE) {
        return None;
    }
    let shift = u16_at(b, 30)?;
    if shift != 9 && shift != 12 {
        return None;
    }
    let size = 1usize << shift;
    let per = size / 4;
    let sector = |id: u32| -> Option<&[u8]> {
        let start = (id as usize).checked_add(1)?.checked_mul(size)?;
        b.get(start..start.checked_add(size)?)
    };
    // The DIFAT: the FAT's sector ids, 109 in the header then chained DIFAT sectors.
    let mut difat: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 76 + i * 4)).filter(|&s| s < MAX_REG_SECT).collect();
    let mut next = u32_at(b, 68)?;
    for _ in 0..MAX_DIFAT_SECTORS {
        if next >= MAX_REG_SECT {
            break;
        }
        let s = sector(next)?;
        difat.extend((0..per - 1).filter_map(|i| u32_at(s, i * 4)).filter(|&x| x < MAX_REG_SECT));
        next = u32_at(s, (per - 1) * 4)?;
    }
    let fat_next = |id: u32| -> Option<u32> {
        let idx = id as usize;
        let fat_sector = *difat.get(idx / per)?;
        u32_at(sector(fat_sector)?, (idx % per) * 4)
    };
    let mut names = Vec::new();
    let mut id = u32_at(b, 48)?;
    for _ in 0..MAX_DIR_SECTORS {
        if id == END_OF_CHAIN {
            return Some(names);
        }
        if id >= MAX_REG_SECT {
            return None;
        }
        let s = sector(id)?;
        for e in s.as_chunks::<128>().0 {
            let len = usize::from(u16_at(e, 64)?);
            if !(2..=64).contains(&len) {
                continue;
            }
            let units: Vec<u16> = (0..(len - 2) / 2).filter_map(|i| u16_at(e, i * 2)).collect();
            names.push(String::from_utf16_lossy(&units));
        }
        id = fat_next(id)?;
    }
    // A directory longer than the bound (or a cyclic chain): report what was read.
    Some(names)
}

/// Directory entry type of a stream object ([MS-CFB] 2.6.1).
const STREAM_OBJECT: u8 = 2;
/// "No entry" in the directory tree (NOSTREAM).
const NO_STREAM: u32 = 0xFFFF_FFFF;
/// Streams larger than this are refused (the format allows more, a presentation never needs it).
const MAX_STREAM: u64 = 1 << 30;

/// One directory entry ([MS-CFB] 2.6).
struct DirEntry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

/// A parsed compound file: the FAT, the mini FAT and the directory, all bounds-checked.
/// Implemented from Microsoft's public [MS-CFB] specification; the streams it holds are the
/// encryption descriptors MS-OFFCRYPTO reads.
pub(crate) struct Compound<'a> {
    bytes: &'a [u8],
    sector_size: usize,
    mini_sector_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    entries: Vec<DirEntry>,
}

impl<'a> Compound<'a> {
    /// Parses the header, FAT, mini FAT and directory. Errors on anything that can't be followed.
    pub(crate) fn open(b: &'a [u8]) -> Result<Compound<'a>, String> {
        let bad = |m: &str| format!("damaged compound file: {m}");
        if !b.starts_with(&SIGNATURE) {
            return Err(bad("missing signature"));
        }
        let major = u16_at(b, 26).ok_or_else(|| bad("truncated header"))?;
        let shift = u16_at(b, 30).ok_or_else(|| bad("truncated header"))?;
        if shift != 9 && shift != 12 {
            return Err(bad("unsupported sector size"));
        }
        let mini_shift = u16_at(b, 32).ok_or_else(|| bad("truncated header"))?;
        if mini_shift != 6 {
            return Err(bad("unsupported mini sector size"));
        }
        let sector_size = 1usize << shift;
        let per = sector_size / 4;
        // Sectors that physically exist after the header sector: every chain is bounded by it.
        let sectors = (b.len() / sector_size).saturating_sub(1);
        let sector = |id: u32| -> Option<&[u8]> {
            let start = (id as usize).checked_add(1)?.checked_mul(sector_size)?;
            b.get(start..start.checked_add(sector_size)?)
        };
        // DIFAT: 109 FAT sector ids in the header, then chained DIFAT sectors.
        let mut fat_sectors: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 76 + i * 4)).filter(|&s| s < MAX_REG_SECT).collect();
        let mut next = u32_at(b, 68).ok_or_else(|| bad("truncated header"))?;
        let mut hops = 0usize;
        while next < MAX_REG_SECT {
            hops += 1;
            if hops > sectors.min(MAX_DIFAT_SECTORS * 64) {
                return Err(bad("DIFAT chain loops"));
            }
            let s = sector(next).ok_or_else(|| bad("DIFAT sector out of range"))?;
            fat_sectors.extend((0..per - 1).filter_map(|i| u32_at(s, i * 4)).filter(|&x| x < MAX_REG_SECT));
            next = u32_at(s, (per - 1) * 4).ok_or_else(|| bad("truncated DIFAT sector"))?;
        }
        if fat_sectors.len() > sectors {
            return Err(bad("more FAT sectors than the file holds"));
        }
        let mut fat = Vec::with_capacity(fat_sectors.len().saturating_mul(per));
        for id in fat_sectors {
            let s = sector(id).ok_or_else(|| bad("FAT sector out of range"))?;
            fat.extend((0..per).filter_map(|i| u32_at(s, i * 4)));
        }
        let mut cf = Compound { bytes: b, sector_size, mini_sector_size: 64, mini_cutoff: 4096, fat, mini_fat: vec![], entries: vec![] };
        if let Some(c) = u32_at(b, 56) {
            cf.mini_cutoff = u64::from(c);
        }
        // Directory.
        let dir_start = u32_at(b, 48).ok_or_else(|| bad("truncated header"))?;
        let dir = cf.read_chain(dir_start, u64::MAX)?;
        for e in dir.as_chunks::<128>().0 {
            let len = usize::from(u16_at(e, 64).unwrap_or(0)).min(64);
            let units: Vec<u16> = (0..len.saturating_sub(2) / 2).filter_map(|i| u16_at(e, i * 2)).collect();
            let mut size = u32_at(e, 120).map(u64::from).unwrap_or(0);
            if major >= 4 {
                size |= u64::from(u32_at(e, 124).unwrap_or(0)) << 32;
            }
            cf.entries.push(DirEntry {
                name: String::from_utf16_lossy(&units),
                kind: e.get(66).copied().unwrap_or(0),
                left: u32_at(e, 68).unwrap_or(NO_STREAM),
                right: u32_at(e, 72).unwrap_or(NO_STREAM),
                child: u32_at(e, 76).unwrap_or(NO_STREAM),
                start: u32_at(e, 116).unwrap_or(END_OF_CHAIN),
                size,
            });
        }
        if cf.entries.first().is_none_or(|r| r.kind != 5) {
            return Err(bad("missing root entry"));
        }
        // Mini FAT.
        let mini_start = u32_at(b, 60).ok_or_else(|| bad("truncated header"))?;
        if mini_start < MAX_REG_SECT {
            let raw = cf.read_chain(mini_start, u64::MAX)?;
            cf.mini_fat = (0..raw.len() / 4).filter_map(|i| u32_at(&raw, i * 4)).collect();
        }
        Ok(cf)
    }

    /// Follows a FAT chain from `start`, reading at most `limit` bytes. A chain longer than the
    /// FAT (so a cycle) or pointing outside the file is an error. The file may end inside the
    /// chain's last sector (some writers don't pad it, and PowerPoint opens those files): that
    /// sector reads short, and the caller checks the stream's declared size against what was read.
    fn read_chain(&self, start: u32, limit: u64) -> Result<Vec<u8>, String> {
        let limit = usize::try_from(limit.min(MAX_STREAM)).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        let mut id = start;
        let mut seen = vec![false; self.fat.len()];
        let mut cut = false;
        while id != END_OF_CHAIN && out.len() < limit {
            if cut {
                return Err("damaged compound file: sector out of range (the file may be truncated)".into());
            }
            if id >= MAX_REG_SECT {
                return Err("damaged compound file: broken sector chain".into());
            }
            match seen.get_mut(id as usize) {
                Some(v) if !*v => *v = true,
                Some(_) => return Err("damaged compound file: sector chain loops".into()),
                None => return Err("damaged compound file: sector missing from the FAT".into()),
            }
            let start = (id as usize).checked_add(1).and_then(|i| i.checked_mul(self.sector_size));
            let s = start
                .and_then(|s| self.bytes.get(s..s.checked_add(self.sector_size)?.min(self.bytes.len())))
                .ok_or("damaged compound file: sector out of range (the file may be truncated)")?;
            cut = s.len() < self.sector_size;
            out.extend_from_slice(s);
            id = *self.fat.get(id as usize).ok_or("damaged compound file: sector missing from the FAT")?;
        }
        Ok(out)
    }

    /// Follows a mini FAT chain inside the mini stream container. Like `read_chain`, the
    /// container may end inside the chain's last mini sector.
    fn read_mini_chain(&self, container: &[u8], start: u32, limit: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut id = start;
        let mut seen = vec![false; self.mini_fat.len()];
        let mut cut = false;
        while id != END_OF_CHAIN && out.len() < limit {
            if cut {
                return Err("damaged compound file: mini sector out of range".into());
            }
            if id >= MAX_REG_SECT {
                return Err("damaged compound file: broken mini sector chain".into());
            }
            match seen.get_mut(id as usize) {
                Some(v) if !*v => *v = true,
                Some(_) => return Err("damaged compound file: mini sector chain loops".into()),
                None => return Err("damaged compound file: mini sector missing from the mini FAT".into()),
            }
            let at = (id as usize).checked_mul(self.mini_sector_size);
            let s = at
                .and_then(|a| container.get(a..a.checked_add(self.mini_sector_size)?.min(container.len())))
                .ok_or("damaged compound file: mini sector out of range")?;
            cut = s.len() < self.mini_sector_size;
            out.extend_from_slice(s);
            id = *self.mini_fat.get(id as usize).ok_or("damaged compound file: mini sector missing from the mini FAT")?;
        }
        Ok(out)
    }

    /// The directory entry of the stream called `name` (case-insensitive) directly under the
    /// root storage.
    fn root_entry(&self, name: &str) -> Result<Option<&DirEntry>, String> {
        let Some(root) = self.entries.first() else { return Ok(None) };
        // Walk the root's children (a red-black tree through left/right siblings), bounded.
        let mut stack = vec![root.child];
        let mut seen = 0usize;
        while let Some(i) = stack.pop() {
            if i == NO_STREAM {
                continue;
            }
            seen += 1;
            if seen > self.entries.len() {
                return Err("damaged compound file: directory tree loops".into());
            }
            let Some(e) = self.entries.get(i as usize) else { continue };
            if e.kind == STREAM_OBJECT && e.name.eq_ignore_ascii_case(name) {
                return Ok(Some(e));
            }
            stack.push(e.left);
            stack.push(e.right);
        }
        Ok(None)
    }

    /// The contents of the stream called `name` (case-insensitive), refused when larger than `cap`.
    /// A stream is untrusted input: its declared size is a number from the file.
    pub(crate) fn root_stream_capped(&self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, String> {
        let Some(root) = self.entries.first() else { return Ok(None) };
        let Some(e) = self.root_entry(name)? else { return Ok(None) };
        let cap = cap.min(MAX_STREAM);
        if e.size > cap {
            return Err(format!("the {name} stream is larger than the {cap}-byte limit"));
        }
        let size = usize::try_from(e.size).map_err(|_| "stream too large".to_string())?;
        let mut data = if e.size < self.mini_cutoff {
            let container = self.read_chain(root.start, root.size)?;
            self.read_mini_chain(&container, e.start, size)?
        } else {
            self.read_chain(e.start, e.size)?
        };
        if data.len() < size {
            return Err("damaged compound file: a stream is shorter than its declared size (the file may be truncated)".into());
        }
        data.truncate(size);
        Ok(Some(data))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A minimal valid compound file (512-byte sectors: FAT at sector 0, directory at sector 1)
    /// whose directory holds a root entry plus the given stream names.
    fn build(streams: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 512 * 3];
        b[..8].copy_from_slice(&SIGNATURE);
        b[24..26].copy_from_slice(&0x3Eu16.to_le_bytes()); // minor version
        b[26..28].copy_from_slice(&3u16.to_le_bytes()); // major version
        b[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes()); // byte order
        b[30..32].copy_from_slice(&9u16.to_le_bytes()); // sector shift
        b[32..34].copy_from_slice(&6u16.to_le_bytes()); // mini sector shift
        b[44..48].copy_from_slice(&1u32.to_le_bytes()); // FAT sectors
        b[48..52].copy_from_slice(&1u32.to_le_bytes()); // first directory sector
        b[56..60].copy_from_slice(&4096u32.to_le_bytes()); // mini stream cutoff
        b[60..64].copy_from_slice(&END_OF_CHAIN.to_le_bytes()); // no mini FAT
        b[68..72].copy_from_slice(&END_OF_CHAIN.to_le_bytes()); // no DIFAT sectors
        for i in 0..109 {
            b[76 + i * 4..80 + i * 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        }
        b[76..80].copy_from_slice(&0u32.to_le_bytes()); // FAT lives in sector 0
        // FAT (sector 0, at byte 512): sector 0 is a FAT sector, sector 1 ends its chain.
        for i in 0..128 {
            b[512 + i * 4..516 + i * 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        }
        b[512..516].copy_from_slice(&0xFFFF_FFFDu32.to_le_bytes());
        b[516..520].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        // Directory (sector 1, at byte 1024).
        for (k, name) in std::iter::once("Root Entry").chain(streams.iter().copied()).take(4).enumerate() {
            let e = 1024 + k * 128;
            let units: Vec<u16> = name.encode_utf16().take(31).collect();
            for (i, u) in units.iter().enumerate() {
                b[e + i * 2..e + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            b[e + 64..e + 66].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
            b[e + 66] = if k == 0 { 5 } else { 2 };
        }
        b
    }

    #[test]
    fn reads_directory_names() {
        let b = build(&["EncryptionInfo", "EncryptedPackage"]);
        assert_eq!(directory_names(&b), Some(vec!["Root Entry".into(), "EncryptionInfo".into(), "EncryptedPackage".into()]));
        assert!(is_encrypted(&b));
        // A PowerPoint 97-2003 .ppt is a compound file too, without the encryption streams.
        assert!(!is_encrypted(&build(&["PowerPoint Document", "Current User"])));
        assert!(!is_encrypted(b"PK\x03\x04"));
    }

    #[test]
    fn hostile_headers_never_panic() {
        let mut b = build(&["PowerPoint Document"]);
        for at in [30usize, 48, 68, 76, 512, 516, 1024 + 64] {
            for v in [0u32, 1, 7, 0xFFFF_FFFA, 0xFFFF_FFFF, 0x7FFF_FFFF] {
                let mut c = b.clone();
                c[at..at + 4].copy_from_slice(&v.to_le_bytes());
                let _ = is_encrypted(&c);
                for cut in [0usize, 8, 100, 600, 1100] {
                    let _ = is_encrypted(&c[..cut.min(c.len())]);
                }
            }
        }
        // A directory chain that loops back on itself stops at the bound.
        b[516..520].copy_from_slice(&1u32.to_le_bytes());
        assert!(!is_encrypted(&b));
    }

    /// The agile reader: an encrypted fixture's two streams read back through the local reader.
    #[test]
    fn an_encrypted_fixture_reads_through_the_compound_reader() {
        let bytes = include_bytes!("../tests/fixtures/agile-aes256-sha512.pptx");
        let cf = Compound::open(bytes).expect("the fixture is a compound file");
        let info = cf.root_stream_capped("EncryptionInfo", MAX_STREAM).expect("stream reads").expect("EncryptionInfo is present");
        assert!(!info.is_empty());
        let package = cf.root_stream_capped("EncryptedPackage", bytes.len() as u64).expect("stream reads").expect("EncryptedPackage is present");
        assert!(!package.is_empty());
        assert!(cf.root_stream_capped("Nope", MAX_STREAM).expect("an absent stream is not an error").is_none());
        // Detection and the reader agree about the same file.
        assert!(is_encrypted(bytes));
    }

    /// A truncated container is refused, never panicked on: the file came from outside.
    #[test]
    fn a_truncated_container_is_an_error() {
        let bytes = include_bytes!("../tests/fixtures/agile-aes256-sha512.pptx");
        assert!(Compound::open(&bytes[..1000]).is_err());
        assert!(Compound::open(b"PK\x03\x04").is_err());
        assert!(Compound::open(&[]).is_err());
    }

    /// #346: opening a password-protected presentation says so instead of failing as an unknown
    /// file; since #435 the same call also names the deck for the password dialog.
    #[test]
    fn a_password_protected_presentation_is_named_as_such() {
        let locked = build(&["EncryptionInfo", "EncryptedPackage"]);
        let err = deckcraft_engine::cmd::file::open_presentation("Locked.pptx", &locked).unwrap_err().to_string();
        assert!(err.contains("Locked.pptx: this file is password-protected"), "{err}");
        let ppt = build(&["PowerPoint Document", "Current User"]);
        let err = deckcraft_engine::cmd::file::open_presentation("Old.ppt", &ppt).unwrap_err().to_string();
        assert!(!err.contains("password"), "{err}");
        assert!(err.contains("legacy binary PowerPoint"), "{err}");
    }
}
