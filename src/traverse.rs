//! Graph navigation: a lazy walker over one relationship, with cycle detection.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet, VecDeque},
};

use crate::{
    ast::{Edge, Order},
    source::{EntityId, RelationMap},
};

/// Lazily yields the entities reachable from a start id along one edge kind.
///
/// The start is never yielded. Each entity is yielded at most once per walk, so
/// cycles terminate. A node reached again by a shorter path is expanded again at
/// the shorter depth, so `max_depth` does not hide nodes behind a longer route.
/// Work is O(reached nodes): each node's neighbours are looked up through the
/// [`RelationMap`], never by scanning the collection.
pub struct Walker<'a> {
    rel: &'a dyn RelationMap,
    edge: &'a Edge,
    order: Order,
    max_depth: Option<usize>,
    /// Decides whether a node found below the start is expanded. `None` expands all.
    descend: Option<Box<dyn Fn(EntityId) -> bool + 'a>>,
    /// Discovered nodes not yet expanded, with the depth they were found at.
    frontier: VecDeque<(EntityId, usize)>,
    /// Shallowest depth each discovered node was found at. The start is at 0.
    best: BTreeMap<EntityId, usize>,
    /// Nodes already yielded, so a node re-expanded at a shorter depth is not yielded twice.
    yielded: BTreeSet<EntityId>,
}

impl<'a> Walker<'a> {
    /// Starts a walk from `start` along `edge`. `descend`, if given, must hold for a node
    /// below the start before the walk continues from it.
    pub fn new(
        rel: &'a dyn RelationMap,
        start: EntityId,
        edge: &'a Edge,
        order: Order,
        max_depth: Option<usize>,
        descend: Option<Box<dyn Fn(EntityId) -> bool + 'a>>,
    ) -> Self {
        let mut best = BTreeMap::new();
        best.insert(start, 0);
        let mut frontier = VecDeque::new();
        frontier.push_back((start, 0));
        Self {
            rel,
            edge,
            order,
            max_depth,
            descend,
            frontier,
            best,
            yielded: BTreeSet::new(),
        }
    }

    /// Queues `id` at `depth` unless it was already found at that depth or shallower.
    fn enqueue(&mut self, id: EntityId, depth: usize) {
        if self.best.get(&id).is_some_and(|&d| d <= depth) {
            return;
        }
        self.best.insert(id, depth);
        self.frontier.push_back((id, depth));
    }

    fn expand(&mut self, id: EntityId, depth: usize) {
        if depth > 0 && self.descend.as_ref().is_some_and(|ok| !ok(id)) {
            return;
        }
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
            // A shorter path reached this node after this entry was queued. That
            // shorter entry is still in the frontier, so this one is stale.
            if self.best.get(&id).is_some_and(|&d| d < depth) {
                continue;
            }
            self.expand(id, depth);
            if depth > 0 && self.yielded.insert(id) {
                return Some(id);
            }
        }
    }
}
