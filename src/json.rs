//! JSON form of the query AST (feature `json`), for cross-language clients.

use alloc::string::{String, ToString};

use crate::{ast::Query, engine::QueryError};

/// Parses and validates a query from JSON.
///
/// Field paths are dotted strings, e.g. `"material.name"`. Values use serde's
/// default externally tagged enum form, e.g. `{"Str": "Titanium"}`.
pub fn query_from_json(input: &str) -> Result<Query, QueryError> {
    let query: Query =
        serde_json::from_str(input).map_err(|e| QueryError::InvalidQuery(e.to_string()))?;
    query.validate()?;
    Ok(query)
}

/// Serializes a query to JSON.
pub fn query_to_json(query: &Query) -> Result<String, QueryError> {
    serde_json::to_string(query).map_err(|e| QueryError::InvalidQuery(e.to_string()))
}
