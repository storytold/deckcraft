//! Document-owned geographic lineage. Parent and child query tables complement each other;
//! their order is not a key, and missing polygon data does not erase an ancestor.
use crate::xml::El;
use deckcraft_model::geography::GeoEntity;
use std::collections::HashMap;

pub(super) fn bounds(e: &El) -> Option<[f64; 4]> {
    let b = [e.f64("west")?, e.f64("south")?, e.f64("east")?, e.f64("north")?];
    (b.iter().all(|v| v.is_finite()) && b[0].abs() <= 180. && b[2].abs() <= 180. && b[1] >= -90. && b[3] <= 90. && b[1] <= b[3]).then_some(b)
}
#[derive(Default)]
struct Builder {
    nodes: Vec<GeoEntity>,
    ids: HashMap<String, usize>,
    work: usize,
    bytes: usize,
}
impl Builder {
    fn node(&mut self, id: &str, name: &str, kind: &str) -> Option<usize> {
        self.work += 1;
        self.bytes = self.bytes.saturating_add(id.len() + name.len() + kind.len());
        if self.work > 20000 || self.bytes > 2 << 20 || id.is_empty() || id.len() > 2048 {
            return None;
        }
        let index = if let Some(&index) = self.ids.get(id) {
            index
        } else {
            if self.nodes.len() >= 4096 {
                return None;
            }
            let index = self.nodes.len();
            self.ids.insert(id.into(), index);
            self.nodes.push(GeoEntity { id: id.into(), ..Default::default() });
            index
        };
        let node = self.nodes.get_mut(index)?;
        if !name.is_empty() {
            node.name = name.into();
        }
        if !kind.is_empty() {
            node.kind = kind.into();
        }
        Some(index)
    }
    fn parent(&mut self, child: usize, parent: &str) -> Option<()> {
        self.node(parent, "", "")?;
        let child = self.nodes.get_mut(child)?;
        if child.id != parent && !child.parents.iter().any(|id| id == parent) {
            child.parents.push(parent.into());
        }
        Some(())
    }
}
pub(super) fn read(clear: &El) -> Option<Vec<GeoEntity>> {
    let mut b = Builder::default();
    if let Some(group) = clear.child("geoDataEntityQueryResults") {
        for row in group.children_named("geoDataEntityQueryResult") {
            if let Some(data) = row.child("geoData")
                && let Some(id) = data.attr("entityId").filter(|s| !s.is_empty())
            {
                b.node(id, data.attr("entityName").unwrap_or(""), row.child("geoDataEntityQuery").and_then(|q| q.attr("entityType")).unwrap_or(""))?;
            }
        }
    }
    if let Some(group) = clear.child("geoChildEntitiesQueryResults") {
        for row in group.children_named("geoChildEntitiesQueryResult") {
            let Some(parent) = row.child("geoChildEntitiesQuery").and_then(|q| q.attr("entityId")).filter(|s| !s.is_empty()) else { continue };
            b.node(parent, "", "")?;
            if let Some(children) = row.child("geoChildEntities") {
                for child in children.children_named("geoHierarchyEntity") {
                    if let Some(id) = child.attr("entityId").filter(|s| !s.is_empty()) {
                        let index = b.node(id, child.attr("entityName").unwrap_or(""), child.attr("entityType").unwrap_or(""))?;
                        b.parent(index, parent)?;
                    }
                }
            }
        }
    }
    if let Some(group) = clear.child("geoParentEntitiesQueryResults") {
        for row in group.children_named("geoParentEntitiesQueryResult") {
            let Some(id) = row.child("geoParentEntitiesQuery").and_then(|q| q.attr("entityId")).filter(|s| !s.is_empty()) else { continue };
            let entity = row.child("geoEntity");
            let index =
                b.node(id, entity.and_then(|e| e.attr("entityName")).unwrap_or(""), entity.and_then(|e| e.attr("entityType")).unwrap_or(""))?;
            if let Some(parent) = row.child("geoParentEntity").and_then(|e| e.attr("entityId")).filter(|s| !s.is_empty()) {
                b.parent(index, parent)?;
            }
        }
    }
    Some(b.nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_lineage_tables_join_without_geometry_or_cache_order() {
        let doc = crate::xml::parse(br#"<clear><geoChildEntitiesQueryResults>
          <geoChildEntitiesQueryResult><geoChildEntitiesQuery entityId="continent"/>
          <geoChildEntities><geoHierarchyEntity entityId="country" entityName="Original country" entityType="countryRegion"/></geoChildEntities>
          </geoChildEntitiesQueryResult></geoChildEntitiesQueryResults><geoParentEntitiesQueryResults>
          <geoParentEntitiesQueryResult><geoParentEntitiesQuery entityId="region"/><geoEntity entityName="Original region" entityType="adminDistrict"/><geoParentEntity entityId="country"/></geoParentEntitiesQueryResult>
          <geoParentEntitiesQueryResult><geoParentEntitiesQuery entityId="continent"/><geoEntity entityName="Original continent" entityType="continent"/><geoParentEntity entityId="world"/></geoParentEntitiesQueryResult>
          </geoParentEntitiesQueryResults></clear>"#).unwrap();
        let nodes = read(&doc.root).unwrap();
        let country = nodes.iter().find(|e| e.id == "country").unwrap();
        assert_eq!(country.kind, "countryRegion");
        assert_eq!(country.parents, ["continent"]);
        assert_eq!(nodes.iter().find(|e| e.id == "region").unwrap().parents, ["country"]);
        assert!(nodes.iter().any(|e| e.id == "world" && e.parents.is_empty()));
    }
    #[test]
    fn malformed_extents_and_excessive_lineage_are_rejected() {
        for attrs in ["west='NaN' east='1' south='0' north='1'", "west='0' east='1' south='2' north='1'", "west='190' east='1' south='0' north='1'"] {
            assert!(bounds(&crate::xml::parse(format!("<geoData {attrs}/>").as_bytes()).unwrap().root).is_none());
        }
        assert_eq!(bounds(&crate::xml::parse(b"<geoData west='170' east='-170' south='-5' north='5'/>").unwrap().root), Some([170., -5., -170., 5.]));
        let mut b = Builder::default();
        for i in 0..4096 {
            b.node(&i.to_string(), "", "").unwrap();
        }
        assert!(b.node("over-budget", "", "").is_none());
    }
}
