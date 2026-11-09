//! Paint order of materialized geographic regions, before boundary reduction.
use deckcraft_model::geography::{GeoRegion, Geography};
use kurbo::BezPath;
use std::cmp::Ordering;

pub(super) fn regions(map: &Geography, paths: Vec<BezPath>) -> Vec<(&GeoRegion, BezPath)> {
    // Projection has already bounded the region count and excluded invisible or
    // invalid rings. Extents without geometry do not occupy a drawing slot.
    let mut regions: Vec<_> = map.regions.iter().zip(paths).filter(|(_, path)| !path.elements().is_empty()).collect();
    regions.sort_by(|(a, _), (b, _)| match (a.point, b.point) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.entity_id.cmp(&b.entity_id),
    });
    // Native geography batches exchange the first and middle slots above 128
    // regions, including all-background batches. Count before the distance filter:
    // a tiny ring that later collapses still occupies its original drawing slot.
    if regions.len() > 128 {
        let middle = regions.len() / 2;
        regions.swap(0, middle);
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Point, Shape as _};

    fn triangle(size: f64) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(Point::ZERO);
        path.line_to((size, 0.));
        path.line_to((0., size));
        path.close_path();
        path
    }

    #[test]
    fn data_rows_precede_lexical_background_keys_regardless_of_cache_order() {
        let map = Geography {
            regions: [("data-later", Some(7)), ("region-2", None), ("data-earlier", Some(2)), ("region-10", None)]
                .into_iter()
                .map(|(id, point)| GeoRegion { entity_id: id.into(), point, ..Default::default() })
                .collect(),
            ..Default::default()
        };
        let ordered = regions(&map, vec![triangle(4.); 4]);
        let ids: Vec<_> = ordered.iter().map(|(region, _)| region.entity_id.as_str()).collect();
        assert_eq!(ids, ["data-earlier", "data-later", "region-10", "region-2"]);
    }

    fn background_map(count: usize) -> Geography {
        Geography {
            regions: (0..count).rev().map(|i| GeoRegion { entity_id: format!("region-{i:03}"), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn large_batches_exchange_slots_even_without_data_values() {
        for count in [128, 129, 181] {
            let map = background_map(count);
            let ordered = regions(&map, vec![triangle(4.); count]);
            let first = if count > 128 { count / 2 } else { 0 };
            assert_eq!(ordered[0].0.entity_id, format!("region-{first:03}"));
            let middle = if count > 128 { 0 } else { count / 2 };
            assert_eq!(ordered[count / 2].0.entity_id, format!("region-{middle:03}"));
            assert_eq!(ordered.last().unwrap().0.entity_id, format!("region-{:03}", count - 1));
        }
    }

    #[test]
    fn missing_boundaries_do_not_count_but_collapsed_rings_do() {
        let mut map = background_map(129);
        map.regions.insert(0, GeoRegion { point: Some(0), bounds: Some([-180., -90., 180., 90.]), ..Default::default() });
        let mut paths = vec![triangle(4.); 130];
        paths[0] = BezPath::new();
        paths[1] = BezPath::new();
        let ordered = regions(&map, paths.clone());
        assert_eq!(ordered.len(), 128);
        assert_eq!(ordered[0].0.entity_id, "region-000");

        paths[1] = triangle(0.01);
        assert_eq!(super::super::simplify::reduce(&paths[1]).area(), 0.);
        let ordered = regions(&map, paths);
        assert_eq!(ordered.len(), 129);
        assert_eq!(ordered[0].0.entity_id, "region-064");
        assert_eq!(ordered[64].0.entity_id, "region-000");
    }
}
