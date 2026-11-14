---
title: Can DeckCraft open my PowerPoint files?
description: DeckCraft opens and saves .pptx, .potx and .ppsx files, but some details do not import or export correctly yet.
order: 20
verified: 0.7.0
---

Yes. DeckCraft opens and saves PowerPoint `.pptx` files, and its Open dialog also accepts `.potx` and `.ppsx`. Steps are in [Open and save PowerPoint files](../guides/open-and-save-powerpoint-files.md).

The match is not perfect yet. Reports in the issue tracker say that equations and some animation properties from existing PowerPoint files do not load correctly, and that saving back to `.pptx` can drop some settings. Check an important deck after you open it, and keep your original file.

Sources: the README feature list, the file filters in the app's code, and issues 70, 74, 77, 147 and 148.
