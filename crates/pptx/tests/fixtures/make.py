#!/usr/bin/env python3
"""Generate encrypted-PPTX test fixtures for crates/pptx/tests/crypt.rs.

Provenance
----------
The plaintext is a small OOXML package written here from the ECMA-376 part names (our
own content, no Microsoft material).  The encrypted containers are produced with
msoffcrypto-tool (MIT), an independent implementation of MS-OFFCRYPTO, so agreement with
DeckCraft's own decryptor is real cross-validation rather than a self-consistency check.

Each fixture is verified twice before it is written:
  1. msoffcrypto-tool must decrypt it back to the exact plaintext (proves the fixture is a
     valid ECMA-376 encrypted container).
  2. only then is it written out for the Rust test to consume.

Run:  python3 make_fixtures.py OUTDIR PLAINTEXT.pptx
"""

import base64
import hashlib
import io
import os
import struct
import sys
import zipfile
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import OOXMLFile
from msoffcrypto.method.container.ecma376_encrypted import ECMA376Encrypted
from msoffcrypto.method.ecma376_agile import ECMA376Agile, ECMA376AgileCipherParams
from msoffcrypto.method.ecma376_standard import ECMA376Standard

PASSWORD = "DeckCraft-Test-Passw0rd!"


def build_plaintext(path):
    """A three-slide deck, ~9 KB so the agile path decrypts more than one 4096-byte segment."""
    ct = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>'
        '<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
        '<Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
        '<Override PartName="/ppt/slides/slide3.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
        "</Types>"
    )
    rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>'
        "</Relationships>"
    )
    # Filler text keeps the deflated package above 8192 bytes (more than one agile segment).
    # It must be incompressible, so it is base64 of a deterministic pseudo-random stream.
    state = 0x2545F491
    raw = bytearray()
    while len(raw) < 4600:
        state = (state * 1103515245 + 12345) & 0xFFFFFFFF
        raw.append(state >> 16 & 0xFF)
    filler = base64.b64encode(bytes(raw)).decode()
    pres = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
        ' xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/><p:sldId id="258" r:id="rId4"/></p:sldIdLst>'
        '<p:sldSz cx="12192000" cy="6858000"/></p:presentation>'
    )
    prels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>'
        '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>'
        '<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide3.xml"/>'
        "</Relationships>"
    )

    def slide(title, body):
        return (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
            ' xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
            "<p:cSld><p:spTree>"
            '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'
            f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title {title}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>'
            f'<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>'
            f'<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body {title}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>'
            f'<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{body}</a:t></a:r></a:p></p:txBody></p:sp>'
            "</p:spTree></p:cSld></p:sld>"
        )

    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", rels)
        z.writestr("ppt/presentation.xml", pres)
        z.writestr("ppt/_rels/presentation.xml.rels", prels)
        z.writestr("ppt/slides/slide1.xml", slide("Encrypted fixture", filler))
        z.writestr("ppt/slides/slide2.xml", slide("Second slide", "cross-validated"))
        z.writestr("ppt/slides/slide3.xml", slide("Third slide", "agile segment boundary"))
    return open(path, "rb").read()


# ---------------------------------------------------------------------------------------------
# Standard Encryption (MS-OFFCRYPTO 2.3.4.5-2.3.4.7), AES-128-ECB + SHA-1 — what PowerPoint
# writes for "Password to open" in the 2007-compatible mode, and what the real decks use.

ALG_ID_AES128 = 0x660E
ALG_ID_AES256 = 0x6610
ALG_ID_HASH_SHA1 = 0x8004
PROVIDER_TYPE_AES = 0x18
FLAGS = 0x24  # fCryptoAPI | fAES


def standard_encrypt(plain, password, alg_id=ALG_ID_AES128, key_size=128):
    salt = bytes.fromhex("e88266490c5bd1eebd2b4394e3f830ef")  # fixed, so fixtures are reproducible
    key = ECMA376Standard.makekey_from_password(password, alg_id, ALG_ID_HASH_SHA1, PROVIDER_TYPE_AES, key_size, len(salt), salt)

    aes = Cipher(algorithms.AES(key), modes.ECB()).encryptor()

    verifier = bytes.fromhex("00112233445566778899aabbccddeeff")  # fixed for reproducibility
    hamac = hashlib.sha1(verifier).digest()
    encrypted_verifier = aes.update(verifier)
    aes = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    encrypted_verifier_hash = aes.update(hamac + b"\x00" * 12)  # padded to the 16-byte block

    header = struct.pack("<IIIIIIII", FLAGS, 0, alg_id, ALG_ID_HASH_SHA1, key_size, PROVIDER_TYPE_AES, 0, 0)
    # Version (4), Flags (4), HeaderSize (4), EncryptionHeader, EncryptionVerifier.
    info = struct.pack("<HHI", 4, 2, FLAGS) + struct.pack("<I", len(header)) + header
    info += struct.pack("<I", len(salt)) + salt + encrypted_verifier
    info += struct.pack("<I", 20) + encrypted_verifier_hash

    padded = plain + b"\x00" * ((16 - len(plain) % 16) % 16)
    aes = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    package = struct.pack("<II", len(plain), 0) + aes.update(padded)

    buf = io.BytesIO()
    ECMA376Encrypted(package, info).write_to(buf)
    return buf.getvalue()


# ---------------------------------------------------------------------------------------------


def agile_encrypt(plain, password, hash_name, key_bits, spin_count):
    """msoffcrypto is hardcoded to AES-256/SHA-512, so the cipher params are overridden."""
    original = ECMA376AgileCipherParams.__init__

    def patched(self):
        original(self)
        self.hashName = hash_name
        self.hashSize = hashlib.new(hash_name.lower()).digest_size
        self.keyBits = key_bits

    ECMA376AgileCipherParams.__init__ = patched
    try:
        return ECMA376Agile.encrypt(
            password,
            io.BytesIO(plain),
            salt_value=bytes.fromhex("0f1e2d3c4b5a69788796a5b4c3d2e1f0"),
            spin_count=spin_count,
        )
    finally:
        ECMA376AgileCipherParams.__init__ = original


def verify(blob, plain, password, label):
    """An independent implementation must decrypt the fixture back to the plaintext."""
    f = OOXMLFile(io.BytesIO(blob))
    if not f.is_encrypted():
        raise SystemExit(f"{label}: not recognised as encrypted")
    f.load_key(password, verify_password=True)
    out = io.BytesIO()
    f.decrypt(out, verify_integrity=True)
    if out.getvalue() != plain:
        raise SystemExit(f"{label}: msoffcrypto round trip mismatch")
    print(f"  {label}: verified by msoffcrypto-tool ({len(blob)} bytes)")


def main():
    outdir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)))
    plain_path = os.path.join(outdir, "plain.pptx")
    os.makedirs(outdir, exist_ok=True)
    plain = build_plaintext(plain_path)
    print(f"plaintext: {len(plain)} bytes -> {plain_path}")

    fixtures = {
        # The case that matters for real-world decks: Office 2007 "Password to open".
        "standard-aes128-sha1": standard_encrypt(plain, PASSWORD),
        # Standard encryption with a longer key (the key is x1||x2 truncated, so this covers the
        # 192/256-bit path as well).
        "standard-aes256-sha1": standard_encrypt(plain, PASSWORD, ALG_ID_AES256, 256),
        # Office 2010+ defaults.
        "agile-aes256-sha512": agile_encrypt(plain, PASSWORD, "SHA512", 256, 100000),
        # Other hash/key-size combinations on the same code path (SHA-1 agile containers cannot
        # be produced by msoffcrypto-tool: its HMAC-key helper assumes a 64-byte hash).
        "agile-aes256-sha256": agile_encrypt(plain, PASSWORD, "SHA256", 256, 100000),
        "agile-aes128-sha384": agile_encrypt(plain, PASSWORD, "SHA384", 128, 4000),
    }

    for name, blob in fixtures.items():
        verify(blob, plain, PASSWORD, name)
        with open(os.path.join(outdir, f"{name}.pptx"), "wb") as fh:
            fh.write(blob)
    print(f"wrote {len(fixtures)} encrypted fixtures to {outdir}")


if __name__ == "__main__":
    main()
