//! Aggregation as part of the query: grouped and ungrouped reductions, the split
//! between `execute` and `aggregate`, and validation.

mod common;

use common::PART;
use tpt_query::{
    Condition, Entity, EntityId, MemorySource, Metric, Query, QueryError, Queryable, Value,
};

fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    for (id, material, mass) in [
        (1u64, "Titanium", 12.5f64),
        (2, "Steel", 0.4),
        (3, "Aluminium", 3.25),
        (4, "Steel", 0.05),
        (5, "Copper", 8.0),
    ] {
        s.insert(
            Entity::new(EntityId(id), PART)
                .with_field("material", material)
                .with_field("mass", mass),
        );
    }
    s
}

fn close(v: &Value, want: f64) -> bool {
    v.as_f64().is_some_and(|f| (f - want).abs() < 1e-9)
}

#[test]
fn group_by_sums_each_group_in_key_order() {
    let s = parts();
    let q = Query::new(PART).aggregate(Metric::Sum("mass".into()), Some("material".into()));
    let groups = s.aggregate(&q).unwrap();
    let keys: Vec<&str> = groups.iter().map(|(k, _)| k.as_str().unwrap()).collect();
    assert_eq!(keys, vec!["Aluminium", "Copper", "Steel", "Titanium"]);
    assert!(close(&groups[0].1, 3.25));
    assert!(close(&groups[2].1, 0.45));
}

#[test]
fn ungrouped_aggregate_returns_one_pair_with_a_null_key() {
    let s = parts();
    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "Steel"))
        .aggregate(Metric::Count, None);
    assert_eq!(s.aggregate(&q).unwrap(), vec![(Value::Null, Value::Int(2))]);
}

#[test]
fn execute_refuses_aggregate_queries_and_aggregate_refuses_row_queries() {
    let s = parts();
    let agg = Query::new(PART).aggregate(Metric::Count, None);
    assert!(matches!(s.execute(&agg), Err(QueryError::InvalidQuery(_))));

    let rows = Query::new(PART);
    assert!(matches!(
        s.aggregate(&rows),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[test]
fn aggregate_queries_reject_row_only_clauses() {
    let with_limit = Query::new(PART).aggregate(Metric::Count, None).limit(3);
    assert!(matches!(
        with_limit.validate(),
        Err(QueryError::InvalidQuery(_))
    ));

    let with_projection = Query::new(PART)
        .project("mass")
        .aggregate(Metric::Count, None);
    assert!(matches!(
        with_projection.validate(),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[cfg(feature = "json")]
#[test]
fn aggregate_round_trips_through_json_and_runs() {
    use tpt_query::json::{aggregate_json, query_from_json};

    let s = parts();
    let q = query_from_json(
        r#"{
            "source": 1,
            "aggregate": {"metric": {"Max": "mass"}, "group_by": "material"}
        }"#,
    )
    .unwrap();
    let out: serde_json::Value = serde_json::from_str(&aggregate_json(&s, &q).unwrap()).unwrap();
    assert_eq!(
        out[0],
        serde_json::json!({"key": "Aluminium", "value": 3.25})
    );
    assert_eq!(
        out[3],
        serde_json::json!({"key": "Titanium", "value": 12.5})
    );
}
