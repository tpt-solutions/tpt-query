//! Live queries: an incremental refresh (driven by the source's change feed) must give
//! the same results and deltas as a full re-run, after every insert and remove.

mod common;

use std::collections::BTreeSet;

use common::{Counting, PART, Rng, ids};
use tpt_query::live::LiveQuery;
use tpt_query::{Condition, Entity, EntityId, MemorySource, Query, Value};

fn wanted() -> Query {
    Query::new(PART).filter(Condition::Or(vec![
        Condition::gt("mass", Value::Int(5)),
        Condition::eq_to("kind", "bolt"),
    ]))
}

fn entity(rng: &mut Rng, id: u64) -> Entity {
    let mut e = Entity::new(EntityId(id), PART);
    if rng.below(4) != 0 {
        e = e.with_field("mass", rng.below(10) as i64);
    }
    if rng.below(2) == 0 {
        e = e.with_field("kind", if rng.below(2) == 0 { "bolt" } else { "nut" });
    }
    e
}

#[test]
fn incremental_deltas_match_a_full_run_after_every_change() {
    let mut rng = Rng::new(7);
    let mut s = MemorySource::new();
    let q = wanted();
    let mut direct = LiveQuery::new(q.clone());
    let mut fallback = LiveQuery::new(q.clone());
    let mut prev: BTreeSet<EntityId> = BTreeSet::new();

    for _ in 0..400 {
        let id = 1 + rng.below(30);
        if rng.below(3) == 0 {
            s.remove(EntityId(id));
        } else {
            s.insert(entity(&mut rng, id));
        }

        let want: BTreeSet<EntityId> = ids(&s, &q).into_iter().map(EntityId).collect();
        let added: Vec<EntityId> = want.difference(&prev).copied().collect();
        let removed: Vec<EntityId> = prev.difference(&want).copied().collect();

        // The change feed is used here: `MemorySource` provides one.
        let delta = direct.refresh(&s).unwrap();
        assert_eq!(delta.added, added);
        assert_eq!(delta.removed, removed);
        assert_eq!(direct.current().collect::<BTreeSet<_>>(), want);

        // The wrapper hides the change feed, so this one always re-runs in full.
        let wrapped = Counting::new(&s);
        let delta = fallback.refresh(&wrapped).unwrap();
        assert_eq!(delta.added, added);
        assert_eq!(delta.removed, removed);
        assert_eq!(fallback.current().collect::<BTreeSet<_>>(), want);

        prev = want;
    }
}

#[test]
fn a_query_with_paging_falls_back_to_full_refreshes() {
    let mut s = MemorySource::new();
    for i in 1..=5u64 {
        s.insert(Entity::new(EntityId(i), PART).with_field("mass", i as i64));
    }
    let q = Query::new(PART).order_by("mass", true).limit(2);
    let mut live = LiveQuery::new(q);
    let first = live.refresh(&s).unwrap();
    assert_eq!(first.added, vec![EntityId(4), EntityId(5)]);

    // Adding a heavier part pushes the smallest kept row out of the top two.
    s.insert(Entity::new(EntityId(9), PART).with_field("mass", 10i64));
    let second = live.refresh(&s).unwrap();
    assert_eq!(second.added, vec![EntityId(9)]);
    assert_eq!(second.removed, vec![EntityId(4)]);
}

#[test]
fn a_removed_entity_leaves_the_result_and_a_reinserted_one_returns() {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), PART).with_field("kind", "bolt"));
    let mut live = LiveQuery::new(wanted());
    assert_eq!(live.refresh(&s).unwrap().added, vec![EntityId(1)]);

    s.remove(EntityId(1));
    let gone = live.refresh(&s).unwrap();
    assert_eq!(gone.removed, vec![EntityId(1)]);

    s.insert(Entity::new(EntityId(1), PART).with_field("kind", "bolt"));
    assert_eq!(live.refresh(&s).unwrap().added, vec![EntityId(1)]);
}
