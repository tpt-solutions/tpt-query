//! Streaming aggregation: count, sum, min, max, avg, and group-by.

mod common;

use common::PART;
use tpt_query::FieldPath;
use tpt_query::{
    Condition, Entity, EntityId, MemorySource, Metric, Query, Queryable, Value, aggregate, group_by,
};

fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    for i in 1..=10u64 {
        let material = if i % 3 == 0 { "Titanium" } else { "Steel" };
        s.insert(
            Entity::new(EntityId(i), PART)
                .with_field("material", material)
                .with_field("qty", i as i64)
                .with_field("mass", i as f64 * 1.5),
        );
    }
    s
}

#[test]
fn count_sum_min_max_avg() {
    let s = parts();
    let all = Query::new(PART);

    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Count).unwrap(),
        Value::Int(10)
    );
    // Integer sums stay integers.
    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Sum("qty".into())).unwrap(),
        Value::Int(55)
    );
    // Float sums become floats.
    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Sum("mass".into())).unwrap(),
        Value::Float(82.5)
    );
    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Min("qty".into())).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Max("mass".into())).unwrap(),
        Value::Float(15.0)
    );
    assert_eq!(
        aggregate(s.execute(&all).unwrap(), &Metric::Avg("qty".into())).unwrap(),
        Value::Float(5.5)
    );
}

#[test]
fn aggregates_over_a_filtered_stream() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::eq_to("material", "Titanium"));
    assert_eq!(
        aggregate(s.execute(&q).unwrap(), &Metric::Count).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        aggregate(s.execute(&q).unwrap(), &Metric::Sum("qty".into())).unwrap(),
        Value::Int(18)
    );
}

#[test]
fn empty_inputs_have_defined_results() {
    let s = parts();
    let none = Query::new(PART).filter(Condition::eq_to("material", "Gold"));
    assert_eq!(
        aggregate(s.execute(&none).unwrap(), &Metric::Count).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        aggregate(s.execute(&none).unwrap(), &Metric::Sum("qty".into())).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        aggregate(s.execute(&none).unwrap(), &Metric::Min("qty".into())).unwrap(),
        Value::Null
    );
    assert_eq!(
        aggregate(s.execute(&none).unwrap(), &Metric::Avg("qty".into())).unwrap(),
        Value::Null
    );
}

#[test]
fn group_by_reduces_each_group_in_key_order() {
    let s = parts();
    let q = Query::new(PART);
    let groups = group_by(
        s.execute(&q).unwrap(),
        &FieldPath::from("material"),
        &Metric::Count,
    )
    .unwrap();
    assert_eq!(
        groups,
        vec![
            (Value::Str("Steel".into()), Value::Int(7)),
            (Value::Str("Titanium".into()), Value::Int(3)),
        ]
    );

    let by_material_qty = group_by(
        s.execute(&q).unwrap(),
        &FieldPath::from("material"),
        &Metric::Sum("qty".into()),
    )
    .unwrap();
    assert_eq!(
        by_material_qty[1],
        (Value::Str("Titanium".into()), Value::Int(18))
    );
}

#[test]
fn missing_group_key_falls_into_a_null_group() {
    let mut s = parts();
    s.insert(Entity::new(EntityId(50), PART).with_field("qty", 100i64));
    let q = Query::new(PART);
    let groups = group_by(
        s.execute(&q).unwrap(),
        &FieldPath::from("material"),
        &Metric::Count,
    )
    .unwrap();
    assert_eq!(groups[0], (Value::Null, Value::Int(1)));
}
