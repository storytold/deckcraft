#!/usr/bin/env python3
"""Partial Russian UI migration. No commit, push, downloads, or deletion."""
from pathlib import Path
import json
import re
import subprocess
import sys

CATALOG = Path(__file__).with_name("ru_ui.tsv").read_text(encoding="utf-8")
MARKER = "// RU_BULK_V1"

def build(root):
    tr = dict(line.split("|", 1) for line in CATALOG.splitlines())
    base = root / "crates/ui-egui/src"
    changes = {}
    def read(name):
        path = base / name
        return changes[path] if path in changes else path.read_text(encoding="utf-8")
    def put(name, text):
        changes[base / name] = text
    def replace(name, old, new):
        text = read(name)
        if old not in text:
            raise RuntimeError("Missing source anchor: " + name + " / " + old[:80])
        put(name, text.replace(old, new, 1))
    if MARKER in read("i18n.rs"):
        raise RuntimeError("Migration already applied.")
    fallback = r'''_ => { let normalized = english.replace('\n', " "); match bulk_lookup(&normalized) { Some(value) => value, None => english } },'''
    replace("i18n.rs", "_ => english,", fallback)
    catalog = "\n" + MARKER + "\nfn bulk_lookup(english: &str) -> Option<&'static str> {\n    match english {\n"
    for english, russian in sorted(tr.items()):
        catalog += "        " + json.dumps(english, ensure_ascii=False) + " => Some(" + json.dumps(russian, ensure_ascii=False) + "),\n"
    catalog += "        _ => None,\n    }\n}\n"
    catalog += r'''
pub fn set_context_language(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("deckcraft.bulk-language"), language));
}
pub fn context_language(ctx: &egui::Context) -> Language {
    ctx.data(|d| d.get_temp::<Language>(egui::Id::new("deckcraft.bulk-language"))).unwrap_or_default()
}
pub fn text<'a>(ctx: &egui::Context, english: &'a str) -> &'a str {
    context_language(ctx).text(english)
}
#[cfg(test)]
mod bulk_tests {
    use super::*;
    #[test]
    fn language_contexts_are_independent() {
        let a = egui::Context::default();
        let b = egui::Context::default();
        set_context_language(&a, Language::Russian);
        assert_eq!(text(&a, "Home"), "Главная");
        assert_eq!(text(&b, "Home"), "Home");
        set_context_language(&a, Language::English);
        assert_eq!(text(&a, "Home"), "Home");
        assert_eq!(Language::Russian.text("slide.new"), "slide.new");
        assert_eq!(Language::Russian.text("New\nSlide"), "Новый слайд");
    }
}
'''
    put("i18n.rs", read("i18n.rs") + catalog)
    replace("app/frame.rs", "pub(super) fn ui_setup(&mut self, ctx: &egui::Context) -> bool {", "pub(super) fn ui_setup(&mut self, ctx: &egui::Context) -> bool {\n        crate::i18n::set_context_language(ctx, self.ui.interface_language);")
    def shadow(name, function, parameters):
        text = read(name)
        matches = list(re.finditer(r"\bfn\s+" + re.escape(function) + r"\b[^;{]*\{", text))
        if len(matches) != 1:
            raise RuntimeError("Ambiguous function: " + name + " / " + function)
        at = matches[0].end()
        addition = "".join("\n    let " + p + " = crate::i18n::text(ui.ctx(), " + p + ");" for p in parameters)
        put(name, text[:at] + addition + text[at:])
    for name, params in {"big_button_ex": ["label"], "small_button_named": ["label", "name"], "check_item": ["label"], "icon_toggle": ["tip"], "color_split": ["tip"], "spinner": ["label"], "section": ["title"], "drop_button": ["label"]}.items():
        shadow("widgets.rs", name, params)
    for name, params in {"buttons": ["ok_label"], "field": ["label"], "check": ["label"], "text_row": ["label"], "labeled_text": ["label"]}.items():
        shadow("dialogs.rs", name, params)
    replace("dialogs.rs", "egui::Window::new(title(&d.id))", "egui::Window::new(app.ui.interface_language.text(title(&d.id)))")
    replace("ribbon.rs", "let w = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), font.clone(), t.text).size().x)", "let shown_name = crate::i18n::text(ui.ctx(), name);\n            let w = ui.fonts_mut(|f| f.layout_no_wrap(shown_name.to_string(), font.clone(), t.text).size().x)")
    replace("ribbon.rs", "egui::WidgetInfo::labeled(egui::Role::Button, true, name)", "egui::WidgetInfo::labeled(egui::Role::Button, true, shown_name)")
    replace("ribbon.rs", "text(r.center(), Align2::CENTER_CENTER, name, f, col)", "text(r.center(), Align2::CENTER_CENTER, shown_name, f, col)")
    direct = re.compile(r'(\bui\.(?:label|button|small|heading|menu_button)\(\s*)("(?:\\.|[^"\\])*")')
    for path in sorted(base.rglob("*.rs")):
        if "test" in path.stem or path.name in {"i18n.rs", "status.rs"}:
            continue
        text = changes[path] if path in changes else path.read_text(encoding="utf-8")
        test_module = re.search(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{", text)
        stop = test_module.start() if test_module else len(text)
        lines = text[:stop].splitlines(keepends=True)
        for i, line in enumerate(lines):
            if line.lstrip().startswith(("//", "/*", "*")):
                continue
            def display(match):
                try:
                    value = json.loads(match.group(2))
                except ValueError:
                    return match.group()
                if value not in tr:
                    return match.group()
                return match.group(1) + "crate::i18n::text(ui.ctx(), " + match.group(2) + ")"
            lines[i] = direct.sub(display, line)
        changed = "".join(lines) + text[stop:]
        if changed != text:
            changes[path] = changed
    return {p: t for p, t in changes.items() if t != p.read_text(encoding="utf-8")}

def main():
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    branch = subprocess.check_output(["git", "branch", "--show-current"], cwd=root, text=True).strip()
    if branch != "i18n/russian":
        raise RuntimeError("Switch to i18n/russian first.")
    for args in (["git", "diff", "--quiet"], ["git", "diff", "--cached", "--quiet"]):
        if subprocess.run(args, cwd=root).returncode != 0:
            raise RuntimeError("Tracked local changes exist; nothing was changed.")
    changes = build(root)
    for path in changes:
        print(path.relative_to(root))
    print("Prepared", len(changes), "files. Partial UI translation, not full localization.")
    if "--apply" not in sys.argv:
        print("Preview only. Run again with --apply to write these files.")
        return
    for path, text in changes.items():
        path.write_text(text, encoding="utf-8")
    print("Applied locally. No commit or push. Run cargo fmt and tests, then review git diff.")

if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
