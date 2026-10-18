//! `tpt-query`: an embedded, pure-Rust declarative query engine for entity graphs.
//!
//! The core is `no_std + alloc`. Queries are built with a typed Rust builder
//! (or parsed from JSON with the `json` feature), planned against the source's
//! indexes, and executed as a lazy stream of zero-copy [`Row`]s.
//!
//! Key rules:
//! - Traversals never scan the whole collection. They only follow the source's
//!   [`RelationMap`]; a query that needs relations on a source without one fails
//!   with [`QueryError::NoRelationMap`].
//! - Results are deterministic: sources and indexes yield ids in ascending
//!   order, and traversals follow insertion order of children.
//!
//! # A guide to the modules
//!
//! - [`ast`]: the query, its conditions, and traversals. Build one with [`Query::new`].
//! - [`dsl`]: a text form of the same query (`from 1 where mass > 1.0 limit 5`).
//!   `Display` on [`Query`] prints it, and `parse` reads it back.
//! - [`engine`]: runs queries (`execute`), plans them (`explain`), counts stages
//!   (`analyze`), and aggregates them (`aggregate`).
//! - [`source`]: the entity model, [`EntitySource`], [`RelationMap`], the in-memory
//!   [`MemorySource`], and [`Snapshot`], a read-only copy.
//! - [`mod@index`] and `spatial` (feature `spatial`): hash and B-tree indexes, and the
//!   R-tree with nearest-neighbour and ray queries.
//! - [`schema`]: an optional check of a query against declared field kinds.
//! - [`advisor`] and [`live`]: index suggestions from a workload, and re-run queries
//!   that report what changed.
//! - `json` (feature `json`): the wire format for queries and rows.
//! - [`mod@aggregate`]: streaming reductions, and [`Aggregation`].
//! - [`prelude`]: the common names, in one import.
//!
//! The cookbook (`docs/cookbook.md`) has recipes, and `docs/performance.md` says what
//! each stage costs in time and memory.

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod advisor;
pub mod aggregate;
pub mod ast;
pub mod dsl;
pub mod engine;
pub mod index;
#[cfg(feature = "json")]
pub mod json;
pub mod live;
pub mod prelude;
pub mod schema;
pub mod source;
#[cfg(feature = "spatial")]
pub mod spatial;
pub mod traverse;
#[cfg(feature = "serde")]
mod value_serde;

pub use aggregate::{Aggregation, Metric, aggregate, group_by};
pub use ast::{Condition, Edge, Order, OrderBy, Projection, Query, Traversal};
pub use engine::{Access, Plan, QueryError, QueryResultIterator, Queryable, Row};
pub use index::{Index, IndexKind, ValueIndex};
pub use source::{
    Aabb, Entity, EntityId, EntitySource, EntityTypeId, FieldPath, IndexHit, MemorySource,
    RelationMap, Snapshot, Value,
};
