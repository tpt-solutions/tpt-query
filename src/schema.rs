//! An optional schema: the fields each entity type declares, with their kinds.
//!
//! [`Schema::validate_against`] checks a query before it runs. It catches field
//! names that do not exist and values of the wrong kind, such as an `Int` compared
//! with a `Float` field. Checks are strict: a type with no entry is an error, and
//! every path a query uses must be declared, including nested paths.
//!
//! Traversal stages and `Related` conditions reach other types, so their fields are
//! not checked here.

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
};

use crate::{
    ast::{Condition, Query},
    engine::QueryError,
    source::{EntityTypeId, FieldPath, Value},
};

/// The kind of value a declared field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldKind {
    /// Boolean.
    Bool,
    /// Signed integer.
    Int,
    /// Float.
    Float,
    /// String.
    Str,
    /// Bounding box.
    Aabb,
    /// List.
    List,
    /// Nested map. Declare its leaf paths too, since they are what queries use.
    Map,
    /// Any kind. Use it when the field is genuinely mixed.
    Any,
}

impl FieldKind {
    /// Whether a value of this kind can be stored in a field of this kind. `Null`
    /// fits every field, since it means absent.
    fn accepts(self, value: &Value) -> bool {
        let matches = match value {
            Value::Null => return true,
            Value::Bool(_) => self == FieldKind::Bool,
            Value::Int(_) => self == FieldKind::Int,
            Value::Float(_) => self == FieldKind::Float,
            Value::Str(_) => self == FieldKind::Str,
            Value::Aabb(_) => self == FieldKind::Aabb,
            Value::List(_) => self == FieldKind::List,
            Value::Map(_) => self == FieldKind::Map,
        };
        matches || self == FieldKind::Any
    }
}

/// Declared fields for each entity type. Build it with [`Schema::new`] and
/// [`Schema::field`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Schema {
    /// Per type: canonical path text to kind.
    types: BTreeMap<EntityTypeId, BTreeMap<String, FieldKind>>,
}

impl Schema {
    /// An empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares `path` (dotted, with `\` escapes as in [`FieldPath`]) on type `ty`.
    /// Declaring a type with no fields is how a type is made known to the schema.
    pub fn field(mut self, ty: EntityTypeId, path: &str, kind: FieldKind) -> Self {
        let key = FieldPath::parse(path).to_string();
        self.types.entry(ty).or_default().insert(key, kind);
        self
    }

    /// Declares a type with no fields, so queries on it are not rejected as unknown.
    pub fn entity_type(mut self, ty: EntityTypeId) -> Self {
        self.types.entry(ty).or_default();
        self
    }

    /// Checks `query` against this schema. Only the source type is checked, along with
    /// the projections, ordering, and aggregation of the query itself.
    pub fn validate_against(&self, query: &Query) -> Result<(), QueryError> {
        let Some(fields) = self.types.get(&query.source) else {
            return Err(QueryError::InvalidQuery(format!(
                "entity type {} is not in the schema",
                query.source.0
            )));
        };
        let check_declared = |path: &FieldPath| -> Result<FieldKind, QueryError> {
            fields.get(&path.to_string()).copied().ok_or_else(|| {
                QueryError::InvalidQuery(format!(
                    "field `{path}` is not declared for entity type {}",
                    query.source.0
                ))
            })
        };

        for c in &query.filters {
            check_condition(c, &check_declared)?;
        }
        for p in &query.projections {
            check_declared(&p.field)?;
        }
        if let Some(o) = &query.order_by {
            check_declared(&o.field)?;
        }
        if let Some(n) = &query.nearest {
            match check_declared(&n.field)? {
                FieldKind::Aabb | FieldKind::Any => {}
                other => {
                    return Err(QueryError::InvalidQuery(format!(
                        "field `{}` is {other:?} but `nearest` needs an Aabb field",
                        n.field
                    )));
                }
            }
        }
        if let Some(agg) = &query.aggregate {
            if let Some(key) = &agg.group_by {
                check_declared(key)?;
            }
            if let Some(path) = agg.metric.path() {
                check_declared(path)?;
            }
        }
        Ok(())
    }
}

fn check_value(kind: FieldKind, path: &FieldPath, value: &Value) -> Result<(), QueryError> {
    if kind.accepts(value) {
        return Ok(());
    }
    Err(QueryError::InvalidQuery(format!(
        "field `{path}` is {kind:?} but the query compares it with {}",
        value_kind_name(value)
    )))
}

fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::Str(_) => "Str",
        Value::Aabb(_) => "Aabb",
        Value::List(_) => "List",
        Value::Map(_) => "Map",
    }
}

fn check_condition(
    cond: &Condition,
    declared: &dyn Fn(&FieldPath) -> Result<FieldKind, QueryError>,
) -> Result<(), QueryError> {
    match cond {
        Condition::Eq { field, value }
        | Condition::Ne { field, value }
        | Condition::Gt { field, value }
        | Condition::Gte { field, value }
        | Condition::Lt { field, value }
        | Condition::Lte { field, value } => {
            let kind = declared(field)?;
            check_value(kind, field, value)
        }
        Condition::Contains { field, .. } => declared(field).map(|_| ()),
        Condition::In { field, values } => {
            let kind = declared(field)?;
            values.iter().try_for_each(|v| check_value(kind, field, v))
        }
        Condition::Between { field, low, high } => {
            let kind = declared(field)?;
            check_value(kind, field, low)?;
            check_value(kind, field, high)
        }
        Condition::Exists { field }
        | Condition::StartsWith { field, .. }
        | Condition::EndsWith { field, .. } => declared(field).map(|_| ()),
        Condition::Intersects { field, .. }
        | Condition::ContainsPoint { field, .. }
        | Condition::Ray { field, .. } => match declared(field)? {
            FieldKind::Aabb | FieldKind::Any => Ok(()),
            other => Err(QueryError::InvalidQuery(format!(
                "field `{field}` is {other:?} but a box query needs an Aabb field"
            ))),
        },
        Condition::And(cs) | Condition::Or(cs) => {
            cs.iter().try_for_each(|c| check_condition(c, declared))
        }
        Condition::Not(c) => check_condition(c, declared),
        // These reference other types or only the entity's type, so there is nothing to check.
        Condition::OfType(_) | Condition::OfTypeIn(_) | Condition::Related { .. } => Ok(()),
    }
}
