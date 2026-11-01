//! Offline joins of the public chartEx geographic query tables. Array order is never a key.
use crate::xml::El;
use std::collections::{HashMap, HashSet};

const MAX_QUERIES: usize = 4096;
const MAX_JOINS: usize = 200_000;
type Location = (String, u64, u64);

fn name(value: &str) -> String {
    value.trim().to_lowercase()
}
fn location(e: &El) -> Option<Location> {
    let (lat, lon) = (e.f64("latitude")?, e.f64("longitude")?);
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90. || lon.abs() > 180. {
        return None;
    }
    let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    Some((name(e.attr("entityType")?), bits(lat), bits(lon)))
}
fn unique<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Option<String>>, key: K, value: &str) {
    if value.is_empty() {
        return;
    }
    map.entry(key)
        .and_modify(|v| {
            if v.as_deref() != Some(value) {
                *v = None;
            }
        })
        .or_insert_with(|| Some(value.into()));
}

pub(super) fn resolve(clear: &El, categories: &[String], levels: &[Vec<String>], ids: &[String]) -> HashMap<String, usize> {
    let mut regions = HashMap::new();
    if let Some(results) = clear.child("geoDataEntityQueryResults") {
        for result in results.children_named("geoDataEntityQueryResult").take(MAX_QUERIES) {
            if let Some(data) = result.child("geoData") {
                unique(&mut regions, name(data.attr("entityName").unwrap_or("")), data.attr("entityId").unwrap_or(""));
            }
        }
    }
    let mut locations = HashMap::new();
    if let Some(results) = clear.child("geoDataPointToEntityQueryResults") {
        for result in results.children_named("geoDataPointToEntityQueryResult").take(MAX_QUERIES) {
            if let Some(query) = result.child("geoDataPointQuery").and_then(location)
                && let Some(entity) = result.child("geoDataPointToEntityQuery").and_then(|e| e.attr("entityId"))
            {
                unique(&mut locations, query, entity);
            }
        }
    }
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    let mut chosen: Vec<Option<String>> = vec![None; categories.len()];
    for (i, category) in categories.iter().enumerate() {
        index.entry(name(category)).or_default().push(i);
        if let Some(id) = ids.get(i).filter(|s| !s.is_empty()) {
            chosen[i] = Some(id.clone());
        }
    }
    // Repeated blank hierarchy cells inherit the preceding parent, as chartData levels do.
    let parents: Vec<Vec<String>> = levels
        .iter()
        .map(|level| {
            let mut previous = String::new();
            level
                .iter()
                .take(categories.len())
                .map(|value| {
                    if !value.trim().is_empty() {
                        previous = name(value);
                    }
                    previous.clone()
                })
                .collect()
        })
        .collect();
    let mut ambiguous = HashSet::new();
    let mut work = 0usize;
    if let Some(results) = clear.child("geoLocationQueryResults") {
        for result in results.children_named("geoLocationQueryResult").take(MAX_QUERIES) {
            let Some(query) = result.child("geoLocationQuery") else { continue };
            let fields: Vec<_> = ["postalCode", "adminDistrict2", "adminDistrict1", "countryRegion"]
                .into_iter()
                .filter_map(|key| query.attr(key))
                .map(name)
                .filter(|s| !s.is_empty())
                .collect();
            let Some(leaf) = fields.first() else { continue };
            let Some(points) = index.get(leaf) else { continue };
            let Some(found) = result.child("geoLocations") else { continue };
            for geo in found.children_named("geoLocation").take(64) {
                let entity = location(geo)
                    .and_then(|key| locations.get(&key))
                    .unwrap_or_else(|| regions.get(&name(geo.attr("entityName").unwrap_or(""))).unwrap_or(&None));
                let Some(entity) = entity else { continue };
                for &i in points {
                    work = work.saturating_add(1 + parents.len());
                    if work > MAX_JOINS {
                        break;
                    }
                    if ids.get(i).is_some_and(|id| !id.is_empty()) || ambiguous.contains(&i) {
                        continue;
                    }
                    if !parents.iter().filter_map(|p| p.get(i)).filter(|p| !p.is_empty()).all(|p| fields.contains(p)) {
                        continue;
                    }
                    if chosen[i].as_ref().is_some_and(|old| old != entity) {
                        chosen[i] = None;
                        ambiguous.insert(i);
                    } else {
                        chosen[i] = Some(entity.clone());
                    }
                }
                if work > MAX_JOINS {
                    break;
                }
            }
            if work > MAX_JOINS {
                break;
            }
        }
    }
    if work > MAX_JOINS {
        // Partial query traversal cannot establish uniqueness. Keep authoritative ids only.
        for i in 0..chosen.len() {
            if ids.get(i).is_none_or(|id| id.is_empty()) {
                ambiguous.insert(i);
            }
        }
    }
    let mut output = HashMap::new();
    let mut repeated = HashSet::new();
    for (i, entity) in chosen.into_iter().enumerate() {
        if ambiguous.contains(&i) {
            continue;
        }
        let entity = entity.or_else(|| regions.get(&name(&categories[i])).cloned().flatten());
        if let Some(entity) = entity {
            // Do not silently use the first of conflicting data rows for the same region.
            if output.insert(entity.clone(), i).is_some() {
                repeated.insert(entity);
            }
        }
    }
    for entity in repeated {
        output.remove(&entity);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache() -> El {
        crate::xml::parse(br#"<clear><geoLocationQueryResults>
        <geoLocationQueryResult><geoLocationQuery countryRegion="Country A" adminDistrict1="Local alias" entityType="AdminDistrict"/>
        <geoLocations><geoLocation entityName="Canonical" entityType="AdminDistrict" latitude="10" longitude="20"/></geoLocations></geoLocationQueryResult>
        <geoLocationQueryResult><geoLocationQuery countryRegion="Country B" adminDistrict1="Local alias" entityType="AdminDistrict"/>
        <geoLocations><geoLocation entityName="Canonical" entityType="AdminDistrict" latitude="30" longitude="40"/></geoLocations></geoLocationQueryResult>
        </geoLocationQueryResults><geoDataEntityQueryResults>
        <geoDataEntityQueryResult><geoData entityName="Canonical" entityId="second"/></geoDataEntityQueryResult>
        <geoDataEntityQueryResult><geoData entityName="Canonical" entityId="first"/></geoDataEntityQueryResult>
        <geoDataEntityQueryResult><geoData entityName="Unique" entityId="third"/></geoDataEntityQueryResult>
        </geoDataEntityQueryResults><geoDataPointToEntityQueryResults>
        <geoDataPointToEntityQueryResult><geoDataPointQuery entityType="AdminDistrict" latitude="30" longitude="40"/>
        <geoDataPointToEntityQuery entityType="AdminDistrict" entityId="second"/></geoDataPointToEntityQueryResult>
        <geoDataPointToEntityQueryResult><geoDataPointQuery entityType="AdminDistrict" latitude="10" longitude="20"/>
        <geoDataPointToEntityQuery entityType="AdminDistrict" entityId="first"/></geoDataPointToEntityQueryResult>
        </geoDataPointToEntityQueryResults></clear>"#).unwrap().root
    }
    #[test]
    fn geographic_query_chain_joins_localized_categories_and_disambiguates_parents() {
        let categories = vec!["Local alias".into(), "Local alias".into(), "Unique".into()];
        let levels = vec![vec!["Country A".into(), "Country B".into(), String::new()]];
        let result = resolve(&cache(), &categories, &levels, &[]);
        assert_eq!(result.get("first"), Some(&0));
        assert_eq!(result.get("second"), Some(&1));
        assert_eq!(result.get("third"), Some(&2));
        let ambiguous = resolve(&cache(), &["Local alias".into(), "Canonical".into()], &[], &[]);
        assert!(ambiguous.is_empty(), "ambiguous names must never choose cache order");
    }
    #[test]
    fn explicit_ids_win_and_conflicting_rows_are_not_silently_dropped() {
        let result = resolve(&cache(), &["Local alias".into()], &[], &["second".into()]);
        assert_eq!(result.get("second"), Some(&0));
        assert!(resolve(&cache(), &["Unique".into(), "Unique".into()], &[], &[]).is_empty());
        let query = crate::xml::parse(br#"<geoLocation entityType="Region" latitude="NaN" longitude="0"/>"#).unwrap();
        assert!(location(&query.root).is_none());
    }
}
