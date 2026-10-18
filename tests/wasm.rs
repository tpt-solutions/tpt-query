//! Smoke test for the wasm32 target: the core engine runs under WebAssembly.
//!
//! Run with: `cargo test --target wasm32-unknown-unknown --no-default-features --test wasm`

#![cfg(target_arch = "wasm32")]

use tpt_query::{
    Condition, Edge, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Order, Query,
    Queryable, Traversal,
};
use wasm_bindgen_test::wasm_bindgen_test;

const PART: EntityTypeId = EntityTypeId(1);
const FACE: EntityTypeId = EntityTypeId(2);

#[wasm_bindgen_test]
fn filter_index_and_traversal_run_on_wasm() {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    s.insert(Entity::new(EntityId(1), PART).with_field("material", "Titanium"));
    s.insert(Entity::new(EntityId(2), PART).with_field("material", "Steel"));
    s.insert(Entity::new(EntityId(3), FACE).with_field("area", 4.5f64));
    assert!(s.link_child(EntityId(1), EntityId(3)));

    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "Titanium"))
        .traverse(
            Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                .filter(Condition::OfType(FACE)),
        );

    let ids: Vec<u64> = s.execute(&q).unwrap().map(|r| r.unwrap().id.0).collect();
    assert_eq!(ids, vec![3]);
    assert!(s.explain(&q).unwrap().to_string().contains("by_material"));
}
