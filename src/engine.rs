//! Execution engine: validation, planning, streaming evaluation, and result rows.
//!
//! Pipeline: scan or index candidates -> residual filters -> traversal stages ->
//! optional sort -> projection -> limit. Scans and traversals are lazy, so a
//! `limit` stops work early.

use alloc::{boxed::Box, string::String, vec::Vec};
use core::{cmp::Ordering, fmt};

use crate::{
    ast::{Condition, Order, Query, Traversal},
    source::{
        Entity, EntityId, EntitySource, EntityTypeId, FieldPath, IndexHit, RelationMap, Value,
    },
    traverse::Walker,
};

/// Errors from validating, planning, or executing a query.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryError {
    /// The query traverses or relates entities, but the source has no relationship map.
    NoRelationMap,
    /// A traversal or index returned an id the source cannot resolve, so an index or
    /// relation is stale.
    MissingEntity(EntityId),
    /// The query is malformed, or its JSON form could not be parsed.
    InvalidQuery(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::NoRelationMap => {
                f.write_str("query needs relationships, but the source has no relation map")
            }
            QueryError::MissingEntity(id) => {
                write!(f, "entity {} is referenced but not stored", id.0)
            }
            QueryError::InvalidQuery(msg) => write!(f, "invalid query: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for QueryError {}

/// One result row. Borrows from the source, so producing it copies nothing.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    /// Id of the row's entity.
    pub id: EntityId,
    /// The entity itself.
    pub entity: &'a Entity,
    /// Projected field values, in the order of the query's projections.
    /// `None` means the field is missing.
    pub projected: Vec<Option<&'a Value>>,
}

impl<'a> Row<'a> {
    fn new(entity: &'a Entity, projections: &[crate::ast::Projection]) -> Self {
        Self {
            id: entity.id,
            entity,
            projected: projections.iter().map(|p| entity.field(&p.field)).collect(),
        }
    }
}

/// Streaming result set. Yields `Result<Row, QueryError>` lazily.
pub struct QueryResultIterator<'a> {
    inner: Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a>,
}

impl<'a> Iterator for QueryResultIterator<'a> {
    type Item = Result<Row<'a>, QueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// How the engine reads the source's candidate ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Full scan of every entity of the source type, in id order.
    Scan {
        /// The source type being scanned.
        ty: EntityTypeId,
    },
    /// Candidate ids from an index.
    Index {
        /// Index used.
        index: String,
        /// Number of candidate ids the index returned.
        candidates: usize,
    },
}

/// The strategy the engine will use for a query. Returned by [`Queryable::explain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Candidate access path.
    pub access: Access,
    /// Filters still evaluated per entity after the access path.
    pub residual_filters: usize,
    /// Number of traversal stages.
    pub traversals: usize,
    /// Whether the result is sorted, which materializes it.
    pub ordered: bool,
    /// Row limit, if any.
    pub limit: Option<usize>,
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.access {
            Access::Scan { ty } => writeln!(f, "access: scan entities of type {}", ty.0)?,
            Access::Index { index, candidates } => {
                writeln!(f, "access: index `{index}` ({candidates} candidates)")?
            }
        }
        writeln!(f, "residual filters: {}", self.residual_filters)?;
        writeln!(f, "traversals: {}", self.traversals)?;
        writeln!(f, "ordered: {}", self.ordered)?;
        match self.limit {
            Some(n) => writeln!(f, "limit: {n}"),
            None => writeln!(f, "limit: none"),
        }
    }
}

/// Executes queries against a source.
pub trait Queryable {
    /// Starts executing `query`. Validation and planning errors surface here;
    /// row-level errors surface from the iterator.
    fn execute<'a>(&'a self, query: &'a Query) -> Result<QueryResultIterator<'a>, QueryError>;

    /// Returns the plan the engine would use, without executing it.
    fn explain(&self, query: &Query) -> Result<Plan, QueryError>;
}

impl<S: EntitySource> Queryable for S {
    fn execute<'a>(&'a self, query: &'a Query) -> Result<QueryResultIterator<'a>, QueryError> {
        run(self, query)
    }

    fn explain(&self, query: &Query) -> Result<Plan, QueryError> {
        prepare(self, query).map(|p| p.plan)
    }
}

struct Prepared<'a> {
    plan: Plan,
    ids: Option<Vec<EntityId>>,
    residual: Vec<&'a Condition>,
}

/// Validates the query and picks the access path. Picks the index with the
/// fewest candidates among the top-level filters. The filter that index answers
/// exactly is then dropped from the residual set.
fn prepare<'a>(src: &dyn EntitySource, q: &'a Query) -> Result<Prepared<'a>, QueryError> {
    q.validate()?;
    if q.needs_relations() && src.relations().is_none() {
        return Err(QueryError::NoRelationMap);
    }

    let mut best: Option<(usize, IndexHit)> = None;
    for (i, cond) in q.filters.iter().enumerate() {
        if let Some(hit) = src.index_lookup(q.source, cond) {
            let better = best
                .as_ref()
                .is_none_or(|(_, b)| hit.ids.len() < b.ids.len());
            if better {
                best = Some((i, hit));
            }
        }
    }

    let (access, ids, residual) = match best {
        Some((chosen, hit)) => {
            let residual: Vec<&Condition> = q
                .filters
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != chosen)
                .map(|(_, c)| c)
                .collect();
            let access = Access::Index {
                index: hit.index,
                candidates: hit.ids.len(),
            };
            (access, Some(hit.ids), residual)
        }
        None => (
            Access::Scan { ty: q.source },
            None,
            q.filters.iter().collect(),
        ),
    };

    let plan = Plan {
        access,
        residual_filters: residual.len(),
        traversals: q.traversals.len(),
        ordered: q.order_by.is_some(),
        limit: q.limit,
    };
    Ok(Prepared {
        plan,
        ids,
        residual,
    })
}

type Stream<'a> = Box<dyn Iterator<Item = Result<&'a Entity, QueryError>> + 'a>;

fn run<'a>(src: &'a dyn EntitySource, q: &'a Query) -> Result<QueryResultIterator<'a>, QueryError> {
    let prep = prepare(src, q)?;

    let ids: Box<dyn Iterator<Item = EntityId> + 'a> = match prep.ids {
        Some(ids) => Box::new(ids.into_iter()),
        None => src.ids_of_type(q.source),
    };
    let residual = prep.residual;
    let scanned = ids
        .map(move |id| src.get(id).ok_or(QueryError::MissingEntity(id)))
        .filter(move |r| match r {
            Ok(e) => residual.iter().all(|c| eval(c, e, src)),
            Err(_) => true,
        });

    let mut stream: Stream<'a> = Box::new(scanned);
    if !q.traversals.is_empty() {
        let rel = src.relations().ok_or(QueryError::NoRelationMap)?;
        for t in &q.traversals {
            stream = traversal_stage(src, rel, t, stream);
        }
    }

    let mut rows: Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a> =
        Box::new(stream.map(move |r| r.map(|e| Row::new(e, &q.projections))));

    if let Some(order) = &q.order_by {
        let mut buf: Vec<Row<'a>> = Vec::new();
        for row in rows {
            buf.push(row?);
        }
        buf.sort_by(|a, b| {
            let ord = a
                .entity
                .field(&order.field)
                .cmp(&b.entity.field(&order.field));
            let ord = if order.descending { ord.reverse() } else { ord };
            ord.then(a.id.cmp(&b.id))
        });
        rows = Box::new(buf.into_iter().map(Ok));
    }

    if let Some(n) = q.limit {
        rows = Box::new(rows.take(n));
    }
    Ok(QueryResultIterator { inner: rows })
}

/// One traversal stage: each input entity expands into the entities reachable
/// from it that pass the stage's filters.
fn traversal_stage<'a>(
    src: &'a dyn EntitySource,
    rel: &'a dyn RelationMap,
    t: &'a Traversal,
    input: Stream<'a>,
) -> Stream<'a> {
    Box::new(input.flat_map(move |row| -> Stream<'a> {
        match row {
            Err(e) => Box::new(core::iter::once(Err(e))),
            Ok(start) => Box::new(
                Walker::new(rel, start.id, &t.edge, t.order, t.max_depth)
                    .map(move |id| src.get(id).ok_or(QueryError::MissingEntity(id)))
                    .filter(move |r| match r {
                        Ok(e) => t.filters.iter().all(|c| eval(c, e, src)),
                        Err(_) => true,
                    }),
            ),
        }
    }))
}

/// Evaluates a condition against one entity.
pub(crate) fn eval(cond: &Condition, e: &Entity, src: &dyn EntitySource) -> bool {
    match cond {
        Condition::Eq { field, value } => e.field(field).is_some_and(|v| v == value),
        Condition::Ne { field, value } => e.field(field).is_some_and(|v| v != value),
        Condition::Gt { field, value } => ordered(e, field, value, |o| o == Ordering::Greater),
        Condition::Gte { field, value } => ordered(e, field, value, |o| o != Ordering::Less),
        Condition::Lt { field, value } => ordered(e, field, value, |o| o == Ordering::Less),
        Condition::Lte { field, value } => ordered(e, field, value, |o| o != Ordering::Greater),
        Condition::Contains { field, value } => match e.field(field) {
            Some(Value::Str(s)) => {
                matches!(value, Value::Str(needle) if s.contains(needle.as_str()))
            }
            Some(Value::List(items)) => items.iter().any(|v| v == value),
            _ => false,
        },
        Condition::Intersects { field, bounds } => {
            matches!(e.field(field), Some(Value::Aabb(a)) if a.intersects(bounds))
        }
        Condition::And(cs) => cs.iter().all(|c| eval(c, e, src)),
        Condition::Or(cs) => cs.iter().any(|c| eval(c, e, src)),
        Condition::Not(c) => !eval(c, e, src),
        Condition::OfType(ty) => e.ty == *ty,
        Condition::Related {
            edge,
            max_depth,
            cond,
        } => match src.relations() {
            None => false,
            Some(rel) => Walker::new(rel, e.id, edge, Order::BreadthFirst, *max_depth)
                .any(|id| src.get(id).is_some_and(|n| eval(cond, n, src))),
        },
    }
}

fn ordered(e: &Entity, field: &FieldPath, bound: &Value, pred: impl Fn(Ordering) -> bool) -> bool {
    match e.field(field) {
        Some(v) if v.rank() == bound.rank() => pred(v.cmp(bound)),
        _ => false,
    }
}
