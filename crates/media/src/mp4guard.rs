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
