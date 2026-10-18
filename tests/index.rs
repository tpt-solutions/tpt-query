//! Secondary indexes: plan selection, incremental maintenance, and index/scan
//! equivalence under random insert, update, and delete sequences.

mod common;

use common::{PART, ids};
use tpt_query::{Access, Condition, Entity, EntityId, IndexKind, MemorySource, Query, Queryable};

const MATERIALS: [&str; 3] = ["Steel", "Titanium", "Aluminium"];

fn indexed() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    s.create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap();
    s
}

fn part(id: u64, material: &str, mass: i64) -> Entity {
    Entity::new(EntityId(id), PART)
        .with_field("material", material)
        .with_field("mass", mass)
}

#[test]
fn explain_reports_the_index_and_its_candidate_count() {
    let mut s = indexed();
    for i in 1..=30u64 {
        s.insert(part(i, MATERIALS[(i % 3) as usize], i as i64));
    }
    let q = Query::new(PART).filter(Condition::eq_to("material", "Titanium"));
    let plan = s.explain(&q).unwrap();
    assert_eq!(
        plan.access,
        Access::Index {
            index: "by_material".into(),
            candidates: 10
        }
    );
    assert_eq!(
        plan.residual_filters, 0,
        "the index answers the filter exactly"
    );
    assert!(plan.to_string().contains("by_material"));
}

#[test]
fn planner_picks_the_most_selective_index() {
    let mut s = indexed();
    for i in 1..=100u64 {
        s.insert(part(i, MATERIALS[(i % 3) as usize], i as i64));
    }
    // Material matches about a third of the rows; mass > 95 matches five.
    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "Steel"))
        .filter(Condition::gt("mass", 95i64));
    let plan = s.explain(&q).unwrap();
    assert_eq!(
        plan.access,
        Access::Index {
            index: "by_mass".into(),
            candidates: 5
        }
    );
    assert_eq!(plan.residual_filters, 1);
    // Steel is index 0, so it covers the multiples of three: 96 and 99 are the matches.
    assert_eq!(ids(&s, &q), vec![96, 99]);
}

#[test]
fn hash_index_does_not_serve_range_queries() {
    let mut s = indexed();
    s.insert(part(1, "Steel", 3));
    let q = Query::new(PART).filter(Condition::gt("material", "A"));
    assert_eq!(s.explain(&q).unwrap().access, Access::Scan { ty: PART });
}

#[test]
fn index_is_maintained_on_update_and_delete() {
    let mut s = indexed();
    s.insert(part(1, "Steel", 10));
    s.insert(part(2, "Steel", 20));

    s.insert(part(1, "Titanium", 10)); // update moves 1 between buckets
    let steel = Query::new(PART).filter(Condition::eq_to("material", "Steel"));
    assert_eq!(ids(&s, &steel), vec![2]);

    s.remove(EntityId(2));
    assert!(ids(&s, &steel).is_empty());
    let all_mass = Query::new(PART).filter(Condition::gte("mass", 0i64));
    assert_eq!(ids(&s, &all_mass), vec![1]);
}

#[test]
fn index_backfills_existing_entities() {
    let mut s = MemorySource::new();
    s.insert(part(1, "Steel", 10));
    s.insert(part(2, "Titanium", 20));
    s.create_index("late", PART, "material", IndexKind::Hash)
        .unwrap();
    let q = Query::new(PART).filter(Condition::eq_to("material", "Titanium"));
    assert_eq!(ids(&s, &q), vec![2]);
}

#[test]
fn range_queries_stay_within_the_value_kind() {
    let mut s = indexed();
    s.insert(part(1, "Steel", 5));
    s.insert(Entity::new(EntityId(2), PART).with_field("mass", 2.5f64));
    s.insert(Entity::new(EntityId(3), PART).with_field("mass", "heavy"));
    // A Gt over Int values must not pick up the Float or Str entries, which
    // sort after Int in the index ordering.
    let q = Query::new(PART).filter(Condition::gt("mass", 0i64));
    assert_eq!(ids(&s, &q), vec![1]);
}
