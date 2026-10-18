//! Queries over your own storage: implement `EntitySource` and the engine works
//! unchanged. This source keeps entities in a sorted `Vec`, with no indexes and no
//! relations, so only filters and scans are available.
//!
//! Run with: `cargo run --example custom_source`

use tpt_query::{Condition, Entity, EntityId, EntitySource, EntityTypeId, Query, Queryable, Value};

const PART: EntityTypeId = EntityTypeId(1);

/// Entities sorted by id, as the engine expects.
struct VecSource {
    entities: Vec<Entity>,
}

impl EntitySource for VecSource {
    fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities
            .binary_search_by_key(&id, |e| e.id)
            .ok()
            .map(|i| &self.entities[i])
    }

    fn ids_of_type<'a>(&'a self, ty: EntityTypeId) -> Box<dyn Iterator<Item = EntityId> + 'a> {
        Box::new(
            self.entities
                .iter()
                .filter(move |e| e.ty == ty)
                .map(|e| e.id),
        )
    }
}

fn main() {
    let mut entities: Vec<Entity> = [
        (1u64, "bracket", 12.5f64),
        (2, "bolt", 0.4),
        (3, "plate", 3.25),
    ]
    .into_iter()
    .map(|(id, name, mass)| {
        Entity::new(EntityId(id), PART)
            .with_field("name", name)
            .with_field("mass", mass)
    })
    .collect();
    entities.sort_by_key(|e| e.id);
    let source = VecSource { entities };

    let heavy = Query::new(PART)
        .filter(Condition::gt("mass", Value::Float(1.0)))
        .project("name");
    for row in source.execute(&heavy).unwrap() {
        let row = row.unwrap();
        println!("{} {:?}", row.id.0, row.get("name"));
    }

    // Without relations or indexes the plan is a scan with every condition residual.
    println!("{}", source.explain(&heavy).unwrap());
}
