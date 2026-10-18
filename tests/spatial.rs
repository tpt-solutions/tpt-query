//! R-tree spatial index: exact results against brute force under random
//! insert and remove, and `Intersects` through a source.

#![cfg(feature = "spatial")]

mod common;

use common::{PART, Rng, ids};
use tpt_query::spatial::{RTree, RTreeIndex};
use tpt_query::{Aabb, Condition, Entity, EntityId, MemorySource, Query};

fn random_box(rng: &mut Rng) -> Aabb {
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for i in 0..3 {
        let a = rng.below(1000) as f64;
        let len = rng.below(40) as f64; // zero-length boxes (points) are included
        min[i] = a;
        max[i] = a + len;
    }
    Aabb::new(min, max)
}

#[test]
fn rtree_search_matches_brute_force_under_churn() {
    let mut rng = Rng::new(99);
    let mut tree = RTree::new();
    let mut entries: Vec<(Aabb, EntityId)> = Vec::new();

    for step in 0..3000u64 {
        if entries.is_empty() || rng.below(3) != 0 {
            let bbox = random_box(&mut rng);
            let id = EntityId(step);
            tree.insert(bbox, id);
            entries.push((bbox, id));
        } else {
            let victim = rng.below(entries.len() as u64) as usize;
            let (bbox, id) = entries.swap_remove(victim);
            assert!(tree.remove(&bbox, id), "entry must be found for removal");
        }

        if step % 150 == 0 {
            let query = random_box(&mut rng);
            let mut got = Vec::new();
            tree.search(&query, &mut got);
            got.sort_unstable();
            let mut want: Vec<EntityId> = entries
                .iter()
                .filter(|(b, _)| b.intersects(&query))
                .map(|(_, id)| *id)
                .collect();
            want.sort_unstable();
            assert_eq!(got, want, "step {step}");
        }
    }
}

#[test]
fn touching_boxes_intersect() {
    let mut tree = RTree::new();
    tree.insert(Aabb::new([0.0; 3], [1.0; 3]), EntityId(1));
    let mut out = Vec::new();
    tree.search(&Aabb::new([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]), &mut out);
    assert_eq!(out, vec![EntityId(1)]);
}

#[test]
fn intersects_through_a_source_uses_the_index() {
    let mut s = MemorySource::new();
    s.add_index(Box::new(RTreeIndex::new("by_bbox", PART, "bbox")))
        .unwrap();
    let mut rng = Rng::new(5);
    let mut boxes = Vec::new();
    for i in 1..=400u64 {
        let bbox = random_box(&mut rng);
        boxes.push((i, bbox));
        s.insert(Entity::new(EntityId(i), PART).with_field("bbox", bbox));
    }

    let viewport = Aabb::new([100.0; 3], [400.0; 3]);
    let q = Query::new(PART).filter(Condition::intersects("bbox", viewport));
    let plan = tpt_query::Queryable::explain(&s, &q).unwrap();
    assert!(plan.to_string().contains("by_bbox"), "plan: {plan}");

    let want: Vec<u64> = boxes
        .iter()
        .filter(|(_, b)| b.intersects(&viewport))
        .map(|(i, _)| *i)
        .collect();
    assert_eq!(ids(&s, &q), want);
}
