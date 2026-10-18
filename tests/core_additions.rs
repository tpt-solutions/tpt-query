//! Field path escaping and list indexes, `OfTypeIn`, `ContainsPoint`, `distinct`,
//! and `descend_while` pruning.

mod common;

use common::{PART, ids};
use tpt_query::{
    Aabb, Condition, Edge, Entity, EntityId, EntityTypeId, FieldPath, MemorySource, Order, Query,
    Traversal, Value,
};

#[test]
fn field_paths_escape_dots_and_backslashes() {
    let p = FieldPath::parse(r"a\.b.c\\d");
    assert_eq!(p.segments(), &["a.b".to_string(), "c\\d".to_string()]);
    assert_eq!(FieldPath::parse(&p.to_string()), p);
}

#[test]
fn numeric_segments_index_lists() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field(
        "items",
        Value::List(vec![Value::from("first"), Value::from("second")]),
    ));
    let q = Query::new(PART).filter(Condition::eq_to("items.1", "second"));
    assert_eq!(ids(&s, &q), vec![1]);
    let missing = Query::new(PART).filter(Condition::eq_to("items.9", "x"));
    assert!(ids(&s, &missing).is_empty());
}

#[test]
fn of_type_in_matches_any_listed_type() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), EntityTypeId(1)));
    s.insert(Entity::new(EntityId(2), EntityTypeId(2)));
    s.insert(Entity::new(EntityId(3), EntityTypeId(3)));
    let q = Query::new(EntityTypeId(1)).filter(Condition::OfTypeIn(vec![EntityTypeId(1)]));
    assert_eq!(ids(&s, &q), vec![1]);
    // Source type 1 only; the list is checked against each entity's own type.
    let wide = Query::new(EntityTypeId(1)).filter(Condition::OfTypeIn(vec![]));
    assert!(ids(&s, &wide).is_empty());
}

#[test]
fn contains_point_includes_boundaries() {
    let mut s = MemorySource::new();
    s.insert(
        Entity::new(EntityId(1), PART).with_field("bbox", Aabb::new([0.0; 3], [2.0, 2.0, 2.0])),
    );
    let inside = Query::new(PART).filter(Condition::ContainsPoint {
        field: "bbox".into(),
        point: [2.0, 1.0, 0.0],
    });
    assert_eq!(ids(&s, &inside), vec![1]);
    let outside = Query::new(PART).filter(Condition::ContainsPoint {
        field: "bbox".into(),
        point: [2.1, 1.0, 0.0],
    });
    assert!(ids(&s, &outside).is_empty());
}

#[test]
fn distinct_yields_a_shared_node_once() {
    // Two parts share one face. Without distinct the face appears twice.
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART));
    s.insert(Entity::new(EntityId(2), PART));
    s.insert(Entity::new(EntityId(3), EntityTypeId(2)));
    s.link_named(EntityId(1), "face", EntityId(3));
    s.link_named(EntityId(2), "face", EntityId(3));

    let base = Query::new(PART).traverse(Traversal::step(Edge::Named("face".into())));
    assert_eq!(ids(&s, &base), vec![3, 3]);
    assert_eq!(ids(&s, &base.clone().distinct()), vec![3]);
}

#[test]
fn descend_while_stops_the_walk_below_a_failing_node() {
    // A -> B (kind "stop") -> C. B is yielded, but C is not reached through it.
    // A -> D (kind "go") -> E. Both D and E are reached.
    let mut s = MemorySource::new();
    for (id, kind) in [
        (1u64, "root"),
        (2, "stop"),
        (3, "leaf"),
        (4, "go"),
        (5, "leaf"),
    ] {
        s.insert(Entity::new(EntityId(id), PART).with_field("kind", kind));
    }
    s.link_child(EntityId(1), EntityId(2));
    s.link_child(EntityId(2), EntityId(3));
    s.link_child(EntityId(1), EntityId(4));
    s.link_child(EntityId(4), EntityId(5));

    let q = Query::new(PART)
        .filter(Condition::eq_to("kind", "root"))
        .traverse(
            Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                .descend_while(Condition::ne_to("kind", "stop")),
        );
    assert_eq!(ids(&s, &q), vec![2, 4, 5]);
}

#[test]
fn descend_while_does_not_block_the_start_node() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field("kind", "stop"));
    s.insert(Entity::new(EntityId(2), PART).with_field("kind", "leaf"));
    s.link_child(EntityId(1), EntityId(2));
    let q = Query::new(PART).traverse(
        Traversal::recursive(Edge::Child, Order::BreadthFirst, None)
            .descend_while(Condition::ne_to("kind", "stop")),
    );
    assert_eq!(ids(&s, &q), vec![2]);
}

#[test]
fn usize_converts_checked_to_value() {
    use tpt_query::Value;
    assert_eq!(Value::try_from(7usize), Ok(Value::Int(7)));
    assert!(Value::try_from(usize::MAX).is_err());
}
