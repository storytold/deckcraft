#!/usr/bin/env python3
"""Evaluate ECMA-376 presetShapeDefinitions.xml: for every preset, its default adjust values and
its text rectangle at a few sizes. Writes preset_table.txt, which the geometry tests compare
against. Usage: make_preset_table.py presetShapeDefinitions.xml > preset_table.txt
(The XML ships with LibreOffice's source: oox/source/drawingml/customshapes/.)"""
import math, sys, xml.etree.ElementTree as ET

A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
SIZES = [(200.0, 100.0), (100.0, 200.0), (144.0, 144.0)]

def builtins(w, h):
    ss = min(w, h); ls = max(w, h)
    v = {'w': w, 'h': h, 'l': 0.0, 't': 0.0, 'r': w, 'b': h, 'ss': ss, 'ls': ls, 'hc': w / 2, 'vc': h / 2,
         'cd2': 10800000.0, 'cd4': 5400000.0, 'cd8': 2700000.0, '3cd4': 16200000.0, '3cd8': 8100000.0,
         '5cd8': 13500000.0, '7cd8': 18900000.0}
    for n in (2, 3, 4, 5, 6, 8, 10, 12, 16, 32):
        v[f'wd{n}'] = w / n; v[f'hd{n}'] = h / n; v[f'ssd{n}'] = ss / n
    return v

def ev(f, v):
    p = f.split()
    def g(x):
        return v[x] if x in v else float(x)
    op = p[0]
    a = [g(x) for x in p[1:]]
    rad = lambda x: x / 60000.0 * math.pi / 180.0
    if op == 'val': return a[0]
    if op == '*/': return a[0] * a[1] / a[2] if a[2] else 0.0
    if op == '+-': return a[0] + a[1] - a[2]
    if op == '+/': return (a[0] + a[1]) / a[2] if a[2] else 0.0
    if op == '?:': return a[1] if a[0] > 0 else a[2]
    if op == 'abs': return abs(a[0])
    if op == 'at2': return math.degrees(math.atan2(a[1], a[0])) * 60000.0
    if op == 'cat2': return a[0] * math.cos(math.atan2(a[2], a[1]))
    if op == 'sat2': return a[0] * math.sin(math.atan2(a[2], a[1]))
    if op == 'cos': return a[0] * math.cos(rad(a[1]))
    if op == 'sin': return a[0] * math.sin(rad(a[1]))
    if op == 'tan': return a[0] * math.tan(rad(a[1]))
    if op == 'max': return max(a[0], a[1])
    if op == 'min': return min(a[0], a[1])
    if op == 'mod': return math.sqrt(a[0] ** 2 + a[1] ** 2 + a[2] ** 2)
    if op == 'pin': return a[0] if a[1] < a[0] else (a[2] if a[1] > a[2] else a[1])
    if op == 'sqrt': return math.sqrt(max(a[0], 0.0))
    raise ValueError(f)

def main():
    root = ET.parse(sys.argv[1]).getroot()
    print('# name | default adjust values | text rect l t r b at 200x100 ; 100x200 ; 144x144')
    for sh in root:
        av = sh.find(A + 'avLst'); gd = sh.find(A + 'gdLst'); rect = sh.find(A + 'rect')
        avs = [] if av is None else [(g.get('name'), g.get('fmla')) for g in av]
        defaults = [ev(f, {}) for _, f in avs]
        rects = []
        for w, h in SIZES:
            v = builtins(w, h)
            for n, f in avs:
                v[n] = ev(f, v)
            for g in ([] if gd is None else gd):
                v[g.get('name')] = ev(g.get('fmla'), v)
            if rect is None:
                rects.append('0 0 %g %g' % (w, h))
            else:
                rects.append(' '.join('%.3f' % (v[rect.get(k)] if rect.get(k) in v else float(rect.get(k))) for k in 'ltrb'))
        print('%s | %s | %s' % (sh.tag, ' '.join(repr(round(d, 3)).removesuffix('.0') for d in defaults), ' ; '.join(rects)))

main()
