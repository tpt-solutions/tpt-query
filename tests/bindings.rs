//! Rows expose the query's source entity as well as their own entity, so a traversal
//! result can be traced back to the entity it was reached from.

use std::ptr;

use tpt_query::{
    Edge, Entity, EntityId, EntityTypeId, MemorySource, Order, Query, Queryable, Traversal,
};

const DOC: EntityTypeId = EntityTypeId(1);
const ANY: EntityTypeId = EntityTypeId(2);

fn two_documents() -> MemorySource {
    let mut s = MemorySource::new();
    for id in [1u64, 2, 3, 10, 11] {
        let ty = if matches!(id, 1 | 10) { DOC } else { ANY };
        s.insert(Entity::new(EntityId(id), ty));
    }
    s.link_child(EntityId(1), EntityId(2));
    s.link_child(EntityId(2), EntityId(3));
    s.link_child(EntityId(10), EntityId(11));
    s
}

fn rows_as_pairs(s: &MemorySource, q: &Query) -> Vec<(u64, u64)> {
    s.execute(q)
        .unwrap()
        .map(|r| {
            let r = r.unwrap();
            (r.source.id.0, r.id.0)
        })
        .collect()
}

#[test]
fn a_row_without_traversal_is_its_own_source() {
    let s = two_documents();
    let q = Query::new(DOC);
    for row in s.execute(&q).unwrap().map(Result::unwrap) {
        assert!(ptr::eq(row.source, row.entity));
    }
}

#[test]
fn a_traversed_row_keeps_the_entity_it_started_from() {
    let s = two_documents();
    let q = Query::new(DOC).traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None));
    // Document 1 reaches 2 and 3; document 10 reaches 11.
    assert_eq!(rows_as_pairs(&s, &q), vec![(1, 2), (1, 3), (10, 11)]);
}

#[test]
fn a_second_traversal_stage_keeps_the_original_source() {
    let s = two_documents();
    let q = Query::new(DOC)
        .traverse(Traversal::step(Edge::Child))
        .traverse(Traversal::step(Edge::Child));
    // Two hops from document 1 reach 3, and the source is still document 1.
    assert_eq!(rows_as_pairs(&s, &q), vec![(1, 3)]);
}

#[test]
fn distinct_keeps_the_first_source_that_reached_a_shared_entity() {
    let mut s = two_documents();
    // Named edges can share a target, so documents 1 and 10 both reach entity 11.
    s.link_named(EntityId(1), "ref", EntityId(11));
    s.link_named(EntityId(10), "ref", EntityId(11));
    let q = Query::new(DOC)
        .traverse(Traversal::step(Edge::Named("ref".into())))
        .distinct();
    assert_eq!(rows_as_pairs(&s, &q), vec![(1, 11)]);
}
