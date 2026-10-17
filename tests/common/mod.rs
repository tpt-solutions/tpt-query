#![allow(dead_code)]

use std::cell::Cell;

use tpt_query::{Entity, EntityId, EntitySource, EntityTypeId, MemorySource, Query, RelationMap};

pub const PART: EntityTypeId = EntityTypeId(1);
pub const FACE: EntityTypeId = EntityTypeId(2);
pub const DOC: EntityTypeId = EntityTypeId(3);
pub const PAGE: EntityTypeId = EntityTypeId(4);
pub const PARA: EntityTypeId = EntityTypeId(5);
pub const RUN: EntityTypeId = EntityTypeId(6);

/// Deterministic xorshift64 generator, so randomized tests are reproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// Wraps a source and counts `get` and relation lookups.
pub struct Counting<'a> {
    pub inner: &'a MemorySource,
    pub gets: Cell<usize>,
    pub rel_calls: Cell<usize>,
}

impl<'a> Counting<'a> {
    pub fn new(inner: &'a MemorySource) -> Self {
        Self {
            inner,
            gets: Cell::new(0),
            rel_calls: Cell::new(0),
        }
    }
}

impl EntitySource for Counting<'_> {
    fn get(&self, id: EntityId) -> Option<&Entity> {
        self.gets.set(self.gets.get() + 1);
        self.inner.get(id)
    }

    fn ids_of_type<'a>(&'a self, ty: EntityTypeId) -> Box<dyn Iterator<Item = EntityId> + 'a> {
        self.inner.ids_of_type(ty)
    }

    fn relations(&self) -> Option<&dyn RelationMap> {
        Some(self)
    }

    fn index_lookup(
        &self,
        ty: EntityTypeId,
        cond: &tpt_query::Condition,
    ) -> Option<tpt_query::IndexHit> {
        self.inner.index_lookup(ty, cond)
    }
}

impl RelationMap for Counting<'_> {
    fn parent(&self, id: EntityId) -> Option<EntityId> {
        self.rel_calls.set(self.rel_calls.get() + 1);
        self.inner.parent(id)
    }

    fn children(&self, id: EntityId) -> &[EntityId] {
        self.rel_calls.set(self.rel_calls.get() + 1);
        self.inner.children(id)
    }

    fn named(&self, id: EntityId, label: &str) -> &[EntityId] {
        self.rel_calls.set(self.rel_calls.get() + 1);
        self.inner.named(id, label)
    }
}

/// Runs a query and returns the ids of the rows, panicking on any error.
pub fn ids<S: EntitySource>(src: &S, query: &Query) -> Vec<u64> {
    use tpt_query::Queryable;
    src.execute(query)
        .expect("execute")
        .map(|r| r.expect("row").id.0)
        .collect()
}
