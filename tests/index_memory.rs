//! Memory-limit test for the indexed path: an indexed query with 1,000,000 candidates
//! and `limit: 10` must stream them, not copy them out of the index. A counting
//! global allocator measures peak heap use during execution only.
//!
//! This file holds one test on purpose: the allocator counters are process-wide.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use common::PART;
use tpt_query::{Condition, Entity, EntityId, IndexKind, MemorySource, Query, Queryable};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator and only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const N: u64 = 1_000_000;
const PEAK_LIMIT_BYTES: usize = 5 * 1024 * 1024;

#[test]
fn indexed_limit_ten_over_a_million_candidates_stays_under_five_megabytes() {
    let mut s = MemorySource::new();
    s.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    for i in 1..=N {
        s.insert(Entity::new(EntityId(i), PART).with_field("material", "steel"));
    }

    // Every entity matches, so the index returns a million candidates.
    let q = Query::new(PART)
        .filter(Condition::eq_to("material", "steel"))
        .limit(10);

    let before = CURRENT.load(Relaxed);
    PEAK.store(before, Relaxed);

    let rows: Vec<u64> = s.execute(&q).unwrap().map(|r| r.unwrap().id.0).collect();

    let peak_growth = PEAK.load(Relaxed).saturating_sub(before);
    assert_eq!(rows, (1..=10).collect::<Vec<u64>>());
    assert!(
        peak_growth < PEAK_LIMIT_BYTES,
        "indexed query peak heap growth was {peak_growth} bytes (limit {PEAK_LIMIT_BYTES})"
    );
}
