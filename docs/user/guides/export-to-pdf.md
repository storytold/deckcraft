---
title: Export a deck to PDF
description: Save a deck as a PDF with full-page slides, notes pages or handouts, or export slides as PNG or JPEG images.
order: 50
verified: 0.7.0
---

Open the command palette with Ctrl+Shift+P, run **Export…**, keep **PDF document (.pdf)** selected and click **Export**. On macOS, use Cmd in place of Ctrl.

## Steps

1. Press Ctrl+Shift+P and type `Export`. Choose **Export…**.
2. In the **Export** window, leave **PDF document (.pdf)** selected.
3. Pick a **Print layout:**. The choices are **Full Page Slides**, **Notes Pages**, and handouts with 1, 2, 3, 4, 6 or 9 slides per page.
4. Tick **Include hidden slides** if you want them in the PDF.
5. Click **Export**, then choose where to save the file.

## Other formats in the same window

- **PNG images (current slide)** and **PNG images (all slides)**
- **JPEG image (current slide)**
- **PowerPoint Presentation (.pptx)**
- **Outline (.txt)**

## Copied text

The PDF includes a text layer by default. Text copied from an exported PDF can come out garbled ([issue 164](https://github.com/storytold/deckcraft/issues/164)). A fix is merged but is not in 0.7.0. Check copied text if you rely on it.

## Related

- [Open and save PowerPoint files](open-and-save-powerpoint-files.md)
