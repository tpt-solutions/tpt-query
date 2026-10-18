//! Composite (multi-field) indexes: equality on every indexed field is answered by
//! one lookup, and the planner picks it by candidate count.

mod common;

use common::{PART, ids};
use tpt_query::{
    Condition, Entity, EntityId, IndexKind, MemorySource, Query, QueryError, Queryable,
};

fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_composite_index("by_material_kind", PART, &["material", "kind"])
        .unwrap();
    let rows = [
        (1, "steel", "bolt"),
        (2, "steel", "nut"),
        (3, "copper", "bolt"),
        (4, "steel", "bolt"),
    ];
    for (id, material, kind) in rows {
        s.insert(
            Entity::new(EntityId(id), PART)
                .with_field("material", material)
                .with_field("kind", kind),
        );
    }
    s
}

fn steel_bolts() -> Query {
    Query::new(PART).filter(Condition::all(vec![
        Condition::eq_to("material", "steel"),
        Condition::eq_to("kind", "bolt"),
    ]))
}

#[test]
fn equality_on_every_field_uses_the_composite_index() {
    let s = parts();
    let q = steel_bolts();
    assert_eq!(ids(&s, &q), vec![1, 4]);
    let plan = s.explain(&q).unwrap();
    assert!(
        plan.to_string()
            .contains("index `by_material_kind` (2 candidates)")
    );
    assert_eq!(plan.residual_filters, 0);
}

#[test]
fn partial_equality_falls_back_to_other_paths() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::eq_to("material", "steel"));
    // The composite needs both fields, so it cannot answer this alone.
    assert_eq!(ids(&s, &q), vec![1, 2, 4]);
    let plan = s.explain(&q).unwrap();
    assert!(plan.to_string().contains("scan entities"));
}

#[test]
fn extra_conjuncts_on_indexed_fields_stay_residual() {
    let s = parts();
    // A second condition on `kind` must still be checked after the lookup.
    let q = Query::new(PART).filter(Condition::all(vec![
        Condition::eq_to("material", "steel"),
        Condition::eq_to("kind", "bolt"),
        Condition::Ne {
            field: "kind".into(),
            value: "bolt".into(),
        },
    ]));
    assert!(ids(&s, &q).is_empty());
    assert_eq!(s.explain(&q).unwrap().residual_filters, 1);
}

#[test]
fn single_field_index_wins_when_it_has_fewer_candidates() {
    let mut s = parts();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    // Only one copper part, so the single-field index is not the better choice here.
    let q = Query::new(PART).filter(Condition::all(vec![
        Condition::eq_to("material", "copper"),
        Condition::eq_to("kind", "bolt"),
    ]));
    assert_eq!(ids(&s, &q), vec![3]);
    assert!(s.explain(&q).unwrap().to_string().contains("by_material"));
}

#[test]
fn updates_and_removals_keep_the_index_exact() {
    let mut s = parts();
    // Entity 1 changes kind: it leaves the (steel, bolt) key.
    s.insert(
        Entity::new(EntityId(1), PART)
            .with_field("material", "steel")
            .with_field("kind", "pin"),
    );
    assert_eq!(ids(&s, &steel_bolts()), vec![4]);
    assert!(s.remove(EntityId(4)).is_some());
    assert!(ids(&s, &steel_bolts()).is_empty());
}

#[test]
fn entities_missing_a_field_are_not_indexed() {
    let mut s = parts();
    s.insert(Entity::new(EntityId(9), PART).with_field("material", "steel"));
    // Entity 9 has no `kind`, so no equality can match it, indexed or not.
    assert_eq!(ids(&s, &steel_bolts()), vec![1, 4]);
}

#[test]
fn backfills_existing_entities() {
    let mut s = MemorySource::new();
    s.insert(
        Entity::new(EntityId(1), PART)
            .with_field("material", "steel")
            .with_field("kind", "bolt"),
    );
    s.create_composite_index("late", PART, &["material", "kind"])
        .unwrap();
    assert_eq!(ids(&s, &steel_bolts()), vec![1]);
    assert!(
        s.explain(&steel_bolts())
            .unwrap()
            .to_string()
            .contains("late")
    );
}

#[test]
fn names_are_unique_across_index_kinds_and_drop_works() {
    let mut s = parts();
    assert!(matches!(
        s.create_composite_index("by_material_kind", PART, &["kind"]),
        Err(QueryError::DuplicateIndex(_))
    ));
    assert!(matches!(
        s.create_index("by_material_kind", PART, "kind", IndexKind::Hash),
        Err(QueryError::DuplicateIndex(_))
    ));
    assert!(matches!(
        s.create_composite_index("empty", PART, &[]),
        Err(QueryError::InvalidQuery(_))
    ));
    assert!(s.drop_index("by_material_kind"));
    assert!(!s.drop_index("by_material_kind"));
    assert_eq!(ids(&s, &steel_bolts()), vec![1, 4]);
    assert!(
        s.explain(&steel_bolts())
            .unwrap()
            .to_string()
            .contains("scan")
    );
}
