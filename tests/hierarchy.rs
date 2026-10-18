//! Graph traversal: edges, DFS/BFS order, depth limits, cycles, relation-map rules,
//! and a complexity guard against accidental O(N^2) walks.

mod common;

use common::{Counting, DOC, FACE, PAGE, PARA, PART, RUN, ids};
use tpt_query::{
    Condition, Edge, Entity, EntityId, EntitySource, MemorySource, Order, Query, QueryError,
    Queryable, RelationMap, Traversal,
};

/// Document(1) -> Pages 2, 3
///   Page 2 -> Paragraphs 4 (Bold), 5 (Normal)
///     Paragraph 4 -> Runs 7, 8
///     Paragraph 5 -> Run 9
///   Page 3 -> Paragraph 6 (Bold), Table 11
///     Paragraph 6 -> Run 10
fn document() -> MemorySource {
    let mut s = MemorySource::new();
    let add = |s: &mut MemorySource, id: u64, ty, style: Option<&str>| {
        let mut e = Entity::new(EntityId(id), ty);
        if let Some(style) = style {
            e = e.with_field("style", style);
        }
        s.insert(e);
    };
    add(&mut s, 1, DOC, None);
    add(&mut s, 2, PAGE, None);
    add(&mut s, 3, PAGE, None);
    add(&mut s, 4, PARA, Some("Bold"));
    add(&mut s, 5, PARA, Some("Normal"));
    add(&mut s, 6, PARA, Some("Bold"));
    add(&mut s, 7, RUN, None);
    add(&mut s, 8, RUN, None);
    add(&mut s, 9, RUN, None);
    add(&mut s, 10, RUN, None);
    add(&mut s, 11, FACE, None);
    for (p, c) in [
        (1, 2),
        (1, 3),
        (2, 4),
        (2, 5),
        (4, 7),
        (4, 8),
        (5, 9),
        (3, 6),
        (6, 10),
        (3, 11),
    ] {
        assert!(s.link_child(EntityId(p), EntityId(c)));
    }
    s
}

fn descendants(order: Order, max_depth: Option<usize>) -> Query {
    Query::new(DOC).traverse(Traversal::recursive(Edge::Child, order, max_depth))
}

#[test]
fn depth_first_is_pre_order_in_insertion_order() {
    let s = document();
    assert_eq!(
        ids(&s, &descendants(Order::DepthFirst, None)),
        vec![2, 4, 7, 8, 5, 9, 3, 6, 10, 11]
    );
}

#[test]
fn breadth_first_is_level_order() {
    let s = document();
    assert_eq!(
        ids(&s, &descendants(Order::BreadthFirst, None)),
        vec![2, 3, 4, 5, 6, 11, 7, 8, 9, 10]
    );
}

#[test]
fn max_depth_limits_the_walk() {
    let s = document();
    assert_eq!(
        ids(&s, &descendants(Order::DepthFirst, Some(2))),
        vec![2, 4, 5, 3, 6, 11]
    );
    assert_eq!(
        ids(&s, &descendants(Order::DepthFirst, Some(1))),
        vec![2, 3]
    );
}

#[test]
fn parent_edge_walks_ancestors() {
    let s = document();
    // Run 7 -> paragraph 4 (one hop), then page 2 and document 1 (unbounded).
    let one_hop = Query::new(RUN)
        .filter(Condition::eq_to("missing", 0i64))
        .traverse(Traversal::step(Edge::Parent));
    assert!(ids(&s, &one_hop).is_empty());

    let ancestors = Query::new(RUN)
        .filter(Condition::eq_to("missing", 0i64))
        .traverse(Traversal::recursive(Edge::Parent, Order::DepthFirst, None));
    assert!(ids(&s, &ancestors).is_empty());

    // Every run contributes its ancestors. Run 7 yields 4, 2, 1 first.
    let all = Query::new(RUN).traverse(Traversal::recursive(Edge::Parent, Order::DepthFirst, None));
    let got = ids(&s, &all);
    assert_eq!(&got[..3], &[4, 2, 1]);

    let direct = Query::new(RUN).traverse(Traversal::step(Edge::Parent));
    assert_eq!(ids(&s, &direct), vec![4, 4, 5, 6]);
}

#[test]
fn traversal_filters_select_reached_entities() {
    let s = document();
    let q = Query::new(DOC).traverse(
        Traversal::recursive(Edge::Child, Order::DepthFirst, None).filter(Condition::OfType(RUN)),
    );
    assert_eq!(ids(&s, &q), vec![7, 8, 9, 10]);
}

#[test]
fn traversal_stages_chain() {
    let s = document();
    // Pages -> paragraphs -> runs.
    let q = Query::new(DOC)
        .traverse(Traversal::step(Edge::Child).filter(Condition::OfType(PAGE)))
        .traverse(
            Traversal::recursive(Edge::Child, Order::BreadthFirst, None)
                .filter(Condition::OfType(RUN)),
        );
    assert_eq!(ids(&s, &q), vec![7, 8, 9, 10]);
}

#[test]
fn related_condition_finds_runs_under_bold_paragraphs() {
    let s = document();
    let q = Query::new(RUN).filter(Condition::related(
        Edge::Parent,
        Some(1),
        Condition::eq_to("style", "Bold"),
    ));
    assert_eq!(ids(&s, &q), vec![7, 8, 10]);
}

#[test]
fn related_condition_with_depth_finds_runs_under_a_document() {
    let s = document();
    let q = Query::new(RUN).filter(Condition::related(
        Edge::Parent,
        None,
        Condition::OfType(DOC),
    ));
    assert_eq!(ids(&s, &q), vec![7, 8, 9, 10]);
}

#[test]
fn named_edges_are_followed() {
    let mut s = document();
    assert!(s.link_named(EntityId(4), "style_ref", EntityId(6)));
    let q = Query::new(PARA)
        .filter(Condition::eq_to("style", "Bold"))
        .traverse(Traversal::step(Edge::Named("style_ref".into())));
    assert_eq!(ids(&s, &q), vec![6]);
}

#[test]
fn cycles_terminate_and_yield_each_entity_once() {
    let mut s = document();
    // 7 -> 2 closes the cycle 2 -> 4 -> 7 -> 2.
    assert!(s.link_child(EntityId(7), EntityId(2)));
    let q = Query::new(PAGE)
        .filter(Condition::eq_to("missing", 0i64))
        .traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None));
    assert!(ids(&s, &q).is_empty());

    let q = Query::new(DOC)
        .filter(Condition::eq_to("missing", 0i64))
        .traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None));
    assert!(ids(&s, &q).is_empty());

    // Both pages still have a run somewhere below them, even with the cycle.
    let pages_with_runs = Query::new(PAGE).filter(Condition::related(
        Edge::Child,
        None,
        Condition::OfType(RUN),
    ));
    assert_eq!(ids(&s, &pages_with_runs), vec![2, 3]);

    let walk = Query::new(DOC).traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None));
    let got = ids(&s, &walk);
    let mut unique = got.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(got.len(), unique.len(), "no entity may be yielded twice");
}

#[test]
fn traversal_without_a_relation_map_is_rejected() {
    struct NoRelations(MemorySource);
    impl EntitySource for NoRelations {
        fn get(&self, id: EntityId) -> Option<&Entity> {
            self.0.get(id)
        }
        fn ids_of_type<'a>(
            &'a self,
            ty: tpt_query::EntityTypeId,
        ) -> Box<dyn Iterator<Item = EntityId> + 'a> {
            self.0.ids_of_type(ty)
        }
    }

    let src = NoRelations(document());
    let q = descendants(Order::DepthFirst, None);
    match src.execute(&q) {
        Err(err) => assert_eq!(err, QueryError::NoRelationMap),
        Ok(_) => panic!("traversal must be rejected without a relation map"),
    }
    let related = Query::new(RUN).filter(Condition::related(
        Edge::Parent,
        None,
        Condition::OfType(PARA),
    ));
    assert!(matches!(
        src.explain(&related),
        Err(QueryError::NoRelationMap)
    ));

    // A plain filter does not need relations.
    let flat = Query::new(RUN);
    assert_eq!(src.execute(&flat).unwrap().count(), 4);
}

#[test]
fn removed_named_target_is_reported_as_missing() {
    let mut s = document();
    assert!(s.link_named(EntityId(4), "style_ref", EntityId(6)));
    s.remove(EntityId(6));
    let q = Query::new(PARA)
        .filter(Condition::eq_to("style", "Bold"))
        .traverse(Traversal::step(Edge::Named("style_ref".into())));
    let errors: Vec<_> = s.execute(&q).unwrap().filter_map(Result::err).collect();
    assert_eq!(errors, vec![QueryError::MissingEntity(EntityId(6))]);
}

#[test]
fn removing_an_entity_detaches_it_from_the_tree() {
    let mut s = document();
    s.remove(EntityId(2));
    assert_eq!(s.children(EntityId(1)), &[EntityId(3)]);
    assert_eq!(s.parent(EntityId(4)), None);
}

#[test]
fn walks_do_not_scan_the_whole_collection() {
    // Full binary tree: node i has parent i / 2. Work done by a full-tree
    // descendant walk must grow linearly with N, not quadratically.
    let cost = |n: u64| {
        let mut s = MemorySource::new();
        // Only the root is a DOC, so the query starts from exactly one row.
        s.insert(Entity::new(EntityId(1), DOC));
        for i in 2..=n {
            s.insert(Entity::new(EntityId(i), PART));
        }
        for i in 2..=n {
            assert!(s.link_child(EntityId(i / 2), EntityId(i)));
        }
        let counting = Counting::new(&s);
        let q =
            Query::new(DOC).traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None));
        let found = ids(&counting, &q);
        assert_eq!(found.len() as u64, n - 1);
        counting.gets.get() + counting.rel_calls.get()
    };
    let small = cost(1000) as f64;
    let large = cost(4000) as f64;
    let ratio = large / small;
    // Linear growth gives about 4x for 4x the nodes; quadratic gives about 16x.
    assert!(ratio < 6.0, "work grew by {ratio:.1}x for 4x the nodes");
}

/// A -> [B, C], B -> E -> F, C -> F, F -> G. F is 2 hops from A via C but 3 hops via
/// B, so a depth-limited walk must still reach G (3 hops via C).
#[test]
fn depth_limited_walks_reach_nodes_through_shorter_paths_in_a_dag() {
    let mut s = MemorySource::new();
    for (id, name) in [(1, "A"), (2, "B"), (3, "C"), (4, "E"), (5, "F"), (6, "G")] {
        s.insert(Entity::new(EntityId(id), PART).with_field("name", name));
    }
    for (from, to) in [(1, 2), (1, 3), (2, 4), (4, 5), (3, 5), (5, 6)] {
        s.link_named(EntityId(from), "e", EntityId(to));
    }
    for order in [Order::DepthFirst, Order::BreadthFirst] {
        let q = Query::new(PART)
            .filter(Condition::eq_to("name", "A"))
            .traverse(Traversal::recursive(
                Edge::Named("e".into()),
                order,
                Some(3),
            ));
        let mut got = ids(&s, &q);
        got.sort();
        assert_eq!(got, vec![2, 3, 4, 5, 6], "{order:?}");
    }
}
