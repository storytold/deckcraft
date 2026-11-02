#!/usr/bin/env python3
"""Repair three known local UI test issues. Does not commit, push, reset, or delete."""
from pathlib import Path
from datetime import datetime
import hashlib
import subprocess
import sys

EXPECTED = {'crates/ui-egui/src/i18n.rs': '0f2accef111f47d5a39b5d9e544a4b6b913338f237a1ee1b744ed5a42e60880e', 'crates/ui-egui/src/ribbon.rs': '0ec0f8564b85375551ff5dafa59932fdcd5c958f301a8f30f70fac19d0953c19', 'crates/ui-egui/src/status.rs': '69974234c150b791ac1fafc360679ba92a3226b320db6ef8fa7d2a4d60d638ea'}


def prepare(root):
    before = {}
    for name, digest in EXPECTED.items():
        text = (root / name).read_text(encoding="utf-8")
        if hashlib.sha256(text.encode("utf-8")).hexdigest() != digest:
            raise RuntimeError("Source differs from the supplied snapshot; nothing changed: " + name)
        before[name] = text
    after = dict(before)
    name = "crates/ui-egui/src/i18n.rs"
    duplicate = '    ("Properties", "Свойства"),\n'
    if after[name].count(duplicate) != 2:
        raise RuntimeError("Expected exactly two identical Properties entries.")
    first = after[name].index(duplicate)
    second = after[name].index(duplicate, first + len(duplicate))
    after[name] = after[name][:second] + after[name][second + len(duplicate):]
    old = "egui::Id::new(LANGUAGE_CONTEXT_ID)"
    if after[name].count(old) != 2:
        raise RuntimeError("Expected two context ID calls.")
    after[name] = after[name].replace(old, "egui::Id::unique(LANGUAGE_CONTEXT_ID)")
    name = "crates/ui-egui/src/ribbon.rs"
    old = '.replace("Alt+", "⌥")'
    if after[name].count(old) != 1:
        raise RuntimeError("Expected one shortcut conversion.")
    after[name] = after[name].replace(old, "", 1)
    name = "crates/ui-egui/src/status.rs"
    old = '        let _ = h.get_by_role_and_label(Role::Button, "Комментарии");'
    new = '        let status_y = h.get_by_role_and_label(Role::Button, "Вписать слайд в окно").rect().center().y;\n        let comments_in_status = h\n            .get_all_by_role_and_label(Role::Button, "Комментарии")\n            .filter(|node| node.rect().y_range().contains(status_y))\n            .count();\n        assert_eq!(comments_in_status, 1, "one Comments button in the status row");'
    if after[name].count(old) != 1:
        raise RuntimeError("Expected one ambiguous status query.")
    after[name] = after[name].replace(old, new, 1)
    return before, after


def main():
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    before, after = prepare(root)
    for name in after:
        print(name)
    if "--apply" not in sys.argv:
        print("Preview only. Add --apply to save these repairs and backups.")
        return
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    for name, text in before.items():
        path = root / name
        path.with_name(path.name + ".before-ru-test-fix-" + stamp + ".bak").write_text(text, encoding="utf-8")
    for name, text in after.items():
        (root / name).write_text(text, encoding="utf-8")
    print("Applied. Rust tests have NOT been run by this script.")
    print("Four font/paragraph dialog lock failures still need investigation.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
