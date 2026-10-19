//! Secondary indexes. An index maps a field's values to entity ids and is kept in
//! sync by [`crate::MemorySource`] on every insert, update, and delete.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet, BinaryHeap, btree_set},
    string::String,
    vec::Vec,
};
use core::{
    cmp::Reverse,
    ops::Bound::{Excluded, Included, Unbounded},
};

use crate::{
    ast::Condition,
    source::{Entity, EntityId, EntityTypeId, FieldPath, IndexHit, Value},
};

/// Which lookups a [`ValueIndex`] supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    /// Exact-match lookups (`Eq`) only.
    Hash,
    /// Exact-match and range lookups (`Gt`, `Gte`, `Lt`, `Lte`).
    BTree,
}

/// A secondary index over one field of one entity type.
pub trait Index {
    /// Index name, reported in query plans.
    fn name(&self) -> &str;
    /// The entity type this index covers.
    fn entity_type(&self) -> EntityTypeId;
    /// The field this index covers.
    fn field(&self) -> &FieldPath;
    /// Records that entity `id` has `value` in the indexed field.
    fn on_insert(&mut self, id: EntityId, value: &Value);
    /// Removes the entry recorded by `on_insert` for `id` and `value`.
    fn on_remove(&mut self, id: EntityId, value: &Value);
    /// Candidate ids that satisfy `cond`, as an ascending, unique iterator whose
    /// length is known up front. `None` when this index cannot answer `cond`. The
    /// answer must match the query evaluator exactly.
    fn lookup<'a>(&'a self, cond: &Condition) -> Option<IndexHit<'a>>;
}

/// An ordered map from field value to entity ids.
///
/// Both kinds are backed by a `BTreeMap` (the crate is `no_std` without a hash
/// map), so exact lookups are O(log N) rather than O(1). Lookups are lazy: an
/// exact match iterates the stored id set directly, and a range merges the sets of
/// its keys as it goes. Nothing is copied out of the index.
pub struct ValueIndex {
    name: String,
    ty: EntityTypeId,
    field: FieldPath,
    kind: IndexKind,
    map: BTreeMap<Value, BTreeSet<EntityId>>,
}

impl ValueIndex {
    /// Creates an empty index. Use [`crate::MemorySource::create_index`] to add it
    /// to a source with backfill.
    pub fn new(name: &str, ty: EntityTypeId, field: impl Into<FieldPath>, kind: IndexKind) -> Self {
        Self {
            name: String::from(name),
            ty,
            field: field.into(),
            kind,
            map: BTreeMap::new(),
        }
    }

    /// The id sets of every key whose kind matches `bound` and that lies in the given
    /// direction. Range scans never leave the kind's block of the ordering.
    fn range<'a>(
        &'a self,
        bound: &Value,
        below: bool,
        inclusive: bool,
    ) -> Vec<&'a BTreeSet<EntityId>> {
        let rank = bound.rank();
        if below {
            let hi = if inclusive {
                Included(bound)
            } else {
                Excluded(bound)
            };
            self.map
                .range((Unbounded, hi))
                .rev()
                .take_while(|(k, _)| k.rank() == rank)
                .map(|(_, ids)| ids)
                .collect()
        } else {
            let lo = if inclusive {
                Included(bound)
            } else {
                Excluded(bound)
            };
            self.map
                .range((lo, Unbounded))
                .take_while(|(k, _)| k.rank() == rank)
                .map(|(_, ids)| ids)
                .collect()
        }
    }

    fn hit<'a>(&'a self, sets: Vec<&'a BTreeSet<EntityId>>) -> IndexHit<'a> {
        IndexHit {
            index: self.name.clone(),
            len: sets.iter().map(|s| s.len()).sum(),
            ids: Box::new(Merge::new(sets)),
        }
    }

    fn is_range(&self, field: &FieldPath) -> bool {
        self.kind == IndexKind::BTree && field == &self.field
    }
}

impl Index for ValueIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn entity_type(&self) -> EntityTypeId {
        self.ty
    }

    fn field(&self) -> &FieldPath {
        &self.field
    }

    fn on_insert(&mut self, id: EntityId, value: &Value) {
        self.map.entry(value.clone()).or_default().insert(id);
    }

    fn on_remove(&mut self, id: EntityId, value: &Value) {
        if let Some(ids) = self.map.get_mut(value) {
            ids.remove(&id);
            if ids.is_empty() {
                self.map.remove(value);
            }
        }
    }

    fn lookup<'a>(&'a self, cond: &Condition) -> Option<IndexHit<'a>> {
        let sets = match cond {
            Condition::Eq { field, value } if field == &self.field => {
                self.map.get(value).into_iter().collect()
            }
            Condition::Gt { field, value } if self.is_range(field) => {
                self.range(value, false, false)
            }
            Condition::Gte { field, value } if self.is_range(field) => {
                self.range(value, false, true)
            }
            Condition::Lt { field, value } if self.is_range(field) => {
                self.range(value, true, false)
            }
            Condition::Lte { field, value } if self.is_range(field) => {
                self.range(value, true, true)
            }
            _ => return None,
        };
        Some(self.hit(sets))
    }
}

/// Ascending merge of id sets, produced lazily. Each entity has one value for the
/// indexed field, so the sets are disjoint. A repeated id is still skipped.
struct Merge<'a> {
    sets: Vec<btree_set::Iter<'a, EntityId>>,
    /// The next unread id of each set, smallest first. The `usize` is the set's index.
    heads: BinaryHeap<Reverse<(EntityId, usize)>>,
    last: Option<EntityId>,
}

impl<'a> Merge<'a> {
    fn new(sets: Vec<&'a BTreeSet<EntityId>>) -> Self {
        let mut iters: Vec<_> = sets.into_iter().map(|s| s.iter()).collect();
        let mut heads = BinaryHeap::with_capacity(iters.len());
        for (i, it) in iters.iter_mut().enumerate() {
            if let Some(&id) = it.next() {
                heads.push(Reverse((id, i)));
            }
        }
        Self {
            sets: iters,
            heads,
            last: None,
        }
    }
}

impl Iterator for Merge<'_> {
    type Item = EntityId;

    fn next(&mut self) -> Option<EntityId> {
        loop {
            let Reverse((id, i)) = self.heads.pop()?;
            if let Some(&next) = self.sets[i].next() {
                self.heads.push(Reverse((next, i)));
            }
            if self.last != Some(id) {
                self.last = Some(id);
                return Some(id);
            }
        }
    }
}

/// A secondary index over several fields of one entity type. An entity is indexed
/// only when it has every field. It answers equality on all of its fields at once,
/// which a [`ValueIndex`] cannot do, since it covers one field.
pub struct CompositeIndex {
    name: String,
    ty: EntityTypeId,
    fields: Vec<FieldPath>,
    map: BTreeMap<Vec<Value>, BTreeSet<EntityId>>,
}

impl CompositeIndex {
    /// Creates an empty index over `fields`, in key order. The caller checks that
    /// `fields` is not empty. [`crate::MemorySource::create_composite_index`] does.
    pub(crate) fn new(name: &str, ty: EntityTypeId, fields: Vec<FieldPath>) -> Self {
        Self {
            name: String::from(name),
            ty,
            fields,
            map: BTreeMap::new(),
        }
    }

    /// Index name, reported in query plans.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The entity type this index covers.
    pub fn entity_type(&self) -> EntityTypeId {
        self.ty
    }

    /// The fields this index covers, in key order.
    pub fn fields(&self) -> &[FieldPath] {
        &self.fields
    }

    /// The key for `entity`, or `None` if it lacks one of the fields.
    fn key_of(&self, entity: &Entity) -> Option<Vec<Value>> {
        self.fields
            .iter()
            .map(|f| entity.field(f).cloned())
            .collect()
    }

    pub(crate) fn on_insert(&mut self, entity: &Entity) {
        if let Some(key) = self.key_of(entity) {
            self.map.entry(key).or_default().insert(entity.id);
        }
    }

    pub(crate) fn on_remove(&mut self, entity: &Entity) {
        let Some(key) = self.key_of(entity) else {
            return;
        };
        if let Some(ids) = self.map.get_mut(&key) {
            ids.remove(&entity.id);
            if ids.is_empty() {
                self.map.remove(&key);
            }
        }
    }

    /// Answers a set of conjuncts when each field has an `Eq` among them. Returns the
    /// positions of the conjuncts used, one per field, and the matching ids. Any
    /// other conjunct on those fields stays a residual filter, so the answer is exact.
    pub(crate) fn lookup<'a>(
        &'a self,
        conjuncts: &[&Condition],
    ) -> Option<(Vec<usize>, IndexHit<'a>)> {
        let mut used = Vec::with_capacity(self.fields.len());
        let mut key = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            let found = conjuncts.iter().enumerate().find_map(|(i, c)| match c {
                Condition::Eq { field: f, value } if f == field && !used.contains(&i) => {
                    Some((i, value))
                }
                _ => None,
            })?;
            used.push(found.0);
            key.push(found.1.clone());
        }
        let set = self.map.get(&key);
        let len = set.map_or(0, |s| s.len());
        let ids: Box<dyn Iterator<Item = EntityId> + 'a> = match set {
            Some(s) => Box::new(s.iter().copied()),
            None => Box::new(core::iter::empty()),
        };
        Some((
            used,
            IndexHit {
                index: self.name.clone(),
                len,
                ids,
            },
        ))
    }
}
