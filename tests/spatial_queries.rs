//! Nearest-neighbour and ray queries on the R-tree, checked against brute force.

#![cfg(feature = "spatial")]

mod common;

use common::Rng;
use tpt_query::Aabb;
use tpt_query::EntityId;
use tpt_query::spatial::{RTree, ray_enter};

fn random_boxes(rng: &mut Rng, n: u64) -> Vec<(Aabb, EntityId)> {
    (1..=n)
        .map(|i| {
            let mut min = [0.0f64; 3];
            let mut max = [0.0f64; 3];
            for a in 0..3 {
                let lo = rng.below(1000) as f64 / 10.0;
                min[a] = lo;
                max[a] = lo + rng.below(40) as f64 / 10.0;
            }
            (Aabb::new(min, max), EntityId(i))
        })
        .collect()
}

fn build(boxes: &[(Aabb, EntityId)]) -> RTree {
    let mut t = RTree::new();
    for (b, id) in boxes {
        t.insert(*b, *id);
    }
    t
}

fn dist2(b: &Aabb, p: &[f64; 3]) -> f64 {
    (0..3)
        .map(|i| {
            let d = (b.min[i] - p[i]).max(0.0).max(p[i] - b.max[i]);
            d * d
        })
        .sum()
}

#[test]
fn nearest_returns_the_k_closest_boxes() {
    let mut rng = Rng::new(7);
    let boxes = random_boxes(&mut rng, 400);
    let tree = build(&boxes);
    for _ in 0..20 {
        let p = [
            rng.below(1000) as f64 / 10.0,
            rng.below(1000) as f64 / 10.0,
            rng.below(1000) as f64 / 10.0,
        ];
        let k = 5;
        let got = tree.nearest(p, k);
        assert_eq!(got.len(), k);

        // Compare distances rather than ids, since equal distances may tie.
        let mut want: Vec<f64> = boxes.iter().map(|(b, _)| dist2(b, &p)).collect();
        want.sort_by(f64::total_cmp);
        let got_d: Vec<f64> = got
            .iter()
            .map(|id| dist2(&boxes[(id.0 - 1) as usize].0, &p))
            .collect();
        assert_eq!(got_d, want[..k].to_vec());
    }
}

#[test]
fn nearest_handles_small_and_empty_requests() {
    let mut rng = Rng::new(3);
    let boxes = random_boxes(&mut rng, 3);
    let tree = build(&boxes);
    assert!(tree.nearest([0.0; 3], 0).is_empty());
    assert_eq!(
        tree.nearest([0.0; 3], 10).len(),
        3,
        "k larger than the tree"
    );
    assert!(RTree::new().nearest([0.0; 3], 4).is_empty());
}

#[test]
fn ray_hits_match_brute_force_and_are_sorted() {
    let mut rng = Rng::new(11);
    let boxes = random_boxes(&mut rng, 300);
    let tree = build(&boxes);
    for _ in 0..20 {
        let origin = [
            -5.0,
            rng.below(1000) as f64 / 10.0,
            rng.below(1000) as f64 / 10.0,
        ];
        let dir = [1.0, 0.1, -0.05];
        let max_t = 150.0;
        let hits = tree.ray(origin, dir, max_t);

        let mut want: Vec<(f64, EntityId)> = boxes
            .iter()
            .filter_map(|(b, id)| ray_enter(b, origin, dir, max_t).map(|t| (t, *id)))
            .collect();
        want.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        assert_eq!(hits, want);
        assert!(hits.windows(2).all(|w| w[0].0 <= w[1].0));
    }
}

#[test]
fn ray_entry_distance_is_on_the_box_surface() {
    let b = Aabb::new([10.0, 0.0, 0.0], [20.0, 1.0, 1.0]);
    let t = ray_enter(&b, [0.0, 0.5, 0.5], [1.0, 0.0, 0.0], 100.0).unwrap();
    assert!((t - 10.0).abs() < 1e-9);
    // Pointing away, or stopping short, misses.
    assert!(ray_enter(&b, [0.0, 0.5, 0.5], [-1.0, 0.0, 0.0], 100.0).is_none());
    assert!(ray_enter(&b, [0.0, 0.5, 0.5], [1.0, 0.0, 0.0], 5.0).is_none());
    // Parallel to a face but outside it misses.
    assert!(ray_enter(&b, [0.0, 2.0, 0.5], [1.0, 0.0, 0.0], 100.0).is_none());
    // Starting inside enters at zero.
    assert_eq!(
        ray_enter(&b, [15.0, 0.5, 0.5], [1.0, 0.0, 0.0], 100.0),
        Some(0.0)
    );
}
