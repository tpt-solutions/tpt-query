//! R-tree spatial index for [`crate::Condition::Intersects`] (feature `spatial`).
//!
//! An arena-allocated R-tree with a linear-cost split. Removals do not condense
//! underfull nodes, so heavy churn can leave the tree less tight than a fresh bulk
//! load. Results are always exact.

use alloc::{boxed::Box, vec, vec::Vec};

use crate::{
    ast::Condition,
    index::Index,
    source::{Aabb, EntityId, EntityTypeId, FieldPath, Value},
};

const MAX_ENTRIES: usize = 8;
const MIN_ENTRIES: usize = 3;

#[derive(Debug)]
enum Kind {
    Leaf(Vec<(Aabb, EntityId)>),
    Inner(Vec<usize>),
}

#[derive(Debug)]
struct Node {
    bbox: Aabb,
    kind: Kind,
}

/// An R-tree over bounding boxes keyed by entity id.
#[derive(Debug)]
pub struct RTree {
    nodes: Vec<Node>,
    root: usize,
}

impl Default for RTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RTree {
    /// Creates an empty tree.
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                bbox: Aabb::EMPTY,
                kind: Kind::Leaf(Vec::new()),
            }],
            root: 0,
        }
    }

    /// Adds an entry.
    pub fn insert(&mut self, bbox: Aabb, id: EntityId) {
        let root = self.root;
        if let Some(sibling) = self.insert_at(root, bbox, id) {
            let bbox = self.nodes[root].bbox.union(&self.nodes[sibling].bbox);
            self.nodes.push(Node {
                bbox,
                kind: Kind::Inner(vec![root, sibling]),
            });
            self.root = self.nodes.len() - 1;
        }
    }

    /// Removes the entry for `id` inserted with `bbox`. Returns whether it was found.
    pub fn remove(&mut self, bbox: &Aabb, id: EntityId) -> bool {
        self.remove_at(self.root, bbox, id)
    }

    /// Appends the ids of all entries whose boxes intersect `bounds` to `out`,
    /// in tree order. Callers sort if they need id order.
    pub fn search(&self, bounds: &Aabb, out: &mut Vec<EntityId>) {
        self.search_at(self.root, bounds, out);
    }

    fn insert_at(&mut self, idx: usize, bbox: Aabb, id: EntityId) -> Option<usize> {
        self.nodes[idx].bbox = self.nodes[idx].bbox.union(&bbox);
        match self.choose_child(idx, &bbox) {
            Some(child) => {
                if let Some(sibling) = self.insert_at(child, bbox, id)
                    && let Kind::Inner(children) = &mut self.nodes[idx].kind
                {
                    children.push(sibling);
                }
                let overflow =
                    matches!(&self.nodes[idx].kind, Kind::Inner(c) if c.len() > MAX_ENTRIES);
                overflow.then(|| self.split_inner(idx))
            }
            None => {
                if let Kind::Leaf(entries) = &mut self.nodes[idx].kind {
                    entries.push((bbox, id));
                }
                let overflow =
                    matches!(&self.nodes[idx].kind, Kind::Leaf(e) if e.len() > MAX_ENTRIES);
                overflow.then(|| self.split_leaf(idx))
            }
        }
    }

    /// The child whose box grows least (by margin) to include `bbox`. `None` for a leaf.
    fn choose_child(&self, idx: usize, bbox: &Aabb) -> Option<usize> {
        let Kind::Inner(children) = &self.nodes[idx].kind else {
            return None;
        };
        children.iter().copied().min_by(|&a, &b| {
            let cost = |c: usize| {
                let before = self.nodes[c].bbox.margin();
                self.nodes[c].bbox.union(bbox).margin() - before
            };
            cost(a).total_cmp(&cost(b)).then(
                self.nodes[a]
                    .bbox
                    .margin()
                    .total_cmp(&self.nodes[b].bbox.margin()),
            )
        })
    }

    fn split_leaf(&mut self, idx: usize) -> usize {
        let entries = match &mut self.nodes[idx].kind {
            Kind::Leaf(e) => core::mem::take(e),
            Kind::Inner(_) => Vec::new(),
        };
        let (bbox_a, a, bbox_b, b) = split_items(entries);
        self.nodes[idx] = Node {
            bbox: bbox_a,
            kind: Kind::Leaf(a),
        };
        self.nodes.push(Node {
            bbox: bbox_b,
            kind: Kind::Leaf(b),
        });
        self.nodes.len() - 1
    }

    fn split_inner(&mut self, idx: usize) -> usize {
        let children = match &mut self.nodes[idx].kind {
            Kind::Inner(c) => core::mem::take(c),
            Kind::Leaf(_) => Vec::new(),
        };
        let items: Vec<(Aabb, usize)> = children
            .into_iter()
            .map(|c| (self.nodes[c].bbox, c))
            .collect();
        let (bbox_a, a, bbox_b, b) = split_items(items);
        self.nodes[idx] = Node {
            bbox: bbox_a,
            kind: Kind::Inner(a.into_iter().map(|(_, c)| c).collect()),
        };
        self.nodes.push(Node {
            bbox: bbox_b,
            kind: Kind::Inner(b.into_iter().map(|(_, c)| c).collect()),
        });
        self.nodes.len() - 1
    }

    fn remove_at(&mut self, idx: usize, bbox: &Aabb, id: EntityId) -> bool {
        if !self.nodes[idx].bbox.intersects(bbox) {
            return false;
        }
        let removed = match &mut self.nodes[idx].kind {
            Kind::Leaf(entries) => match entries.iter().position(|(_, e)| *e == id) {
                Some(pos) => {
                    entries.remove(pos);
                    true
                }
                None => false,
            },
            Kind::Inner(children) => {
                let children = children.clone();
                children.into_iter().any(|c| self.remove_at(c, bbox, id))
            }
        };
        if removed {
            self.recalc(idx);
        }
        removed
    }

    fn recalc(&mut self, idx: usize) {
        let bbox = match &self.nodes[idx].kind {
            Kind::Leaf(entries) => entries.iter().fold(Aabb::EMPTY, |acc, (b, _)| acc.union(b)),
            Kind::Inner(children) => children
                .iter()
                .fold(Aabb::EMPTY, |acc, &c| acc.union(&self.nodes[c].bbox)),
        };
        self.nodes[idx].bbox = bbox;
    }

    fn search_at(&self, idx: usize, bounds: &Aabb, out: &mut Vec<EntityId>) {
        let node = &self.nodes[idx];
        if !node.bbox.intersects(bounds) {
            return;
        }
        match &node.kind {
            Kind::Leaf(entries) => {
                out.extend(
                    entries
                        .iter()
                        .filter(|(b, _)| b.intersects(bounds))
                        .map(|(_, id)| *id),
                );
            }
            Kind::Inner(children) => {
                for &c in children {
                    self.search_at(c, bounds, out);
                }
            }
        }
    }
}

/// Two boxes with their item groups, as produced by [`split_items`].
type Split<T> = (Aabb, Vec<(Aabb, T)>, Aabb, Vec<(Aabb, T)>);

/// Splits overflowing items into two groups. Seeds are the pair that wastes the
/// most margin together. Each remaining item joins the group it enlarges least,
/// and both groups keep at least `MIN_ENTRIES` items.
fn split_items<T: Copy>(items: Vec<(Aabb, T)>) -> Split<T> {
    let n = items.len();
    let (mut s1, mut s2, mut worst) = (0, 1, f64::NEG_INFINITY);
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (items[i].0, items[j].0);
            let waste = a.union(&b).margin() - a.margin() - b.margin();
            if waste > worst {
                (s1, s2, worst) = (i, j, waste);
            }
        }
    }

    let mut bbox_a = items[s1].0;
    let mut bbox_b = items[s2].0;
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut rest = Vec::new();
    for (k, item) in items.into_iter().enumerate() {
        if k == s1 {
            a.push(item);
        } else if k == s2 {
            b.push(item);
        } else {
            rest.push(item);
        }
    }

    let total = rest.len();
    for (k, item) in rest.into_iter().enumerate() {
        let remaining = total - k;
        let to_a = if a.len() + remaining <= MIN_ENTRIES {
            true
        } else if b.len() + remaining <= MIN_ENTRIES {
            false
        } else {
            let cost_a = bbox_a.union(&item.0).margin() - bbox_a.margin();
            let cost_b = bbox_b.union(&item.0).margin() - bbox_b.margin();
            cost_a < cost_b || (cost_a == cost_b && a.len() <= b.len())
        };
        if to_a {
            bbox_a = bbox_a.union(&item.0);
            a.push(item);
        } else {
            bbox_b = bbox_b.union(&item.0);
            b.push(item);
        }
    }
    (bbox_a, a, bbox_b, b)
}

/// An [`Index`] answering `Intersects` on a field holding [`Value::Aabb`].
#[derive(Debug)]
pub struct RTreeIndex {
    name: alloc::string::String,
    ty: EntityTypeId,
    field: FieldPath,
    tree: RTree,
}

impl RTreeIndex {
    /// Creates an empty R-tree index. Add it with [`crate::MemorySource::add_index`].
    pub fn new(name: &str, ty: EntityTypeId, field: impl Into<FieldPath>) -> Self {
        Self {
            name: alloc::string::String::from(name),
            ty,
            field: field.into(),
            tree: RTree::new(),
        }
    }
}

impl Index for RTreeIndex {
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
        if let Value::Aabb(bbox) = value {
            self.tree.insert(*bbox, id);
        }
    }

    fn on_remove(&mut self, id: EntityId, value: &Value) {
        if let Value::Aabb(bbox) = value {
            self.tree.remove(bbox, id);
        }
    }

    fn lookup<'a>(&'a self, cond: &Condition) -> Option<crate::source::IndexHit<'a>> {
        match cond {
            Condition::Intersects { field, bounds } if field == &self.field => {
                // The tree search is not lazy, so the hits are collected here.
                let mut out = Vec::new();
                self.tree.search(bounds, &mut out);
                out.sort_unstable();
                out.dedup();
                Some(crate::source::IndexHit {
                    index: self.name.clone(),
                    len: out.len(),
                    ids: Box::new(out.into_iter()),
                })
            }
            Condition::Ray {
                field,
                origin,
                direction,
                within,
            } if field == &self.field => {
                // The ray search is exact per box, so its hits are the answer.
                let mut ids: Vec<EntityId> = self
                    .tree
                    .ray(*origin, *direction, *within)
                    .into_iter()
                    .map(|(_, id)| id)
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                Some(crate::source::IndexHit {
                    index: self.name.clone(),
                    len: ids.len(),
                    ids: Box::new(ids.into_iter()),
                })
            }
            _ => None,
        }
    }
}

/// Distance along a ray, `origin + t * dir` for `t` in `[0, max_t]`, at which the ray
/// first enters `bbox`. `None` if it misses the box or enters after `max_t`. A ray that
/// starts inside the box enters at `0`.
pub fn ray_enter(bbox: &Aabb, origin: [f64; 3], dir: [f64; 3], max_t: f64) -> Option<f64> {
    bbox.ray_enter(origin, dir, max_t)
}

/// Squared distance from `p` to the nearest point of `bbox` (zero inside it).
fn dist2(bbox: &Aabb, p: &[f64; 3]) -> f64 {
    bbox.distance_sq(p)
}

impl RTree {
    /// The `k` entries nearest to `point`, closest first. Distance is measured from the
    /// point to each entry's box, so a box that contains the point is at distance zero.
    /// Ties are broken by id.
    pub fn nearest(&self, point: [f64; 3], k: usize) -> Vec<EntityId> {
        use alloc::collections::BinaryHeap;
        use core::cmp::Reverse;

        let mut out = Vec::new();
        if k == 0 {
            return out;
        }
        // Keys are (squared distance bits, 0 for a node or 1 for an entry, index or id).
        // Non-negative floats order the same as their bit patterns.
        let mut heap = BinaryHeap::new();
        let root = &self.nodes[self.root];
        heap.push(Reverse((
            dist2(&root.bbox, &point).to_bits(),
            0u8,
            self.root as u64,
        )));
        while let Some(Reverse((_, kind, v))) = heap.pop() {
            if kind == 1 {
                out.push(EntityId(v));
                if out.len() == k {
                    break;
                }
                continue;
            }
            match &self.nodes[v as usize].kind {
                Kind::Leaf(entries) => {
                    for (b, id) in entries {
                        heap.push(Reverse((dist2(b, &point).to_bits(), 1, id.0)));
                    }
                }
                Kind::Inner(children) => {
                    for &c in children {
                        let d = dist2(&self.nodes[c].bbox, &point);
                        heap.push(Reverse((d.to_bits(), 0, c as u64)));
                    }
                }
            }
        }
        out
    }

    /// Entries whose box the ray enters within `max_t`, as `(entry distance, id)`,
    /// nearest first. Ties are broken by id. See [`ray_enter`].
    pub fn ray(&self, origin: [f64; 3], dir: [f64; 3], max_t: f64) -> Vec<(f64, EntityId)> {
        let mut out = Vec::new();
        let mut stack = alloc::vec![self.root];
        while let Some(idx) = stack.pop() {
            let node = &self.nodes[idx];
            if ray_enter(&node.bbox, origin, dir, max_t).is_none() {
                continue;
            }
            match &node.kind {
                Kind::Leaf(entries) => {
                    for (b, id) in entries {
                        if let Some(t) = ray_enter(b, origin, dir, max_t) {
                            out.push((t, *id));
                        }
                    }
                }
                Kind::Inner(children) => stack.extend(children.iter().copied()),
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }
}
