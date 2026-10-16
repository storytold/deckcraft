//! Contributor-original ZIP32 records; no external producer files or assets.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{Presentation, text::Run};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

#[derive(Clone, Copy)]
enum Form {
    None,
    Unsigned,
    Signed,
}
#[derive(Clone)]
struct Member {
    name: String,
    data: Vec<u8>,
    method: u16,
    form: Form,
    tail: Option<Vec<u8>>,
}
struct Record {
    local: usize,
    payload: usize,
    descriptor: usize,
    central: usize,
    compressed: u32,
    size: u32,
    crc: u32,
}
struct Built {
    bytes: Vec<u8>,
    records: Vec<Record>,
    cd: usize,
}

fn crc(data: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in data {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(value & 1));
        }
    }
    !value
}
fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn set16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn set32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn compressed(data: &[u8], method: u16) -> Vec<u8> {
    if method == 0 {
        return data.to_vec();
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("data", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    zip.write_all(data).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let size = u32::from_le_bytes(bytes[18..22].try_into().unwrap()) as usize;
    let name = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as usize;
    let extra = u16::from_le_bytes(bytes[28..30].try_into().unwrap()) as usize;
    let start = 30 + name + extra;
    bytes[start..start + size].to_vec()
}
fn trailer(form: Form, crc: u32, compressed: u32, size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    if matches!(form, Form::None) {
        return out;
    }
    if matches!(form, Form::Signed) {
        put32(&mut out, 0x0807_4b50);
    }
    for value in [crc, compressed, size] {
        put32(&mut out, value);
    }
    out
}
fn raw(members: &[Member], shuffled: bool) -> Built {
    let mut out = Vec::new();
    let mut records = Vec::new();
    for member in members {
        let payload = compressed(&member.data, member.method);
        let crc = crc(&member.data);
        let (compressed, size) = (payload.len() as u32, member.data.len() as u32);
        let flags = 0x800 | if matches!(member.form, Form::None) { 0 } else { 8 };
        let local = out.len();
        out.extend_from_slice(b"PK\x03\x04");
        for value in [20u16, flags, member.method, 0, 0] {
            put16(&mut out, value);
        }
        for value in if matches!(member.form, Form::None) { [crc, compressed, size] } else { [0, 0, 0] } {
            put32(&mut out, value);
        }
        put16(&mut out, member.name.len() as u16);
        put16(&mut out, 0);
        out.extend_from_slice(member.name.as_bytes());
        let payload_start = out.len();
        out.extend_from_slice(&payload);
        let descriptor = out.len();
        out.extend_from_slice(member.tail.as_deref().unwrap_or(&trailer(member.form, crc, compressed, size)));
        records.push(Record { local, payload: payload_start, descriptor, central: 0, compressed, size, crc });
    }
    let cd = out.len();
    let mut order: Vec<_> = (0..members.len()).collect();
    if shuffled {
        order.reverse();
    }
    for index in order {
        let member = &members[index];
        let record = &mut records[index];
        record.central = out.len();
        let flags = 0x800 | if matches!(member.form, Form::None) { 0 } else { 8 };
        out.extend_from_slice(b"PK\x01\x02");
        for value in [20u16, 20, flags, member.method, 0, 0] {
            put16(&mut out, value);
        }
        for value in [record.crc, record.compressed, record.size] {
            put32(&mut out, value);
        }
        for value in [member.name.len() as u16, 0, 0, 0, 0] {
            put16(&mut out, value);
        }
        put32(&mut out, 0);
        put32(&mut out, record.local as u32);
        out.extend_from_slice(member.name.as_bytes());
    }
    let cd_size = out.len() - cd;
    out.extend_from_slice(b"PK\x05\x06");
    for value in [0u16, 0, members.len() as u16, members.len() as u16] {
        put16(&mut out, value);
    }
    put32(&mut out, cd_size as u32);
    put32(&mut out, cd as u32);
    put16(&mut out, 0);
    Built { bytes: out, records, cd }
}

fn fixture(method: u16, forms: &[Form]) -> Vec<Member> {
    const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Synthetic' draw:master-page-name='M'><draw:frame xml:id='original-frame' svg:width='240pt' svg:height='120pt'><draw:text-box><text:p text:style-name='P'>OriginalOne</text:p><text:p text:style-name='P'>OriginalTwo</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='960pt' fo:page-height='540pt' style:print-orientation='landscape'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let metadata = format!("<office:document-meta {NS} office:version='1.3'><office:meta/></office:document-meta>");
    // A real original one-pixel PNG plus inert trailing bytes exercise marker-like payloads.
    let mut picture = deckcraft_render::Image { width: 1, height: 1, pixels: vec![17, 71, 139, 255] }.to_png();
    picture.extend_from_slice(b"PK\x07\x08\x11\x22\x33\x44\x55\x66\x77\x88PK\x03\x04");
    let mut data = vec![
        ("mimetype", MIME.as_bytes().to_vec()),
        ("content.xml", content.into_bytes()),
        ("styles.xml", styles.into_bytes()),
        ("meta.xml", metadata.into_bytes()),
        ("Pictures/original.png", picture),
        ("Pictures/", Vec::new()),
    ];
    let mut owners = format!("<manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/>");
    for (name, _) in data.iter().skip(1).filter(|(name, _)| !name.ends_with('/')) {
        owners.push_str(&format!(
            "<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{}'/>",
            if name.ends_with(".png") { "image/png" } else { "text/xml" }
        ));
    }
    owners.push_str("</manifest:manifest>");
    data.push((
        "META-INF/manifest.xml",
        format!("<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'>{owners}")
            .into_bytes(),
    ));
    data.into_iter()
        .enumerate()
        .map(|(index, (name, data))| Member {
            name: name.into(),
            data,
            method: if name == "mimetype" || name.ends_with('/') { 0 } else { method },
            form: forms[index % forms.len()],
            tail: None,
        })
        .collect()
}
fn members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        out.insert(file.name().into(), bytes);
    }
    out
}
fn edit(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    let body = Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap();
    for (paragraph, text) in body.paragraphs.iter_mut().zip(["EditedOne", "EditedTwo"]) {
        paragraph.runs = vec![Run::with(text, paragraph.runs[0].props.clone())];
    }
    p
}
fn assert_edits(source: &[u8]) {
    let p = edit(source);
    let output = export(&p).expect("validated descriptors must permit real source edits");
    let before = members(source);
    let after = members(&output);
    assert_ne!(source, output);
    assert_eq!(before.len(), after.len());
    for (name, bytes) in &before {
        if name == "content.xml" {
            let expected = String::from_utf8(bytes.clone()).unwrap().replace("OriginalOne", "EditedOne").replace("OriginalTwo", "EditedTwo");
            assert_eq!(&after[name], expected.as_bytes());
        } else {
            assert_eq!(bytes, &after[name], "modified {name}");
        }
    }
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "EditedOne\nEditedTwo");
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
}
fn assert_bad(bytes: Vec<u8>) {
    let good = raw(&fixture(0, &[Form::None]), false);
    let mut p = edit(&good.bytes);
    p.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(bytes);
    assert!(export(&p).unwrap_err().to_string().contains("ZIP32"));
}

#[test]
fn signed_unsigned_stored_deflated_mixed_and_shuffled_preserve_real_edits() {
    for method in [0, 8] {
        for forms in [&[Form::Signed][..], &[Form::Unsigned][..], &[Form::None, Form::Signed, Form::Unsigned][..]] {
            for shuffled in [false, true] {
                assert_edits(&raw(&fixture(method, forms), shuffled).bytes);
            }
        }
    }
}

#[test]
fn a_real_payload_crc_equal_to_the_descriptor_marker_is_unambiguous() {
    // Derive a four-byte suffix by solving the 32-bit CRC's affine map over GF(2).
    // This is contributor-original fixture generation, with no copied producer bytes.
    fn suffix(prefix: &[u8], target: u32) -> [u8; 4] {
        let mut data = prefix.to_vec();
        data.extend_from_slice(&[0; 4]);
        let base = crc(&data);
        let start = data.len() - 4;
        let mut columns = [0u32; 32];
        for (bit, column) in columns.iter_mut().enumerate() {
            data[start + bit / 8] = 1 << (bit % 8);
            *column = crc(&data) ^ base;
            data[start + bit / 8] = 0;
        }
        let mut rows = [0u64; 32];
        for (bit, row) in rows.iter_mut().enumerate() {
            *row = u64::from(((target ^ base) >> bit) & 1) << 32;
            for (column, value) in columns.iter().enumerate() {
                *row |= u64::from((value >> bit) & 1) << column;
            }
        }
        for column in 0..32 {
            let pivot = (column..32).find(|row| rows[*row] & (1 << column) != 0).unwrap();
            rows.swap(column, pivot);
            let selected = rows[column];
            for (row, value) in rows.iter_mut().enumerate() {
                if row != column && *value & (1 << column) != 0 {
                    *value ^= selected;
                }
            }
        }
        rows.iter().enumerate().fold(0u32, |out, (bit, row)| out | (((row >> 32) as u32 & 1) << bit)).to_le_bytes()
    }
    for method in [0, 8] {
        for form in [Form::Unsigned, Form::Signed] {
            let mut fixture = fixture(method, &[form]);
            let value = suffix(&fixture[4].data, 0x0807_4b50);
            fixture[4].data.extend_from_slice(&value);
            assert_eq!(crc(&fixture[4].data), 0x0807_4b50);
            let built = raw(&fixture, true);
            assert_eq!(built.records[4].crc, 0x0807_4b50);
            assert_edits(&built.bytes);
        }
    }
}

#[test]
fn descriptor_fields_marker_and_local_placeholders_must_all_agree() {
    for form in [Form::Unsigned, Form::Signed] {
        for field in 0..3 {
            let mut built = raw(&fixture(8, &[form]), false);
            let target = &built.records[1];
            let offset = target.descriptor + if matches!(form, Form::Signed) { 4 } else { 0 };
            let expected = [target.crc, target.compressed, target.size][field];
            set32(&mut built.bytes, offset + field * 4, expected.wrapping_add(1));
            assert_bad(built.bytes);
            let mut built = raw(&fixture(8, &[form]), false);
            let local = built.records[1].local;
            set32(&mut built.bytes, local + 14 + field * 4, 1);
            assert_bad(built.bytes);
        }
    }
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let descriptor = built.records[1].descriptor;
    built.bytes[descriptor] ^= 1;
    assert_bad(built.bytes);
}

#[test]
fn missing_truncated_extra_and_zip64_width_descriptors_are_rejected() {
    for form in [Form::Unsigned, Form::Signed] {
        let fixture = fixture(8, &[form]);
        let valid = raw(&fixture, false);
        let target = &valid.records[1];
        let proper = trailer(form, target.crc, target.compressed, target.size);
        let mut zip64 = Vec::new();
        if matches!(form, Form::Signed) {
            put32(&mut zip64, 0x0807_4b50);
        }
        put32(&mut zip64, target.crc);
        zip64.extend_from_slice(&u64::from(target.compressed).to_le_bytes());
        zip64.extend_from_slice(&u64::from(target.size).to_le_bytes());
        for tail in
            [Vec::new(), proper[..proper.len() - 1].to_vec(), [proper.clone(), vec![0]].concat(), [proper.clone(), proper.clone()].concat(), zip64]
        {
            let mut fixture = fixture.clone();
            fixture[1].tail = Some(tail);
            assert_bad(raw(&fixture, false).bytes);
        }
        let mut fixture = fixture.clone();
        fixture[1].form = Form::None;
        fixture[1].tail = Some(proper);
        assert_bad(raw(&fixture, false).bytes);
    }
    // The last physical descriptor must end exactly where the central directory starts.
    let mut fixture = fixture(0, &[Form::Signed]);
    let valid = raw(&fixture, false);
    let last = fixture.len() - 1;
    let target = &valid.records[last];
    fixture[last].tail = Some(trailer(Form::Signed, target.crc, target.compressed, target.size)[..15].to_vec());
    assert_bad(raw(&fixture, false).bytes);
}

#[test]
fn complete_structural_descriptors_do_not_bypass_crc_or_size_budgets() {
    let mut built = raw(&fixture(0, &[Form::Signed]), false);
    let target = &built.records[4];
    built.bytes[target.payload + 9] ^= 1;
    assert_bad(built.bytes);
    let mut built = raw(&fixture(0, &[Form::Signed]), false);
    let target = &built.records[1];
    let wrong = target.crc.wrapping_add(1);
    set32(&mut built.bytes, target.central + 16, wrong);
    set32(&mut built.bytes, target.descriptor + 4, wrong);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let target = &built.records[1];
    set32(&mut built.bytes, target.central + 24, 33 * 1024 * 1024);
    set32(&mut built.bytes, target.descriptor + 12, 33 * 1024 * 1024);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    for (index, target) in built.records.iter().enumerate() {
        if index != 0 && target.size > 0 {
            set32(&mut built.bytes, target.central + 24, 32 * 1024 * 1024);
            set32(&mut built.bytes, target.descriptor + 12, 32 * 1024 * 1024);
        }
    }
    assert_bad(built.bytes);
}

#[test]
fn descriptor_support_retains_raw_offsets_flags_counts_and_name_guards() {
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let target = &built.records[1];
    set16(&mut built.bytes, target.local + 6, 0x800);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let target = &built.records[1];
    set16(&mut built.bytes, target.local + 8, 0);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let target = &built.records[1];
    built.bytes[target.local + 30] ^= 1;
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let offset = built.records[1].central;
    set32(&mut built.bytes, offset + 42, 0);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let end = built.bytes.len() - 22;
    set16(&mut built.bytes, end + 8, 10_001);
    set16(&mut built.bytes, end + 10, 10_001);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let offset = built.records[1].central;
    set32(&mut built.bytes, offset + 20, built.records[1].compressed - 1);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let end = built.bytes.len() - 22;
    set32(&mut built.bytes, end + 16, built.cd as u32 - 1);
    assert_bad(built.bytes);
    let mut built = raw(&fixture(8, &[Form::Signed]), false);
    let target = &built.records[1];
    set16(&mut built.bytes, target.local + 6, 0x809);
    set16(&mut built.bytes, target.central + 8, 0x809);
    assert_bad(built.bytes);
    let mut entries = fixture(8, &[Form::Signed]);
    entries.push(entries[1].clone());
    assert_bad(raw(&entries, false).bytes);
    let mut entries = fixture(8, &[Form::Signed]);
    entries[1].name = "META-INF/../content.xml".into();
    assert_bad(raw(&entries, false).bytes);
}
