//! Indexed point lookups against full scans at several collection sizes.
//!
//! Run with: `cargo bench --bench index`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tpt_query::{
    Condition, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Query, Queryable,
};
use tpt_testkit::bench::{bench_named, validate_name, warm_up};

const PART: EntityTypeId = EntityTypeId(1);

fn build(n: u64, indexed: bool) -> MemorySource {
    let mut s = MemorySource::new();
    if indexed {
        s.create_index("by_code", PART, "code", IndexKind::Hash);
    }
    for i in 1..=n {
        s.insert(Entity::new(EntityId(i), PART).with_field("code", i as i64));
    }
    s
}

/// Codes 0..1000 exist in every collection of at least 1000 entities, so each lookup finds a row.
fn lookup(s: &MemorySource, k: u64) -> usize {
    let code = (k as i64 * 7919) % 1000;
    let q = Query::new(PART).filter(Condition::eq_to("code", code));
    s.execute(&q).expect("execute").count()
}

fn index_lookups(c: &mut Criterion) {
    for (label, n) in [("10k", 10_000u64), ("100k", 100_000), ("1m", 1_000_000)] {
        let indexed = build(n, true);
        warm_up(100, || lookup(&indexed, 1));
        bench_named(c, &format!("lookup/indexed/{label}"), |b| {
            let mut k = 0u64;
            b.iter(|| {
                k = k.wrapping_add(1);
                black_box(lookup(&indexed, k))
            })
        });
    }
}

fn scan_lookups(c: &mut Criterion) {
    // Scans are slow at 1M entities, so these run with a smaller sample.
    for (label, n) in [("10k", 10_000u64), ("100k", 100_000), ("1m", 1_000_000)] {
        let scan = build(n, false);
        let name = format!("lookup/scan/{label}");
        validate_name(&name).expect("valid benchmark name");
        let mut group = c.benchmark_group("scan");
        group.sample_size(10);
        group.bench_function(name, |b| {
            let mut k = 0u64;
            b.iter(|| {
                k = k.wrapping_add(1);
                black_box(lookup(&scan, k))
            })
        });
        group.finish();
    }
}

criterion_group!(benches, index_lookups, scan_lookups);
criterion_main!(benches);
