//! Password-protected packages (MS-OFFCRYPTO standard and agile encryption).
//!
//! The fixtures in `tests/fixtures/` are encrypted containers of `fixtures/plain.pptx`. They were
//! produced by `fixtures/make.py` and each one was decrypted back to the exact plaintext by
//! msoffcrypto-tool — an independent implementation — before it was committed, so a match here is
//! real cross-validation and not a self-consistency check. See ATTRIBUTION.md.

use deckcraft_pptx::{PptxError, crypt};

/// The password `fixtures/make.py` encrypts every fixture with.
const PASSWORD: &str = "DeckCraft-Test-Passw0rd!";
const WRONG: &str = "not-the-password";

/// Encrypted packages: both schemes, three of the four agile hash sizes, two key sizes.
const ENCRYPTED: &[&str] =
    &["standard-aes128-sha1.pptx", "standard-aes256-sha1.pptx", "agile-aes256-sha512.pptx", "agile-aes256-sha256.pptx", "agile-aes128-sha384.pptx"];

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn encrypted_packages_are_recognised_and_ask_for_a_password() {
    for name in ENCRYPTED {
        let bytes = fixture(name);
        assert!(deckcraft_pptx::is_encrypted(&bytes), "{name}: not detected as encrypted");
        assert!(!deckcraft_pptx::sniff(&bytes), "{name}: an encrypted container is not a package");
        assert!(matches!(deckcraft_pptx::import(&bytes), Err(PptxError::PasswordRequired)), "{name}: a password should be requested");
    }
}

#[test]
fn the_wrong_password_is_reported_as_a_wrong_password() {
    for name in ENCRYPTED {
        let r = deckcraft_pptx::import_with_password(&fixture(name), Some(WRONG));
        assert!(matches!(r, Err(PptxError::WrongPassword)), "{name}: {r:?}");
    }
}

#[test]
fn the_right_password_yields_the_original_package_byte_for_byte() {
    let plain = fixture("plain.pptx");
    for name in ENCRYPTED {
        let decrypted = crypt::decrypt(&fixture(name), Some(PASSWORD)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(decrypted, plain, "{name}: decrypted bytes differ from the original package");
    }
}

#[test]
fn an_encrypted_presentation_opens_with_its_password() {
    for name in ENCRYPTED {
        let p = deckcraft_pptx::import_with_password(&fixture(name), Some(PASSWORD)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.slides.len(), 3, "{name}");
        assert_eq!(p.slides[0].title(), "Encrypted fixture", "{name}");
        assert_eq!(p.slides[2].title(), "Third slide", "{name}");
        assert!(p.validate().is_empty(), "{name}: {:?}", p.validate());
    }
}

#[test]
fn a_plain_package_opens_with_or_without_a_password() {
    let plain = fixture("plain.pptx");
    assert!(!deckcraft_pptx::is_encrypted(&plain));
    assert_eq!(deckcraft_pptx::import_with_password(&plain, Some(PASSWORD)).expect("with a password").slides.len(), 3);
    assert_eq!(deckcraft_pptx::import_with_password(&plain, None).expect("without a password").slides.len(), 3);
}

#[test]
fn truncation_and_corruption_never_panic() {
    // The requirement is that hostile input cannot take the app down, not that every shortened
    // file must fail: dropping the final sector-padding byte still leaves a readable container.
    let good = fixture("agile-aes256-sha512.pptx");
    for cut in [0, 1, 7, 8, 16, 64, 512, 4096, good.len() / 2, good.len() - 1] {
        for password in [None, Some(PASSWORD), Some(WRONG)] {
            let mut bytes = good.clone();
            bytes.truncate(cut);
            let _ = deckcraft_pptx::is_encrypted(&bytes);
            let _ = deckcraft_pptx::import_with_password(&bytes, password);
            let _ = deckcraft_pptx::sniff(&bytes);
        }
    }
    for i in [0, 1, 9, 12, 600, 900, 2000, good.len() / 2, good.len() - 1] {
        for password in [None, Some(PASSWORD), Some(WRONG)] {
            let mut bytes = good.clone();
            bytes[i] ^= 0xFF;
            let _ = deckcraft_pptx::import_with_password(&bytes, password);
        }
    }
}

#[test]
fn a_tampered_encrypted_payload_is_rejected() {
    // Agile containers carry an HMAC over the encrypted package; a flipped byte inside it must be
    // caught rather than decrypted into a corrupt presentation.
    let good = fixture("agile-aes256-sha512.pptx");
    for i in [good.len() / 2, good.len() * 3 / 4, good.len() - 1024] {
        let mut bytes = good.clone();
        bytes[i] ^= 0xFF;
        let r = deckcraft_pptx::import_with_password(&bytes, Some(PASSWORD));
        assert!(r.is_err(), "a flipped byte at {i} was accepted");
    }
}

#[test]
fn a_legacy_compound_file_is_reported_as_unsupported_not_encrypted() {
    // The OLE/CFB magic bytes of a legacy `.ppt`: a compound file with no encryption streams.
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.resize(4096, 0);
    assert!(!deckcraft_pptx::is_encrypted(&bytes));
    assert!(deckcraft_pptx::import(&bytes).is_err());
}
