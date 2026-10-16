//! Packaging regression: Finder needs Editor registrations, not just UTI declarations.
use deckcraft_pptx::{XmlEl, parse_xml};

fn value<'a>(dict: &'a XmlEl, name: &str) -> Option<&'a XmlEl> {
    let mut children = dict.elements();
    while let Some(key) = children.next() {
        let value = children.next()?;
        if key.is("key") && key.text() == name {
            return Some(value);
        }
    }
    None
}

#[test]
fn finder_routes_supported_presentations_to_the_application_event_handler() {
    let plist = parse_xml(include_bytes!("../../../packaging/macos/Info.plist.in")).unwrap();
    let dict = plist.root.child("dict").unwrap();
    let types = value(dict, "CFBundleDocumentTypes").expect("Finder cannot open documents without CFBundleDocumentTypes");
    for (ext, uti) in [
        ("deckcraft", "ai.storyteller.deckcraft.presentation"),
        ("pptx", "org.openxmlformats.presentationml.presentation"),
        ("potx", "org.openxmlformats.presentationml.template"),
        ("ppsx", "org.openxmlformats.presentationml.slideshow"),
        ("odp", "org.oasis-open.opendocument.presentation"),
    ] {
        let doc = types
            .elements()
            .find(|d| value(d, "CFBundleTypeExtensions").is_some_and(|extensions| extensions.elements().any(|s| s.text() == ext)))
            .unwrap_or_else(|| panic!("{ext} must be accepted by Finder"));
        assert_eq!(value(doc, "CFBundleTypeRole").unwrap().text(), "Editor", "{ext}");
        assert!(value(doc, "LSItemContentTypes").unwrap().elements().any(|s| s.text() == uti), "{ext}: {uti}");
        assert!(value(doc, "NSDocumentClass").is_none(), "AppKit must deliver an open event rather than invoke NSDocument's unsupported reader");
    }
    let imported = value(dict, "UTImportedTypeDeclarations").unwrap();
    let odp =
        imported.elements().find(|d| value(d, "UTTypeIdentifier").is_some_and(|s| s.text() == "org.oasis-open.opendocument.presentation")).unwrap();
    let tags = value(odp, "UTTypeTagSpecification").unwrap();
    assert_eq!(value(tags, "public.filename-extension").unwrap().elements().next().unwrap().text(), "odp");
    assert_eq!(value(tags, "public.mime-type").unwrap().elements().next().unwrap().text(), "application/vnd.oasis.opendocument.presentation");
}
