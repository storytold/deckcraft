//! Geographic chart caches. Coordinates are longitude, latitude in degrees; no map assets
//! or network service are bundled. PCA is the public Bing point-compression wire format.
use serde::{Deserialize, Serialize};

pub const MAX_GEO_POINTS: usize = 2_000_000;
pub const MAX_GEO_RINGS: usize = 100_000;
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Geography {
    pub projection: String,
    pub attribution: String,
    pub regions: Vec<GeoRegion>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeoRegion {
    pub entity_id: String,
    pub name: String,
    /// Data index is resolved by entity id, geocoding result or category name, never order.
    pub point: Option<usize>,
    pub rings: Vec<Vec<[f64; 2]>>,
    pub copyrights: Vec<String>,
}

/// Decode one PCA ring with integer Cantor unpairing and checked accumulation. A global
/// caller budget covers all rings, including invalid input; no attacker-sized reserve.
pub fn decode_ring(encoded: &str, budget: &mut usize) -> Option<Vec<[f64; 2]>> {
    if encoded.len() > MAX_GEO_POINTS * 11 {
        return None;
    }
    let mut out = Vec::new();
    let (mut lat, mut lon) = (0i64, 0i64);
    let mut bytes = encoded.bytes();
    while let Some(first) = bytes.next() {
        *budget = budget.checked_sub(1)?;
        let mut value = 0u64;
        let mut b = first;
        for shift in (0..55).step_by(5) {
            let digit = ALPHABET.iter().position(|c| *c == b)? as u64;
            value |= (digit & 31) << shift;
            if digit < 32 {
                break;
            }
            if shift == 50 {
                return None;
            }
            b = bytes.next()?;
        }
        // All valid coordinate deltas fit in 53 bits, exactly representable by f64.
        if value > (1u64 << 53) {
            return None;
        }
        let mut diagonal = (((8.0 * value as f64 + 1.0).sqrt() - 1.0) / 2.0) as u64;
        while diagonal * (diagonal + 1) / 2 > value {
            diagonal = diagonal.checked_sub(1)?;
        }
        while (diagonal + 1) * (diagonal + 2) / 2 <= value {
            diagonal += 1;
        }
        let dy = value - diagonal * (diagonal + 1) / 2;
        let dx = diagonal.checked_sub(dy)?;
        let signed = |v: u64| if v & 1 == 0 { (v / 2) as i64 } else { -((v / 2) as i64) - 1 };
        lat = lat.checked_add(signed(dy))?;
        lon = lon.checked_add(signed(dx))?;
        if lat.abs() > 9_000_000 || lon.abs() > 54_000_000 {
            return None;
        }
        let longitude = lon as f64 / 100_000.0;
        // +180 and -180 identify opposite clipped boundaries in a world polygon. Do not
        // fold the positive boundary onto the negative one and draw a chord across the map.
        let longitude = if longitude.abs() <= 180.0 { longitude } else { (longitude + 180.0).rem_euclid(360.0) - 180.0 };
        out.push([longitude, lat as f64 / 100_000.0]);
    }
    (out.len() >= 3).then_some(out)
}

pub fn encode_ring(points: &[[f64; 2]]) -> Option<String> {
    if points.len() < 3 || points.len() > MAX_GEO_POINTS {
        return None;
    }
    let mut text = String::new();
    let (mut lat, mut lon) = (0i64, 0i64);
    for &[x, y] in points {
        if !x.is_finite() || !y.is_finite() || x.abs() > 180.0 || y.abs() > 90.0 {
            return None;
        }
        let (next_lat, next_lon) = ((y * 100_000.0).round() as i64, (x * 100_000.0).round() as i64);
        let dy = next_lat - lat;
        let dx = (next_lon - lon + 18_000_000).rem_euclid(36_000_000) - 18_000_000;
        let zig = |v: i64| if v < 0 { (-v * 2 - 1) as u64 } else { (v * 2) as u64 };
        let (a, b) = (zig(dy), zig(dx));
        let mut n = (a + b) * (a + b + 1) / 2 + a;
        loop {
            let digit = (n & 31) as usize;
            n >>= 5;
            text.push(*ALPHABET.get(digit + if n > 0 { 32 } else { 0 })? as char);
            if n == 0 {
                break;
            }
        }
        (lat, lon) = (next_lat, next_lon);
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pca_roundtrip_dateline_and_hostile_input() {
        let ring = [[179.0, -20.0], [-179.0, -20.0], [-179.0, 20.0], [179.0, -20.0]];
        let encoded = encode_ring(&ring).unwrap();
        assert_eq!(decode_ring(&encoded, &mut 20).unwrap(), ring);
        for bad in ["!", "_", "____________A", "AA"] {
            assert!(decode_ring(bad, &mut 20).is_none());
        }
        assert!(decode_ring(&encoded, &mut 2).is_none());
        assert!(encode_ring(&[[f64::NAN, 0.0]; 3]).is_none());
    }
}

#[cfg(test)]
mod wire_vectors {
    #[test]
    fn published_pca_numeric_vector_has_longitude_latitude_order() {
        // Mathematical wire-format vector from the public Point Compression Algorithm
        // specification; these four coordinates are not a bundled map boundary asset.
        let points = super::decode_ring("vx1vilihnM6hR7mEl2Q", &mut 10).unwrap();
        let expected = [[-110.72522, 35.89431], [-110.72578, 35.89393], [-110.72606, 35.89374], [-110.72662, 35.89337]];
        for (actual, expected) in points.iter().zip(expected) {
            assert!((actual[0] - expected[0]).abs() < 1e-10 && (actual[1] - expected[1]).abs() < 1e-10);
        }
    }
}

#[cfg(test)]
mod meridian_tests {
    use super::*;
    #[test]
    fn positive_world_boundary_stays_positive_after_pca_decode() {
        let ring = vec![[170., 10.], [180., 10.], [180., 20.], [170., 20.], [170., 10.]];
        let encoded = encode_ring(&ring).unwrap();
        let actual = decode_ring(&encoded, &mut 100).unwrap();
        assert_eq!(actual, ring);
    }
}
