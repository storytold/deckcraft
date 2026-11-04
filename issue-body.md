## Goal
Every recording feature that writes to the PPTX must round-trip with PowerPoint in both directions and produce no unexplained XML differences.

## Matrix
- Narration audio + timings
- Pen ink and laser trace
- Cameo video
- Screen-recording video
- Recorded audio clip
- Clear Recording / Reset to Cameo

## Procedure
For each feature: create a minimal deck, apply the feature in DeckCraft, save, open in PowerPoint, inspect/save, reopen in DeckCraft; then do the reverse from PowerPoint. Unzip both files and diff `[Content_Types].xml`, `ppt/presentation.xml`, slide XML, media parts, relationships and timing XML. Each diff is either fixed or documented with the public schema element that explains it.

## Acceptance
A checked-in round-trip script/test corpus covers the matrix, and the remaining XML differences are zero or documented per feature.
