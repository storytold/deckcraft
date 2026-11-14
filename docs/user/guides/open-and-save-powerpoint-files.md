---
title: Open and save PowerPoint files
description: Open a .pptx in DeckCraft, save it back as .pptx, or keep it as a DeckCraft file.
order: 20
verified: 0.7.0
---

Press Ctrl+O to open a `.pptx`. To save as PowerPoint, press Ctrl+Shift+S and give the file a name that ends in `.pptx`. On macOS, use Cmd in place of Ctrl.

## Open a file

1. Press Ctrl+O. Or open the **File** tab, choose **Open** and click **Browse**.
2. Pick a file. The dialog accepts `.deckcraft`, `.pptx`, `.potx` and `.ppsx` presentations, and `.txt` or `.md` outlines.

Files you opened lately are listed under **Recent** on the start screen and on the **Open** page of the **File** tab.

## Save

1. Press Ctrl+S, or click the save icon in the title bar. If the deck has no file yet, DeckCraft asks where to save it.
2. The suggested name ends in `.deckcraft`, DeckCraft's own format.

Saving again writes to the same file. The title bar shows the deck name with "Edited" after it while there are unsaved changes.

## Save as a PowerPoint file

1. Press Ctrl+Shift+S.
2. Type a file name that ends in `.pptx`, for example `Talk.pptx`. DeckCraft picks the format from the ending, so `.pptx`, `.potx` and `.ppsx` save as PowerPoint files and anything else saves as a DeckCraft file.

You can also write a copy without changing the open deck's file: open the command palette (Ctrl+Shift+P), run **Export…**, choose **PowerPoint Presentation (.pptx)** and click **Export**.

## What may not survive a round trip

Some settings are lost when a deck is opened and saved again as `.pptx`. Reports in the issue tracker include the animation rewind setting, audio and video playback options, and comment reply threads. Fixes for the rewind setting and comment threads are merged but are not in 0.7.0. Keep your original file until you have checked the saved copy.

## If DeckCraft closes before you save

DeckCraft keeps unsaved work with AutoRecover and reopens it the next time it starts, with a message such as "Recovered 1 unsaved presentation from the last session".

## Related

- [Export a deck to PDF](export-to-pdf.md)
- [Can DeckCraft open my PowerPoint files?](../faq/can-deckcraft-open-powerpoint-files.md)
