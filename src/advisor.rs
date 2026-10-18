//! Index advisor: suggests indexes for a workload of queries.
//!
//! A field used in a top-level condition is a candidate. A field that appears in a
//! range (`Gt`, `Gte`, `Lt`, `Lte`, `Between`) gets a B-tree, since it also serves
//! equality. A field that only appears in equality or membership gets a hash index,
//! and a box field gets an R-tree. The advisor does not know what already exists, so
//! the caller drops suggestions it has already built.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    ast::{Condition, Query},
    source::{EntityTypeId, FieldPath},
};

/// The index a suggestion asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SuggestedKind {
    /// Hash index: equality and membership.
    Hash,
    /// B-tree index: equality and ranges.
    BTree,
    /// R-tree index: box intersection (feature `spatial`).
    RTree,
}

/// One suggested index, with how many workload queries it would help.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexSuggestion {
    /// Entity type the index covers.
    pub ty: EntityTypeId,
    /// Field to index.
    pub field: FieldPath,
    /// Index kind to build.
    pub kind: SuggestedKind,
    /// Number of queries in the workload that filter on this field.
    pub queries: usize,
}

#[derive(Default)]
struct Needs {
    equality: bool,
    range: bool,
    boxes: bool,
}

/// Suggests indexes for `workload`, most useful first. Ties break by type and field.
pub fn suggest_indexes(workload: &[Query]) -> Vec<IndexSuggestion> {
    let mut needs: BTreeMap<(EntityTypeId, String), (FieldPath, Needs, usize)> = BTreeMap::new();
    for query in workload {
        let mut seen = BTreeSet::new();
        let mut conjuncts = Vec::new();
        flatten(&query.filters, &mut conjuncts);
        for cond in conjuncts {
            let Some((field, need)) = classify(cond) else {
                continue;
            };
            let key = (query.source, field.to_string());
            if !seen.insert(key.clone()) {
                // One query counts once per field, however many conditions it has on it.
                let entry = needs.get_mut(&key).expect("recorded above");
                merge(&mut entry.1, need);
                continue;
            }
            let entry = needs
                .entry(key)
                .or_insert_with(|| (field.clone(), Needs::default(), 0));
            entry.2 += 1;
            merge(&mut entry.1, need);
        }
    }

    let mut out: Vec<IndexSuggestion> = needs
        .into_iter()
        .map(|((ty, _), (field, need, queries))| IndexSuggestion {
            ty,
            field,
            kind: if need.boxes {
                SuggestedKind::RTree
            } else if need.range {
                SuggestedKind::BTree
            } else {
                SuggestedKind::Hash
            },
            queries,
        })
        .collect();
    out.sort_by(|a, b| {
        b.queries
            .cmp(&a.queries)
            .then(a.ty.cmp(&b.ty))
            .then(a.field.to_string().cmp(&b.field.to_string()))
    });
    out
}

fn merge(into: &mut Needs, from: Needs) {
    into.equality |= from.equality;
    into.range |= from.range;
    into.boxes |= from.boxes;
}

fn flatten<'a>(conds: &'a [Condition], out: &mut Vec<&'a Condition>) {
    for c in conds {
        match c {
            Condition::And(inner) => flatten(inner, out),
            _ => out.push(c),
        }
    }
}

fn classify(cond: &Condition) -> Option<(&FieldPath, Needs)> {
    let one = |field, need: fn(&mut Needs)| {
        let mut n = Needs::default();
        need(&mut n);
        Some((field, n))
    };
    match cond {
        Condition::Eq { field, .. } | Condition::In { field, .. } => {
            one(field, |n| n.equality = true)
        }
        Condition::Gt { field, .. }
        | Condition::Gte { field, .. }
        | Condition::Lt { field, .. }
        | Condition::Lte { field, .. }
        | Condition::Between { field, .. } => one(field, |n| n.range = true),
        Condition::Intersects { field, .. } => one(field, |n| n.boxes = true),
        _ => None,
    }
}
