//! Keeps `docs/query.schema.json` in step with the serde form of the query AST.
//! Runs with the `json` feature.

#![cfg(feature = "json")]

use serde_json::{Value as Json, json};
use tpt_query::{
    Aabb, Condition, Edge, EntityTypeId, FieldPath, Metric, Order, OrderBy, Query, Traversal, Value,
};

fn schema() -> Json {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/query.schema.json"
    ))
    .expect("read schema");
    serde_json::from_str(&text).expect("schema is JSON")
}

/// Collects every `$ref` in the document.
fn refs(v: &Json, out: &mut Vec<String>) {
    match v {
        Json::Object(map) => {
            for (k, child) in map {
                if k == "$ref" {
                    out.push(child.as_str().unwrap().to_string());
                }
                refs(child, out);
            }
        }
        Json::Array(items) => items.iter().for_each(|i| refs(i, out)),
        _ => {}
    }
}

#[test]
fn every_ref_points_at_a_definition() {
    let s = schema();
    let mut found = Vec::new();
    refs(&s, &mut found);
    assert!(!found.is_empty());
    for r in found {
        let name = r
            .strip_prefix("#/$defs/")
            .unwrap_or_else(|| panic!("unexpected ref {r}"));
        assert!(s["$defs"].get(name).is_some(), "missing definition {name}");
    }
}

#[test]
fn top_level_properties_match_the_query_form() {
    let s = schema();
    let mut schema_keys: Vec<&str> = s["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    schema_keys.sort_unstable();

    let query = Query::new(EntityTypeId(1))
        .filter(Condition::eq_to("material", "Steel"))
        .traverse(Traversal::step(Edge::Child))
        .project("mass")
        .order_by("mass", true)
        .limit(3)
        .offset(1)
        .aggregate(Metric::Count, None);
    let json = serde_json::to_value(&query).unwrap();
    let mut query_keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    query_keys.sort_unstable();
    assert_eq!(schema_keys, query_keys);
}

#[test]
fn every_condition_variant_has_a_schema_branch() {
    let s = schema();
    let branches = s["$defs"]["Condition"]["oneOf"].as_array().unwrap();
    let branch_keys: Vec<String> = branches
        .iter()
        .map(|b| b["required"][0].as_str().unwrap().to_string())
        .collect();

    let field: FieldPath = "f".into();
    let conditions = vec![
        Condition::eq_to("f", 1i64),
        Condition::ne_to("f", 1i64),
        Condition::gt("f", 1i64),
        Condition::gte("f", 1i64),
        Condition::lt("f", 1i64),
        Condition::lte("f", 1i64),
        Condition::contains("f", "x"),
        Condition::intersects("f", Aabb::new([0.0; 3], [1.0; 3])),
        Condition::all(vec![]),
        Condition::any(vec![]),
        Condition::negate(Condition::OfType(EntityTypeId(1))),
        Condition::OfType(EntityTypeId(1)),
        Condition::OfTypeIn(vec![EntityTypeId(1)]),
        Condition::ContainsPoint {
            field: "f".into(),
            point: [0.0; 3],
        },
        Condition::related(Edge::Parent, None, Condition::OfType(EntityTypeId(1))),
        Condition::In {
            field: field.clone(),
            values: vec![Value::Null],
        },
        Condition::Exists {
            field: field.clone(),
        },
        Condition::StartsWith {
            field: field.clone(),
            value: Value::from("a"),
        },
        Condition::EndsWith {
            field: field.clone(),
            value: Value::from("a"),
        },
        Condition::Between {
            field,
            low: Value::Int(0),
            high: Value::Int(1),
        },
    ];
    for c in conditions {
        let json = serde_json::to_value(&c).unwrap();
        let key = json
            .as_object()
            .and_then(|o| o.keys().next())
            .cloned()
            .unwrap_or_else(|| json.as_str().unwrap().to_string());
        assert!(
            branch_keys.contains(&key),
            "condition variant {key} has no branch in docs/query.schema.json"
        );
    }
}

#[test]
fn documented_example_parses_with_the_engine_reader() {
    // A query in the documented form parses with the crate's own reader.
    let doc = json!({
        "source": 1,
        "filters": [
            {"Between": {"field": "mass", "low": 1, "high": 9}},
            {"Exists": {"field": "material"}}
        ],
        "traversals": [{"edge": "Child", "order": "DepthFirst", "max_depth": 2}],
        "projections": [{"field": "name"}],
        "order_by": {"field": "mass", "descending": true},
        "limit": 5
    });
    let q: Query = serde_json::from_value(doc).expect("documented form parses");
    assert_eq!(
        q.order_by,
        Some(OrderBy {
            field: "mass".into(),
            descending: true
        })
    );
    assert_eq!(q.traversals[0].order, Order::DepthFirst);
}
