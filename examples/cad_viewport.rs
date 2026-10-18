//! A CAD viewport: which parts overlap the visible box. An R-tree index answers the
//! box query, and the plan shows the index in use.
//!
//! Run with: `cargo run --example cad_viewport --features spatial`

use tpt_query::spatial::RTreeIndex;
use tpt_query::{
    Aabb, Condition, Entity, EntityId, EntityTypeId, MemorySource, Query, Queryable, Value,
};

const PART: EntityTypeId = EntityTypeId(1);

fn main() {
    let mut scene = MemorySource::new();
    scene
        .add_index(Box::new(RTreeIndex::new("by_bbox", PART, "bbox")))
        .unwrap();

    // A row of unit boxes along the x axis, one every two units.
    for i in 0..10u64 {
        let x = (i * 2) as f64;
        scene.insert(
            Entity::new(EntityId(i + 1), PART)
                .with_field("name", format!("bracket-{i}"))
                .with_field(
                    "bbox",
                    Value::Aabb(Aabb::new([x, 0.0, 0.0], [x + 1.0, 1.0, 1.0])),
                ),
        );
    }

    // The camera sees x from 5 to 9.
    let view = Aabb::new([5.0, -1.0, -1.0], [9.0, 2.0, 2.0]);
    let query = Query::new(PART)
        .filter(Condition::intersects("bbox", view))
        .project("name");

    for row in scene.execute(&query).unwrap() {
        let row = row.unwrap();
        println!("visible {:?}", row.get("name").and_then(|v| v.as_str()));
    }
    println!("{}", scene.explain(&query).unwrap());
}
