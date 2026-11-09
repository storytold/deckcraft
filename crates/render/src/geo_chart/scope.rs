//! Select an administrative view from document-owned lineage, never from country names.
use deckcraft_model::geography::Geography;
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) struct Scope {
    pub visible: Vec<bool>,
    pub reference: Vec<bool>,
    pub regional: bool,
    pub world: bool,
    pub bounds: Option<[f64; 4]>,
}
fn level(kind: &str) -> usize {
    match kind.to_ascii_lowercase().as_str() {
        "postcode1" | "postcode2" | "postcode3" | "postcode4" | "postalcode" => 1,
        "admindistrict2" | "county" => 2,
        "admindistrict" | "admindistrict1" | "state" => 3,
        "countryregion" => 4,
        "continent" | "countryregionlist" => 6,
        "planet" | "world" => 7,
        _ => 0,
    }
}
fn walk(start: usize, edges: &[Vec<usize>], budget: &mut usize) -> Option<HashMap<usize, usize>> {
    let mut seen = HashMap::from([(start, 0)]);
    let mut queue = VecDeque::from([(start, 0)]);
    while let Some((node, distance)) = queue.pop_front() {
        for &next in edges.get(node)? {
            *budget = budget.checked_sub(1)?;
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(next) {
                e.insert(distance + 1);
                queue.push_back((next, distance + 1));
            }
        }
    }
    Some(seen)
}
fn selected(map: &Geography) -> Option<Scope> {
    let nodes = &map.entities;
    if nodes.is_empty() || nodes.len() > 4096 || map.regions.len() > 4096 {
        return None;
    }
    let ids: HashMap<_, _> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    if ids.len() != nodes.len() || ids.contains_key("") {
        return None;
    }
    let mut parents = vec![vec![]; nodes.len()];
    let mut children = vec![vec![]; nodes.len()];
    let mut budget = 200000usize;
    for (i, node) in nodes.iter().enumerate() {
        for parent in &node.parents {
            budget = budget.checked_sub(1)?;
            if let Some(&j) = ids.get(parent.as_str()) {
                parents.get_mut(i)?.push(j);
                children.get_mut(j)?.push(i);
            }
        }
    }
    let mut active = HashSet::new();
    for region in map.regions.iter().filter(|r| r.point.is_some()) {
        active.insert(*ids.get(region.entity_id.as_str())?);
    }
    if active.is_empty() {
        return None;
    }
    let mut common: Option<HashMap<usize, (usize, usize)>> = None;
    for node in active {
        let ancestors = walk(node, &parents, &mut budget)?;
        if let Some(common) = &mut common {
            common.retain(|i, (max, sum)| {
                if let Some(&distance) = ancestors.get(i) {
                    *max = (*max).max(distance);
                    *sum += distance;
                    true
                } else {
                    false
                }
            });
        } else {
            common = Some(ancestors.into_iter().map(|(i, d)| (i, (d, d))).collect());
        }
    }
    let requested = level(&map.viewed_region);
    let (root, _) = common?
        .into_iter()
        .filter(|(i, _)| nodes.get(*i).is_some_and(|n| level(&n.kind) >= requested))
        .min_by_key(|(i, distance)| (*distance, nodes.get(*i).map(|n| n.id.as_str())))?;
    let root = nodes.get(root)?;
    let descendants = walk(*ids.get(root.id.as_str())?, &children, &mut budget)?;
    if level(&root.kind) == 0 && descendants.len() == 1 {
        // A legacy cache with only an untyped region does not establish a world scope.
        return None;
    }
    let world = map.viewed_region == "world" || level(&root.kind) == 7 || root.parents.is_empty() && level(&root.kind) == 0;
    let mut reference = Vec::with_capacity(map.regions.len());
    let mut visible = Vec::with_capacity(map.regions.len());
    let bounds = map
        .regions
        .iter()
        .find(|r| r.entity_id == root.id)
        .and_then(|r| r.bounds)
        .filter(|b| b.iter().all(|v| v.is_finite()) && b[0].abs() <= 180. && b[2].abs() <= 180. && b[1] >= -90. && b[3] <= 90. && b[1] <= b[3]);
    // Cached ancestor polygons are context, not another gray layer over their children.
    let available: HashSet<_> = map.regions.iter().filter_map(|r| ids.get(r.entity_id.as_str()).copied()).collect();
    let mut non_leaf = HashSet::new();
    for &id in &available {
        for (ancestor, distance) in walk(id, &parents, &mut budget)? {
            if distance > 0 {
                non_leaf.insert(ancestor);
            }
        }
    }
    for r in &map.regions {
        let id = ids.get(r.entity_id.as_str());
        let inside = world || id.is_some_and(|i| descendants.contains_key(i));
        let leaf = id.is_none_or(|i| !non_leaf.contains(i)) || r.point.is_some();
        reference.push(inside && (leaf || r.entity_id == root.id));
        visible.push(inside && leaf && (map.viewed_region != "dataOnly" || r.point.is_some()));
    }
    Some(Scope { visible, reference, regional: !world, world, bounds })
}
pub(super) fn resolve(map: &Geography) -> Scope {
    selected(map).unwrap_or_else(|| Scope {
        visible: map.regions.iter().take(4096).map(|r| map.viewed_region != "dataOnly" || r.point.is_some()).collect(),
        reference: vec![true; map.regions.len().min(4096)],
        regional: false,
        world: map.viewed_region == "world",
        bounds: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::geography::{GeoEntity, GeoRegion};
    fn fixture() -> Geography {
        let nodes = [
            ("earth", "planet", ""),
            ("continent", "continent", "earth"),
            ("a", "countryRegion", "continent"),
            ("b", "countryRegion", "continent"),
            ("c", "countryRegion", "earth"),
        ];
        Geography {
            entities: nodes
                .iter()
                .map(|&(id, kind, parent)| GeoEntity {
                    id: id.into(),
                    kind: kind.into(),
                    parents: if parent.is_empty() { vec![] } else { vec![parent.into()] },
                    ..Default::default()
                })
                .collect(),
            regions: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(i, &id)| GeoRegion { entity_id: id.into(), point: (i == 0).then_some(0), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn viewport_uses_common_ancestor_and_requested_minimum_level() {
        let mut map = fixture();
        assert_eq!(resolve(&map).visible, [true, false, false]);
        map.viewed_region = "countryRegionList".into();
        assert_eq!(resolve(&map).visible, [true, true, false]);
        map.viewed_region.clear();
        map.regions[1].point = Some(1);
        assert_eq!(resolve(&map).visible, [true, true, false]);
        map.viewed_region = "dataOnly".into();
        let cropped = resolve(&map);
        assert!(cropped.regional && !cropped.world);
        assert_eq!(cropped.reference, [true, true, false]);
        map.regions[2].point = Some(2);
        map.viewed_region.clear();
        assert!(resolve(&map).world);
        map.viewed_region = "world".into();
        map.regions[1].point = None;
        map.regions[2].point = None;
        assert_eq!(resolve(&map).visible, [true, true, true]);
    }
    #[test]
    fn cached_parent_is_projection_context_and_never_overpaints_children() {
        let mut map = fixture();
        map.regions[1].point = Some(1);
        map.regions.push(GeoRegion { entity_id: "continent".into(), bounds: Some([-20., -10., 30., 40.]), ..Default::default() });
        let scope = resolve(&map);
        assert_eq!(scope.visible, [true, true, false, false]);
        assert_eq!(scope.reference, [true, true, false, true]);
        assert_eq!(scope.bounds, Some([-20., -10., 30., 40.]));
        map.entities[0].parents = vec!["a".into()];
        let _ = resolve(&map); // Hostile cycles terminate without recursive traversal.
    }
    #[test]
    fn untyped_singletons_keep_legacy_projection_and_empty_ids_fall_back() {
        let map = Geography {
            entities: vec![GeoEntity { id: "a".into(), kind: "Region".into(), ..Default::default() }],
            regions: vec![GeoRegion { entity_id: "a".into(), point: Some(0), ..Default::default() }],
            ..Default::default()
        };
        let scope = resolve(&map);
        assert!(!scope.regional && !scope.world);
        let mut map = map;
        map.entities[0].id.clear();
        assert!(!resolve(&map).world);
    }
}
