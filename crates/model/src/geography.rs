//! Geographic chart caches. Coordinates are longitude, latitude in degrees; no map assets
//! or network service are bundled. PCA is the public Bing point-compression wire format.
use serde::{Deserialize, Serialize};

pub const MAX_GEO_POINTS: usize = 2_000_000;
pub const MAX_GEO_RINGS: usize = 100_000;
pub const MAX_GEO_POLYGON_RINGS: usize = 4096;
pub const MAX_GEO_PROVIDER_BYTES: usize = 256;
pub const MAX_GEO_CHILD_QUERIES: usize = 4096;
pub const MAX_GEO_CHILD_REFERENCES: usize = 20_000;
pub const MAX_GEO_QUERY_BYTES: usize = 2 << 20;
/// Compressed rings can accumulate longitude beyond one turn before projection.
pub const MAX_GEO_LONGITUDE: f64 = 540.;
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

/// chartEx's two/three-stop numeric colour scale. Positions are data values or percentages
/// of the complete numeric range, not quantiles of the regions that happen to be visible.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ValueColorScale {
    pub three_color: bool,
    pub minimum: ValueColorStop,
    pub middle: ValueColorStop,
    pub maximum: ValueColorStop,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ValueColorStop {
    pub color: Option<crate::style::ColorRef>,
    pub position: Option<ValueColorPosition>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ValueColorPosition {
    Extreme,
    Number(f64),
    Percent(f64),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Geography {
    /// Document-owned cache provider identity; an empty value has no native provider.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub provider: String,
    pub projection: String,
    /// Authored viewport scope, e.g. `dataOnly` or `world`; empty means automatic.
    pub viewed_region: String,
    pub attribution: String,
    pub regions: Vec<GeoRegion>,
    /// Document-owned administrative lineage, including ancestors without boundary polygons.
    pub entities: Vec<GeoEntity>,
    /// Cached child-enumeration keys and results. Parent edges alone do not identify
    /// which query was cached or distinguish an absent result from an empty one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub child_queries: Vec<GeoChildQuery>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeoChildQuery {
    pub entity_id: String,
    /// Preserve the wire vocabulary, order and multiplicity rather than deriving types.
    pub child_types: Option<Vec<String>>,
    /// IDs refer to `Geography.entities`, including entities with no boundary geometry.
    /// `None` is an absent result element; `Some([])` is an explicitly empty result.
    pub children: Option<Vec<String>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeoEntity {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parents: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeoRegion {
    pub entity_id: String,
    pub name: String,
    /// Data index is resolved by entity id, geocoding result or category name, never order.
    pub point: Option<usize>,
    /// Preserve accumulated longitude; projection and extent metadata determine its seam.
    pub rings: Vec<Vec<[f64; 2]>>,
    pub copyrights: Vec<String>,
    /// Provider's geographic extent: west, south, east, north (east < west crosses the seam).
    pub bounds: Option<[f64; 4]>,
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
        // Folding here loses both accumulated turns and the distinction between a
        // long encoded edge and a seam crossing. The renderer owns that decision.
        out.push([lon as f64 / 100_000.0, lat as f64 / 100_000.0]);
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
        if !x.is_finite() || !y.is_finite() || x.abs() > MAX_GEO_LONGITUDE || y.abs() > 90.0 {
            return None;
        }
        let (next_lat, next_lon) = ((y * 100_000.0).round() as i64, (x * 100_000.0).round() as i64);
        let dy = next_lat - lat;
        let dx = next_lon - lon;
        let zig = |v: i64| if v < 0 { (-v * 2 - 1) as u64 } else { (v * 2) as u64 };
        let (a, b) = (zig(dy), zig(dx));
        let sum = a.checked_add(b)?;
        let mut n = (sum.checked_mul(sum.checked_add(1)?)? / 2).checked_add(a)?;
        if n > (1u64 << 53) {
            return None;
        }
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

    #[test]
    fn pca_preserves_accumulated_turns_and_explicit_long_edges() {
        for ring in [
            vec![[-207.5, 12.], [-204.5, 14.], [-206., 17.], [-207.5, 12.]],
            vec![[-497., -12.], [-495., -14.], [-492., -11.], [-497., -12.]],
            vec![[178., 10.], [182., 10.], [182., 15.], [178., 10.]],
            vec![[178., 10.], [-178., 10.], [-178., 15.], [178., 10.]],
            vec![[-180., 10.], [180., 10.], [180., 15.], [-180., 10.]],
        ] {
            let encoded = encode_ring(&ring).unwrap();
            assert_eq!(decode_ring(&encoded, &mut 100).unwrap(), ring);
        }
        let short = [[178., 10.], [182., 10.], [182., 15.]];
        let long = [[178., 10.], [-178., 10.], [-178., 15.]];
        assert_ne!(encode_ring(&short), encode_ring(&long));
    }

    #[test]
    fn accumulated_coordinate_limits_and_wire_integer_budget_are_checked() {
        for lon in [-540., 540.] {
            let ring = [[lon, 90.]; 3];
            let encoded = encode_ring(&ring).unwrap();
            assert_eq!(decode_ring(&encoded, &mut 3).unwrap(), ring);
        }
        for lon in [-540.00001, 540.00001, f64::INFINITY] {
            assert!(encode_ring(&[[lon, 0.]; 3]).is_none());
        }
        // A bounded coordinate still cannot create a delta outside the decoder's
        // exactly representable wire-integer range.
        assert!(encode_ring(&[[-540., 90.], [540., -90.], [-540., 90.]]).is_none());
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
