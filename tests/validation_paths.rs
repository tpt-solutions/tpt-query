//! Invalid-query errors name the failing part of the query, and `Row::to_map`
//! keys projected values by their field path.

mod common;

use common::PART;
use tpt_query::{Condition, Entity, EntityId, MemorySource, Query, QueryError, Queryable, Value};

fn invalid_message(q: &Query) -> String {
    match q.validate() {
        Err(QueryError::InvalidQuery(msg)) => msg,
        other => panic!("expected InvalidQuery, got {other:?}"),
    }
}

#[test]
fn errors_name_the_nested_condition_that_failed() {
    let q = Query::new(PART)
        .filter(Condition::eq_to("ok", 1i64))
        .filter(Condition::And(vec![
            Condition::eq_to("ok", 1i64),
            Condition::Or(vec![Condition::eq_to("", 1i64)]),
        ]));
    let msg = invalid_message(&q);
    assert!(msg.starts_with("filters[1].and[1].or[0]: "), "{msg}");
}

#[test]
fn errors_name_projections_order_and_traversals() {
    let q = Query::new(PART).project("a").project("b..c");
    assert!(invalid_message(&q).starts_with("projections[1]: "));

    let q = Query::new(PART).order_by("", false);
    assert!(invalid_message(&q).starts_with("order_by.field: "));

    let q = Query::new(PART).traverse(tpt_query::Traversal::recursive(
        tpt_query::Edge::Child,
        tpt_query::Order::DepthFirst,
        Some(0),
    ));
    assert!(invalid_message(&q).starts_with("traversals[0]: "));
}

#[test]
fn descend_conditions_are_validated() {
    let q = Query::new(PART).traverse(
        tpt_query::Traversal::step(tpt_query::Edge::Child)
            .descend_while(Condition::eq_to("", 1i64)),
    );
    assert!(invalid_message(&q).starts_with("traversals[0].descend[0]: "));
}

#[test]
fn row_to_map_keys_values_by_projection_path() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field("name", Value::from("bolt")));
    let q = Query::new(PART).project("name").project("missing");
    let rows: Vec<_> = s.execute(&q).unwrap().map(Result::unwrap).collect();
    let map = rows[0].to_map();
    assert_eq!(map.len(), 2);
    assert_eq!(map["name"], Some(&Value::from("bolt")));
    assert_eq!(map["missing"], None);
}
