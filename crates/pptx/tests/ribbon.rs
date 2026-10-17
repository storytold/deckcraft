//! The presentation's ribbon part (customUI): modelled when it is tabs, groups and macro buttons,
//! kept unchanged when it is richer, written as `customUI/customUI14.xml`, in `.pptx` and `.pptm`.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_model::{Presentation, Ribbon, RibbonButton, RibbonGroup, RibbonTab};
use deckcraft_pptx::PptxKind;
use deckcraft_pptx::custom_ui::{current, parse_buttons, parse_exact};
use zip::write::SimpleFileOptions;

const SIMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<customUI xmlns="http://schemas.microsoft.com/office/2009/07/customui">
  <ribbon>
    <tabs>
      <tab id="tabTools" label="Tools">
        <group id="grpSlides" label="Slides">
          <button id="btnStamp" label="Stamp &amp; date" size="large" imageMso="HappyFace" onAction="Module1.Stamp" screentip="Stamps every slide"/>
          <button id="btnClear" label="Clear" onAction="Clear" tag="all"/>
        </group>
      </tab>
      <tab idMso="TabHome">
        <group id="grpQuick" label="Quick">
          <button id="btnHello" label="Hello" onAction="Hello"/>
        </group>
      </tab>
    </tabs>
  </ribbon>
</customUI>"#;

const RICH: &str = r#"<customUI xmlns="http://schemas.microsoft.com/office/2009/07/customui" onLoad="RibbonLoaded"><ribbon><tabs><tab id="t" label="Mine"><group id="g" label="G"><menu id="m" label="More"><button id="b1" label="Inside" onAction="Inside"/></menu><button id="b2" image="logo" onAction="Logo"/><button idMso="Copy"/></group></tab></tabs></ribbon></customUI>"#;
const RICH_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="logo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="images/logo.png"/></Relationships>"#;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR not really a picture \xff\xfe";

fn deck() -> Presentation {
    Presentation { vba_project: Some(Arc::new(b"project".to_vec())), ..Default::default() }
}

/// A saved deck with `custom_ui` added at `customUI/customUI14.xml` (and its own relationships).
fn package(custom_ui: &str, ui_rels: Option<&str>) -> Vec<u8> {
    let bytes = deckcraft_pptx::export_as(&deck(), PptxKind::MacroPresentation).expect("export");
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut f = src.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).expect("read");
        if name == "_rels/.rels" {
            data = String::from_utf8(data)
                .expect("utf8")
                .replacen(
                    "</Relationships>",
                    r#"<Relationship Id="rIdUI" Type="http://schemas.microsoft.com/office/2007/relationships/ui/extensibility" Target="customUI/customUI14.xml"/></Relationships>"#,
                    1,
                )
                .into_bytes();
        }
        if name == "[Content_Types].xml" {
            data = String::from_utf8(data)
                .expect("utf8")
                .replacen("</Types>", r#"<Default Extension="png" ContentType="image/png"/></Types>"#, 1)
                .into_bytes();
        }
        out.start_file(name, SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    let mut extra: Vec<(&str, &[u8])> = vec![("customUI/customUI14.xml", custom_ui.as_bytes())];
    if let Some(r) = ui_rels {
        extra.push(("customUI/_rels/customUI14.xml.rels", r.as_bytes()));
        extra.push(("customUI/images/logo.png", PNG));
    }
    for (n, d) in extra {
        out.start_file(n, SimpleFileOptions::default()).expect("start");
        out.write_all(d).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut s = Vec::new();
    z.by_name(name).ok()?.read_to_end(&mut s).ok()?;
    Some(s)
}

fn text(bytes: &[u8], name: &str) -> Option<String> {
    part(bytes, name).and_then(|b| String::from_utf8(b).ok())
}

#[test]
fn a_ribbon_of_macro_buttons_is_modelled_and_round_trips() {
    let p = deckcraft_pptx::import(&package(SIMPLE, None)).unwrap();
    let r = p.ribbon.as_ref().expect("modelled");
    assert_eq!(r.tabs.len(), 2);
    assert_eq!(r.tabs[0].label, "Tools");
    assert_eq!(r.tabs[1].builtin.as_deref(), Some("TabHome"));
    let stamp = r.button("btnStamp").unwrap();
    assert_eq!((stamp.label.as_str(), stamp.on_action.as_str(), stamp.large), ("Stamp & date", "Module1.Stamp", true));
    assert_eq!(stamp.image.as_deref(), Some("HappyFace"));
    assert_eq!(r.button("btnClear").unwrap().tag.as_deref(), Some("all"));
    // Modelled means not kept as well: one ribbon part on save, the model's.
    assert!(p.kept_parts.iter().all(|k| !k.name.to_ascii_lowercase().contains("customui")), "{:?}", p.kept_parts);
    assert_eq!(current(&p), (r.clone(), true));

    for kind in [PptxKind::MacroPresentation, PptxKind::Presentation] {
        let out = deckcraft_pptx::export_as(&p, kind).unwrap();
        let xml = text(&out, "customUI/customUI14.xml").expect("written");
        assert!(xml.contains("2009/07/customui") && xml.contains("onAction=\"Module1.Stamp\""), "{xml}");
        let rels = text(&out, "_rels/.rels").unwrap();
        assert_eq!(rels.matches("ui/extensibility").count(), 1, "{rels}");
        let back = deckcraft_pptx::import(&out).unwrap();
        assert_eq!(back.ribbon, p.ribbon, "the ribbon survives a round trip");
        assert!(back.kept_parts.is_empty());
    }
}

#[test]
fn a_richer_ribbon_is_kept_unchanged_with_its_pictures_and_its_buttons_still_found() {
    assert!(parse_exact(RICH.as_bytes()).is_none());
    let p = deckcraft_pptx::import(&package(RICH, Some(RICH_RELS))).unwrap();
    assert!(p.ribbon.is_none());
    let (shown, editable) = current(&p);
    assert!(!editable);
    let labels: Vec<&str> = shown.tabs[0].groups[0].buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Inside", "b2"], "menu buttons found, built-in commands left out");
    // Kept byte for byte, pictures too, and written once even with a modelled ribbon beside it.
    let mut q = p.clone();
    q.ribbon = Some(Ribbon {
        tabs: vec![RibbonTab {
            builtin: Some("TabHome".into()),
            groups: vec![RibbonGroup {
                id: "g9".into(),
                label: "Mine".into(),
                buttons: vec![RibbonButton { id: "b9".into(), label: "Go".into(), on_action: "Go".into(), ..Default::default() }],
            }],
            ..Default::default()
        }],
    });
    let out = deckcraft_pptx::export_as(&q, PptxKind::MacroPresentation).unwrap();
    assert_eq!(text(&out, "customUI/customUI14.xml").as_deref(), Some(RICH));
    assert_eq!(part(&out, "customUI/images/logo.png").as_deref(), Some(PNG));
    assert_eq!(text(&out, "customUI/_rels/customUI14.xml.rels").as_deref(), Some(RICH_RELS));
    assert_eq!(text(&out, "_rels/.rels").unwrap().matches("ui/extensibility").count(), 1);
    let back = deckcraft_pptx::import(&out).unwrap();
    assert_eq!(back.kept_parts, p.kept_parts, "a second save keeps them once");
}

#[test]
fn a_ribbon_is_written_for_a_new_deck_and_not_when_empty() {
    let mut p = deck();
    p.ribbon = Some(Ribbon::default());
    assert!(text(&deckcraft_pptx::export(&p).unwrap(), "customUI/customUI14.xml").is_none(), "an empty ribbon writes nothing");
    p.ribbon = Some(Ribbon {
        tabs: vec![RibbonTab {
            builtin: Some("TabDeveloper".into()),
            groups: vec![RibbonGroup {
                id: "g1".into(),
                label: "My <macros>".into(),
                buttons: vec![RibbonButton { id: "b1".into(), label: "Run \"it\"".into(), on_action: "Module1.Go".into(), ..Default::default() }],
            }],
            ..Default::default()
        }],
    });
    let out = deckcraft_pptx::export_as(&p, PptxKind::MacroPresentation).unwrap();
    assert_eq!(deckcraft_pptx::import(&out).unwrap().ribbon, p.ribbon);
    // The native JSON keeps it too.
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<Presentation>(&json).unwrap().ribbon, p.ribbon);
    assert!(parse_exact(b"<customUI><ribbon startFromScratch=\"true\"/></customUI>").is_none());
    assert!(parse_exact(b"<notUI/>").is_none());
    assert!(parse_buttons(b"<<<").tabs.is_empty(), "hostile input finds nothing");
}
