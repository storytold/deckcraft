//! The presentation's ribbon part (`customUI/customUI14.xml`, Office 2010 namespace, or the Office
//! 2007 `customUI/customUI.xml`), from the published RibbonX schema. It hangs off the package root.
//!
//! [`parse_exact`] models a ribbon only when the model can write all of it back (tabs, groups,
//! macro buttons); anything richer stays in the file as kept parts, and [`parse_buttons`] still
//! finds its macro buttons so they show and run.

use std::fmt::Write as _;

use deckcraft_model::ribbon::MAX_RIBBON_ITEMS;
use deckcraft_model::{Presentation, Ribbon, RibbonButton, RibbonGroup, RibbonTab};

use crate::opc::{self, Package};
use crate::xml::{self, El, esc_attr};

pub const NS_CUSTOM_UI14: &str = "http://schemas.microsoft.com/office/2009/07/customui";
/// The relationship type of the Office 2010 part (`customUI14.xml`).
pub const REL_CUSTOM_UI14: &str = "http://schemas.microsoft.com/office/2007/relationships/ui/extensibility";
/// The relationship type of the Office 2007 part (`customUI.xml`).
pub const REL_CUSTOM_UI: &str = "http://schemas.microsoft.com/office/2006/relationships/ui/extensibility";
pub const PART_CUSTOM_UI14: &str = "customUI/customUI14.xml";
/// The ribbon's own XML is small; a larger part isn't a ribbon we model.
const MAX_RIBBON_XML: usize = 4 * 1024 * 1024;
/// How deep [`parse_buttons`] looks for buttons inside menus and split buttons.
const MAX_NESTING: usize = 8;

/// Is `ty` one of the two ribbon relationship types?
pub fn is_ribbon_rel(ty: &str) -> bool {
    ty == REL_CUSTOM_UI14 || ty == REL_CUSTOM_UI
}

/// Whether the part is the Office 2010 flavour (preferred when a file holds both).
pub fn is_v14(bytes: &[u8]) -> bool {
    bytes.windows(b"2009/07/customui".len()).any(|w| w == b"2009/07/customui")
}

/// No text and only the `allowed` attributes (namespace declarations aside).
fn only(e: &El, allowed: &[&str]) -> bool {
    e.text().trim().is_empty()
        && e.attrs.iter().all(|(k, v)| k.starts_with("xmlns") || allowed.contains(&k.as_str()) || (k == "startFromScratch" && v == "false"))
}

/// The ribbon, when it is made only of what the model holds; `None` otherwise (or when it isn't
/// a ribbon part at all).
pub fn parse_exact(bytes: &[u8]) -> Option<Ribbon> {
    if bytes.len() > MAX_RIBBON_XML {
        return None;
    }
    let root = xml::parse(bytes).ok()?.root;
    if !root.is("customUI") || !only(&root, &[]) {
        return None;
    }
    let mut out = Ribbon::default();
    for ribbon in root.elements() {
        if !ribbon.is("ribbon") || !only(ribbon, &[]) {
            return None;
        }
        for tabs in ribbon.elements() {
            if !tabs.is("tabs") || !only(tabs, &[]) {
                return None;
            }
            for tab in tabs.elements() {
                if !tab.is("tab") || !only(tab, &["id", "idMso", "label"]) || tab.elements().count() > MAX_RIBBON_ITEMS {
                    return None;
                }
                let builtin = tab.attr("idMso").map(str::to_string);
                let id = tab.attr("id").unwrap_or("").to_string();
                if builtin.is_some() == !id.is_empty() {
                    return None;
                }
                let mut t = RibbonTab { id, builtin, label: tab.attr("label").unwrap_or("").to_string(), groups: Vec::new() };
                for group in tab.elements() {
                    if !group.is("group") || !only(group, &["id", "label"]) || group.elements().count() > MAX_RIBBON_ITEMS {
                        return None;
                    }
                    let mut g =
                        RibbonGroup { id: group.attr("id")?.to_string(), label: group.attr("label").unwrap_or("").to_string(), buttons: Vec::new() };
                    for b in group.elements() {
                        let allowed = ["id", "label", "onAction", "imageMso", "size", "screentip", "supertip", "tag"];
                        if !b.is("button") || !only(b, &allowed) || b.elements().next().is_some() {
                            return None;
                        }
                        let large = match b.attr("size") {
                            None | Some("normal") => false,
                            Some("large") => true,
                            Some(_) => return None,
                        };
                        g.buttons.push(button(b, b.attr("id")?, large)?);
                    }
                    t.groups.push(g);
                }
                out.tabs.push(t);
                if out.tabs.len() > MAX_RIBBON_ITEMS {
                    return None;
                }
            }
        }
    }
    Some(out)
}

fn button(b: &El, id: &str, large: bool) -> Option<RibbonButton> {
    let s = |k: &str| b.attr(k).map(str::to_string);
    Some(RibbonButton {
        id: id.to_string(),
        label: s("label").unwrap_or_default(),
        on_action: s("onAction")?,
        image: s("imageMso"),
        large,
        screentip: s("screentip"),
        supertip: s("supertip"),
        tag: s("tag"),
    })
}

/// Whether a control is shown (`visible`, default true).
fn visible(e: &El) -> bool {
    !matches!(e.attr("visible"), Some("false" | "0"))
}

/// The macro buttons of any ribbon part, wherever they sit in a group (menus and split buttons
/// included), for showing a ribbon that isn't modelled. Buttons without a fixed label are named
/// by their id; built-in commands (`idMso`) and buttons without `onAction` are left out.
pub fn parse_buttons(bytes: &[u8]) -> Ribbon {
    let mut out = Ribbon::default();
    if bytes.len() > MAX_RIBBON_XML {
        return out;
    }
    let Ok(doc) = xml::parse(bytes) else { return out };
    let Some(tabs) = doc.root.path(&["ribbon", "tabs"]) else { return out };
    for tab in tabs.children_named("tab").take(MAX_RIBBON_ITEMS) {
        if !visible(tab) {
            continue;
        }
        let mut t = RibbonTab {
            id: tab.attr("id").or(tab.attr("idQ")).unwrap_or("").to_string(),
            builtin: tab.attr("idMso").map(str::to_string),
            label: tab.attr("label").unwrap_or("").to_string(),
            groups: Vec::new(),
        };
        for group in tab.children_named("group").take(MAX_RIBBON_ITEMS) {
            let mut g = RibbonGroup {
                id: group.attr("id").or(group.attr("idQ")).unwrap_or("").to_string(),
                label: group.attr("label").unwrap_or("").to_string(),
                buttons: Vec::new(),
            };
            collect(group, &mut g.buttons, 0);
            if !g.buttons.is_empty() {
                t.groups.push(g);
            }
        }
        if !t.groups.is_empty() {
            out.tabs.push(t);
        }
    }
    out
}

fn collect(e: &El, out: &mut Vec<RibbonButton>, depth: usize) {
    if depth > MAX_NESTING {
        return;
    }
    for c in e.elements() {
        if out.len() >= MAX_RIBBON_ITEMS {
            return;
        }
        if c.is("button") && c.attr("idMso").is_none() && visible(c) {
            let large = c.attr("size") == Some("large");
            if let Some(mut b) = button(c, c.attr("id").or(c.attr("idQ")).unwrap_or(""), large) {
                if b.label.is_empty() {
                    b.label = b.id.clone();
                }
                out.push(b);
            }
        } else {
            collect(c, out, depth + 1);
        }
    }
}

/// The ribbon as a `customUI14.xml` part.
pub fn write(r: &Ribbon) -> String {
    let mut x = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<customUI xmlns=\"{NS_CUSTOM_UI14}\"><ribbon><tabs>");
    for t in &r.tabs {
        match &t.builtin {
            Some(mso) => {
                let _ = write!(x, "<tab idMso=\"{}\"", esc_attr(mso));
            }
            None => {
                let _ = write!(x, "<tab id=\"{}\"", esc_attr(&t.id));
            }
        }
        if !t.label.is_empty() {
            let _ = write!(x, " label=\"{}\"", esc_attr(&t.label));
        }
        x.push('>');
        for g in &t.groups {
            let _ = write!(x, "<group id=\"{}\" label=\"{}\">", esc_attr(&g.id), esc_attr(&g.label));
            for b in &g.buttons {
                let _ = write!(x, "<button id=\"{}\" label=\"{}\"", esc_attr(&b.id), esc_attr(&b.label));
                if b.large {
                    x.push_str(" size=\"large\"");
                }
                for (k, v) in [("imageMso", &b.image), ("screentip", &b.screentip), ("supertip", &b.supertip), ("tag", &b.tag)] {
                    if let Some(v) = v {
                        let _ = write!(x, " {k}=\"{}\"", esc_attr(v));
                    }
                }
                let _ = write!(x, " onAction=\"{}\"/>", esc_attr(&b.on_action));
            }
            x.push_str("</group>");
        }
        x.push_str("</tab>");
    }
    x.push_str("</tabs></ribbon></customUI>");
    x
}

/// Reads the package's ribbon into [`Presentation::ribbon`] when the model holds all of it (tabs,
/// groups, macro buttons, no pictures of its own). The part names it took over are returned, so
/// they aren't also kept as package parts. The Office 2010 part wins over the Office 2007 one.
pub fn read(pkg: &Package, p: &mut Presentation) -> Vec<String> {
    let root = pkg.rels("");
    let parts: Vec<(String, &[u8])> =
        root.items.iter().filter(|r| !r.external && is_ribbon_rel(&r.ty)).filter_map(|r| pkg.get(&r.target).map(|d| (r.target.clone(), d))).collect();
    let Some((name, data)) = parts.iter().find(|(_, d)| is_v14(d)).or(parts.first()) else { return Vec::new() };
    // Pictures of its own (a relationships part) mean it isn't only tabs, groups and buttons.
    if pkg.get(&opc::rels_path(name)).is_some() {
        return Vec::new();
    }
    let Some(ribbon) = parse_exact(data) else { return Vec::new() };
    if parts.len() > 1 {
        log::info!("pptx: the ribbon's Office 2007 copy is replaced by its Office 2010 one on save");
    }
    p.ribbon = Some(ribbon);
    parts.into_iter().map(|(n, _)| n).collect()
}

/// The ribbon a presentation shows: its own (`true`: editable), or the buttons of a richer one
/// kept from the file (`false`).
pub fn current(p: &Presentation) -> (Ribbon, bool) {
    let kept: Vec<&deckcraft_model::KeptPart> = p.kept_parts.iter().filter(|k| k.from_root && is_ribbon_rel(&k.rel_type)).collect();
    if let Some(k) = kept.iter().find(|k| is_v14(k.xml.as_bytes())).or(kept.first()) {
        return (parse_buttons(k.xml.as_bytes()), false);
    }
    (p.ribbon.clone().unwrap_or_default(), true)
}

/// Does the presentation keep a ribbon of its own from the file (one DeckCraft doesn't model)?
pub fn has_kept(p: &Presentation) -> bool {
    p.kept_parts.iter().any(|k| k.from_root && is_ribbon_rel(&k.rel_type))
}
