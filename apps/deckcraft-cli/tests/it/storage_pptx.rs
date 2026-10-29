//! A real presentation through the public trait only: export a [`Presentation`] to PPTX bytes,
//! store them atomically in a local backend, read them back, and re-import — the deck that
//! comes back has the same slides. Storage never parses; this test proves the bytes it
//! carries are a file PowerPoint would open (see `sniff`).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use deckcraft_storage::local::LocalBackend;
use deckcraft_storage::{StorageBackend, WriteOptions};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn create() -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("deckcraft-storage-pptx-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn real_presentation_survives_an_atomic_save_and_reopen() {
    let deck = deckcraft_model::Presentation::default();
    let bytes = deckcraft_pptx::export(&deck).unwrap();
    assert!(deckcraft_pptx::sniff(&bytes), "exported bytes must sniff as a PPTX package");

    let scratch = Scratch::create();
    let store = LocalBackend::open(&scratch.path).unwrap();
    let meta = store.write("decks/quarterly.pptx", &bytes, &WriteOptions { atomic: true, ..Default::default() }).unwrap();
    assert_eq!(meta.size, bytes.len() as u64);

    let back = store.read("decks/quarterly.pptx").unwrap();
    assert_eq!(back, bytes);
    assert!(deckcraft_pptx::sniff(&back));

    let reopened = deckcraft_pptx::import(&back).unwrap();
    assert_eq!(reopened.slides.len(), deck.slides.len());
}
