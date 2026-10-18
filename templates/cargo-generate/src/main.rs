//! Starter for tpt-query: builds a small source, runs a query, and prints the rows.
//! Replace the entities and the query with your own data.

use tpt_query::{Condition, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Query, Queryable};

const PART: EntityTypeId = EntityTypeId(1);

fn main() {
    let mut src = MemorySource::new();
    src.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    src.insert(Entity::new(EntityId(1), PART).with_field("material", "Steel"));
    src.insert(Entity::new(EntityId(2), PART).with_field("material", "Copper"));

    let query = Query::new(PART)
        .filter(Condition::eq_to("material", "Steel"))
        .project("material");

    println!("{}", src.explain(&query).expect("valid query"));
    for row in src.execute(&query).expect("valid query") {
        let row = row.expect("row");
        println!("{} {:?}", row.id.0, row.projected);
    }
}
