"""Build the fallback-style test fonts (contributor-original, CC0-1.0).

Usage: python make_han_font.py <dir>  (needs fontTools)

han-regular.ttf, han-bold.ttf, han-italic.ttf: family "DeckCraft Han" in Regular (weight 400),
Bold (700) and Italic (400, italic). han-vf.ttf: family "DeckCraft Han VF", one Regular face with
a `wght` axis (100-900, default 400) and no named instances. han-plain.ttf: family
"DeckCraft Han Plain", Regular only. Glyphs: .notdef and squares for U+31350 and U+31351 (CJK
Extension H, which installed fonts rarely draw), plus U+31352 in DeckCraft Han Plain.
"""
import os
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable


def square(stem):
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0))
    pen.lineTo((100, 700))
    pen.lineTo((100 + stem, 700))
    pen.lineTo((100 + stem, 0))
    pen.closePath()
    return pen.glyph()


def build(path, family, style, weight, italic=False, chars=(0x31350, 0x31351), wght_axis=False):
    names = [f"u{c:05X}" for c in chars]
    stem = 200 if weight >= 600 else 100
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder([".notdef"] + names)
    fb.setupCharacterMap({c: n for c, n in zip(chars, names)})
    fb.setupGlyf({".notdef": TTGlyphPen(None).glyph(), **{n: square(stem) for n in names}})
    fb.setupHorizontalMetrics({".notdef": (500, 0), **{n: (1000, 100) for n in names}})
    fb.setupHorizontalHeader(ascent=880, descent=-120)
    fb.setupNameTable({
        "familyName": family,
        "styleName": style,
        "uniqueFontIdentifier": f"{family} {style}",
        "fullName": f"{family} {style}",
        "psName": f"{family.replace(' ', '')}-{style}",
    })
    fs_selection = 0x01 if italic else (0x20 if weight >= 700 else 0x40)
    fb.setupOS2(usWeightClass=weight, fsSelection=fs_selection, sTypoAscender=880, sTypoDescender=-120, usWinAscent=880, usWinDescent=120)
    fb.setupPost(italicAngle=-12 if italic else 0)
    fb.font["head"].macStyle = (0x01 if weight >= 700 else 0) | (0x02 if italic else 0)
    if wght_axis:
        fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
        gvar = newTable("gvar")
        gvar.version, gvar.reserved, gvar.variations = 1, 0, {}
        fb.font["gvar"] = gvar
    fb.save(path)


out = sys.argv[1]
build(os.path.join(out, "han-regular.ttf"), "DeckCraft Han", "Regular", 400)
build(os.path.join(out, "han-bold.ttf"), "DeckCraft Han", "Bold", 700)
build(os.path.join(out, "han-italic.ttf"), "DeckCraft Han", "Italic", 400, italic=True)
build(os.path.join(out, "han-vf.ttf"), "DeckCraft Han VF", "Regular", 400, wght_axis=True)
build(os.path.join(out, "han-plain.ttf"), "DeckCraft Han Plain", "Regular", 400, chars=(0x31350, 0x31351, 0x31352))
