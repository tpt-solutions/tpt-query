//! Property test: after any sequence of inserts, updates, and deletes, every indexed
//! query returns exactly what a full scan of an identical unindexed source returns.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::{PART, ids};
use proptest::prelude::*;
use tpt_query::{Condition, Entity, EntityId, IndexKind, MemorySource, Query};

const MATERIALS: [&str; 3] = ["Steel", "Titanium", "Aluminium"];

#[derive(Debug, Clone)]
enum Op {
    /// Inserts or updates entity `id` with the given material index and mass.
    Upsert {
        id: u64,
        material: usize,
        mass: i64,
    },
    Remove(u64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (1u64..120, 0usize..3, -5i64..40)
            .prop_map(|(id, material, mass)| Op::Upsert { id, material, mass }),
        1 => (1u64..120).prop_map(Op::Remove),
    ]
}

fn indexed() -> MemorySource {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    s.create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap();
    s
}

fn queries() -> Vec<Query> {
    vec![
        Query::new(PART).filter(Condition::eq_to("material", "Titanium")),
        Query::new(PART).filter(Condition::gt("mass", 10i64)),
        Query::new(PART).filter(Condition::gte("mass", -5i64)),
        Query::new(PART).filter(Condition::lt("mass", 3i64)),
        Query::new(PART).filter(Condition::lte("mass", 0i64)),
        Query::new(PART)
            .filter(Condition::eq_to("material", "Steel"))
            .filter(Condition::lt("mass", 20i64)),
        Query::new(PART).filter(Condition::negate(Condition::eq_to("material", "Steel"))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn indexed_queries_match_a_full_scan(ops in prop::collection::vec(op(), 1..150)) {
        let mut with_index = indexed();
        let mut scan = MemorySource::new();
        let queries = queries();

        for (step, op) in ops.iter().enumerate() {
            match *op {
                Op::Upsert { id, material, mass } => {
                    let e = Entity::new(EntityId(id), PART)
                        .with_field("material", MATERIALS[material])
                        .with_field("mass", mass);
                    with_index.insert(e.clone());
                    scan.insert(e);
                }
                Op::Remove(id) => {
                    with_index.remove(EntityId(id));
                    scan.remove(EntityId(id));
                }
            }
            for q in &queries {
                prop_assert_eq!(ids(&with_index, q), ids(&scan, q), "after step {}: {:?}", step, op);
            }
        }
    }
}
