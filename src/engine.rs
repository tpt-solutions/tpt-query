//! Execution engine: validation, planning, streaming evaluation, and result rows.
//!
//! Pipeline: scan or index candidates -> residual filters -> traversal stages ->
//! optional sort -> projection -> limit. Scans and traversals are lazy, so a
//! `limit` stops work early.

use alloc::{boxed::Box, collections::BinaryHeap, rc::Rc, string::String, vec, vec::Vec};
use core::{cell::Cell, cmp::Ordering, fmt};

use crate::{
    ast::{Condition, Order, OrderBy, Query, Traversal},
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
    /// An index with this name already exists on the source.
    DuplicateIndex(String),
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
            QueryError::DuplicateIndex(name) => write!(f, "index `{name}` already exists"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for QueryError {}

/// One result row. Borrows from the source, so producing it copies nothing.
#[derive(Debug, Clone)]
#[non_exhaustive]
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

    /// The entity's field at `path`, e.g. `"material.name"`. `None` if it is missing.
    pub fn get(&self, path: &str) -> Option<&'a Value> {
        let entity: &'a Entity = self.entity;
        entity.field(&FieldPath::parse(path))
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
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// Conditions that no index answers, so they are checked per candidate. Each entry is
    /// a short description such as `gt on `mass``.
    pub unindexed: Vec<String>,
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
        }?;
        if self.unindexed.is_empty() {
            writeln!(f, "unindexed: none")
        } else {
            writeln!(f, "unindexed: {}", self.unindexed.join(", "))
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

    /// Runs an aggregate query (one with [`Query::aggregate`] set). Returns
    /// `(key, value)` pairs in key order. Without `group_by` there is one pair, with
    /// a `Null` key.
    fn aggregate(&self, query: &Query) -> Result<Vec<(Value, Value)>, QueryError>;

    /// Runs `query` and reports the row count at each stage. Rows are read and discarded.
    /// Use it to see where a query spends its candidates.
    fn analyze(&self, query: &Query) -> Result<Analysis, QueryError>;
}

impl<S: EntitySource> Queryable for S {
    fn execute<'a>(&'a self, query: &'a Query) -> Result<QueryResultIterator<'a>, QueryError> {
        if query.aggregate.is_some() {
            return Err(QueryError::InvalidQuery(alloc::string::String::from(
                "aggregate queries run through `aggregate`, not `execute`",
            )));
        }
        run(self, query)
    }

    fn explain(&self, query: &Query) -> Result<Plan, QueryError> {
        prepare(self, query).map(|p| p.plan)
    }

    fn analyze(&self, query: &Query) -> Result<Analysis, QueryError> {
        if query.aggregate.is_some() {
            return Err(QueryError::InvalidQuery(alloc::string::String::from(
                "analyze runs row queries; aggregate queries run through `aggregate`",
            )));
        }
        let plan = prepare(self, query)?.plan;
        let counts = Rc::new(Counts::default());
        let mut rows = run_with(self, query, Some(Rc::clone(&counts)))?;
        rows.try_for_each(|row| row.map(|_| ()))?;
        Ok(Analysis {
            plan,
            read: counts.read.get(),
            passed_filters: counts.passed_filters.get(),
            after_traversal: counts.after_traversal.get(),
            returned: counts.returned.get(),
        })
    }

    fn aggregate(&self, query: &Query) -> Result<Vec<(Value, Value)>, QueryError> {
        let Some(spec) = &query.aggregate else {
            return Err(QueryError::InvalidQuery(alloc::string::String::from(
                "query has no aggregation; set one with `Query::aggregate`",
            )));
        };
        let rows = run(self, query)?;
        match &spec.group_by {
            Some(key) => crate::aggregate::group_by(rows, key, &spec.metric),
            None => Ok(alloc::vec![(
                Value::Null,
                crate::aggregate::aggregate(rows, &spec.metric)?
            )]),
        }
    }
}

struct Prepared<'a> {
    plan: Plan,
    /// Candidates from the index that answers one conjunct, if any index applies.
    indexed: Option<IndexHit<'a>>,
    /// Conjuncts still evaluated per candidate.
    residual: Vec<&'a Condition>,
}

/// Validates the query and picks the access path. Top-level `And`s are split into
/// conjuncts, and the index with the fewest candidates for any conjunct answers it.
/// That conjunct is then dropped from the residual set, since the index answers it
/// exactly. Only counts are needed for planning, and index lookups are lazy, so
/// planning does not read the candidates.
fn prepare<'a>(src: &'a dyn EntitySource, q: &'a Query) -> Result<Prepared<'a>, QueryError> {
    q.validate()?;
    if q.needs_relations() && src.relations().is_none() {
        return Err(QueryError::NoRelationMap);
    }

    let mut conjuncts = Vec::new();
    flatten_and(&q.filters, &mut conjuncts);

    let mut best: Option<(usize, IndexHit<'a>)> = None;
    for (i, cond) in conjuncts.iter().enumerate() {
        if let Some(hit) = src.index_lookup(q.source, cond) {
            let better = best.as_ref().is_none_or(|(_, b)| hit.len < b.len);
            if better {
                best = Some((i, hit));
            }
        }
    }

    let (access, indexed, residual) = match best {
        Some((chosen, hit)) => {
            let access = Access::Index {
                index: hit.index.clone(),
                candidates: hit.len,
            };
            let residual = conjuncts
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != chosen)
                .map(|(_, c)| *c)
                .collect();
            (access, Some(hit), residual)
        }
        None => (Access::Scan { ty: q.source }, None, conjuncts),
    };

    let plan = Plan {
        access,
        residual_filters: residual.len(),
        traversals: q.traversals.len(),
        ordered: q.order_by.is_some(),
        limit: q.limit,
        unindexed: residual.iter().map(|c| describe(c)).collect(),
    };
    Ok(Prepared {
        plan,
        indexed,
        residual,
    })
}

/// Splits top-level `And`s so that each conjunct can be answered by an index.
fn flatten_and<'a>(conds: &'a [Condition], out: &mut Vec<&'a Condition>) {
    for cond in conds {
        match cond {
            Condition::And(inner) => flatten_and(inner, out),
            _ => out.push(cond),
        }
    }
}

type Stream<'a> = Box<dyn Iterator<Item = Result<&'a Entity, QueryError>> + 'a>;

fn run<'a>(src: &'a dyn EntitySource, q: &'a Query) -> Result<QueryResultIterator<'a>, QueryError> {
    run_with(src, q, None)
}

fn run_with<'a>(
    src: &'a dyn EntitySource,
    q: &'a Query,
    counts: Option<Rc<Counts>>,
) -> Result<QueryResultIterator<'a>, QueryError> {
    let prep = prepare(src, q)?;

    let ids: Box<dyn Iterator<Item = EntityId> + 'a> = match prep.indexed {
        Some(hit) => hit.ids,
        None => src.ids_of_type(q.source),
    };
    let ids = tally(ids, &counts, |c| &c.read);
    let ids: Box<dyn Iterator<Item = EntityId> + 'a> = match q.after {
        Some(after) => Box::new(ids.filter(move |id| *id > after)),
        None => ids,
    };
    let residual = prep.residual;
    let scanned = ids
        .map(move |id| src.get(id).ok_or(QueryError::MissingEntity(id)))
        .filter(move |r| match r {
            Ok(e) => residual.iter().all(|c| eval(c, e, src)),
            Err(_) => true,
        });

    let scanned = tally(Box::new(scanned), &counts, |c| &c.passed_filters);
    let mut stream: Stream<'a> = Box::new(scanned);
    if !q.traversals.is_empty() {
        let rel = src.relations().ok_or(QueryError::NoRelationMap)?;
        for t in &q.traversals {
            stream = traversal_stage(src, rel, t, stream);
        }
    }

    if q.distinct {
        let mut seen = alloc::collections::BTreeSet::new();
        stream = Box::new(stream.filter(move |r| match r {
            Ok(e) => seen.insert(e.id),
            Err(_) => true,
        }));
    }

    stream = tally(stream, &counts, |c| &c.after_traversal);
    let mut rows: Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a> =
        Box::new(stream.map(move |r| r.map(|e| Row::new(e, &q.projections))));

    if let Some(order) = &q.order_by {
        rows = sort_rows(rows, order, q.limit);
    }

    if q.offset > 0 {
        rows = Box::new(rows.skip(q.offset));
    }

    if let Some(n) = q.limit {
        rows = Box::new(rows.take(n));
    }
    let rows = tally(rows, &counts, |c| &c.returned);
    Ok(QueryResultIterator { inner: rows })
}

/// Row counts at each stage of a run, filled in by [`Queryable::analyze`].
#[derive(Default)]
struct Counts {
    read: Cell<usize>,
    passed_filters: Cell<usize>,
    after_traversal: Cell<usize>,
    returned: Cell<usize>,
}

/// Counts the items that pass through `it`, if counting is on.
fn tally<'a, T: 'a>(
    it: Box<dyn Iterator<Item = T> + 'a>,
    counts: &Option<Rc<Counts>>,
    slot: fn(&Counts) -> &Cell<usize>,
) -> Box<dyn Iterator<Item = T> + 'a> {
    match counts {
        None => it,
        Some(c) => {
            let c = Rc::clone(c);
            Box::new(it.inspect(move |_| {
                let s = slot(&c);
                s.set(s.get() + 1);
            }))
        }
    }
}

/// Sorts rows by `order`, ties broken by ascending id. The sort runs on the first
/// pull, not when the query starts. Row errors are yielded before the sorted rows
/// instead of aborting the query. With a limit, only the best `limit` rows are kept,
/// so memory is O(limit).
fn sort_rows<'a>(
    rows: Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a>,
    order: &'a OrderBy,
    limit: Option<usize>,
) -> Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a> {
    let mut input = Some(rows);
    let mut sorted: Option<vec::IntoIter<Result<Row<'a>, QueryError>>> = None;
    Box::new(core::iter::from_fn(move || {
        if sorted.is_none() {
            let rows = input.take()?;
            sorted = Some(materialize_sorted(rows, order, limit));
        }
        sorted.as_mut()?.next()
    }))
}

fn materialize_sorted<'a>(
    rows: Box<dyn Iterator<Item = Result<Row<'a>, QueryError>> + 'a>,
    order: &'a OrderBy,
    limit: Option<usize>,
) -> vec::IntoIter<Result<Row<'a>, QueryError>> {
    let mut out = Vec::new();
    // A max-heap, so its top is the row that sorts last. Popping it keeps the best `limit`.
    let mut top = BinaryHeap::new();
    for row in rows {
        match row {
            Ok(row) => {
                top.push(Ranked { order, row });
                if limit.is_some_and(|k| top.len() > k) {
                    top.pop();
                }
            }
            Err(e) => out.push(Err(e)),
        }
    }
    out.extend(top.into_sorted_vec().into_iter().map(|r| Ok(r.row)));
    out.into_iter()
}

/// A row with its sort key, ordered as the result set is.
struct Ranked<'a> {
    order: &'a OrderBy,
    row: Row<'a>,
}

impl Ranked<'_> {
    fn sort_key(&self, other: &Self) -> Ordering {
        let ord = self
            .row
            .entity
            .field(&self.order.field)
            .cmp(&other.row.entity.field(&self.order.field));
        let ord = if self.order.descending {
            ord.reverse()
        } else {
            ord
        };
        ord.then(self.row.id.cmp(&other.row.id))
    }
}

impl PartialEq for Ranked<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.sort_key(other) == Ordering::Equal
    }
}

impl Eq for Ranked<'_> {}

impl PartialOrd for Ranked<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key(other)
    }
}

/// One traversal stage: each input entity expands into the entities reachable
/// from it that pass the stage's filters.
fn traversal_stage<'a>(
    src: &'a dyn EntitySource,
    rel: &'a dyn RelationMap,
    t: &'a Traversal,
    input: Stream<'a>,
) -> Stream<'a> {
    // Built once per stage; the walker owns a copy of the predicate per start node.
    let descend_conditions: &'a [Condition] = &t.descend;
    Box::new(input.flat_map(move |row| -> Stream<'a> {
        let descend: Option<Box<dyn Fn(EntityId) -> bool + 'a>> = if descend_conditions.is_empty() {
            None
        } else {
            Some(Box::new(move |id: EntityId| {
                src.get(id)
                    .is_some_and(|e| descend_conditions.iter().all(|c| eval(c, e, src)))
            }))
        };
        match row {
            Err(e) => Box::new(core::iter::once(Err(e))),
            Ok(start) => Box::new(
                Walker::new(rel, start.id, &t.edge, t.order, t.max_depth, descend)
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
        Condition::OfTypeIn(types) => types.contains(&e.ty),
        Condition::ContainsPoint { field, point } => {
            matches!(e.field(field), Some(Value::Aabb(a))
                if (0..3).all(|i| a.min[i] <= point[i] && point[i] <= a.max[i]))
        }
        Condition::Related {
            edge,
            max_depth,
            cond,
        } => match src.relations() {
            None => false,
            Some(rel) => Walker::new(rel, e.id, edge, Order::BreadthFirst, *max_depth, None)
                .any(|id| src.get(id).is_some_and(|n| eval(cond, n, src))),
        },
        Condition::In { field, values } => e.field(field).is_some_and(|v| values.contains(v)),
        Condition::Exists { field } => e.field(field).is_some_and(|v| !matches!(v, Value::Null)),
        Condition::StartsWith { field, value } => matches!(
            (e.field(field), value),
            (Some(Value::Str(s)), Value::Str(p)) if s.starts_with(p.as_str())
        ),
        Condition::EndsWith { field, value } => matches!(
            (e.field(field), value),
            (Some(Value::Str(s)), Value::Str(p)) if s.ends_with(p.as_str())
        ),
        Condition::Between { field, low, high } => {
            ordered(e, field, low, |o| o != Ordering::Less)
                && ordered(e, field, high, |o| o != Ordering::Greater)
        }
    }
}

fn ordered(e: &Entity, field: &FieldPath, bound: &Value, pred: impl Fn(Ordering) -> bool) -> bool {
    match e.field(field) {
        Some(v) if v.rank() == bound.rank() => pred(v.cmp(bound)),
        _ => false,
    }
}

/// Result of [`Queryable::analyze`]: the plan, and how many rows reached each stage.
/// Stages count only the work the run did, so a `limit` stops the counts early.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Analysis {
    /// The plan the run used.
    pub plan: Plan,
    /// Candidates read from the access path, before the cursor and residual filters.
    pub read: usize,
    /// Entities that passed the residual filters.
    pub passed_filters: usize,
    /// Rows after traversal stages and `distinct`.
    pub after_traversal: usize,
    /// Rows returned, after sorting, `offset`, and `limit`.
    pub returned: usize,
}

/// A short description of a condition for plans: its operator and field.
fn describe(c: &Condition) -> String {
    let on = |op: &str, field: &FieldPath| alloc::format!("{op} on `{field}`");
    match c {
        Condition::Eq { field, .. } => on("eq", field),
        Condition::Ne { field, .. } => on("ne", field),
        Condition::Gt { field, .. } => on("gt", field),
        Condition::Gte { field, .. } => on("gte", field),
        Condition::Lt { field, .. } => on("lt", field),
        Condition::Lte { field, .. } => on("lte", field),
        Condition::Contains { field, .. } => on("contains", field),
        Condition::Intersects { field, .. } => on("intersects", field),
        Condition::ContainsPoint { field, .. } => on("contains_point", field),
        Condition::In { field, .. } => on("in", field),
        Condition::Exists { field } => on("exists", field),
        Condition::StartsWith { field, .. } => on("starts_with", field),
        Condition::EndsWith { field, .. } => on("ends_with", field),
        Condition::Between { field, .. } => on("between", field),
        Condition::And(_) => String::from("and"),
        Condition::Or(_) => String::from("or"),
        Condition::Not(_) => String::from("not"),
        Condition::OfType(_) => String::from("of_type"),
        Condition::OfTypeIn(_) => String::from("of_type_in"),
        Condition::Related { .. } => String::from("related"),
    }
}
