//! Geographic binary caches: base64-encoded RFC1951 raw DEFLATE containing cx:clear XML.
//! Native Office also emits a SYNC_FLUSH-terminated stream without a final DEFLATE block.
//! Decode only complete XML, with explicit compressed/expanded limits; retain source bytes.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Decompress, FlushDecompress, Status};

const MAX_ENCODED: usize = 16 << 20;
const MAX_COMPRESSED: usize = 8 << 20;
const MAX_EXPANDED: usize = 32 << 20;

pub(super) fn decode(encoded: &str) -> Option<crate::xml::Doc> {
    if encoded.len() > MAX_ENCODED {
        return None;
    }
    let compact: Vec<u8> = encoded.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.len() > MAX_COMPRESSED.div_ceil(3) * 4 {
        return None;
    }
    let bytes = STANDARD.decode(compact).ok()?;
    if bytes.len() > MAX_COMPRESSED {
        return None;
    }
    let xml = inflate(&bytes, MAX_EXPANDED)?;
    if !complete_xml(&xml) {
        return None;
    }
    let doc = crate::xml::parse(&xml).ok()?;
    if !doc.root.is("clear") {
        return None;
    }
    Some(doc)
}

fn complete_xml(xml: &[u8]) -> bool {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let (mut depth, mut roots) = (0usize, 0usize);
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if depth > crate::xml::MAX_DEPTH {
                    return false;
                }
            }
            Ok(Event::Empty(_)) => {
                if depth == 0 {
                    roots += 1;
                }
            }
            Ok(Event::End(_)) => {
                let Some(next) = depth.checked_sub(1) else { return false };
                depth = next;
            }
            Ok(Event::Text(t)) if depth == 0 && !t.as_ref().iter().all(u8::is_ascii_whitespace) => return false,
            Ok(Event::DocType(_)) | Err(_) => return false,
            Ok(Event::Eof) => return depth == 0 && roots == 1,
            _ => (),
        }
        if roots > 1 {
            return false;
        }
    }
}

fn inflate(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut decoder = Decompress::new(false);
    let mut output = Vec::new();
    let mut buffer = [0u8; 32768];
    let mut offset = 0;
    for _ in 0..limit.div_ceil(buffer.len()).saturating_add(2) {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder.decompress(bytes.get(offset..)?, &mut buffer, FlushDecompress::None).ok()?;
        let consumed = usize::try_from(decoder.total_in().checked_sub(before_in)?).ok()?;
        let produced = usize::try_from(decoder.total_out().checked_sub(before_out)?).ok()?;
        offset = offset.checked_add(consumed)?;
        if output.len().checked_add(produced)? > limit {
            return None;
        }
        output.extend_from_slice(buffer.get(..produced)?);
        if status == Status::StreamEnd {
            return (offset == bytes.len()).then_some(output);
        }
        if offset == bytes.len() && produced < buffer.len() {
            // A native cache's final empty stored block is flush-delimited. Anything else
            // without StreamEnd is truncated; XML well-formedness is checked by the caller.
            return bytes.ends_with(&[0, 0, 255, 255]).then_some(output);
        }
        if consumed == 0 && produced == 0 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn encode(xml: &[u8], sync: bool) -> String {
        let mut writer = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(xml).unwrap();
        let data = if sync {
            writer.flush().unwrap();
            writer.get_ref().clone()
        } else {
            writer.finish().unwrap()
        };
        STANDARD.encode(data)
    }
    #[test]
    fn finished_and_native_flush_streams_decode_original_clear_xml() {
        let xml = br#"<cx:clear xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:geoDataEntityQueryResults/></cx:clear>"#;
        for sync in [false, true] {
            let encoded = encode(xml, sync);
            let wrapped = encoded.as_bytes().chunks(11).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("\r\n ");
            assert!(decode(&wrapped).unwrap().root.child("geoDataEntityQueryResults").is_some());
            let mut bytes = STANDARD.decode(&encoded).unwrap();
            bytes.pop();
            assert!(decode(&STANDARD.encode(bytes)).is_none());
        }
    }
    #[test]
    fn malformed_wrong_root_trailing_bytes_and_expansion_are_rejected() {
        assert!(decode("not base64!").is_none());
        assert!(decode(&encode(b"<clear>", true)).is_none());
        assert!(decode(&encode(b"<other/>", false)).is_none());
        let mut bytes = STANDARD.decode(encode(b"<clear/>", false)).unwrap();
        bytes.push(0);
        assert!(decode(&STANDARD.encode(bytes)).is_none());
        let bytes = STANDARD.decode(encode(&vec![b' '; 16384], false)).unwrap();
        assert!(inflate(&bytes, 1024).is_none());
    }
}
