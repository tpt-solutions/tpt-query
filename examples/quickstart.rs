//! The README quick start, runnable as-is.
//!
//! Run with: `cargo run --example quickstart`

use tpt_query::{
    Condition, Edge, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Order, Query,
    Queryable, Traversal,
};

const PART: EntityTypeId = EntityTypeId(1);
const FACE: EntityTypeId = EntityTypeId(2);

fn main() {
    let mut src = MemorySource::new();
    src.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    src.insert(Entity::new(EntityId(1), PART).with_field("material", "Titanium"));
    src.insert(Entity::new(EntityId(2), FACE).with_field("area", 4.5f64));
    src.link_child(EntityId(1), EntityId(2));

    // Faces of every titanium part.
    let query = Query::new(PART)
        .filter(Condition::eq_to("material", "Titanium"))
        .traverse(
            Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                .filter(Condition::OfType(FACE)),
        )
        .project("area");

    for row in src.execute(&query).unwrap() {
        let row = row.unwrap();
        println!("face {} {:?}", row.id.0, row.projected);
    }
    println!("{}", src.explain(&query).unwrap());
}
