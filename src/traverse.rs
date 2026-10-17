//! Graph navigation: a lazy walker over one relationship, with cycle detection.

use alloc::collections::{BTreeSet, VecDeque};

use crate::{
    ast::{Edge, Order},
    source::{EntityId, RelationMap},
};

/// Lazily yields the entities reachable from a start id along one edge kind.
///
/// The start is never yielded. Each entity is yielded at most once per walk, so
/// cycles terminate. Work is O(reached nodes): each node's neighbours are looked
/// up once through the [`RelationMap`], never by scanning the collection.
pub struct Walker<'a> {
    rel: &'a dyn RelationMap,
    edge: &'a Edge,
    order: Order,
    max_depth: Option<usize>,
    /// Discovered nodes not yet expanded, with their depth.
    frontier: VecDeque<(EntityId, usize)>,
    /// Every node discovered so far, including the start.
    visited: BTreeSet<EntityId>,
}

impl<'a> Walker<'a> {
    /// Starts a walk from `start` along `edge`.
    pub fn new(
        rel: &'a dyn RelationMap,
        start: EntityId,
        edge: &'a Edge,
        order: Order,
        max_depth: Option<usize>,
    ) -> Self {
        let mut visited = BTreeSet::new();
        visited.insert(start);
        let mut frontier = VecDeque::new();
        frontier.push_back((start, 0));
        Self {
            rel,
            edge,
            order,
            max_depth,
            frontier,
            visited,
        }
    }

    fn enqueue(&mut self, id: EntityId, depth: usize) {
        if self.visited.insert(id) {
            self.frontier.push_back((id, depth));
        }
    }

    fn expand(&mut self, id: EntityId, depth: usize) {
        if self.max_depth.is_some_and(|max| depth >= max) {
            return;
        }
        let rel = self.rel;
        let edge = self.edge;
        match edge {
            Edge::Parent => {
                if let Some(parent) = rel.parent(id) {
                    self.enqueue(parent, depth + 1);
                }
            }
            Edge::Child | Edge::Named(_) => {
                let kids = match edge {
                    Edge::Named(label) => rel.named(id, label),
                    _ => rel.children(id),
                };
                match self.order {
                    Order::BreadthFirst => {
                        for &kid in kids {
                            self.enqueue(kid, depth + 1);
                        }
                    }
                    // Pushed in reverse so the first child is popped first.
                    Order::DepthFirst => {
                        for &kid in kids.iter().rev() {
                            self.enqueue(kid, depth + 1);
                        }
                    }
                }
            }
        }
    }
}

impl Iterator for Walker<'_> {
    type Item = EntityId;

    fn next(&mut self) -> Option<EntityId> {
        loop {
            let (id, depth) = match self.order {
                Order::DepthFirst => self.frontier.pop_back()?,
                Order::BreadthFirst => self.frontier.pop_front()?,
            };
            self.expand(id, depth);
            if depth > 0 {
                return Some(id);
            }
        }
    }
}
