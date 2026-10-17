//! Flat collections: filters, ordering, limits, projection, determinism.

mod common;

use common::{Counting, FACE, PART, Rng, ids};
use tpt_query::{Condition, Entity, EntityId, MemorySource, Query, QueryError, Queryable, Value};

/// Parts 1..=n. Every third part is Titanium, the rest Steel. Mass is `i * 1.5`.
fn flat_source(n: u64) -> MemorySource {
    let mut s = MemorySource::new();
    for i in 1..=n {
        let material = if i % 3 == 0 { "Titanium" } else { "Steel" };
        s.insert(
            Entity::new(EntityId(i), PART)
                .with_field("material", material)
                .with_field("mass", i as f64 * 1.5)
                .with_field("name", format!("part-{i}").as_str())
                .with_field("qty", i as i64),
        );
    }
    s
}

#[test]
fn eq_on_string_field() {
    let s = flat_source(10);
    let q = Query::new(PART).filter(Condition::eq_to("material", "Titanium"));
    assert_eq!(ids(&s, &q), vec![3, 6, 9]);
}

#[test]
fn ne_and_not_differ_on_missing_fields() {
    let mut s = flat_source(4);
    s.insert(Entity::new(EntityId(99), PART).with_field("other", 1i64));
    // Ne requires the field to exist, so entity 99 is excluded.
    let ne = Query::new(PART).filter(Condition::ne_to("material", "Steel"));
    assert_eq!(ids(&s, &ne), vec![3]);
    // Not(Eq) is true for entity 99 because its field is missing.
    let not = Query::new(PART).filter(Condition::negate(Condition::eq_to("material", "Steel")));
    assert_eq!(ids(&s, &not), vec![3, 99]);
}

#[test]
fn comparisons_match_only_the_same_kind() {
    let s = flat_source(10);
    // mass is Float; an Int bound never matches.
    let q = Query::new(PART).filter(Condition::gt("mass", 6i64));
    assert!(ids(&s, &q).is_empty());

    let q = Query::new(PART).filter(Condition::gt("mass", 6.0));
    assert_eq!(ids(&s, &q), vec![5, 6, 7, 8, 9, 10]);

    let q = Query::new(PART).filter(Condition::lt("mass", 3.0));
    assert_eq!(ids(&s, &q), vec![1]);

    let q = Query::new(PART).filter(Condition::lte("mass", 3.0));
    assert_eq!(ids(&s, &q), vec![1, 2]);

    let q = Query::new(PART).filter(Condition::gte("qty", 9i64));
    assert_eq!(ids(&s, &q), vec![9, 10]);
}

#[test]
fn contains_on_strings_and_lists() {
    let mut s = flat_source(12);
    s.insert(
        Entity::new(EntityId(100), PART)
            .with_field("tags", Value::List(vec!["hot".into(), "heavy".into()])),
    );
    let q = Query::new(PART).filter(Condition::contains("name", "part-1"));
    assert_eq!(ids(&s, &q), vec![1, 10, 11, 12]);

    let q = Query::new(PART).filter(Condition::contains("tags", "heavy"));
    assert_eq!(ids(&s, &q), vec![100]);
}

#[test]
fn and_or_combinations() {
    let s = flat_source(10);
    let steel = Condition::eq_to("material", "Steel");
    let heavy = Condition::gte("mass", 3.0);
    let q = Query::new(PART).filter(Condition::all(vec![steel.clone(), heavy.clone()]));
    assert_eq!(ids(&s, &q), vec![2, 4, 5, 7, 8, 10]);

    let q = Query::new(PART).filter(Condition::any(vec![
        Condition::eq_to("material", "Titanium"),
        Condition::lt("mass", 2.0),
    ]));
    assert_eq!(ids(&s, &q), vec![1, 3, 6, 9]);

    // Filters on the query combine with AND as well.
    let q = Query::new(PART).filter(steel).filter(heavy);
    assert_eq!(ids(&s, &q), vec![2, 4, 5, 7, 8, 10]);
}

#[test]
fn of_type_filters_by_entity_type() {
    let mut s = flat_source(3);
    s.insert(Entity::new(EntityId(50), FACE));
    let q = Query::new(PART).filter(Condition::OfType(FACE));
    assert!(ids(&s, &q).is_empty());
    let q = Query::new(FACE);
    assert_eq!(ids(&s, &q), vec![50]);
}

#[test]
fn scan_is_in_ascending_id_order_regardless_of_insert_order() {
    let mut s = MemorySource::new();
    for i in (1..=20u64).rev() {
        s.insert(Entity::new(EntityId(i), PART));
    }
    assert_eq!(ids(&s, &Query::new(PART)), (1..=20).collect::<Vec<u64>>());
}

#[test]
fn limit_stops_work_early() {
    let s = flat_source(1000);
    let counting = Counting::new(&s);
    let q = Query::new(PART).limit(3);
    assert_eq!(ids(&counting, &q), vec![1, 2, 3]);
    assert_eq!(
        counting.gets.get(),
        3,
        "only the yielded rows should be fetched"
    );
}

#[test]
fn order_by_descending_with_id_tie_break() {
    let mut s = MemorySource::new();
    for (i, score) in [(1u64, 5i64), (2, 9), (3, 5), (4, 1)] {
        s.insert(Entity::new(EntityId(i), PART).with_field("score", score));
    }
    let q = Query::new(PART).order_by("score", true);
    assert_eq!(ids(&s, &q), vec![2, 1, 3, 4]);

    let q = Query::new(PART).order_by("score", false).limit(2);
    assert_eq!(ids(&s, &q), vec![4, 1]);
}

#[test]
fn missing_sort_field_sorts_first() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field("score", 3i64));
    s.insert(Entity::new(EntityId(2), PART));
    let q = Query::new(PART).order_by("score", false);
    assert_eq!(ids(&s, &q), vec![2, 1]);
}

#[test]
fn projection_borrows_field_values() {
    let s = flat_source(5);
    let q = Query::new(PART).project("name").project("nope");
    let rows: Vec<_> = s.execute(&q).unwrap().map(Result::unwrap).collect();
    assert_eq!(rows[2].projected[0], Some(&Value::Str("part-3".into())));
    assert_eq!(rows[2].projected[1], None);
}

#[test]
fn nested_field_paths_descend_through_maps() {
    let mut s = MemorySource::new();
    let mut material = std::collections::BTreeMap::new();
    material.insert("name".to_string(), Value::Str("Titanium".into()));
    s.insert(Entity::new(EntityId(1), PART).with_field("material", Value::Map(material)));
    let q = Query::new(PART).filter(Condition::eq_to("material.name", "Titanium"));
    assert_eq!(ids(&s, &q), vec![1]);
}

#[test]
fn execution_is_deterministic() {
    let s = flat_source(200);
    let q = Query::new(PART)
        .filter(Condition::gte("mass", 30.0))
        .order_by("material", false)
        .limit(50);
    assert_eq!(ids(&s, &q), ids(&s, &q));
}

#[test]
fn invalid_queries_are_rejected_before_execution() {
    let s = flat_source(3);
    let q = Query::new(PART).filter(Condition::eq_to("", 1i64));
    assert!(matches!(s.execute(&q), Err(QueryError::InvalidQuery(_))));

    let q = Query::new(PART).project("a..b");
    assert!(matches!(s.explain(&q), Err(QueryError::InvalidQuery(_))));
}

#[test]
fn random_filters_match_a_brute_force_check() {
    let mut rng = Rng::new(42);
    let mut s = MemorySource::new();
    let mut expected_mass = Vec::new();
    for i in 1..=300u64 {
        let mass = rng.below(100) as i64;
        expected_mass.push((i, mass));
        s.insert(Entity::new(EntityId(i), PART).with_field("mass", mass));
    }
    for threshold in [0i64, 17, 50, 99, 100] {
        let q = Query::new(PART).filter(Condition::gt("mass", threshold));
        let want: Vec<u64> = expected_mass
            .iter()
            .filter(|(_, m)| *m > threshold)
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(ids(&s, &q), want, "threshold {threshold}");
    }
}
