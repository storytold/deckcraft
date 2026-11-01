# Asset attribution

Every non-code asset in this repository is listed here with its author, source and licence.
The rows are copied from WordCraft's `ATTRIBUTION.md` at `dbfe1b1` for the files that were
carried over; the font files are byte-identical to WordCraft's `assets/fonts/`.

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `assets/fonts/SourceSerif4-Regular.ttf` | Frank Grießhammer, Adobe (released as open source) | https://github.com/adobe-fonts/source-serif | OFL-1.1 (`assets/fonts/OFL-SourceSerif4.txt`) | Default equation family (`equashion_font::DEFAULT_FAMILY`) |
| `assets/fonts/SourceSerif4-It.ttf` | Frank Grießhammer, Adobe (released as open source) | https://github.com/adobe-fonts/source-serif | OFL-1.1 (`assets/fonts/OFL-SourceSerif4.txt`) | Math italic |
| `assets/fonts/SourceSerif4-Bold.ttf` | Frank Grießhammer, Adobe (released as open source) | https://github.com/adobe-fonts/source-serif | OFL-1.1 (`assets/fonts/OFL-SourceSerif4.txt`) | Bold math |
| `assets/fonts/SourceSerif4-BoldIt.ttf` | Frank Grießhammer, Adobe (released as open source) | https://github.com/adobe-fonts/source-serif | OFL-1.1 (`assets/fonts/OFL-SourceSerif4.txt`) | Bold italic math |
| `assets/fonts/Inter-Regular.ttf` | Rasmus Andersson / The Inter Project Authors | https://github.com/rsms/inter | OFL-1.1 (`assets/fonts/OFL-Inter.txt`) | Second bundled family; the ported WordCraft layout unit tests use it |
| `assets/fonts/STIXTwoMath-Regular.otf` | The STIX Fonts Project Authors | https://github.com/stipub/stixfonts (copied unmodified from Fedora's `stix-fonts-2.13b171` package, `/usr/share/fonts/stix-fonts/`) | OFL-1.1 (`assets/fonts/OFL-STIXTwoMath.txt`) | Math symbol fallback (∇ ∮ ⌈⌉⌊⌋ ⟨⟩ ...); not subset (838 KB) |
| `assets/fonts/Geist-Regular.ttf` | The Geist Project Authors (Vercel) | https://github.com/vercel/geist-font, release v1.7.2 (`geist-font-v1.7.2.zip`, `Geist/ttf/`), unmodified | OFL-1.1 (`assets/fonts/OFL-Geist.txt`) | Web showcase interface text only (`apps/equashion-web`), 126 KB |
| `assets/fonts/Geist-SemiBold.ttf` | The Geist Project Authors (Vercel) | as above | OFL-1.1 (`assets/fonts/OFL-Geist.txt`) | Web showcase headings, 128 KB |
| `assets/fonts/GeistMono-Regular.ttf` | The Geist Project Authors (Vercel) | as above, `GeistMono/ttf/` | OFL-1.1 (`assets/fonts/OFL-Geist.txt`) | Web showcase code and readouts, 149 KB |
| `assets/fonts/OFL-SourceSerif4.txt` | (licence text) | upstream project | — | |
| `assets/fonts/OFL-Inter.txt` | (licence text) | upstream project | — | |
| `assets/fonts/OFL-Geist.txt` | (licence text) | the `OFL.txt` of the same release | — | |
| `assets/fonts/OFL-STIXTwoMath.txt` | (licence text) | Fedora `stix-fonts` package (`/usr/share/licenses/stix-fonts/OFL.txt`) | — | |

WordCraft's table credits the Source Serif 4 files jointly with Source Sans 3 ("Paul D. Hunt /
Frank Grießhammer"); Source Sans 3 is not bundled here, so only the Source Serif designer is
named. The OFL text in `OFL-SourceSerif4.txt` carries Adobe's copyright notice.

The OMML samples in `corpus/omml/` are copied from the WordCraft and DeckCraft test suites
(MIT OR Apache-2.0, like the code); PROVENANCE.md lists the file and line each came from. The
typed equations in `corpus/equations.tsv` and the generated files in `tests/golden/` are
original to this repository.
