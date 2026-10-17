//! The presentation's own ribbon: tabs, groups and buttons that run the presentation's macros
//! (the Office ribbon customisation part, `customUI/customUI14.xml`). Buttons the user adds from
//! the Developer tab live here and travel with the file; PowerPoint shows them too.
//!
//! Only ribbons made of tabs, groups and macro buttons are modelled. Richer customisations
//! (pictures of their own, menus, callbacks other than a button's `onAction`) stay in the file
//! unchanged ([`crate::KeptPart`]); their buttons still show and run, read-only.

use serde::{Deserialize, Serialize};

/// Most tabs, groups per tab and buttons per group a presentation's ribbon may hold.
pub const MAX_RIBBON_ITEMS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ribbon {
    pub tabs: Vec<RibbonTab>,
}

/// A tab: one of the built-in tabs (`idMso`, `TabHome`) gaining groups, or a tab of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RibbonTab {
    /// The control id of a custom tab (`id`); empty for a built-in tab.
    pub id: String,
    /// The built-in tab (`idMso`: `TabHome`, `TabDeveloper`), when this is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    pub label: String,
    pub groups: Vec<RibbonGroup>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RibbonGroup {
    pub id: String,
    pub label: String,
    pub buttons: Vec<RibbonButton>,
}

/// A button that runs a macro: `on_action` is the macro's name (`Module1.Hello`), which is called
/// with the button (`Sub Hello(control As IRibbonControl)`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RibbonButton {
    pub id: String,
    pub label: String,
    pub on_action: String,
    /// A built-in picture by name (`imageMso`): DeckCraft draws its own icon for the names it
    /// knows; the name is kept for PowerPoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub large: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screentip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supertip: Option<String>,
    /// Free text handed to the macro (`control.Tag`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl Ribbon {
    pub fn is_empty(&self) -> bool {
        self.tabs.iter().all(|t| t.groups.iter().all(|g| g.buttons.is_empty()))
    }

    /// Every control id in use (tabs, groups, buttons).
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.tabs.iter().flat_map(|t| {
            std::iter::once(t.id.as_str())
                .chain(t.groups.iter().flat_map(|g| std::iter::once(g.id.as_str()).chain(g.buttons.iter().map(|b| b.id.as_str()))))
        })
    }

    /// A control id not in use yet: `prefix1`, `prefix2`… (the ribbon is bounded, so one is
    /// always found).
    pub fn fresh_id(&self, prefix: &str) -> String {
        let mut n = 1usize;
        loop {
            let id = format!("{prefix}{n}");
            if !self.ids().any(|x| x == id) {
                return id;
            }
            n = n.saturating_add(1);
            if n == usize::MAX {
                return id;
            }
        }
    }

    pub fn button(&self, id: &str) -> Option<&RibbonButton> {
        self.tabs.iter().flat_map(|t| &t.groups).flat_map(|g| &g.buttons).find(|b| b.id == id)
    }

    pub fn button_mut(&mut self, id: &str) -> Option<&mut RibbonButton> {
        self.tabs.iter_mut().flat_map(|t| &mut t.groups).flat_map(|g| &mut g.buttons).find(|b| b.id == id)
    }

    /// Removes the button `id`, and its group and tab when they are left empty. Whether it was there.
    pub fn remove_button(&mut self, id: &str) -> bool {
        let mut found = false;
        for t in &mut self.tabs {
            for g in &mut t.groups {
                let before = g.buttons.len();
                g.buttons.retain(|b| b.id != id);
                found |= g.buttons.len() != before;
            }
            t.groups.retain(|g| !g.buttons.is_empty());
        }
        self.tabs.retain(|t| !t.groups.is_empty());
        found
    }
}

/// The ribbon tab names DeckCraft shows for the built-in tab ids (`TabHome` → Home).
pub const BUILTIN_TABS: &[(&str, &str)] = &[
    ("TabHome", "Home"),
    ("TabInsert", "Insert"),
    ("TabDesign", "Design"),
    ("TabTransitions", "Transitions"),
    ("TabAnimations", "Animations"),
    ("TabSlideShow", "Slide Show"),
    ("TabReview", "Review"),
    ("TabView", "View"),
    ("TabDeveloper", "Developer"),
];

/// The tab DeckCraft shows a built-in tab id as (`TabReview` → Review).
pub fn builtin_tab_name(id_mso: &str) -> Option<&'static str> {
    BUILTIN_TABS.iter().find(|(k, _)| k.eq_ignore_ascii_case(id_mso)).map(|(_, v)| *v)
}

/// The built-in tab id of a DeckCraft tab name (Review → `TabReview`).
pub fn builtin_tab_id(name: &str) -> Option<&'static str> {
    BUILTIN_TABS.iter().find(|(_, v)| v.eq_ignore_ascii_case(name)).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: &str) -> RibbonButton {
        RibbonButton { id: id.into(), label: id.into(), on_action: "Go".into(), ..Default::default() }
    }

    #[test]
    fn ids_buttons_and_removal() {
        let mut r = Ribbon {
            tabs: vec![RibbonTab {
                id: "t1".into(),
                label: "Mine".into(),
                groups: vec![RibbonGroup { id: "g1".into(), label: "G".into(), buttons: vec![b("b1"), b("b2")] }],
                ..Default::default()
            }],
        };
        assert_eq!(r.fresh_id("b"), "b3");
        assert!(r.button("b2").is_some());
        assert!(r.remove_button("b1"));
        assert!(!r.remove_button("b1"));
        assert!(!r.is_empty());
        assert!(r.remove_button("b2"));
        assert!(r.tabs.is_empty() && r.is_empty(), "empty groups and tabs go too");
        assert_eq!(builtin_tab_id("slide show"), Some("TabSlideShow"));
        assert_eq!(builtin_tab_name("TABHOME"), Some("Home"));
        assert_eq!(builtin_tab_id("Macros"), None);
    }
}
