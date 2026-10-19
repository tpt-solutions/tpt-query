//! The query representation: a typed Rust builder whose shape also serves as the
//! JSON AST (with the `serde` feature).

use alloc::{boxed::Box, format, string::String, vec::Vec};

use crate::{
    aggregate::{Aggregation, Metric},
    engine::QueryError,
    source::{Aabb, EntityId, EntityTypeId, FieldPath, Value},
};

/// A complete query against one entity type.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Query {
    /// Entity type the query starts from.
    pub source: EntityTypeId,
    /// Conditions on source entities. All must hold (implicit AND).
    #[cfg_attr(feature = "serde", serde(default))]
    pub filters: Vec<Condition>,
    /// Traversal stages, applied in order to the filtered source entities.
    #[cfg_attr(feature = "serde", serde(default))]
    pub traversals: Vec<Traversal>,
    /// Fields to extract into each row. Empty means no projection.
    #[cfg_attr(feature = "serde", serde(default))]
    pub projections: Vec<Projection>,
    /// Maximum number of rows to yield. Short-circuits streaming execution.
    #[cfg_attr(feature = "serde", serde(default))]
    pub limit: Option<usize>,
    /// Sort order. Materializes the result set, so use it only when needed.
    #[cfg_attr(feature = "serde", serde(default))]
    pub order_by: Option<OrderBy>,
    /// Rows to skip before `limit` applies. Counted after traversals and sorting.
    #[cfg_attr(feature = "serde", serde(default))]
    pub offset: usize,
    /// Resume after this source entity id. Source ids are read in ascending order, so
    /// the next page starts at the first id greater than the last id seen. Not allowed
    /// with `order_by`, since sorted results have no such cursor.
    #[cfg_attr(feature = "serde", serde(default))]
    pub after: Option<EntityId>,
    /// Turns the query into a reduction over its rows. Use `Queryable::aggregate` to
    /// run it. Aggregate queries take no projections, ordering, limit, offset, or cursor.
    #[cfg_attr(feature = "serde", serde(default))]
    pub aggregate: Option<Aggregation>,
    /// Yield each source-or-reached entity once, the first time it appears. Costs memory
    /// proportional to the number of distinct entities yielded.
    #[cfg_attr(feature = "serde", serde(default))]
    pub distinct: bool,
    /// Keep only the `k` rows whose box is nearest to a point, nearest first. Ties break
    /// by ascending entity id. Rows whose field is not a box are dropped. Cannot be
    /// combined with `order_by`, `after`, `offset`, `limit`, or `aggregate`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub nearest: Option<Nearest>,
}

/// The nearest-`k` selection in a query. See [`Query::nearest`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Nearest {
    /// Field holding a [`Value::Aabb`].
    pub field: FieldPath,
    /// The point to measure from, as `[x, y, z]`.
    pub point: [f64; 3],
    /// How many rows to keep.
    pub k: usize,
}

/// A field to extract into each result row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Projection {
    /// Field to extract.
    pub field: FieldPath,
}

/// Sort key for the result set. Ties break by ascending entity id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OrderBy {
    /// Field to sort by. Missing fields sort first.
    pub field: FieldPath,
    /// Sort descending instead of ascending.
    pub descending: bool,
}

/// A relationship to follow from each row.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Traversal {
    /// Relationship to follow.
    pub edge: Edge,
    /// Visit order when `max_depth` is greater than one.
    pub order: Order,
    /// Maximum depth. `Some(1)` is a single hop, `None` is unbounded.
    /// Cycles are cut by a visited set.
    pub max_depth: Option<usize>,
    /// Filters on reached entities. All must hold. Applied to the entities the walk
    /// yields. A node that fails them is still walked through, so its descendants
    /// can still match.
    #[cfg_attr(feature = "serde", serde(default))]
    pub filters: Vec<Condition>,
    /// Conditions a node must meet before the walk continues below it. The start node
    /// is always expanded, and a node that fails these is still yielded if it passes
    /// `filters`. Empty means descend everywhere.
    #[cfg_attr(feature = "serde", serde(default))]
    pub descend: Vec<Condition>,
}

impl Traversal {
    /// A single hop along `edge`.
    pub fn step(edge: Edge) -> Self {
        Self {
            edge,
            order: Order::DepthFirst,
            max_depth: Some(1),
            filters: Vec::new(),
            descend: Vec::new(),
        }
    }

    /// A walk along `edge` up to `max_depth` (`None` for unbounded).
    pub fn recursive(edge: Edge, order: Order, max_depth: Option<usize>) -> Self {
        Self {
            edge,
            order,
            max_depth,
            filters: Vec::new(),
            descend: Vec::new(),
        }
    }

    /// Stops the walk below nodes that fail `condition`. See [`Traversal::descend`].
    pub fn descend_while(mut self, condition: Condition) -> Self {
        self.descend.push(condition);
        self
    }

    /// Adds a filter on reached entities.
    pub fn filter(mut self, condition: Condition) -> Self {
        self.filters.push(condition);
        self
    }

    /// Checks the traversal; `loc` names it in any error (e.g. `traversals[0]`).
    pub(crate) fn validate_at(&self, loc: &str) -> Result<(), QueryError> {
        if self.max_depth == Some(0) {
            return Err(locate(
                loc,
                QueryError::InvalidQuery(String::from("traversal max_depth must be >= 1")),
            ));
        }
        self.edge.validate().map_err(|e| locate(loc, e))?;
        for (i, c) in self.descend.iter().enumerate() {
            c.validate_at(&join(loc, &format!("descend[{i}]")))?;
        }
        for (i, c) in self.filters.iter().enumerate() {
            c.validate_at(&join(loc, &format!("filters[{i}]")))?;
        }
        Ok(())
    }
}

/// A relationship between entities.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Edge {
    /// The parent in the containment tree.
    Parent,
    /// Children in the containment tree.
    Child,
    /// A named outgoing edge.
    Named(String),
}

impl Edge {
    pub(crate) fn validate(&self) -> Result<(), QueryError> {
        match self {
            Edge::Named(label) if label.is_empty() => Err(QueryError::InvalidQuery(String::from(
                "named edge label must not be empty",
            ))),
            _ => Ok(()),
        }
    }
}

/// Visit order for recursive traversals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Order {
    /// Pre-order, following children in insertion order.
    DepthFirst,
    /// Level by level.
    BreadthFirst,
}

/// A predicate over an entity. Comparison operators only match values of the
/// same kind (see [`Value`]) and are false when the field is missing.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Condition {
    /// Field equals value. Values of different kinds never match, so `Int(1)` does
    /// not equal `Float(1.0)`.
    Eq {
        /// Field to test.
        field: FieldPath,
        /// Expected value.
        value: Value,
    },
    /// Field exists and differs from value. Unlike `Not(Eq)`, this is false when the
    /// field is missing, so `Not(Eq)` also matches entities without the field.
    Ne {
        /// Field to test.
        field: FieldPath,
        /// Value to differ from.
        value: Value,
    },
    /// Field is greater than value (same kind only).
    Gt {
        /// Field to test.
        field: FieldPath,
        /// Lower bound.
        value: Value,
    },
    /// Field is greater than or equal to value (same kind only).
    Gte {
        /// Field to test.
        field: FieldPath,
        /// Lower bound, inclusive.
        value: Value,
    },
    /// Field is less than value (same kind only).
    Lt {
        /// Field to test.
        field: FieldPath,
        /// Upper bound.
        value: Value,
    },
    /// Field is less than or equal to value (same kind only).
    Lte {
        /// Field to test.
        field: FieldPath,
        /// Upper bound, inclusive.
        value: Value,
    },
    /// A string contains a substring, or a list contains an element.
    Contains {
        /// Field to test.
        field: FieldPath,
        /// Substring or element.
        value: Value,
    },
    /// Field is a bounding box that intersects `bounds`.
    Intersects {
        /// Field holding a [`Value::Aabb`].
        field: FieldPath,
        /// Query box.
        bounds: Aabb,
    },
    /// All conditions hold. An empty list is true.
    And(Vec<Condition>),
    /// Any condition holds. An empty list is false.
    Or(Vec<Condition>),
    /// The condition does not hold.
    Not(Box<Condition>),
    /// The entity has this type.
    OfType(EntityTypeId),
    /// The entity has one of these types.
    OfTypeIn(Vec<EntityTypeId>),
    /// The field is a bounding box that contains `point` (boundaries included).
    ContainsPoint {
        /// Field holding a [`Value::Aabb`].
        field: FieldPath,
        /// Point to test, as `[x, y, z]`.
        point: [f64; 3],
    },
    /// The field is a bounding box that a ray enters at a distance of at most `within`.
    /// The ray is `origin + t * direction` for `t` in `[0, within]`. A box that the ray
    /// starts inside counts as entered. A zero direction component means the ray runs
    /// parallel to that axis.
    Ray {
        /// Field holding a [`Value::Aabb`].
        field: FieldPath,
        /// Where the ray starts, as `[x, y, z]`.
        origin: [f64; 3],
        /// Direction of the ray, as `[x, y, z]`. It need not be normalized.
        direction: [f64; 3],
        /// Largest distance along the ray that counts, in units of `direction`.
        within: f64,
    },
    /// Some entity reached along `edge` (within `max_depth`) satisfies `cond`.
    Related {
        /// Relationship to follow.
        edge: Edge,
        /// Maximum depth. `None` is unbounded.
        max_depth: Option<usize>,
        /// Condition on the reached entity.
        cond: Box<Condition>,
    },
    /// Field equals one of `values`. Same matching rules as `Eq`.
    In {
        /// Field to test.
        field: FieldPath,
        /// Accepted values.
        values: Vec<Value>,
    },
    /// Field is present and not null. Use `Not(Exists)` to match missing fields too.
    Exists {
        /// Field to test.
        field: FieldPath,
    },
    /// String field starts with `value`. `value` must be a string.
    StartsWith {
        /// Field to test.
        field: FieldPath,
        /// Required prefix.
        value: Value,
    },
    /// String field ends with `value`. `value` must be a string.
    EndsWith {
        /// Field to test.
        field: FieldPath,
        /// Required suffix.
        value: Value,
    },
    /// Field lies between `low` and `high`, both inclusive (same kind only).
    Between {
        /// Field to test.
        field: FieldPath,
        /// Lower bound, inclusive.
        low: Value,
        /// Upper bound, inclusive.
        high: Value,
    },
}

impl Condition {
    /// `field == value`.
    pub fn eq_to(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Eq {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field != value`.
    pub fn ne_to(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Ne {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field > value`.
    pub fn gt(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Gt {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field >= value`.
    pub fn gte(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Gte {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field < value`.
    pub fn lt(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Lt {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field <= value`.
    pub fn lte(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Lte {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field` contains `value`.
    pub fn contains(field: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Condition::Contains {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field` intersects `bounds`.
    pub fn intersects(field: impl Into<FieldPath>, bounds: Aabb) -> Self {
        Condition::Intersects {
            field: field.into(),
            bounds,
        }
    }

    /// All of `conditions`.
    pub fn all(conditions: Vec<Condition>) -> Self {
        Condition::And(conditions)
    }

    /// Any of `conditions`.
    pub fn any(conditions: Vec<Condition>) -> Self {
        Condition::Or(conditions)
    }

    /// Negation of `condition`.
    pub fn negate(condition: Condition) -> Self {
        Condition::Not(Box::new(condition))
    }

    /// Some entity reached along `edge` (within `max_depth`) satisfies `condition`.
    pub fn related(edge: Edge, max_depth: Option<usize>, condition: Condition) -> Self {
        Condition::Related {
            edge,
            max_depth,
            cond: Box::new(condition),
        }
    }

    /// Checks the condition; `loc` names it in any error, e.g. `filters[2].or[0]`.
    /// Nested conditions extend `loc`, so the error points at the failing leaf.
    pub(crate) fn validate_at(&self, loc: &str) -> Result<(), QueryError> {
        match self {
            Condition::And(cs) => {
                for (i, c) in cs.iter().enumerate() {
                    c.validate_at(&join(loc, &format!("and[{i}]")))?;
                }
                Ok(())
            }
            Condition::Or(cs) => {
                for (i, c) in cs.iter().enumerate() {
                    c.validate_at(&join(loc, &format!("or[{i}]")))?;
                }
                Ok(())
            }
            Condition::Not(c) => c.validate_at(&join(loc, "not")),
            Condition::Related {
                edge,
                max_depth,
                cond,
            } => {
                if *max_depth == Some(0) {
                    return Err(locate(
                        loc,
                        QueryError::InvalidQuery(String::from("related max_depth must be >= 1")),
                    ));
                }
                edge.validate().map_err(|e| locate(loc, e))?;
                cond.validate_at(&join(loc, "related"))
            }
            leaf => leaf.check_leaf().map_err(|e| locate(loc, e)),
        }
    }

    /// Checks a condition with no nested conditions.
    fn check_leaf(&self) -> Result<(), QueryError> {
        match self {
            Condition::Eq { field, value }
            | Condition::Ne { field, value }
            | Condition::Gt { field, value }
            | Condition::Gte { field, value }
            | Condition::Lt { field, value }
            | Condition::Lte { field, value }
            | Condition::Contains { field, value } => {
                check_path(field)?;
                check_value(value)
            }
            Condition::Intersects { field, bounds } => {
                check_path(field)?;
                check_aabb(bounds)
            }
            Condition::In { field, values } => {
                check_path(field)?;
                values.iter().try_for_each(check_value)
            }
            Condition::Exists { field } => check_path(field),
            Condition::StartsWith { field, value } | Condition::EndsWith { field, value } => {
                check_path(field)?;
                match value {
                    Value::Str(_) => Ok(()),
                    _ => Err(QueryError::InvalidQuery(String::from(
                        "StartsWith and EndsWith need a string value",
                    ))),
                }
            }
            Condition::Between { field, low, high } => {
                check_path(field)?;
                check_value(low)?;
                check_value(high)?;
                if low.rank() == high.rank() {
                    Ok(())
                } else {
                    Err(QueryError::InvalidQuery(String::from(
                        "Between bounds must be the same kind of value",
                    )))
                }
            }
            Condition::OfType(_) | Condition::OfTypeIn(_) => Ok(()),
            Condition::ContainsPoint { field, point } => {
                check_path(field)?;
                if point.iter().all(|c| c.is_finite()) {
                    Ok(())
                } else {
                    Err(QueryError::InvalidQuery(String::from(
                        "point coordinates must be finite",
                    )))
                }
            }
            Condition::Ray {
                field,
                origin,
                direction,
                within,
            } => {
                check_path(field)?;
                if origin.iter().chain(direction).all(|c| c.is_finite())
                    && within.is_finite()
                    && *within >= 0.0
                {
                    Ok(())
                } else {
                    Err(QueryError::InvalidQuery(String::from(
                        "ray origin and direction must be finite, and `within` a finite non-negative distance",
                    )))
                }
            }
            // Handled by `validate_at`.
            Condition::And(_)
            | Condition::Or(_)
            | Condition::Not(_)
            | Condition::Related { .. } => Ok(()),
        }
    }

    pub(crate) fn uses_relations(&self) -> bool {
        match self {
            Condition::Related { .. } => true,
            Condition::And(cs) | Condition::Or(cs) => cs.iter().any(Condition::uses_relations),
            Condition::Not(c) => c.uses_relations(),
            Condition::Eq { .. }
            | Condition::Ne { .. }
            | Condition::Gt { .. }
            | Condition::Gte { .. }
            | Condition::Lt { .. }
            | Condition::Lte { .. }
            | Condition::Contains { .. }
            | Condition::Intersects { .. }
            | Condition::In { .. }
            | Condition::Exists { .. }
            | Condition::StartsWith { .. }
            | Condition::EndsWith { .. }
            | Condition::Between { .. }
            | Condition::OfType(_)
            | Condition::OfTypeIn(_)
            | Condition::ContainsPoint { .. }
            | Condition::Ray { .. } => false,
        }
    }
}

impl Query {
    /// Starts a query over all entities of type `source`.
    pub fn new(source: EntityTypeId) -> Self {
        Self {
            source,
            filters: Vec::new(),
            traversals: Vec::new(),
            projections: Vec::new(),
            limit: None,
            order_by: None,
            offset: 0,
            after: None,
            aggregate: None,
            distinct: false,
            nearest: None,
        }
    }

    /// Yields each entity once. See [`Query::distinct`].
    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    /// Keeps the `k` rows whose box in `field` is nearest to `point`. See
    /// [`Query::nearest`] (the field).
    pub fn nearest(mut self, field: impl Into<FieldPath>, point: [f64; 3], k: usize) -> Self {
        self.nearest = Some(Nearest {
            field: field.into(),
            point,
            k,
        });
        self
    }

    /// Skips the first `n` rows.
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    /// Turns the query into a reduction. See [`Aggregation`].
    pub fn aggregate(mut self, metric: Metric, group_by: Option<FieldPath>) -> Self {
        self.aggregate = Some(Aggregation { metric, group_by });
        self
    }

    /// Resumes after source entity `id`, for keyset pagination.
    pub fn after(mut self, id: EntityId) -> Self {
        self.after = Some(id);
        self
    }

    /// Adds a source filter. Filters combine with AND.
    pub fn filter(mut self, condition: Condition) -> Self {
        self.filters.push(condition);
        self
    }

    /// Adds a traversal stage.
    pub fn traverse(mut self, traversal: Traversal) -> Self {
        self.traversals.push(traversal);
        self
    }

    /// Adds a projected field.
    pub fn project(mut self, field: impl Into<FieldPath>) -> Self {
        self.projections.push(Projection {
            field: field.into(),
        });
        self
    }

    /// Sets the sort order.
    pub fn order_by(mut self, field: impl Into<FieldPath>, descending: bool) -> Self {
        self.order_by = Some(OrderBy {
            field: field.into(),
            descending,
        });
        self
    }

    /// Caps the number of rows.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Checks the query's shape: no empty field paths, no zero depths, no empty
    /// edge labels. Does not check the source. Runs automatically on execute.
    ///
    /// Errors name the failing part of the query, e.g. `filters[2].and[0]: ...`
    /// for a bad leaf inside the third top-level filter.
    pub fn validate(&self) -> Result<(), QueryError> {
        for (i, c) in self.filters.iter().enumerate() {
            c.validate_at(&format!("filters[{i}]"))?;
        }
        for (i, t) in self.traversals.iter().enumerate() {
            t.validate_at(&format!("traversals[{i}]"))?;
        }
        for (i, p) in self.projections.iter().enumerate() {
            check_path(&p.field).map_err(|e| locate(&format!("projections[{i}]"), e))?;
        }
        if let Some(agg) = &self.aggregate {
            if let Some(key) = &agg.group_by {
                check_path(key).map_err(|e| locate("aggregate.group_by", e))?;
            }
            if let Some(path) = agg.metric.path() {
                check_path(path).map_err(|e| locate("aggregate.metric", e))?;
            }
            if !self.projections.is_empty()
                || self.order_by.is_some()
                || self.limit.is_some()
                || self.offset != 0
                || self.after.is_some()
            {
                return Err(locate(
                    "aggregate",
                    QueryError::InvalidQuery(String::from(
                        "aggregate queries take no projections, order_by, limit, offset, or after",
                    )),
                ));
            }
        }
        if let Some(o) = &self.order_by {
            check_path(&o.field).map_err(|e| locate("order_by.field", e))?;
            if self.after.is_some() {
                return Err(locate(
                    "after",
                    QueryError::InvalidQuery(String::from(
                        "`after` cannot be combined with `order_by`",
                    )),
                ));
            }
        }
        if let Some(n) = &self.nearest {
            check_path(&n.field).map_err(|e| locate("nearest.field", e))?;
            if !n.point.iter().all(|c| c.is_finite()) {
                return Err(locate(
                    "nearest.point",
                    QueryError::InvalidQuery(String::from("point coordinates must be finite")),
                ));
            }
            if self.order_by.is_some()
                || self.after.is_some()
                || self.offset != 0
                || self.limit.is_some()
                || self.aggregate.is_some()
            {
                return Err(locate(
                    "nearest",
                    QueryError::InvalidQuery(String::from(
                        "`nearest` cannot be combined with order_by, after, offset, limit, or aggregate; its k is the limit",
                    )),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn needs_relations(&self) -> bool {
        !self.traversals.is_empty() || self.filters.iter().any(Condition::uses_relations)
    }
}

/// `loc` joined to `seg` with a dot; `seg` alone when `loc` is empty.
fn join(loc: &str, seg: &str) -> String {
    if loc.is_empty() {
        String::from(seg)
    } else {
        format!("{loc}.{seg}")
    }
}

/// Prefixes an invalid-query message with the location it came from.
fn locate(loc: &str, err: QueryError) -> QueryError {
    match err {
        QueryError::InvalidQuery(msg) if !loc.is_empty() => {
            QueryError::InvalidQuery(format!("{loc}: {msg}"))
        }
        other => other,
    }
}

fn check_path(path: &FieldPath) -> Result<(), QueryError> {
    if path.segments().is_empty() || path.segments().iter().any(|s| s.is_empty()) {
        return Err(QueryError::InvalidQuery(format!(
            "invalid field path `{path}`"
        )));
    }
    Ok(())
}

/// Rejects NaN and infinite floats, including inside boxes, lists, and maps. They
/// have no JSON form, so they would not survive a round trip.
fn check_value(value: &Value) -> Result<(), QueryError> {
    match value {
        Value::Float(f) if !f.is_finite() => Err(non_finite()),
        Value::Aabb(bounds) => check_aabb(bounds),
        Value::List(items) => items.iter().try_for_each(check_value),
        Value::Map(map) => map.values().try_for_each(check_value),
        _ => Ok(()),
    }
}

fn check_aabb(bounds: &Aabb) -> Result<(), QueryError> {
    if bounds.min.iter().chain(&bounds.max).all(|c| c.is_finite()) {
        Ok(())
    } else {
        Err(non_finite())
    }
}

fn non_finite() -> QueryError {
    QueryError::InvalidQuery(String::from(
        "non-finite numbers (NaN or infinity) are not allowed in queries",
    ))
}
