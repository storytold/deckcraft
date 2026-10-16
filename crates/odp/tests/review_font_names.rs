//! Independent, contributor-original opaque-font-alias preservation regressions.
//! Only synthetic XML and the name of the OFL BIZ UDPGothic face are used.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const NS: &str = "xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:t='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:v='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:f='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:x='http://www.w3.org/1999/xlink'";
const SLOTS: [&str; 3] = ["font-name", "font-name-asian", "font-name-complex"];

fn escaped(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}
fn face(raw_alias: &str) -> String {
    format!("<s:font-face s:name=\"{raw_alias}\" v:font-family=\"BIZ UDPGothic\"/>")
}
fn fixture(version: &str, aliases: [&str; 3], cache: bool) -> BTreeMap<String, Vec<u8>> {
    let declarations: String = aliases.into_iter().map(face).collect();
    let references: String = SLOTS.into_iter().zip(aliases).map(|(slot, alias)| format!(" s:{slot}=\"{alias}\"")).collect();
    let content = format!(
        "<o:document-content {NS} o:version=\"{version}\">\r\n<!--font review opening--><o:font-face-decls>{declarations}</o:font-face-decls><o:automatic-styles><s:style s:name=\"Paragraph\" s:family=\"paragraph\"><s:paragraph-properties f:keep-with-next=\"always\"/><s:text-properties{references}/></s:style></o:automatic-styles><o:body><o:presentation><d:page d:name=\"Font review\" d:master-page-name=\"Master\"><d:frame xml:id=\"Frame\" v:width=\"190pt\" v:height=\"80pt\"><d:text-box><t:p t:style-name=\"Paragraph\">Origin!</t:p></d:text-box></d:frame></d:page></o:presentation></o:body><!--closing survivor--></o:document-content>"
    );
    let styles = format!(
        "<o:document-styles {NS} o:version=\"{version}\"><o:font-face-decls/><o:styles/><o:automatic-styles><s:page-layout s:name=\"Paper\"><s:page-layout-properties f:page-width=\"500pt\" f:page-height=\"300pt\"/></s:page-layout></o:automatic-styles><o:master-styles><s:master-page s:name=\"Master\" s:page-layout-name=\"Paper\"/></o:master-styles></o:document-styles>"
    );
    let statistic = if cache { "<m:document-statistic m:word-count=\"1\" m:character-count=\"7\" m:page-count=\"1\"/>" } else { "" };
    let meta = format!("<o:document-meta {NS} o:version=\"{version}\"><o:meta><!--metadata survivor-->{statistic}</o:meta></o:document-meta>");
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
    ]);
    if cache {
        files.insert("Thumbnails/thumbnail.png".into(), deckcraft_render::Image { width: 1, height: 1, pixels: vec![121, 45, 167, 255] }.to_png());
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("\n<!--owner survivor--><manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>\t"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}
fn standard() -> BTreeMap<String, Vec<u8>> {
    fixture("1.3", ["West 日本", "East العربية", "Complex mark"], true)
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(old), "missing independent mutation in {part}: {old}");
    files.insert(part.into(), text.replace(old, new).into_bytes());
}
fn package(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("mimetype", options).unwrap();
    archive.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        archive.start_file(name, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(files.insert(entry.name().into(), bytes).is_none());
    }
    files
}
fn edit(model: &mut Presentation) {
    // Equal byte length permits whole-model equality, including all source ranges.
    Arc::make_mut(&mut model.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Change!".into();
}
fn succeeds(files: &BTreeMap<String, Vec<u8>>) {
    let original = package(files);
    let mut model = import(&original).expect("independent synthetic font package imports");
    assert!(export(&model).is_err(), "passive unsupported paragraph property must select preservation and require a real edit");
    let props = &model.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0].props;
    for resolved in [&props.font, &props.font_ea, &props.font_cs] {
        assert_eq!(resolved.as_deref(), Some("BIZ UDPGothic"));
    }
    edit(&mut model);
    let snapshot = model.clone();
    let output = export(&model).expect("independent opaque alias package permits real text export");
    assert_eq!(model, snapshot);
    let mut expected = files.clone();
    replace(&mut expected, "content.xml", ">Origin!</t:p>", ">Change!</t:p>");
    if expected.remove("Thumbnails/thumbnail.png").is_some() {
        replace(&mut expected, MANIFEST, "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>", "");
        replace(&mut expected, "meta.xml", " m:word-count=\"1\"", "");
        replace(&mut expected, "meta.xml", " m:character-count=\"7\"", "");
    }
    assert_eq!(unpack(&output), expected, "all surviving source gaps and package payloads remain byte-exact");
    let mut reopened = import(&output).expect("independent edited package reimports");
    let mut attachments = reopened.media.iter_mut().filter(|media| media.content_type == SOURCE);
    let attachment = attachments.next().unwrap();
    assert_eq!(attachment.data.as_slice(), output);
    attachment.data = Arc::new(original.clone());
    assert!(attachments.next().is_none());
    assert_eq!(reopened, model, "full semantic model including source ranges survives equal-length edit");
    assert_eq!(model.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), original);
}
fn refuses(files: &BTreeMap<String, Vec<u8>>, reason: &str) {
    let original = package(files);
    let mut model = import(&original).unwrap_or_else(|_| import(&package(&standard())).unwrap());
    edit(&mut model);
    model.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(original);
    let snapshot = model.clone();
    let error = export(&model).expect_err("independent hostile alias package must refuse").to_string();
    assert!(error.ends_with(reason), "wrong refusal for {reason}: {error}");
    assert!(!error.contains("changes outside plain slide text"), "proof must refuse before model comparison: {error}");
    assert_eq!(model, snapshot, "refusal leaves every field and source byte unchanged");
}

#[test]
fn independent_three_distinct_script_aliases_preserve_both_versions_with_and_without_caches() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            succeeds(&fixture(version, ["9 reviewer 日本", "العربية &amp; alias", "p:undeclared-prefix/face"], cache));
        }
    }
}

#[test]
fn independent_alias_punctuation_is_data_and_default_element_namespace_is_proven() {
    for alias in ["../Thumbnails/thumbnail.png?x#y", "url(https://example.invalid/f)", " Face, 'one' & two \"end\" ", "\u{301}", "A\u{200d}B"] {
        let alias = escaped(alias);
        let mut files = fixture("1.3", [&alias, "Separate East", "Separate Complex"], true);
        replace(
            &mut files,
            "content.xml",
            &face(&alias),
            &format!("<font-face xmlns='{STYLE}' s:name=\"{alias}\" v:font-family=\"BIZ UDPGothic\"/>"),
        );
        succeeds(&files);
    }
}

#[test]
fn independent_mixed_literal_attribute_whitespace_normalizes_once_but_references_preserve_controls() {
    for (raw, normalized) in [("West\t日本", "West 日本"), ("West\r\n日本", "West 日本"), ("West\t\n\r\r\n日本", "West    日本")] {
        for (declaration, reference) in [(true, false), (false, true), (true, true)] {
            let mut files = fixture("1.3", [normalized, "East", "Complex"], true);
            if declaration {
                replace(&mut files, "content.xml", &format!("s:name=\"{normalized}\""), &format!("s:name=\"{raw}\""));
            }
            if reference {
                replace(&mut files, "content.xml", &format!("s:font-name=\"{normalized}\""), &format!("s:font-name=\"{raw}\""));
            }
            succeeds(&files);
        }
        let mut duplicate = fixture("1.3", [normalized, "East", "Complex"], true);
        replace(&mut duplicate, "content.xml", "</o:font-face-decls>", &format!("{}</o:font-face-decls>", face(raw)));
        refuses(&duplicate, "cache ambiguous definition");
    }
    for control in ["&#x9;", "&#10;", "&#13;", "&#127;", "&#x85;", "&#x9f;"] {
        for slot in SLOTS {
            let mut files = standard();
            let alias = match slot {
                "font-name" => "West 日本",
                "font-name-asian" => "East العربية",
                _ => "Complex mark",
            };
            replace(&mut files, "content.xml", &format!("s:{slot}=\"{alias}\""), &format!("s:{slot}=\"West{control}日本\""));
            refuses(&files, "cache unresolved font reference");
        }
        let mut files = standard();
        replace(&mut files, "content.xml", "s:name=\"West 日本\"", &format!("s:name=\"West{control}日本\""));
        refuses(&files, "cache font name grammar");
    }
}

#[test]
fn independent_each_script_reference_requires_exact_unicode_and_whitespace_identity() {
    for (defined, other) in [("Face", " Face"), ("Face", "Face "), ("Face Space", "Face\u{a0}Space"), ("Face", "face"), ("é", "e\u{301}")] {
        for (index, slot) in SLOTS.into_iter().enumerate() {
            let mut aliases = ["West", "East", "Complex"];
            aliases[index] = defined;
            let mut files = fixture("1.3", aliases, true);
            replace(&mut files, "content.xml", &format!("s:{slot}=\"{defined}\""), &format!("s:{slot}=\"{other}\""));
            refuses(&files, "cache unresolved font reference");
            replace(&mut files, "content.xml", "</o:font-face-decls>", &format!("{}</o:font-face-decls>", face(other)));
            succeeds(&files);
        }
    }
}

#[test]
fn independent_decoded_utf8_bound_and_entity_collisions_apply_to_both_typed_paths() {
    let boundary = format!("{}x", "語".repeat(85));
    assert_eq!(boundary.len(), 256);
    succeeds(&fixture("1.3", [&boundary, "East", "Complex"], true));
    let long = format!("{boundary}y");
    refuses(&fixture("1.3", [&long, "East", "Complex"], true), "cache font name grammar");
    for slot in SLOTS {
        let mut files = standard();
        let alias = match slot {
            "font-name" => "West 日本",
            "font-name-asian" => "East العربية",
            _ => "Complex mark",
        };
        replace(&mut files, "content.xml", &format!("s:{slot}=\"{alias}\""), &format!("s:{slot}=\"{}\"", "&#x754c;".repeat(86)));
        refuses(&files, "cache unresolved font reference");
    }
    let mut collision = standard();
    replace(&mut collision, "content.xml", "</o:font-face-decls>", &format!("{}</o:font-face-decls>", face("West&#32;日本")));
    refuses(&collision, "cache ambiguous definition");
    for empty in ["", " \u{a0}\u{2003} ", "\u{7f}"] {
        refuses(&fixture("1.3", [empty, "East", "Complex"], true), "cache font name grammar");
    }
}

#[test]
fn independent_font_lookup_is_part_scoped_typed_and_checks_unused_styles() {
    let declarations = ["West 日本", "East العربية", "Complex mark"].into_iter().map(face).collect::<String>();
    let mut fallback = standard();
    replace(&mut fallback, "content.xml", &declarations, "");
    replace(&mut fallback, "styles.xml", "<o:font-face-decls/>", &format!("<o:font-face-decls>{declarations}</o:font-face-decls>"));
    succeeds(&fallback);
    for slot in SLOTS {
        let mut reverse = standard();
        replace(
            &mut reverse,
            "styles.xml",
            "<o:styles/>",
            &format!("<o:styles><s:style s:name=\"Unused\" s:family=\"text\"><s:text-properties s:{slot}=\"West 日本\"/></s:style></o:styles>"),
        );
        refuses(&reverse, "cache unresolved font reference");
    }
    let mut untyped = fixture("1.3", ["FontKey", "East", "Complex"], true);
    replace(&mut untyped, "content.xml", &face("FontKey"), "");
    replace(&mut untyped, "content.xml", "<o:automatic-styles>", "<o:automatic-styles><s:style s:name=\"FontKey\" s:family=\"text\"/>");
    replace(&mut untyped, "content.xml", "xml:id=\"Frame\"", "xml:id=\"FontKey\"");
    refuses(&untyped, "cache unresolved font reference");
}

#[test]
fn independent_cross_part_alias_equivalence_keeps_existing_empty_declaration_owner_policy() {
    let mut equal = standard();
    replace(&mut equal, "styles.xml", "<o:font-face-decls/>", &format!("<o:font-face-decls>{}</o:font-face-decls>", face("West&#x20;日本")));
    succeeds(&equal);
    for extra in [" s:font-pitch=\"fixed\"", " s:font-family-generic=\"swiss\""] {
        let mut conflict = equal.clone();
        replace(&mut conflict, "styles.xml", &face("West&#x20;日本"), &face("West&#x20;日本").replace("/>", &format!("{extra}/>")));
        refuses(&conflict, "overlapping common and automatic ODF style identities are not supported");
    }
    let mut duplicate = equal;
    replace(&mut duplicate, "styles.xml", "</o:font-face-decls>", &format!("{}</o:font-face-decls>", face("West 日本")));
    refuses(&duplicate, "cache ambiguous definition");
}

#[test]
fn independent_namespace_rebinding_and_wrong_context_cannot_mint_font_names() {
    for replacement in ["xmlns:q='urn:font-review:foreign' q:name=\"West 日本\"", "name=\"West 日本\""] {
        let mut files = standard();
        replace(&mut files, "content.xml", "s:name=\"West 日本\"", replacement);
        refuses(&files, "cache XML identity/depth");
    }
    for slot in SLOTS {
        let mut files = standard();
        let alias = match slot {
            "font-name" => "West 日本",
            "font-name-asian" => "East العربية",
            _ => "Complex mark",
        };
        replace(&mut files, "content.xml", &format!("s:{slot}=\"{alias}\""), &format!("xmlns:q='urn:font-review:foreign' q:{slot}=\"{alias}\""));
        refuses(&files, "cache XML identity/depth");
    }
    let mut misplaced = standard();
    replace(&mut misplaced, "content.xml", &face("West 日本"), "");
    replace(&mut misplaced, "content.xml", "<o:automatic-styles>", &format!("<o:automatic-styles>{}", face("West 日本")));
    refuses(&misplaced, "cache unclassified XML context");
}

#[test]
fn independent_alias_admission_does_not_relax_family_identifier_or_resource_guards() {
    for (old, new, reason) in [
        ("s:name=\"Paragraph\"", "s:name=\"Paragraph 日本\"", "cache identifier grammar"),
        ("xml:id=\"Frame\"", "xml:id=\"Frame 日本\"", "cache identifier grammar"),
        ("v:font-family=\"BIZ UDPGothic\"", "v:font-family=\"url(font)\"", "cache unclassified attribute/value"),
        ("v:font-family=\"BIZ UDPGothic\"", "v:font-family=\"日本\"", "cache unclassified attribute/value"),
    ] {
        let mut files = standard();
        replace(&mut files, "content.xml", old, new);
        refuses(&files, reason);
    }
    for extra in [" x:href=\"https://example.invalid/font\"", " xml:base=\"file:///\"", " s:font-charset=\"utf-8\""] {
        let mut files = standard();
        replace(&mut files, "content.xml", &face("West 日本"), &face("West 日本").replace("/>", &format!("{extra}/>")));
        refuses(&files, "cache unclassified attribute/value");
    }
    let mut source = standard();
    replace(
        &mut source,
        "content.xml",
        &face("West 日本"),
        &face("West 日本").replace("/>", "><v:font-face-src><v:font-face-uri x:href=\"font.bin\"/></v:font-face-src></s:font-face>"),
    );
    refuses(&source, "cache unclassified XML context");
    let mut resource = fixture("1.3", ["Thumbnails/thumbnail.png", "East", "Complex"], true);
    replace(
        &mut resource,
        "content.xml",
        "</d:page>",
        "<d:frame v:width=\"10pt\" v:height=\"10pt\"><d:image x:href=\"Thumbnails/thumbnail.png\"/></d:frame></d:page>",
    );
    refuses(&resource, "cache unsafe part reference");
}
