# tpt-query TODO

Decisions: single crate with modules · edition 2024 · `no_std + alloc` core · local stub trait layer for UirNode/EntityId (swap for real `tpt-uir` later) · `tpt-math-spatial` via path dep behind `spatial` feature · `tpt-testkit` as path dev-dep · JSON AST via serde for string format (no Datalog text parser).

## Phase 0: Project setup
- [ ] `Cargo.toml` (edition 2024, `#![no_std]` + `extern crate alloc`, features: `std`, `spatial`, `serde`/`json`)
- [ ] `#![forbid(unsafe_code)]`, `missing_docs` lint, README, license headers
- [ ] Module skeleton: `ast`, `engine`, `traverse`, `index`, `aggregate`, `source`
- [ ] Local stub layer (`source`): `EntityId`, `EntityTypeId`, `FieldPath`, `Value`/`UirNode`-like enum, `EntitySource` trait (get by id, iterate by type, relationship lookup)
- [ ] Verify `no_std` build (e.g. `thumbv7em-none-eabi` and `wasm32-unknown-unknown`)
- [ ] CI script: fmt, clippy, test, no_std build check
- [ ] Wire `tpt-testkit` dev-dependency (path) and confirm it builds

## Phase 1: AST & basic engine
- [ ] `Query`, `Condition` (Eq, Gt, Contains, And, Or), `Traversal`, `Projection`, `OrderBy`, `Aggregate` types
- [ ] Typed Rust builder API
- [ ] Add Lt, Gte, Lte, Ne, Not conditions beyond the spec's Eq/Gt/Contains/And/Or (decided: add)
- [ ] Mark `Condition` `#[non_exhaustive]` to allow future variants without breaking changes
- [ ] Add `Condition::Intersects { field, bounds }` with a local AABB type (evaluated as a plain filter now; `spatial` feature converts to `tpt-math-spatial` AABB)
- [ ] `Queryable` trait + `QueryError`
- [ ] Streaming `QueryResultIterator` (lazy, full-scan source)
- [ ] Filter evaluation incl. field-path resolution
- [ ] `limit` support (short-circuits iteration)
- [ ] Deterministic ordering (stable source order by EntityId) + explicit `order_by`
- [ ] Zero-copy: results return references/views, no clone on traversal
- [ ] Unit tests for flat collections
- **Milestone:** query flat collections of entities

## Phase 2: Traversal & projection
- [ ] Relationship map abstraction in source trait (O(1)/O(log N) children/parent lookup; no blind scans)
- [ ] Edge navigation (parent/child/named edge), e.g. TextRun where parent Paragraph style == Bold
- [ ] Recursive traversal, DFS and BFS, max depth
- [ ] Cycle detection (visited set)
- [ ] Reject/err on traversals lacking a relationship map (enforce architect's rule)
- [ ] Predicate pushdown during traversal
- [ ] `Project` (partial field extraction)
- [ ] Nested/hierarchical doc tests (Document > Page > Paragraph > TextRun)
- **Milestone:** query nested hierarchical structures

## Phase 3: Indexing
- [ ] `Index` trait + incremental maintenance (insert/update/delete)
- [ ] Hash index
- [ ] B-Tree index (range queries)
- [ ] Spatial R-Tree (feature `spatial`, using `tpt-math-spatial` AABB)
- [ ] Accelerate `Condition::Intersects` (added to AST in Phase 1) via the R-Tree; no AST change
- [ ] Query planner: index discovery, selectivity heuristic, candidate-set fetch, residual filters
- [ ] Planner: explain/plan output for debugging
- [ ] Sub-millisecond indexed lookup target validated
- **Milestone:** large-dataset queries are fast

## Phase 4: Aggregation, parser, release
- [ ] Streaming Count, Sum, Min, Max, Avg (O(1) memory)
- [ ] Streaming GroupBy (O(K) memory)
- [ ] JSON AST (serde, no_std + alloc) serialize/deserialize
- [ ] AST validation with clear errors on parse
- [ ] WASM build + smoke test
- [ ] Docs, examples, API review, changelog
- [ ] Publish v1.0.0

## Testing & quality
- [ ] Golden-file tests: mock CAD assembly + predefined queries, JSON results
- [ ] Index invariant property tests (random insert/update/delete vs full scan, `tpt-testkit::property`)
- [ ] Memory limit test: 1M entities, `limit: 10`, peak < 5MB
- [ ] Benchmarks with/without indexes (`tpt-testkit::bench`), confirm O(1)/O(log N)
- [ ] Complexity test guarding against O(N²) traversal
- [ ] Determinism test (repeat runs identical)

## Later / follow-ups
- [ ] Replace stub layer with real `tpt-uir` / `tpt-model` once they exist
- [ ] Integration examples for tpt-vertex, tpt-doc, tpt-lattice
- [ ] Optional Datalog text syntax (currently out of scope)
