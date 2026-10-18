//! Ray conditions (a predicate on boxes; routed through the R-tree with the `spatial`
//! feature) and the nearest-`k` selection on a query.

mod common;

use common::{PART, ids};
use tpt_query::dsl::parse;
use tpt_query::{
    Aabb, Condition, Entity, EntityId, MemorySource, Query, QueryError, Queryable, Value,
};

fn boxed(id: u64, min: [f64; 3], max: [f64; 3]) -> Entity {
    Entity::new(EntityId(id), PART).with_field("bbox", Value::Aabb(Aabb::new(min, max)))
}

/// Six boxes, plus one entity with no box and one whose field is not a box.
fn scene() -> MemorySource {
    let mut s = MemorySource::new();
    s.insert(boxed(1, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    s.insert(boxed(2, [5.0, 0.0, 0.0], [6.0, 1.0, 1.0]));
    s.insert(boxed(3, [10.0, -1.0, -1.0], [11.0, 1.0, 1.0]));
    s.insert(boxed(4, [3.0, 3.0, 3.0], [4.0, 4.0, 4.0]));
    s.insert(boxed(5, [-2.0, -2.0, -2.0], [-1.0, -1.0, -1.0]));
    s.insert(boxed(6, [20.0, 20.0, 20.0], [21.0, 21.0, 21.0]));
    s.insert(Entity::new(EntityId(7), PART));
    s.insert(Entity::new(EntityId(8), PART).with_field("bbox", 3i64));
    s
}

fn ray(origin: [f64; 3], direction: [f64; 3], within: f64) -> Condition {
    Condition::Ray {
        field: "bbox".into(),
        origin,
        direction,
        within,
    }
}

#[test]
fn ray_hits_boxes_it_enters_within_the_distance() {
    let s = scene();
    // Along +x at y = z = 0.5: enters box 1 at t = 10 and box 2 at t = 15.
    let q = Query::new(PART).filter(ray([-10.0, 0.5, 0.5], [1.0, 0.0, 0.0], 15.0));
    assert_eq!(ids(&s, &q), vec![1, 2]);
    let q = Query::new(PART).filter(ray([-10.0, 0.5, 0.5], [1.0, 0.0, 0.0], 12.0));
    assert_eq!(ids(&s, &q), vec![1]);
}

#[test]
fn a_ray_starting_inside_a_box_enters_it_at_zero() {
    let s = scene();
    let q = Query::new(PART).filter(ray([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 0.0));
    assert_eq!(ids(&s, &q), vec![1]);
}

#[test]
fn a_ray_parallel_to_an_axis_misses_boxes_outside_that_slab() {
    let s = scene();
    // x = -10 is outside every box, and the direction has no x component.
    let q = Query::new(PART).filter(ray([-10.0, 0.5, 0.5], [0.0, 0.0, 1.0], 100.0));
    assert!(ids(&s, &q).is_empty());
}

#[test]
fn nearest_keeps_k_rows_closest_first() {
    let s = scene();
    // Box 1 contains the origin (distance 0), box 5 is at distance sqrt(3), box 2 at 5.
    let q = Query::new(PART).nearest("bbox", [0.0, 0.0, 0.0], 3);
    assert_eq!(ids(&s, &q), vec![1, 5, 2]);
}

#[test]
fn nearest_breaks_distance_ties_by_id() {
    let mut s = MemorySource::new();
    s.insert(boxed(7, [1.0, 0.0, 0.0], [2.0, 1.0, 1.0]));
    s.insert(boxed(3, [-2.0, 0.0, 0.0], [-1.0, 1.0, 1.0]));
    let q = Query::new(PART).nearest("bbox", [0.0, 0.5, 0.5], 2);
    assert_eq!(ids(&s, &q), vec![3, 7]);
}

#[test]
fn nearest_drops_rows_without_a_box_and_handles_small_k() {
    let s = scene();
    let all = Query::new(PART).nearest("bbox", [0.0, 0.0, 0.0], 100);
    assert_eq!(ids(&s, &all), vec![1, 5, 2, 4, 3, 6]);
    let none = Query::new(PART).nearest("bbox", [0.0, 0.0, 0.0], 0);
    assert!(ids(&s, &none).is_empty());
}

#[test]
fn nearest_respects_filters_and_reports_its_k_in_the_plan() {
    let s = scene();
    let q = Query::new(PART)
        .filter(Condition::Exists {
            field: "bbox".into(),
        })
        .filter(ray([-100.0, 0.5, 0.5], [1.0, 0.0, 0.0], 1000.0))
        .nearest("bbox", [0.0, 0.0, 0.0], 2);
    let plan = s.explain(&q).unwrap();
    assert_eq!(plan.nearest, Some(2));
    assert!(plan.to_string().contains("nearest: 2"));
    // Boxes 1 and 2 are on the ray at y = z = 0.5 and nearest the origin.
    assert_eq!(ids(&s, &q), vec![1, 2]);
}

#[test]
fn nearest_rejects_combinations_it_cannot_honour() {
    let s = scene();
    let base = Query::new(PART).nearest("bbox", [0.0, 0.0, 0.0], 2);
    for bad in [
        base.clone().limit(1),
        base.clone().offset(1),
        base.clone().order_by("bbox", false),
    ] {
        assert!(matches!(s.execute(&bad), Err(QueryError::InvalidQuery(_))));
    }
    let non_finite = Query::new(PART).nearest("bbox", [f64::NAN, 0.0, 0.0], 2);
    assert!(matches!(
        s.execute(&non_finite),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[test]
fn ray_and_nearest_round_trip_through_the_text_form() {
    let text = "from 1 where bbox ray from [-10.0, 0.5, 0.5] dir [1.0, 0.0, 0.0] within 15.0 \
                nearest bbox to [0.0, 0.0, 0.0] k 3";
    let q = parse(text).unwrap();
    assert_eq!(parse(&q.to_string()).unwrap(), q);
}

#[test]
fn a_ray_with_a_negative_distance_is_rejected() {
    let s = scene();
    let q = Query::new(PART).filter(ray([0.0; 3], [1.0, 0.0, 0.0], -1.0));
    assert!(matches!(s.execute(&q), Err(QueryError::InvalidQuery(_))));
}

#[cfg(feature = "spatial")]
mod routed {
    use super::*;
    use tpt_query::spatial::RTreeIndex;

    fn indexed() -> MemorySource {
        let mut s = scene();
        s.add_index(Box::new(RTreeIndex::new("by_bbox", PART, "bbox")))
            .unwrap();
        s
    }

    #[test]
    fn ray_uses_the_rtree_and_matches_a_scan() {
        let q = Query::new(PART).filter(ray([-10.0, 0.5, 0.5], [1.0, 0.0, 0.0], 15.0));
        let plan = indexed().explain(&q).unwrap();
        assert!(plan.to_string().contains("by_bbox"));
        assert_eq!(ids(&indexed(), &q), ids(&scene(), &q));
    }
}
