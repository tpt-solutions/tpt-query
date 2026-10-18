//! The text form: parsing, printing, and round trips over every condition kind.

mod common;

use tpt_query::aggregate::Metric;
use tpt_query::dsl::parse;
use tpt_query::{
    Aabb, Condition, Edge, EntityId, EntityTypeId, Order, Query, QueryError, Traversal, Value,
};

const PART: EntityTypeId = EntityTypeId(1);

fn round_trip(q: &Query) {
    let text = q.to_string();
    let back = parse(&text).unwrap_or_else(|e| panic!("`{text}` does not parse: {e}"));
    assert_eq!(&back, q, "round trip through `{text}`");
}

#[test]
fn documented_example_parses_to_the_expected_query() {
    let q = parse(
        r#"from 1
             where material = "Steel" and mass > 2.5
             traverse child depth 2 order bfs filter (kind = "face")
             select name, mass
             order by mass desc
             offset 10 limit 5"#,
    )
    .unwrap();

    assert_eq!(q.source, PART);
    assert_eq!(
        q.filters,
        vec![Condition::And(vec![
            Condition::eq_to("material", "Steel"),
            Condition::gt("mass", 2.5),
        ])]
    );
    assert_eq!(q.traversals.len(), 1);
    assert_eq!(q.traversals[0].edge, Edge::Child);
    assert_eq!(q.traversals[0].max_depth, Some(2));
    assert_eq!(q.traversals[0].order, Order::BreadthFirst);
    assert_eq!(
        q.traversals[0].filters,
        vec![Condition::eq_to("kind", "face")]
    );
    assert_eq!(q.projections.len(), 2);
    assert!(q.order_by.as_ref().unwrap().descending);
    assert_eq!((q.offset, q.limit), (10, Some(5)));
}

#[test]
fn and_binds_tighter_than_or() {
    let q = parse("from 1 where a = 1 or b = 2 and c = 3").unwrap();
    assert_eq!(
        q.filters[0],
        Condition::Or(vec![
            Condition::eq_to("a", 1i64),
            Condition::And(vec![
                Condition::eq_to("b", 2i64),
                Condition::eq_to("c", 3i64)
            ]),
        ])
    );
}

#[test]
fn every_condition_kind_round_trips() {
    let conditions = vec![
        Condition::eq_to("material", "Steel"),
        Condition::ne_to("mass", 0i64),
        Condition::gt("mass", 2.5),
        Condition::gte("mass", -1.0),
        Condition::lt("mass", 1e-7),
        Condition::lte("mass", 1e21),
        Condition::contains("tags", "x"),
        Condition::In {
            field: "material".into(),
            values: vec![Value::from("Steel"), Value::Null, Value::Bool(true)],
        },
        Condition::Between {
            field: "mass".into(),
            low: Value::Int(1),
            high: Value::Int(9),
        },
        Condition::Exists {
            field: "material".into(),
        },
        Condition::StartsWith {
            field: "name".into(),
            value: Value::from("bolt"),
        },
        Condition::EndsWith {
            field: "name".into(),
            value: Value::from("-s"),
        },
        Condition::intersects("bbox", Aabb::new([0.0; 3], [2.0, 2.0, 2.0])),
        Condition::ContainsPoint {
            field: "bbox".into(),
            point: [0.5, 1.0, 0.0],
        },
        Condition::all(vec![]),
        Condition::any(vec![Condition::OfType(PART)]),
        Condition::negate(Condition::Exists { field: "x".into() }),
        Condition::OfType(PART),
        Condition::OfTypeIn(vec![PART, EntityTypeId(2)]),
        Condition::related(Edge::Named("bolt".into()), Some(3), Condition::OfType(PART)),
    ];
    for c in conditions {
        let q = Query::new(PART).filter(c);
        round_trip(&q);
    }
}

#[test]
fn traversals_projections_and_clauses_round_trip() {
    let q = Query::new(PART)
        .filter(Condition::eq_to("name", "root"))
        .traverse(
            Traversal::recursive(Edge::Child, Order::BreadthFirst, Some(4))
                .filter(Condition::eq_to("kind", "face"))
                .descend_while(Condition::ne_to("kind", "stop")),
        )
        .traverse(Traversal::step(Edge::Parent))
        .traverse(Traversal::recursive(
            Edge::Named("e".into()),
            Order::DepthFirst,
            None,
        ))
        .project("name")
        .project("mass")
        .order_by("mass", false)
        .offset(3)
        .limit(7)
        .distinct();
    round_trip(&q);

    let cursor = Query::new(PART).after(EntityId(42)).limit(10);
    round_trip(&cursor);

    let agg = Query::new(PART).aggregate(Metric::Avg("mass".into()), Some("material".into()));
    round_trip(&agg);
    let plain = Query::new(PART).aggregate(Metric::Count, None);
    round_trip(&plain);
}

#[test]
fn strings_with_quotes_backslashes_and_newlines_round_trip() {
    let q = Query::new(PART).filter(Condition::eq_to("note", "say \"hi\"\\ now\nthen\tend"));
    round_trip(&q);
}

#[test]
fn field_names_that_are_keywords_or_have_dots_round_trip() {
    let q = Query::new(PART)
        .filter(Condition::eq_to("limit", 1i64))
        .filter(Condition::eq_to("material.grade", "Ti"))
        .filter(Condition::eq_to("with space", 2i64))
        .filter(Condition::eq_to(r"a\.b", 3i64));
    round_trip(&q);
}

#[test]
fn floats_read_back_as_floats_and_ints_as_ints() {
    let q = Query::new(PART)
        .filter(Condition::eq_to("a", 2i64))
        .filter(Condition::eq_to("b", 2.0f64));
    let text = q.to_string();
    assert!(text.contains("a = 2 "), "{text}");
    assert!(text.contains("b = 2.0"), "{text}");
    round_trip(&q);
}

#[test]
fn bad_input_reports_a_position() {
    for (text, needle) in [
        ("from", "expected"),
        ("from 1 where", "expected a field name"),
        ("from 1 where a ~ 1", "unexpected character"),
        ("from 1 where a = \"open", "unterminated string"),
        ("from 1 limit 2 limit 3", "`limit` appears twice"),
        ("from 1 where a = 1 flavour", "expected a clause"),
        (
            "from 1 traverse sideways",
            "expected parent, child, or named",
        ),
        ("from 1 aggregate count group by", "expected a field name"),
    ] {
        match parse(text) {
            Err(QueryError::InvalidQuery(msg)) => {
                assert!(msg.contains(needle), "`{text}` gave `{msg}`");
                assert!(msg.contains("character"), "`{text}` gave `{msg}`");
            }
            other => panic!("`{text}` should fail, got {other:?}"),
        }
    }
}

#[test]
fn parsed_queries_are_validated() {
    // Parses, but an aggregate may not take a limit.
    let err = parse("from 1 aggregate count limit 3").unwrap_err();
    assert!(matches!(err, QueryError::InvalidQuery(_)));
    // Parses, but a NaN-free float check still applies to bounds: infinities cannot be written.
    assert!(parse("from 1 where a > 1e400").is_err());
}

#[test]
fn from_str_matches_parse() {
    let q: Query = "from 2 where a = 1".parse().unwrap();
    assert_eq!(q, parse("from 2 where a = 1").unwrap());
}
