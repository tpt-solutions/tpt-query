//! Live queries: report which results were added or removed since the last refresh.
//!
//! When the source keeps a change feed (see [`crate::EntitySource::changes_since`]),
//! and the query's results depend only on each entity alone (no traversals, no
//! relation conditions, no ordering, paging, distinct, or nearest selection), a refresh
//! re-checks just the entities that changed. Otherwise it re-runs the whole query and
//! diffs the ids. [`MemorySource`](crate::MemorySource) keeps a change feed.
//!
//! # Example
//!
//! ```
//! use tpt_query::live::LiveQuery;
//! use tpt_query::prelude::*;
//!
//! let mut src = MemorySource::new();
//! src.insert(Entity::new(EntityId(1), EntityTypeId(1)).with_field("mass", 3.0));
//! let mut live = LiveQuery::new(Query::new(EntityTypeId(1)).filter(Condition::gt("mass", 2.0)));
//!
//! let first = live.refresh(&src).unwrap();
//! assert_eq!(first.added, [EntityId(1)]);
//!
//! src.remove(EntityId(1));
//! let second = live.refresh(&src).unwrap();
//! assert_eq!(second.removed, [EntityId(1)]);
//! ```

use alloc::{collections::BTreeSet, vec::Vec};

use crate::{
    ast::Query,
    engine::{self, QueryError, Queryable},
    source::{Changes, EntityId, EntitySource},
};

/// Results that appeared or disappeared since the previous refresh. Both lists are in
/// ascending id order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Delta {
    /// Ids in the result now that were not in the previous result.
    pub added: Vec<EntityId>,
    /// Ids in the previous result that are gone now.
    pub removed: Vec<EntityId>,
}

/// A query and the result ids from its last run.
#[derive(Debug, Clone)]
pub struct LiveQuery {
    query: Query,
    last: BTreeSet<EntityId>,
    /// The source's change version at the last refresh, when incremental refresh is
    /// possible. `None` until a full refresh has run.
    version: Option<u64>,
}

impl LiveQuery {
    /// Creates a live query. Nothing runs until the first [`LiveQuery::refresh`].
    pub fn new(query: Query) -> Self {
        Self {
            query,
            last: BTreeSet::new(),
            version: None,
        }
    }

    /// Runs the query against `src` and returns what changed since the last refresh.
    /// On error the previous result is kept.
    pub fn refresh<S: EntitySource>(&mut self, src: &S) -> Result<Delta, QueryError> {
        if let Some(since) = self.version
            && self.is_per_entity()
            && let Some(changes) = src.changes_since(since)
        {
            return Ok(self.apply(src, changes));
        }
        self.full_refresh(src)
    }

    /// The ids from the last refresh, in ascending order.
    pub fn current(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.last.iter().copied()
    }

    /// Whether each entity's membership depends only on its own fields.
    fn is_per_entity(&self) -> bool {
        let q = &self.query;
        !q.needs_relations()
            && q.nearest.is_none()
            && q.order_by.is_none()
            && q.limit.is_none()
            && q.offset == 0
            && q.after.is_none()
            && !q.distinct
            && q.aggregate.is_none()
    }

    fn full_refresh<S: EntitySource>(&mut self, src: &S) -> Result<Delta, QueryError> {
        // Read the version first, so a change made while the query runs is seen next time.
        let version = src.change_version();
        let mut now = BTreeSet::new();
        for row in src.execute(&self.query)? {
            now.insert(row?.id);
        }
        let added = now.difference(&self.last).copied().collect();
        let removed = self.last.difference(&now).copied().collect();
        self.last = now;
        self.version = version.filter(|_| self.is_per_entity());
        Ok(Delta { added, removed })
    }

    /// Re-checks only the changed ids. Each id's membership is decided from its current
    /// state, so the result matches a full run.
    fn apply<S: EntitySource>(&mut self, src: &S, changes: Changes) -> Delta {
        let mut added = BTreeSet::new();
        let mut removed = BTreeSet::new();
        for id in changes.ids {
            let member = src
                .get(id)
                .is_some_and(|e| engine::matches(&self.query, e, src));
            match (self.last.contains(&id), member) {
                (false, true) => {
                    self.last.insert(id);
                    added.insert(id);
                }
                (true, false) => {
                    self.last.remove(&id);
                    removed.insert(id);
                }
                _ => {}
            }
        }
        self.version = Some(changes.version);
        Delta {
            added: added.into_iter().collect(),
            removed: removed.into_iter().collect(),
        }
    }
}
