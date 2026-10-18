//! A snapshot is isolated from later writes, and answers like the source does.

mod common;

use common::{PART, ids};
use tpt_query::{Condition, Entity, EntityId, IndexKind, MemorySource, Query, Queryable, Value};

fn source() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    for i in 1..=6u64 {
        let material = if i % 2 == 0 { "Steel" } else { "Copper" };
        s.insert(
            Entity::new(EntityId(i), PART)
                .with_field("material", material)
                .with_field("mass", Value::Int(i as i64)),
        );
    }
    s
}

#[test]
fn writes_after_the_snapshot_are_not_visible_in_it() {
    let mut s = source();
    let snap = s.snapshot();

    s.remove(EntityId(2));
    s.insert(Entity::new(EntityId(7), PART).with_field("material", "Steel"));

    let steel = Query::new(PART).filter(Condition::eq_to("material", "Steel"));
    assert_eq!(ids(&snap, &steel), vec![2, 4, 6]);
    assert_eq!(ids(&s, &steel), vec![4, 6, 7]);
}

#[test]
fn snapshot_answers_match_the_indexed_source() {
    let s = source();
    let snap = s.snapshot();
    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "Copper"))
        .filter(Condition::gt("mass", Value::Int(1)));
    assert_eq!(ids(&snap, &q), ids(&s, &q));
    // The snapshot has no indexes, so its plan is a scan.
    assert!(snap.explain(&q).unwrap().to_string().contains("scan"));
}
