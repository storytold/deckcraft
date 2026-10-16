//! Strict ZIP32 inventory for the first source-patch slice. No extraction or repair.
use crate::{MAX_PART, MAX_TOTAL, MIME, OdpError, Result};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{CompressionMethod, write::SimpleFileOptions};

pub(super) struct Package {
    pub files: BTreeMap<String, Vec<u8>>,
}

fn invalid() -> OdpError {
    OdpError("source preservation requires an unambiguous bounded ZIP32 package".into())
}
fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let b = bytes.get(at..at.saturating_add(2)).ok_or_else(invalid)?;
    Ok(u16::from_le_bytes(b.try_into().map_err(|_| invalid())?))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let b = bytes.get(at..at.saturating_add(4)).ok_or_else(invalid)?;
    Ok(u32::from_le_bytes(b.try_into().map_err(|_| invalid())?))
}
fn path(bytes: &[u8], flags: u16) -> Result<String> {
    let name = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if name.is_empty()
        || name.len() > 1024
        || (!bytes.is_ascii() && flags & 0x800 == 0)
        || name.starts_with('/')
        || name.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '%' | '?' | '#'))
        || name.trim_end_matches('/').split('/').any(|p| p.is_empty() || matches!(p, "." | ".."))
        || name.ends_with("//")
    {
        return Err(invalid());
    }
    Ok(name.into())
}

struct Entry {
    name: String,
    start: usize,
    payload_end: usize,
    compressed: u32,
    size: u32,
    crc: u32,
    descriptor: bool,
}

fn descriptor(bytes: &[u8], start: usize, limit: usize, crc: u32, compressed: u32, size: u32) -> Result<()> {
    // The next validated record, not a marker within compressed data, determines width.
    // An unsigned descriptor's CRC may itself equal the optional signature value.
    let values = match limit.checked_sub(start) {
        Some(12) => start,
        Some(16) if u32_at(bytes, start)? == 0x0807_4b50 => start.checked_add(4).ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    if u32_at(bytes, values)? != crc || u32_at(bytes, values + 4)? != compressed || u32_at(bytes, values + 8)? != size {
        return Err(invalid());
    }
    Ok(())
}

fn directory(bytes: &[u8]) -> Result<Vec<Entry>> {
    if bytes.len() as u64 > MAX_TOTAL || bytes.len() < 22 {
        return Err(invalid());
    }
    // EOCD may occur in the comment. Require exactly one structurally positioned candidate.
    let mut end = None;
    for at in bytes.len().saturating_sub(65_557)..=bytes.len().saturating_sub(22) {
        if bytes.get(at..at + 4) == Some(b"PK\x05\x06")
            && at.checked_add(22).and_then(|v| v.checked_add(usize::from(u16_at(bytes, at + 20).ok()?))) == Some(bytes.len())
            && end.replace(at).is_some()
        {
            return Err(invalid());
        }
    }
    let end = end.ok_or_else(invalid)?;
    let count = usize::from(u16_at(bytes, end + 10)?);
    let cd = u32_at(bytes, end + 16)? as usize;
    let cd_size = u32_at(bytes, end + 12)? as usize;
    if count == 0
        || count > 10_000
        || u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || usize::from(u16_at(bytes, end + 8)?) != count
        || cd.checked_add(cd_size) != Some(end)
    {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(count);
    let mut names = std::collections::BTreeSet::new();
    let mut at = cd;
    let mut total = 0u64;
    for _ in 0..count {
        if bytes.get(at..at.saturating_add(4)) != Some(b"PK\x01\x02") {
            return Err(invalid());
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let compressed = u32_at(bytes, at + 20)? as usize;
        let size = u32_at(bytes, at + 24)?;
        let name_len = usize::from(u16_at(bytes, at + 28)?);
        let extra_len = usize::from(u16_at(bytes, at + 30)?);
        let comment_len = usize::from(u16_at(bytes, at + 32)?);
        let local = u32_at(bytes, at + 42)? as usize;
        // Admit only ordinary ZIP32 descriptors in addition to the existing strict profile.
        if flags & !0x80e != 0
            || !matches!(method, 0 | 8)
            || (method == 0 && flags & 6 != 0)
            || extra_len != 0
            || comment_len != 0
            || u16_at(bytes, at + 34)? != 0
            || u16_at(bytes, at + 6)? > 20
            || u32_at(bytes, at + 38)? >> 16 & 0xf000 == 0xa000
            || u64::from(size) > MAX_PART
        {
            return Err(invalid());
        }
        total = total.checked_add(u64::from(size)).ok_or_else(invalid)?;
        if total > MAX_TOTAL {
            return Err(invalid());
        }
        let name_at = at.checked_add(46).ok_or_else(invalid)?;
        let next = name_at.checked_add(name_len).ok_or_else(invalid)?;
        if next > end {
            return Err(invalid());
        }
        let raw_name = bytes.get(name_at..next).ok_or_else(invalid)?;
        let name = path(raw_name, flags)?;
        if !names.insert(name.clone()) {
            return Err(invalid());
        }
        if bytes.get(local..local.saturating_add(4)) != Some(b"PK\x03\x04")
            || u16_at(bytes, local + 4)? > 20
            || u16_at(bytes, local + 6)? != flags
            || u16_at(bytes, local + 8)? != method
            || usize::from(u16_at(bytes, local + 26)?) != name_len
            || u16_at(bytes, local + 28)? != 0
        {
            return Err(invalid());
        }
        let descriptor = flags & 8 != 0;
        let local_values = (u32_at(bytes, local + 14)?, u32_at(bytes, local + 18)?, u32_at(bytes, local + 22)?);
        let central_values = (u32_at(bytes, at + 16)?, compressed as u32, size);
        if local_values != if descriptor { (0, 0, 0) } else { central_values } {
            return Err(invalid());
        }
        let data = local.checked_add(30).and_then(|n| n.checked_add(name_len)).ok_or_else(invalid)?;
        if bytes.get(local + 30..data) != Some(raw_name) {
            return Err(invalid());
        }
        let payload_end = data.checked_add(compressed).ok_or_else(invalid)?;
        if payload_end > cd || (method == 0 && compressed != size as usize) || (name.ends_with('/') && (size != 0 || compressed != 0 || method != 0))
        {
            return Err(invalid());
        }
        entries.push(Entry { name, start: local, payload_end, compressed: compressed as u32, size, crc: central_values.0, descriptor });
        at = next;
    }
    if at != end {
        return Err(invalid());
    }
    entries.sort_by_key(|e| e.start);
    for (index, entry) in entries.iter().enumerate() {
        let limit = entries.get(index.saturating_add(1)).map_or(cd, |next| next.start);
        if entry.descriptor {
            descriptor(bytes, entry.payload_end, limit, entry.crc, entry.compressed, entry.size)?;
        } else if entry.payload_end != limit {
            return Err(invalid());
        }
    }
    if entries.first().is_none_or(|e| e.name != "mimetype" || e.start != 0) {
        return Err(invalid());
    }
    Ok(entries)
}

impl Package {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let entries = directory(bytes)?;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
        if zip.len() != entries.len() {
            return Err(invalid());
        }
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        for e in entries {
            let file = zip.by_name(&e.name).map_err(|_| invalid())?;
            if file.encrypted()
                || file.is_symlink()
                || file.name_raw() != e.name.as_bytes()
                || file.size() != u64::from(e.size)
                || file.crc32() != e.crc
            {
                return Err(invalid());
            }
            if e.name == "mimetype" && file.compression() != CompressionMethod::Stored {
                return Err(invalid());
            }
            let mut data = Vec::new();
            file.take(MAX_PART + 1).read_to_end(&mut data).map_err(|_| invalid())?;
            total = total.saturating_add(data.len() as u64);
            if data.len() != e.size as usize || total > MAX_TOTAL {
                return Err(invalid());
            }
            files.insert(e.name, data);
        }
        if files.get("mimetype").is_none_or(|b| b != MIME.as_bytes()) {
            return Err(invalid());
        }
        Ok(Self { files })
    }

    pub fn write(&self, content: &[u8], cache: Option<&super::cache::CachePlan>) -> Result<Vec<u8>> {
        if content.len() as u64 > MAX_PART {
            return Err(invalid());
        }
        fn payload<'a>(name: &str, original: &'a [u8], content: &'a [u8], cache: Option<&'a super::cache::CachePlan>) -> &'a [u8] {
            if name == "content.xml" { content } else { cache.and_then(|plan| plan.replacement(name)).unwrap_or(original) }
        }
        let mut predicted = 22usize;
        for (name, bytes) in &self.files {
            if cache.is_some_and(|plan| plan.omits(name)) {
                continue;
            }
            let size = if name == "content.xml" { content.len() } else { cache.and_then(|plan| plan.replacement(name)).unwrap_or(bytes).len() };
            predicted = predicted
                .checked_add(76)
                .and_then(|n| n.checked_add(name.len().checked_mul(2)?))
                .and_then(|n| n.checked_add(size))
                .ok_or_else(invalid)?;
            if predicted as u64 > MAX_TOTAL {
                return Err(invalid());
            }
        }
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        output.start_file("mimetype", options).map_err(|_| invalid())?;
        output.write_all(MIME.as_bytes()).map_err(|_| invalid())?;
        for (name, original) in &self.files {
            if name == "mimetype" || cache.is_some_and(|plan| plan.omits(name)) {
                continue;
            }
            if name.ends_with('/') {
                output.add_directory(name, options).map_err(|_| invalid())?;
            } else {
                output.start_file(name, options).map_err(|_| invalid())?;
                output.write_all(payload(name, original, content, cache)).map_err(|_| invalid())?;
            }
        }
        let output = output.finish().map_err(|_| invalid())?.into_inner();
        if output.len() as u64 > MAX_TOTAL {
            return Err(invalid());
        }
        let checked = Self::read(&output)?;
        let removed = self.files.keys().filter(|name| cache.is_some_and(|plan| plan.omits(name))).count();
        if checked.files.len().checked_add(removed) != Some(self.files.len())
            || self.files.iter().any(|(name, bytes)| {
                if cache.is_some_and(|plan| plan.omits(name)) {
                    return checked.files.contains_key(name);
                }
                checked.files.get(name).is_none_or(|after| after.as_slice() != payload(name, bytes, content, cache))
            })
        {
            return Err(invalid());
        }
        if let Some(plan) = cache {
            plan.verify_output(&checked.files)?;
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::descriptor;

    #[test]
    fn descriptor_width_resolves_crc_equal_to_optional_marker() {
        const MARKER: u32 = 0x0807_4b50;
        for marked in [false, true] {
            let mut bytes = Vec::new();
            if marked {
                bytes.extend_from_slice(&MARKER.to_le_bytes());
            }
            for value in [MARKER, 7, 11] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let boundary = bytes.len();
            // The following header never participates in descriptor interpretation.
            bytes.extend_from_slice(b"PK\x03\x04");
            assert!(descriptor(&bytes, 0, boundary, MARKER, 7, 11).is_ok());
            assert!(descriptor(&bytes, 0, boundary, MARKER, 8, 11).is_err());
            assert!(descriptor(&bytes, 0, boundary + 1, MARKER, 7, 11).is_err());
        }
    }
}
