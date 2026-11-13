//! Checks an MP4's box tree before symphonia reads it. Symphonia's MP4 reader reserves memory
//! from the entry counts in the sample tables (and reads each sample whole at its `stsz` size)
//! without checking them against the file, so a damaged count asks for gigabytes and the process
//! aborts — an allocation failure cannot be caught. A file passes when every box fits inside its
//! parent, every table's entries fit inside its box and every sample fits inside the file.

/// Boxes whose payload is a sequence of boxes.
const CONTAINERS: [&[u8; 4]; 11] = [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts", b"dinf", b"udta", b"mvex", b"moof", b"traf"];

/// Whether symphonia can be given `b` (an MP4) without a damaged size reaching an allocation.
pub(crate) fn tables_fit(b: &[u8]) -> bool {
    boxes_fit(b, b.len() as u64, 0)
}

/// Bytes symphonia's probe scans for a format marker before giving up.
const PROBE_SEARCH_LIMIT: usize = 1024 * 1024;

/// Whether symphonia can be given `b`, whatever the sniffer made of it: its probe takes the first
/// `ftyp` within its search window as the start of an MP4 (the size field 4 bytes before it), so
/// the table check must start where that parser will, not at offset 0.
pub(crate) fn probe_fits(b: &[u8]) -> bool {
    if crate::Container::sniff(b) == crate::Container::Mp4 && !tables_fit(b) {
        return false;
    }
    // The probe reads leading ID3v2 tags as metadata and searches again after the last one.
    let mut rest = b;
    while rest.starts_with(b"ID3") {
        let size = rest.get(6..10).map(|s| s.iter().fold(0usize, |a, &x| (a << 7) | (x & 0x7f) as usize)).unwrap_or(0);
        rest = rest.get(10 + size..).unwrap_or(&[]);
    }
    let head = &rest[..rest.len().min(PROBE_SEARCH_LIMIT + 16)];
    match head.windows(4).position(|w| w == b"ftyp") {
        Some(p) => tables_fit(&rest[p.saturating_sub(4)..]),
        None => true,
    }
}

fn be32(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at.checked_add(4)?).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as u64)
}

fn boxes_fit(b: &[u8], file_len: u64, depth: u32) -> bool {
    if depth > 16 {
        return false;
    }
    let mut at = 0usize;
    while b.len() - at >= 8 {
        let Some(size32) = be32(b, at) else { return false };
        let kind = &b[at + 4..at + 8];
        let (header, size) = match size32 {
            0 => (8, (b.len() - at) as u64),
            1 => match b.get(at + 8..at + 16) {
                Some(s) => (16, u64::from_be_bytes(s.try_into().unwrap_or_default())),
                None => return false,
            },
            n => (8, n),
        };
        if size < header as u64 || size > (b.len() - at) as u64 {
            // A box that runs past the end of the file is fine at the top level (the file was cut
            // short; symphonia stops there), but never inside a parent.
            return depth == 0 && !matches!(kind, b"moov" | b"moof");
        }
        let body = &b[at + header..at + size as usize];
        let ok = if CONTAINERS.iter().any(|c| c.as_slice() == kind) {
            boxes_fit(body, file_len, depth + 1)
        } else if kind == b"meta" {
            // ISO `meta` is a full box (4 bytes of version and flags first); QuickTime's is not.
            let skip = if be32(body, 0) == Some(0) { 4 } else { 0 };
            boxes_fit(&body[skip.min(body.len())..], file_len, depth + 1)
        } else {
            table_fits(kind, body, file_len)
        };
        if !ok {
            return false;
        }
        at += size as usize;
    }
    true
}

/// A sample table (a full box: version and flags, then a count and that many entries).
fn table_fits(kind: &[u8], body: &[u8], file_len: u64) -> bool {
    let entry = match kind {
        b"stts" | b"ctts" | b"co64" => 8,
        b"stsc" => 12,
        b"stco" | b"stss" => 4,
        b"stsz" => {
            let (Some(size), Some(count)) = (be32(body, 4), be32(body, 8)) else { return false };
            if size != 0 {
                return size <= file_len;
            }
            let Some(entries) = body.get(12..) else { return false };
            return count <= entries.len() as u64 / 4
                && entries.as_chunks::<4>().0.iter().take(count as usize).all(|s| u32::from_be_bytes(*s) as u64 <= file_len);
        }
        _ => return true,
    };
    be32(body, 4).is_some_and(|count| count <= body.len().saturating_sub(8) as u64 / entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An MP4 whose `stsc` count asks for far more entries than the file holds.
    fn mp4_with_huge_stsc() -> Vec<u8> {
        let stsc = [&24u32.to_be_bytes()[..], b"stsc", &[0, 0, 0, 0], &u32::MAX.to_be_bytes(), &[0; 8]].concat();
        let stbl = [&(stsc.len() as u32 + 8).to_be_bytes()[..], b"stbl", &stsc].concat();
        let moov = [&(stbl.len() as u32 + 8).to_be_bytes()[..], b"moov", &stbl].concat();
        [&16u32.to_be_bytes()[..], b"ftyp", b"isom", &[0; 4], &moov].concat()
    }

    #[test]
    fn a_prefixed_mp4_is_checked_where_symphonia_starts_it() {
        let mp4 = mp4_with_huge_stsc();
        assert!(!tables_fit(&mp4));
        assert!(!probe_fits(&mp4));
        let prefixed = [&b"junk before the file "[..], &mp4].concat();
        assert_eq!(crate::Container::sniff(&prefixed), crate::Container::Unknown);
        assert!(tables_fit(&prefixed), "offset 0 sees no MP4 boxes");
        assert!(!probe_fits(&prefixed));
        let far = [&vec![0u8; PROBE_SEARCH_LIMIT + 16][..], &mp4].concat();
        assert!(probe_fits(&far), "past the probe window symphonia never reads it as an MP4");
    }
}
