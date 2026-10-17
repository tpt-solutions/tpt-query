# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Declarative query AST and typed builder: filters (`Eq`, `Ne`, `Gt`, `Gte`, `Lt`,
  `Lte`, `Contains`, `Intersects`, `And`, `Or`, `Not`, `OfType`, `Related`),
  traversal stages, projection, ordering, and limits.
- Streaming, zero-copy execution with `limit` short-circuiting and deterministic
  id order.
- Graph traversal along parent, child, and named edges: DFS and BFS, depth limits,
  and cycle detection. Sources without a relation map are rejected.
- Hash and B-tree value indexes, maintained on insert, update, and delete, with a
  cost-based planner and `explain` output.
- R-tree index for bounding-box intersection (feature `spatial`).
- Streaming `Count`, `Sum`, `Min`, `Max`, `Avg`, and `group_by`.
- JSON AST (feature `json`) with query validation.
- `no_std + alloc` core; builds for `thumbv7em-none-eabihf` and `wasm32-unknown-unknown`.
- Test and benchmark infrastructure via `tpt-testkit` (golden fixtures, property
  tests, criterion benchmarks).

### Known limitations
- Hash indexes are ordered maps, so exact lookups are O(log N).
- `order_by` materializes the result set.
- Named edges are outgoing only; removing a target leaves dangling references that
  traversals report as `MissingEntity`.
- The R-tree does not condense underfull nodes on removal.
- The data model is a local stub pending `tpt-uir` / `tpt-model`.
