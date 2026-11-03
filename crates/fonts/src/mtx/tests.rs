use super::*;

#[test]
fn independent_lzcomp_vector() {
    // sfntly's Apache-2.0 encoder, applied to our original byte sequence below. No font asset.
    let encoded = "000238383bbda1c94ac6451e46123cca5e2258e9748ac8ca4604a1388c484322383bbd91e4a56322a46123e5e22589a452a36894462421911c1ddee56490c64548c247cbc44b42114e8fe251189086443245a1e5648a852913d972b930ac84229fa0094462421910b8741213698bcc8473d9d5007430228944662432210c8e242f9284a705f3a9d0ec8a6c23352210ce4486e6625229a1e5c119108624129144644218904a45119108624129144644218904a45119108624129144644218904a45119108624129144644218904a45119108624129144644218904a45119108624129144644218904a452991086542a914a644218b05a4543910864c269150e44219309a4538910865e2f914e25429978be5538950a65e2f934e2842d978be8538a10b65e2fa14e27a2e978be7b389e90a63319eced2ec0ca3960f4";
    let bytes: Vec<u8> = encoded.as_bytes().chunks(2).map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap()).collect();
    let input: Vec<_> = b"MTX original test: ABABABAB ".iter().copied().chain(0..=255).collect();
    assert_eq!(lz::decode(&bytes, 3, MAX_FONT).unwrap(), input.repeat(4));
}

#[test]
fn compact_contours_and_composite_restore() {
    // Empty .notdef, an original triangular outline, and a translated component of it.
    let mut data = vec![0, 0, 0, 1, 2, 11, 1, 10, 100, 100, 100, 0, 0];
    data.extend([255, 255, 0, 10, 0, 20, 0, 110, 0, 120, 0, 3, 0, 1, 0, 10, 0, 20]);
    let decoded = glyph::decode(&data, &[], &[], 3).unwrap();
    let (glyf, offsets) = (decoded.glyphs, decoded.offsets);
    assert_eq!(offsets, [0, 0, 32, 52]);
    assert_eq!(&glyf[..10], &[0, 1, 0, 0, 0, 0, 0, 100, 0, 100]);
    assert_eq!(&glyf[14..17], &[1, 1, 1]);
    assert_eq!(&glyf[32..42], &[255, 255, 0, 10, 0, 20, 0, 110, 0, 120]);
}

#[test]
fn compact_control_values_wrap_as_signed_words() {
    assert_eq!(
        metrics::cvt(&[0, 5, 100, 239, 50, 248, 12, 238, 127, 255, 238, 128, 0]).unwrap(),
        [100i16, 50, 300, -32469, 299].into_iter().flat_map(i16::to_be_bytes).collect::<Vec<_>>()
    );
}

#[test]
fn compressed_and_passthrough_device_metrics() {
    let mut head = vec![0; 54];
    put16(&mut head, 18, 1000).unwrap();
    let mut hhea = vec![0; 36];
    put16(&mut hhea, 34, 2).unwrap();
    let hmtx = [1, 244, 0, 0, 2, 88, 0, 0]; // Advances 500 and 600.
    let source = [0, 0, 0, 1, 0, 0, 0, 4, 10, 7, 2]; // Widths 5+0 and 6+1; LSB unary deltas.
    assert_eq!(metrics::hdmx(&source, &head, &hhea, &hmtx, 2).unwrap(), [0, 0, 0, 1, 0, 0, 0, 4, 10, 7, 5, 7]);
    let mut plain = vec![255, 255, 0, 1, 0, 0, 0, 4, 10, 7, 5, 7];
    let decoded = metrics::hdmx(&plain, &[], &[], &[], 2).unwrap();
    plain[..2].fill(0);
    assert_eq!(decoded, plain);
}

#[test]
fn vertical_metrics_restore_groups_and_ratio_offsets() {
    let source = [0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 12, 0, 2, 8, 0, 4, 0, 0];
    let out = metrics::vdmx(&source).unwrap();
    assert_eq!(out, [0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 12, 0, 2, 8, 9, 0, 8, 0, 8, 255, 252, 0, 9, 0, 9, 255, 251]);
}

#[test]
fn truncated_and_hostile_font_streams_are_rejected() {
    for size in 0..32 {
        assert!(decode(&vec![0; size]).is_err());
    }
    assert!(decode(&[3, 0, 0, 0, 255, 255, 255, 0, 0, 10]).is_err());
    assert!(glyph::decode(&[255, 254], &[], &[], 1).is_err());
    assert!(glyph::decode(&[0, 1, 253, 255, 255], &[], &[], 1).is_err());
    assert!(metrics::vdmx(&[0, 1, 255, 255, 255, 255]).is_err());
    assert!(metrics::hdmx(&[0, 0, 255, 255, 127, 255, 255, 255], &[], &[], &[], 1).is_err());
    // Deterministic fuzz smoke: malformed inputs must return rather than panic or allocate
    // according to untrusted 32-bit table lengths.
    let mut seed = 314159u32;
    for n in 0..512 {
        let mut bytes = vec![0; n];
        for b in &mut bytes {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (seed >> 24) as u8;
        }
        let _ = decode(&bytes);
    }
}

/// Cross-check a local MTX file from an independent encoder. Fonts and external encoded files
/// stay outside the repository; the optional test also verifies whole-font checksums.
#[test]
#[ignore = "requires MTX_ORACLE_INPUT and MTX_ORACLE_OUTPUT paths"]
fn independent_encoder_oracle() {
    let input = std::env::var("MTX_ORACLE_INPUT").expect("input path");
    let output = std::env::var("MTX_ORACLE_OUTPUT").expect("output path");
    let bytes = decode(&std::fs::read(input).expect("read input")).expect("decode MTX");
    assert_eq!(checksum(&bytes), 0xb1b0afba);
    std::fs::write(output, bytes).expect("write output");
}
