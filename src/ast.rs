//! The query representation: a typed Rust builder whose shape also serves as the
//! JSON AST (with the `serde` feature).

use alloc::{boxed::Box, format, string::String, vec::Vec};

use crate::{
    engine::QueryError,
    source::{Aabb, EntityTypeId, FieldPath, Value},
};

/// A complete query against one entity type.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Query {
    /// Entity type the query starts from.
    pub source: EntityTypeId,
    /// Conditions on source entities. All must hold (implicit AND).
    pub filters: Vec<Condition>,
    /// Traversal stages, applied in order to the filtered source entities.
    pub traversals: Vec<Traversal>,
    /// Fields to extract into each row. Empty means no projection.
    pub projections: Vec<Projection>,
    /// Maximum number of rows to yield. Short-circuits streaming execution.
    pub limit: Option<usize>,
    /// Sort order. Materializes the result set, so use it only when needed.
    pub order_by: Option<OrderBy>,
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
    /// Filters on reached entities. All must hold. Pruned during the walk.
    pub filters: Vec<Condition>,
}

impl Traversal {
    /// A single hop along `edge`.
    pub fn step(edge: Edge) -> Self {
        Self {
            edge,
            order: Order::DepthFirst,
            max_depth: Some(1),
            filters: Vec::new(),
        }
    }

    /// A walk along `edge` up to `max_depth` (`None` for unbounded).
    pub fn recursive(edge: Edge, order: Order, max_depth: Option<usize>) -> Self {
        Self {
            edge,
            order,
            max_depth,
            filters: Vec::new(),
        }
    }

    /// Adds a filter on reached entities.
    pub fn filter(mut self, condition: Condition) -> Self {
        self.filters.push(condition);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), QueryError> {
        if self.max_depth == Some(0) {
            return Err(QueryError::InvalidQuery(String::from(
                "traversal max_depth must be >= 1",
            )));
        }
        self.edge.validate()?;
        self.filters.iter().try_for_each(Condition::validate)
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
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Condition {
    /// Field equals value.
    Eq {
        /// Field to test.
        field: FieldPath,
        /// Expected value.
        value: Value,
    },
    /// Field exists and differs from value.
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
    /// Some entity reached along `edge` (within `max_depth`) satisfies `cond`.
    Related {
        /// Relationship to follow.
        edge: Edge,
        /// Maximum depth. `None` is unbounded.
        max_depth: Option<usize>,
        /// Condition on the reached entity.
        cond: Box<Condition>,
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

    pub(crate) fn validate(&self) -> Result<(), QueryError> {
        match self {
            Condition::Eq { field, .. }
            | Condition::Ne { field, .. }
            | Condition::Gt { field, .. }
            | Condition::Gte { field, .. }
            | Condition::Lt { field, .. }
            | Condition::Lte { field, .. }
            | Condition::Contains { field, .. }
            | Condition::Intersects { field, .. } => check_path(field),
            Condition::And(cs) | Condition::Or(cs) => cs.iter().try_for_each(Condition::validate),
            Condition::Not(c) => c.validate(),
            Condition::OfType(_) => Ok(()),
            Condition::Related {
                edge,
                max_depth,
                cond,
            } => {
                if *max_depth == Some(0) {
                    return Err(QueryError::InvalidQuery(String::from(
                        "related max_depth must be >= 1",
                    )));
                }
                edge.validate()?;
                cond.validate()
            }
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
            | Condition::OfType(_) => false,
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
        }
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
    pub fn validate(&self) -> Result<(), QueryError> {
        self.filters.iter().try_for_each(Condition::validate)?;
        self.traversals.iter().try_for_each(Traversal::validate)?;
        for p in &self.projections {
            check_path(&p.field)?;
        }
        if let Some(o) = &self.order_by {
            check_path(&o.field)?;
        }
        Ok(())
    }

    pub(crate) fn needs_relations(&self) -> bool {
        !self.traversals.is_empty() || self.filters.iter().any(Condition::uses_relations)
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
