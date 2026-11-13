#!/usr/bin/env python3
"""Move choice localization outside widget_info to avoid re-locking egui::Context."""
from pathlib import Path
from datetime import datetime
import re
import subprocess
import sys


def repaired(text):
    start = text.find("fn choice(")
    if start < 0:
        raise RuntimeError("choice function not found; nothing changed.")
    following = re.search(r"\n(?:pub(?:\([^)]*\))?\s+)?fn\s+", text[start + 3:])
    end = start + 3 + following.start() if following else len(text)
    segment = text[start:end]
    if "let choice_accessible_name =" in segment:
        return text
    call = re.compile(r"crate::i18n::context_text\(\s*ui\.ctx\(\),\s*name\s*\)")
    lines = segment.splitlines(keepends=True)
    matches = [i for i, line in enumerate(lines) if ".widget_info(" in line and call.search(line)]
    if len(matches) != 1:
        raise RuntimeError("Expected one single-line choice widget_info localization call; nothing changed.")
    index = matches[0]
    line = lines[index]
    if "||" not in line or "WidgetInfo::labeled" not in line:
        raise RuntimeError("Unexpected callback shape; nothing changed.")
    indent = line[:len(line) - len(line.lstrip())]
    declaration = indent + "let choice_accessible_name = crate::i18n::context_text(ui.ctx(), name);\n"
    lines[index] = declaration + call.sub("&choice_accessible_name", line)
    result = text[:start] + "".join(lines) + text[end:]
    return result


def main():
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    branch = subprocess.check_output(["git", "branch", "--show-current"], cwd=root, text=True).strip()
    if branch != "i18n/russian":
        raise RuntimeError("Switch to i18n/russian first.")
    path = root / "crates/ui-egui/src/dialogs.rs"
    before = path.read_text(encoding="utf-8")
    after = repaired(before)
    if after == before:
        print("This choice repair is already applied.")
        return
    print("Prepared repair for crates/ui-egui/src/dialogs.rs only.")
    print("Translation is evaluated before widget_info; the callback borrows the ready string.")
    if "--apply" not in sys.argv:
        print("Preview only. Add --apply to write the repair.")
        return
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    backup = path.with_name(path.name + ".before-choice-lock-fix-" + stamp + ".bak")
    backup.write_text(before, encoding="utf-8")
    path.write_text(after, encoding="utf-8")
    print("Applied; backup: " + backup.name)
    print("No commit or push performed. Rust tests have NOT been run.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
