//! Set, presence, string, and range conditions; offset and keyset paging; JSON rows.

mod common;

use common::{PART, ids};
use tpt_query::{Condition, Entity, EntityId, MemorySource, Query, QueryError, Value};

fn parts() -> MemorySource {
    let mut s = MemorySource::new();
    let rows = [
        (1u64, "Titanium", 12.5f64, "bracket-main"),
        (2, "Steel", 0.4, "bolt-m4"),
        (3, "Aluminium", 3.25, "plate-top"),
        (4, "Steel", 0.05, "washer-s"),
        (5, "Copper", 8.0, "bus-bar"),
    ];
    for (id, material, mass, name) in rows {
        s.insert(
            Entity::new(EntityId(id), PART)
                .with_field("material", material)
                .with_field("mass", mass)
                .with_field("name", name),
        );
    }
    s
}

#[test]
fn in_matches_any_listed_value() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::In {
        field: "material".into(),
        values: vec![Value::from("Steel"), Value::from("Copper")],
    });
    assert_eq!(ids(&s, &q), vec![2, 4, 5]);
}

#[test]
fn exists_treats_null_as_absent() {
    let mut s = parts();
    s.insert(Entity::new(EntityId(6), PART).with_field("material", Value::Null));
    let present = Query::new(PART).filter(Condition::Exists {
        field: "material".into(),
    });
    assert_eq!(ids(&s, &present), vec![1, 2, 3, 4, 5]);

    // Missing and null fields are both "not exists", so `Not(Exists)` finds them.
    let absent = Query::new(PART).filter(Condition::negate(Condition::Exists {
        field: "material".into(),
    }));
    assert_eq!(ids(&s, &absent), vec![6]);
}

#[test]
fn starts_with_and_ends_with_match_strings_only() {
    let s = parts();
    let starts = Query::new(PART).filter(Condition::StartsWith {
        field: "name".into(),
        value: Value::from("bolt"),
    });
    assert_eq!(ids(&s, &starts), vec![2]);

    let ends = Query::new(PART).filter(Condition::EndsWith {
        field: "name".into(),
        value: Value::from("-s"),
    });
    assert_eq!(ids(&s, &ends), vec![4]);

    // A non-string field never matches.
    let on_number = Query::new(PART).filter(Condition::StartsWith {
        field: "mass".into(),
        value: Value::from("1"),
    });
    assert!(ids(&s, &on_number).is_empty());
}

#[test]
fn between_is_inclusive_on_both_ends() {
    let s = parts();
    let q = Query::new(PART).filter(Condition::Between {
        field: "mass".into(),
        low: Value::Float(0.4),
        high: Value::Float(8.0),
    });
    assert_eq!(ids(&s, &q), vec![2, 3, 5]);
}

#[test]
fn between_rejects_mixed_kinds_and_bad_string_bounds() {
    let mixed = Query::new(PART).filter(Condition::Between {
        field: "mass".into(),
        low: Value::Int(1),
        high: Value::Float(2.0),
    });
    assert!(matches!(mixed.validate(), Err(QueryError::InvalidQuery(_))));

    let bad_prefix = Query::new(PART).filter(Condition::StartsWith {
        field: "name".into(),
        value: Value::Int(1),
    });
    assert!(matches!(
        bad_prefix.validate(),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[test]
fn offset_skips_rows_before_limit() {
    let s = parts();
    let q = Query::new(PART).offset(1).limit(2);
    assert_eq!(ids(&s, &q), vec![2, 3]);
}

#[test]
fn offset_and_limit_pages_cover_every_row_once() {
    let s = parts();
    let mut all = Vec::new();
    for page in 0..3 {
        let q = Query::new(PART).offset(page * 2).limit(2);
        all.extend(ids(&s, &q));
    }
    assert_eq!(all, vec![1, 2, 3, 4, 5]);
}

#[test]
fn keyset_cursor_resumes_after_the_last_id() {
    let s = parts();
    let first = ids(&s, &Query::new(PART).limit(2));
    assert_eq!(first, vec![1, 2]);

    let next = ids(&s, &Query::new(PART).after(EntityId(2)).limit(2));
    assert_eq!(next, vec![3, 4]);

    let last = ids(&s, &Query::new(PART).after(EntityId(4)).limit(2));
    assert_eq!(last, vec![5]);
}

#[test]
fn keyset_cursor_is_rejected_with_order_by() {
    let q = Query::new(PART).after(EntityId(1)).order_by("mass", true);
    assert!(matches!(q.validate(), Err(QueryError::InvalidQuery(_))));
}

#[cfg(feature = "json")]
#[test]
fn rows_serialize_to_json_with_projected_keys() {
    use tpt_query::json::{execute_json, query_from_json};

    let s = parts();
    let q = query_from_json(
        r#"{
            "source": 1,
            "filters": [{"Eq": {"field": "material", "value": "Steel"}}],
            "projections": [{"field": "name"}, {"field": "mass"}],
            "order_by": null,
            "limit": 1
        }"#,
    )
    .unwrap();

    let out: serde_json::Value = serde_json::from_str(&execute_json(&s, &q).unwrap()).unwrap();
    assert_eq!(
        out,
        serde_json::json!([{"id": 2, "name": "bolt-m4", "mass": 0.4}])
    );
}

#[cfg(feature = "json")]
#[test]
fn plain_json_values_work_in_queries() {
    use tpt_query::json::query_from_json;

    let json = r#"{
        "source": 1,
        "filters": [
            {"Eq": {"field": "material", "value": "Steel"}},
            {"Gt": {"field": "mass", "value": 0.1}}
        ]
    }"#;
    let q = query_from_json(json).unwrap();
    let s = parts();
    assert_eq!(ids(&s, &q), vec![2]);
}
