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

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod aggregate;
pub mod ast;
pub mod engine;
pub mod index;
#[cfg(feature = "json")]
pub mod json;
pub mod source;
#[cfg(feature = "spatial")]
pub mod spatial;
pub mod traverse;

pub use aggregate::{Metric, aggregate, group_by};
pub use ast::{Condition, Edge, Order, OrderBy, Projection, Query, Traversal};
pub use engine::{Access, Plan, QueryError, QueryResultIterator, Queryable, Row};
pub use index::{Index, IndexKind, ValueIndex};
pub use source::{
    Aabb, Entity, EntityId, EntitySource, EntityTypeId, FieldPath, IndexHit, MemorySource,
    RelationMap, Value,
};
