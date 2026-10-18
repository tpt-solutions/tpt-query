//! Live queries: re-run a query and report which results were added or removed.
//!
//! This is not incremental. Each refresh runs the whole query and diffs the ids
//! against the previous run, so its cost is that of a full run. The first refresh
//! reports every current result as added. Incremental maintenance, driven by the
//! source's insert, update, and remove calls, is still to do.

use alloc::{collections::BTreeSet, vec::Vec};

use crate::{
    ast::Query,
    engine::{QueryError, Queryable},
    source::{EntityId, EntitySource},
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
}

impl LiveQuery {
    /// Creates a live query. Nothing runs until the first [`LiveQuery::refresh`].
    pub fn new(query: Query) -> Self {
        Self {
            query,
            last: BTreeSet::new(),
        }
    }

    /// Runs the query against `src` and returns what changed since the last refresh.
    /// On error the previous result is kept.
    pub fn refresh<S: EntitySource>(&mut self, src: &S) -> Result<Delta, QueryError> {
        let mut now = BTreeSet::new();
        for row in src.execute(&self.query)? {
            now.insert(row?.id);
        }
        let added = now.difference(&self.last).copied().collect();
        let removed = self.last.difference(&now).copied().collect();
        self.last = now;
        Ok(Delta { added, removed })
    }

    /// The ids from the last refresh, in ascending order.
    pub fn current(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.last.iter().copied()
    }
}
