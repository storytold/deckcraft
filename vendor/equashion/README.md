# equaSHion source subset for DeckCraft

Source: [shuff57/equaSHion](https://github.com/shuff57/equaSHion), revision
`386b16cff9fddb69db2f1759aea384e8887e3970` (MIT OR Apache-2.0).
The math, font, layout, rendering and egui library crates are included. The web application,
font binaries, screenshots, corpus, corpus-dependent integration tests and golden images are excluded. Original copyright and
licence notices are retained. PROVENANCE.md and DIVERGENCE.md describe the upstream extraction;
they are historical records, not a list of files shipped in this subset.

DeckCraft-specific changes:

- Fonts come from `deckcraft-fonts`, with STIX Two Math preferred and Ubuntu as a fallback.
  The slide renderer uses the equation's selected family and imports selected installed or
  session-added faces from the host font database. No Adobe fonts are embedded.
  `scripts/equation-fonts.py` prepares STIX Two Math and Latin Modern Math in the external
  craft-fonts checkout; it verifies the pinned fonts and licence files by SHA-256.
- The math caret and transient brace-input state can be saved with DeckCraft's selection and
  undo snapshots. The session owns edits and history; the egui widget is used for gallery
  previews only.
- Layout exposes immutable sequence geometry so slide carets, selection and hit testing use
  the same fractions, scripts and matrices as the renderer.
- Empty runs with font or style metadata remain empty equation fields after a PPTX round-trip.
- Compiled-in gallery data and the last-resort font have non-panicking fallback paths.
- Minor equivalent-expression updates keep the source clean under Rust 1.95 Clippy.

The root Cargo.lock controls application builds. To run this dependency's own tests:

```sh
cargo test --manifest-path vendor/equashion/Cargo.toml --workspace
```

Updates should compare against the recorded upstream revision and preserve these adaptations.
