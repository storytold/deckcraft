"""Generate contributor-original font lookup fixtures (MIT OR Apache-2.0).

Run with fontTools installed: python make_localized_fonts.py
Only .notdef and an original rectangle mapped to A and 字 are included. Family names in
English, Chinese and French test name-table aliases, including a Windows
family that denotes a specific weight.
"""

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def build(style, weight):
    legacy = "DeckCraft Localized Semibold" if weight == 600 else "DeckCraft Localized"
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder([".notdef", "A"])
    fb.setupCharacterMap({ord("A"): "A", ord("字"): "A"})
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0))
    pen.lineTo((100, 700))
    pen.lineTo((600, 700))
    pen.lineTo((600, 0))
    pen.closePath()
    fb.setupGlyf({".notdef": TTGlyphPen(None).glyph(), "A": pen.glyph()})
    fb.setupHorizontalMetrics({".notdef": (500, 0), "A": (700, 100)})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({
        "familyName": legacy,
        "styleName": "Regular" if weight == 600 else style,
        "typographicFamily": "DeckCraft Localized",
        "typographicSubfamily": style,
        "uniqueFontIdentifier": f"DeckCraft Localized {style}",
        "fullName": f"DeckCraft Localized {style}",
        "psName": f"DeckCraftLocalized-{style}",
        "licenseDescription": "MIT OR Apache-2.0; contributor-original test font",
    })
    names = fb.font["name"]
    for language, family, semibold in [
        (0x0804, "DeckCraft 测试字体", "DeckCraft 测试字体 半粗"),
        (0x040C, "DeckCraft Police locale", "DeckCraft Police locale demi-gras"),
    ]:
        names.setName(family, 16, 3, 1, language)
        names.setName(semibold if weight == 600 else family, 1, 3, 1, language)
    fb.setupOS2(usWeightClass=weight, sTypoAscender=800, sTypoDescender=-200, usWinAscent=900, usWinDescent=100)
    fb.setupPost()
    fb.font["head"].created = fb.font["head"].modified = 3406620153
    fb.save(Path(__file__).with_name(f"localized-{style.lower()}.ttf"))


for style, weight in [("Regular", 400), ("Bold", 700), ("Semibold", 600)]:
    build(style, weight)
