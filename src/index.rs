//! Secondary indexes. An index maps a field's values to entity ids and is kept in
//! sync by [`crate::MemorySource`] on every insert, update, and delete.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::ops::Bound::{Excluded, Included, Unbounded};

use crate::{
    ast::Condition,
    source::{EntityId, EntityTypeId, FieldPath, Value},
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
    /// Candidate ids that satisfy `cond`, ascending and unique. `None` when this
    /// index cannot answer `cond`. The answer must match the query evaluator exactly.
    fn lookup(&self, cond: &Condition) -> Option<Vec<EntityId>>;
}

/// An ordered map from field value to entity ids.
///
/// Both kinds are backed by a `BTreeMap` (the crate is `no_std` without a hash
/// map), so exact lookups are O(log N) rather than O(1).
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

    /// Ids whose value has the same kind as `bound` and lies in the given
    /// direction. Range scans never leave the kind's block of the ordering.
    fn range(&self, bound: &Value, below: bool, inclusive: bool) -> Vec<EntityId> {
        let rank = bound.rank();
        let mut out = BTreeSet::new();
        if below {
            let hi = if inclusive {
                Included(bound)
            } else {
                Excluded(bound)
            };
            for (_, ids) in self
                .map
                .range((Unbounded, hi))
                .rev()
                .take_while(|(k, _)| k.rank() == rank)
            {
                out.extend(ids.iter().copied());
            }
        } else {
            let lo = if inclusive {
                Included(bound)
            } else {
                Excluded(bound)
            };
            for (_, ids) in self
                .map
                .range((lo, Unbounded))
                .take_while(|(k, _)| k.rank() == rank)
            {
                out.extend(ids.iter().copied());
            }
        }
        out.into_iter().collect()
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

    fn lookup(&self, cond: &Condition) -> Option<Vec<EntityId>> {
        match cond {
            Condition::Eq { field, value } if field == &self.field => Some(
                self.map
                    .get(value)
                    .map(|ids| ids.iter().copied().collect())
                    .unwrap_or_default(),
            ),
            Condition::Gt { field, value } if self.is_range(field) => {
                Some(self.range(value, false, false))
            }
            Condition::Gte { field, value } if self.is_range(field) => {
                Some(self.range(value, false, true))
            }
            Condition::Lt { field, value } if self.is_range(field) => {
                Some(self.range(value, true, false))
            }
            Condition::Lte { field, value } if self.is_range(field) => {
                Some(self.range(value, true, true))
            }
            _ => None,
        }
    }
}

impl ValueIndex {
    fn is_range(&self, field: &FieldPath) -> bool {
        self.kind == IndexKind::BTree && field == &self.field
    }
}
