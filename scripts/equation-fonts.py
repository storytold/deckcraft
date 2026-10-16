#!/usr/bin/env python3
"""Install pinned STIX Two Math and Latin Modern Math into an external craft-fonts checkout.

Run once before building; Cargo itself never downloads fonts. Existing unrelated manifest
entries and fonts are preserved. No font binaries are stored in the DeckCraft repository.
"""
import argparse
import hashlib
from io import BytesIO
from pathlib import Path
from urllib.request import urlopen
from zipfile import ZipFile

REV = "744a22a4dd626cd14d75728aef34fc8ad7c85db0"
SOURCE = f"https://raw.githubusercontent.com/stipub/stixfonts/{REV}/"
FILES = {
    "STIXTwoMath-Regular.otf": ("fonts/static_otf/STIXTwoMath-Regular.otf", "3a5f3f26f40d5698b3c62dd085d48d6663696a3f80825aab8b553d5097518e8c"),
    "OFL.txt": ("OFL.txt", "0c8825913b60d858aacdb33c4ca6660a7d64b0d6464702efbb19313f5765861a"),
}
LM_SOURCE = "https://www.gust.org.pl/projects/e-foundry/lm-math/download/latinmodern-math-1959.zip"
LM_FILES = {
    "latinmodern-math.otf": ("otf/latinmodern-math.otf", "6075562b771f8b82f0c179e363389684f2dd09de30038269e2628e504bd7be0f"),
    "GUST-FONT-LICENSE.txt": ("doc/GUST-FONT-LICENSE.txt", "2bd69affc3da00715116f713f57eab9707e96daf3562ad0215987b15b9c16f73"),
    "README-Latin-Modern-Math.txt": ("doc/README-Latin-Modern-Math.txt", "e6fdd5fb44b656146fa988d191f58901c09a347d3f4fbaa3b45b24a6ad7ec30b"),
    "MANIFEST-Latin-Modern-Math.txt": ("doc/MANIFEST-Latin-Modern-Math.txt", "1daa729f7922c42ba5ce8da7dfe4ecdc9e5c878ad86217fd6f32c4671505b007"),
}


def latin_modern_files(target):
    verified = {}
    archive = None
    for name, (member, sha) in LM_FILES.items():
        path = target / name
        data = path.read_bytes() if path.exists() else b""
        if hashlib.sha256(data).hexdigest() != sha:
            if archive is None:
                with urlopen(LM_SOURCE, timeout=30) as response:
                    archive = response.read(4 * 1024 * 1024 + 1)
                if len(archive) > 4 * 1024 * 1024:
                    raise SystemExit("Latin Modern archive is too large; no files were installed")
            with ZipFile(BytesIO(archive)) as bundle:
                info = bundle.getinfo("latinmodern-math-1959/" + member)
                if info.file_size > 2 * 1024 * 1024:
                    raise SystemExit(f"Archive member too large: {name}; no files were installed")
                data = bundle.read(info)
        if hashlib.sha256(data).hexdigest() != sha:
            raise SystemExit(f"Checksum mismatch for {name}; no files were installed")
        verified[name] = data
    return verified


def install(root):
    if not (root / "fonts/manifest.txt").is_file():
        raise SystemExit(f"Not a craft-fonts checkout: {root}")
    target = root / "fonts/stix-two-math"
    verified = {}
    for name, (remote, sha) in FILES.items():
        path = target / name
        data = path.read_bytes() if path.exists() else b""
        if hashlib.sha256(data).hexdigest() != sha:
            with urlopen(SOURCE + remote, timeout=30) as response:
                data = response.read(2 * 1024 * 1024)
        if hashlib.sha256(data).hexdigest() != sha:
            raise SystemExit(f"Checksum mismatch for {name}; no files were installed")
        verified[name] = data
    lm_target = root / "fonts/latin-modern-math"
    lm_verified = latin_modern_files(lm_target)
    for directory, files in [(target, verified), (lm_target, lm_verified)]:
        directory.mkdir(parents=True, exist_ok=True)
        for name, data in files.items():
            (directory / name).write_bytes(data)
    manifest = root / "fonts/latin-manifest.txt"
    previous = manifest.read_text() if manifest.exists() else ""
    lines = [line for line in previous.splitlines() if not line.startswith(("STIX Two Math | ", "Latin Modern Math | "))]
    lines.append(" | ".join([
        "STIX Two Math", "Regular", "fonts/stix-two-math/STIXTwoMath-Regular.otf", "Latn,Grek,Math", "OFL-1.1",
        "fonts/stix-two-math/OFL.txt", FILES["STIXTwoMath-Regular.otf"][1],
        f"https://github.com/stipub/stixfonts/tree/{REV}",
    ]))
    lines.append(" | ".join([
        "Latin Modern Math", "Regular", "fonts/latin-modern-math/latinmodern-math.otf", "Latn,Grek,Math", "LicenseRef-GUST-Font-License",
        "fonts/latin-modern-math/GUST-FONT-LICENSE.txt", LM_FILES["latinmodern-math.otf"][1],
        LM_SOURCE + "#latinmodern-math-1959/otf/latinmodern-math.otf",
    ]))
    manifest.write_text("\n".join(lines) + "\n")
    print(f"STIX Two Math v2.13b171 and Latin Modern Math v1.959 installed in {root / 'fonts'}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, nargs="?", default=Path(__file__).resolve().parents[2] / "craft-fonts")
    install(parser.parse_args().directory.resolve())
