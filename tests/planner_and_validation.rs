//! Planner, index management, validation, and ordering behaviour.

mod common;

use common::{PART, ids};
use tpt_query::{
    Aabb, Access, Condition, Edge, Entity, EntityId, IndexKind, MemorySource, Order, Query,
    QueryError, Queryable, Traversal, Value,
};

/// 100 parts: "common" material except ids 1 and 2, which are "rare". Mass is 1..=100.
fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    s.create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap();
    for i in 1..=100u64 {
        let material = if i <= 2 { "rare" } else { "common" };
        s.insert(
            Entity::new(EntityId(i), PART)
                .with_field("material", material)
                .with_field("mass", Value::Int(i as i64)),
        );
    }
    s
}

fn used_index(s: &MemorySource, q: &Query) -> String {
    match s.explain(q).unwrap().access {
        Access::Index { index, .. } => index,
        Access::Scan { .. } => String::from("scan"),
        _ => String::from("other"),
    }
}

#[test]
fn and_conjuncts_are_indexed_and_the_smallest_candidate_set_wins() {
    let s = parts();

    // Mass > 90 has 10 candidates. The material index has 98 for "common".
    let common = Query::new(PART).filter(Condition::all(vec![
        Condition::eq_to("material", "common"),
        Condition::gt("mass", Value::Int(90)),
    ]));
    assert_eq!(used_index(&s, &common), "by_mass");

    // The material index has 2 candidates for "rare". Mass > 0 has 100.
    let rare = Query::new(PART).filter(Condition::all(vec![
        Condition::eq_to("material", "rare"),
        Condition::gt("mass", Value::Int(0)),
    ]));
    assert_eq!(used_index(&s, &rare), "by_material");

    let got = ids(&s, &common);
    assert_eq!(got, (91..=100).collect::<Vec<u64>>());
    assert_eq!(ids(&s, &rare), vec![1, 2]);
}

#[test]
fn range_over_many_keys_returns_ascending_unique_ids() {
    let s = parts();
    // Gte 10 merges 91 keys. Each entity appears once, in ascending order.
    let q = Query::new(PART).filter(Condition::gte("mass", Value::Int(10)));
    let got = ids(&s, &q);
    assert_eq!(got, (10..=100).collect::<Vec<u64>>());
}

#[test]
fn explain_reports_candidate_count_without_reading_candidates() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::eq_to("material", "common"));
    match s.explain(&q).unwrap().access {
        Access::Index { index, candidates } => {
            assert_eq!(index, "by_material");
            assert_eq!(candidates, 98);
        }
        other => panic!("expected an index access, got {other:?}"),
    }
}

#[test]
fn duplicate_index_names_are_rejected_and_indexes_can_be_dropped() {
    let mut s = parts();
    let err = s
        .create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap_err();
    assert_eq!(err, QueryError::DuplicateIndex(String::from("by_mass")));

    assert!(s.drop_index("by_mass"));
    assert!(!s.drop_index("by_mass"));

    // With the index gone, the same query falls back to a scan and still answers.
    let q = Query::new(PART).filter(Condition::gt("mass", Value::Int(98)));
    assert_eq!(used_index(&s, &q), "scan");
    assert_eq!(ids(&s, &q), vec![99, 100]);

    // The name is free again after dropping it.
    s.create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap();
}

#[test]
fn non_finite_numbers_are_rejected_by_validation() {
    let nan = Query::new(PART).filter(Condition::gt("mass", f64::NAN));
    assert!(matches!(nan.validate(), Err(QueryError::InvalidQuery(_))));

    let inf = Query::new(PART).filter(Condition::intersects("bbox", Aabb::EMPTY));
    assert!(matches!(inf.validate(), Err(QueryError::InvalidQuery(_))));

    let nested = Query::new(PART).filter(Condition::eq_to(
        "tags",
        Value::List(vec![Value::Float(f64::INFINITY)]),
    ));
    assert!(matches!(
        nested.validate(),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[cfg(feature = "json")]
#[test]
fn non_finite_numbers_do_not_serialize() {
    use tpt_query::json::{query_from_json, query_to_json};

    let q = Query::new(PART).filter(Condition::intersects("bbox", Aabb::EMPTY));
    assert!(query_to_json(&q).is_err());

    let finite = Query::new(PART).filter(Condition::gt("mass", 2.5));
    let round = query_from_json(&query_to_json(&finite).unwrap()).unwrap();
    assert_eq!(round, finite);
}

#[test]
fn order_by_with_limit_matches_a_full_sort() {
    let mut s = MemorySource::new();
    for i in 1..=50u64 {
        // Masses repeat, so the id tie-break is exercised.
        let mass = (i * 37 % 11) as i64;
        s.insert(Entity::new(EntityId(i), PART).with_field("mass", Value::Int(mass)));
    }

    for descending in [false, true] {
        let mut expected: Vec<(i64, u64)> =
            (1..=50u64).map(|i| ((i * 37 % 11) as i64, i)).collect();
        expected.sort_by(|a, b| {
            let ord = a.0.cmp(&b.0);
            let ord = if descending { ord.reverse() } else { ord };
            ord.then(a.1.cmp(&b.1))
        });

        let q = Query::new(PART).order_by("mass", descending).limit(7);
        let got = ids(&s, &q);
        let want: Vec<u64> = expected.iter().take(7).map(|&(_, id)| id).collect();
        assert_eq!(got, want, "descending = {descending}");
    }
}

#[test]
fn traversal_filter_prunes_yields_but_not_the_walk() {
    // A -> B -> C. B fails the filter, but C is still reached through it.
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field("name", "A"));
    s.insert(Entity::new(EntityId(2), PART).with_field("name", "B"));
    s.insert(Entity::new(EntityId(3), PART).with_field("name", "C"));
    s.link_child(EntityId(1), EntityId(2));
    s.link_child(EntityId(2), EntityId(3));

    let q = Query::new(PART)
        .filter(Condition::eq_to("name", "A"))
        .traverse(
            Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                .filter(Condition::eq_to("name", "C")),
        );
    assert_eq!(ids(&s, &q), vec![3]);
}

#[test]
fn row_get_and_typed_value_getters() {
    let mut s = MemorySource::new();
    let mut spec = std::collections::BTreeMap::new();
    spec.insert(String::from("grade"), Value::from("Ti-6Al-4V"));
    s.insert(
        Entity::new(EntityId(1), PART)
            .with_field("mass", Value::Int(7))
            .with_field("material", Value::Map(spec)),
    );
    let q = Query::new(PART);
    let row = s.execute(&q).unwrap().next().unwrap().unwrap();

    assert_eq!(row.get("mass").and_then(Value::as_i64), Some(7));
    assert_eq!(
        row.get("material.grade").and_then(Value::as_str),
        Some("Ti-6Al-4V")
    );
    assert_eq!(row.get("mass").and_then(Value::as_str), None);
    assert!(row.get("missing").is_none());
    assert_eq!(Value::from(2.5).as_f64(), Some(2.5));
    assert_eq!(Value::from(true).as_bool(), Some(true));
    assert_eq!(Value::from(3u32), Value::Int(3));
}
