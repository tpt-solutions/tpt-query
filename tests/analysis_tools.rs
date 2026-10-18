//! Tools built on the query engine: `analyze`, `Schema`, the index advisor, and
//! live queries.

mod common;

use common::PART;
use tpt_query::advisor::{SuggestedKind, suggest_indexes};
use tpt_query::live::LiveQuery;
use tpt_query::schema::{FieldKind, Schema};
use tpt_query::{
    Condition, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Metric, Query, QueryError,
    Queryable, Value,
};

fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    for i in 1..=10u64 {
        let material = if i <= 2 { "rare" } else { "common" };
        s.insert(
            Entity::new(EntityId(i), PART)
                .with_field("material", material)
                .with_field("mass", Value::Int(i as i64)),
        );
    }
    s
}

#[test]
fn analyze_counts_each_stage() {
    let s = parts();
    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "common"))
        .filter(Condition::gt("mass", Value::Int(7)))
        .limit(2);
    let a = s.analyze(&q).unwrap();
    // The limit stops the run after two matches, so only the work it needed is counted.
    assert_eq!(a.read, 7, "ids 3 through 9 were read to find two matches");
    assert_eq!(a.passed_filters, 2, "mass 8 and 9");
    assert_eq!(a.after_traversal, 2);
    assert_eq!(a.returned, 2, "limit applies last");
    assert!(a.plan.unindexed.iter().any(|d| d.contains("gt on `mass`")));
}

#[test]
fn plans_say_which_conditions_no_index_answers() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::gt("mass", Value::Int(7)));
    let plan = s.explain(&q).unwrap();
    assert_eq!(plan.unindexed, vec!["gt on `mass`".to_string()]);
    assert!(plan.to_string().contains("unindexed: gt on `mass`"));
}

#[test]
fn analyze_refuses_aggregate_queries() {
    let s = parts();
    let q = Query::new(PART).aggregate(Metric::Count, None);
    assert!(matches!(s.analyze(&q), Err(QueryError::InvalidQuery(_))));
}

fn schema() -> Schema {
    Schema::new()
        .field(PART, "material", FieldKind::Str)
        .field(PART, "mass", FieldKind::Float)
        .field(PART, "grade.rev", FieldKind::Int)
        .field(PART, "bbox", FieldKind::Aabb)
}

#[test]
fn schema_accepts_declared_fields_of_the_right_kind() {
    let ok = Query::new(PART)
        .filter(Condition::eq_to("material", "Steel"))
        .filter(Condition::gt("mass", 2.5))
        .filter(Condition::eq_to("grade.rev", Value::Int(2)));
    assert!(schema().validate_against(&ok).is_ok());
}

#[test]
fn schema_rejects_unknown_fields_and_type_mixups() {
    let typo = Query::new(PART).filter(Condition::eq_to("materail", "Steel"));
    let err = schema().validate_against(&typo).unwrap_err();
    assert!(matches!(&err, QueryError::InvalidQuery(m) if m.contains("materail")));

    // A whole-number literal against a float field is the classic mix-up.
    let mixed = Query::new(PART).filter(Condition::eq_to("mass", Value::Int(2)));
    let err = schema().validate_against(&mixed).unwrap_err();
    assert!(matches!(&err, QueryError::InvalidQuery(m) if m.contains("Float")));

    let unknown_type = Query::new(EntityTypeId(99));
    assert!(schema().validate_against(&unknown_type).is_err());
}

#[test]
fn schema_checks_projection_and_box_fields() {
    let bad_projection = Query::new(PART).project("colour");
    assert!(schema().validate_against(&bad_projection).is_err());

    let bad_box =
        Query::new(PART).filter(Condition::intersects("material", tpt_query::Aabb::EMPTY));
    assert!(schema().validate_against(&bad_box).is_err());
}

#[test]
fn advisor_prefers_btree_for_fields_with_ranges() {
    let workload = vec![
        Query::new(PART).filter(Condition::eq_to("material", "Steel")),
        Query::new(PART).filter(Condition::eq_to("material", "Copper")),
        Query::new(PART).filter(Condition::gt("mass", 1.0)),
        Query::new(PART).filter(Condition::all(vec![
            Condition::gte("mass", 2.0),
            Condition::eq_to("material", "Steel"),
        ])),
    ];
    let suggestions = suggest_indexes(&workload);
    assert_eq!(suggestions.len(), 2);
    assert_eq!(suggestions[0].field.to_string(), "material");
    assert_eq!(suggestions[0].kind, SuggestedKind::Hash);
    assert_eq!(suggestions[0].queries, 3);
    assert_eq!(suggestions[1].field.to_string(), "mass");
    assert_eq!(suggestions[1].kind, SuggestedKind::BTree);
    assert_eq!(suggestions[1].queries, 2);
}

#[test]
fn live_query_reports_added_and_removed_ids() {
    let mut s = parts();
    let mut live = LiveQuery::new(Query::new(PART).filter(Condition::eq_to("material", "rare")));

    let first = live.refresh(&s).unwrap();
    assert_eq!(first.added, vec![EntityId(1), EntityId(2)]);
    assert!(first.removed.is_empty());

    s.insert(Entity::new(EntityId(11), PART).with_field("material", "rare"));
    let second = live.refresh(&s).unwrap();
    assert_eq!(second.added, vec![EntityId(11)]);
    assert!(second.removed.is_empty());

    s.remove(EntityId(1));
    let third = live.refresh(&s).unwrap();
    assert!(third.added.is_empty());
    assert_eq!(third.removed, vec![EntityId(1)]);
    assert_eq!(
        live.current().collect::<Vec<_>>(),
        vec![EntityId(2), EntityId(11)]
    );
}
