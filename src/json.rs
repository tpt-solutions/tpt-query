//! JSON form of the query AST and of result rows (feature `json`), for
//! cross-language clients.
//!
//! # Example
//!
//! ```
//! use tpt_query::json::{query_from_json, query_to_json};
//!
//! let input = r#"{"source":1,"filters":[{"Eq":{"field":"material","value":"Steel"}}]}"#;
//! let query = query_from_json(input).unwrap();
//! let text = query_to_json(&query).unwrap();
//! assert_eq!(query_from_json(&text).unwrap(), query);
//! ```

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use serde_json::{Map, Value as Json};

use crate::{
    ast::Query,
    engine::{QueryError, Queryable, Row},
    source::EntitySource,
};

/// Parses and validates a query from JSON.
///
/// Field paths are dotted strings, e.g. `"material.name"`. Values are plain JSON:
/// `"Titanium"`, `5`, `2.5`, `true`, `null`, lists, and objects. A bounding box is
/// `{"Aabb": {"min": [..], "max": [..]}}`.
pub fn query_from_json(input: &str) -> Result<Query, QueryError> {
    let query: Query =
        serde_json::from_str(input).map_err(|e| QueryError::InvalidQuery(e.to_string()))?;
    query.validate()?;
    Ok(query)
}

/// Serializes a query to JSON.
pub fn query_to_json(query: &Query) -> Result<String, QueryError> {
    // serde_json writes NaN and infinity as `null`, which would not parse back.
    query.validate()?;
    serde_json::to_string(query).map_err(|e| QueryError::InvalidQuery(e.to_string()))
}

/// Serializes rows as a JSON array. Each row is an object with its `id` and one key
/// per projection, named by the projected path. A missing field is `null`. A
/// projection named `id` overwrites the row id in the output.
pub fn rows_to_json<'a, I>(query: &Query, rows: I) -> Result<String, QueryError>
where
    I: IntoIterator<Item = Result<Row<'a>, QueryError>>,
{
    let mut out: Vec<Json> = Vec::new();
    for row in rows {
        let row = row?;
        let mut obj = Map::new();
        obj.insert(String::from("id"), Json::from(row.id.0));
        for (projection, value) in query.projections.iter().zip(&row.projected) {
            let json = match value {
                Some(v) => serde_json::to_value(v).map_err(json_error)?,
                None => Json::Null,
            };
            obj.insert(projection.field.to_string(), json);
        }
        out.push(Json::Object(obj));
    }
    serde_json::to_string(&Json::Array(out)).map_err(json_error)
}

/// Runs `query` against `src` and returns the rows as JSON. See [`rows_to_json`].
pub fn execute_json<S: EntitySource>(src: &S, query: &Query) -> Result<String, QueryError> {
    let rows = src.execute(query)?;
    rows_to_json(query, rows)
}

fn json_error(e: serde_json::Error) -> QueryError {
    QueryError::InvalidQuery(e.to_string())
}

/// Runs an aggregate query and returns `[{"key": .., "value": ..}]`. Without
/// `group_by` the single entry has a `null` key. See [`Queryable::aggregate`].
pub fn aggregate_json<S: EntitySource>(src: &S, query: &Query) -> Result<String, QueryError> {
    let pairs = src.aggregate(query)?;
    let mut out: Vec<Json> = Vec::with_capacity(pairs.len());
    for (key, value) in &pairs {
        let mut obj = Map::new();
        obj.insert(
            String::from("key"),
            serde_json::to_value(key).map_err(json_error)?,
        );
        obj.insert(
            String::from("value"),
            serde_json::to_value(value).map_err(json_error)?,
        );
        out.push(Json::Object(obj));
    }
    serde_json::to_string(&Json::Array(out)).map_err(json_error)
}
