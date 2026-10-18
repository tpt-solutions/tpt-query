//! Random condition trees, checked against a brute-force evaluator. Each generated
//! query runs on a source with a B-tree index on `a`, a hash index on `b`, and a
//! composite index on both, so the planner takes index and scan paths, and nested
//! `And`/`Or`/`Not` cases are covered. The oracle reads the entities directly, with
//! no engine code.

mod common;

use common::{PART, ids};
use proptest::prelude::*;
use tpt_query::{Condition, Entity, EntityId, FieldPath, IndexKind, MemorySource, Query, Value};

const TEXT: [&str; 3] = ["x", "y", "z"];

fn leaf() -> impl Strategy<Value = Condition> {
    prop_oneof![
        (0i64..10).prop_map(|v| Condition::eq_to("a", v)),
        (0i64..10).prop_map(|v| Condition::Ne {
            field: "a".into(),
            value: Value::Int(v),
        }),
        (0i64..10).prop_map(|v| Condition::gt("a", v)),
        (0i64..10).prop_map(|v| Condition::Lt {
            field: "a".into(),
            value: Value::Int(v),
        }),
        (0i64..10, 0i64..10).prop_map(|(l, h)| Condition::Between {
            field: "a".into(),
            low: Value::Int(l.min(h)),
            high: Value::Int(l.max(h)),
        }),
        prop::collection::vec(0i64..10, 1..3).prop_map(|vs| Condition::In {
            field: "a".into(),
            values: vs.into_iter().map(Value::Int).collect(),
        }),
        prop::sample::select(TEXT.to_vec()).prop_map(|s| Condition::eq_to("b", s)),
        Just(Condition::Exists { field: "a".into() }),
        Just(Condition::Exists { field: "b".into() }),
    ]
}

fn condition() -> impl Strategy<Value = Condition> {
    leaf().prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..3).prop_map(Condition::And),
            prop::collection::vec(inner.clone(), 1..3).prop_map(Condition::Or),
            inner.prop_map(|c| Condition::Not(Box::new(c))),
        ]
    })
}

type Row = (Option<i64>, Option<&'static str>);

fn rows() -> impl Strategy<Value = Vec<Row>> {
    prop::collection::vec(
        (
            prop::option::of(0i64..10),
            prop::option::of(prop::sample::select(TEXT.to_vec())),
        ),
        0..40,
    )
}

/// The value of `field` for one row, or `None` if the row lacks it.
fn value_of(field: &FieldPath, row: Row) -> Option<Value> {
    match field.to_string().as_str() {
        "a" => row.0.map(Value::Int),
        "b" => row.1.map(Value::from),
        _ => None,
    }
}

fn int_compare(field: &FieldPath, row: Row, v: &Value, ok: fn(i64, i64) -> bool) -> bool {
    match (value_of(field, row), v) {
        (Some(Value::Int(x)), Value::Int(y)) => ok(x, *y),
        _ => false,
    }
}

/// Brute-force truth of `c` for one row. Written from the documented semantics:
/// `Ne` needs the field, `Not` is plain negation, and a missing field fails `Eq`.
fn holds(c: &Condition, row: Row) -> bool {
    match c {
        Condition::Eq { field, value } => value_of(field, row).is_some_and(|x| x == *value),
        Condition::Ne { field, value } => value_of(field, row).is_some_and(|x| x != *value),
        Condition::Gt { field, value } => int_compare(field, row, value, |x, y| x > y),
        Condition::Lt { field, value } => int_compare(field, row, value, |x, y| x < y),
        Condition::Between { field, low, high } => {
            int_compare(field, row, low, |x, y| x >= y)
                && int_compare(field, row, high, |x, y| x <= y)
        }
        Condition::In { field, values } => {
            value_of(field, row).is_some_and(|x| values.contains(&x))
        }
        Condition::Exists { field } => value_of(field, row).is_some(),
        Condition::And(cs) => cs.iter().all(|c| holds(c, row)),
        Condition::Or(cs) => cs.iter().any(|c| holds(c, row)),
        Condition::Not(c) => !holds(c, row),
        other => panic!("the generator does not produce {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn indexed_engine_matches_brute_force(rows in rows(), cond in condition()) {
        let mut s = MemorySource::new();
        s.create_index("a_btree", PART, "a", IndexKind::BTree).unwrap();
        s.create_index("b_hash", PART, "b", IndexKind::Hash).unwrap();
        s.create_composite_index("a_and_b", PART, &["a", "b"]).unwrap();
        let mut want = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let id = i as u64 + 1;
            let mut e = Entity::new(EntityId(id), PART);
            if let Some(a) = row.0 {
                e = e.with_field("a", a);
            }
            if let Some(b) = row.1 {
                e = e.with_field("b", b);
            }
            s.insert(e);
            if holds(&cond, *row) {
                want.push(id);
            }
        }
        let q = Query::new(PART).filter(cond.clone());
        prop_assert_eq!(ids(&s, &q), want, "condition {:?}", cond);
    }
}
