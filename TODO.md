# tpt-query TODO

Decisions: single crate with modules · edition 2024 (Rust 1.88+) · `no_std + alloc` core · local stub trait layer for UirNode/EntityId (swap for real `tpt-uir` later) · `tpt-math-spatial` via path dep behind `spatial` feature · `tpt-testkit` as path dev-dep · JSON AST via serde for string format (no Datalog text parser).

Deviations from the decisions above (see notes on each item):
- `tpt-math-spatial` has no AABB type (it is Featherstone kinematics), so the `spatial` feature has no dependency on it. The AABB and R-tree are local.
- `tpt-testkit` is a dev-dependency pinned to commit `e77a901` (github.com/tpt-solutions/tpt-testkit). It has no release tags yet.
- Licensed MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`), matching the sibling TPT crates.

## Phase 0: Project setup
- [x] `Cargo.toml` (edition 2024, `#![no_std]` + `extern crate alloc`, features: `std`, `spatial`, `serde`/`json`)
- [x] `#![forbid(unsafe_code)]`, `missing_docs` lint, README, license. Note: the repo convention is license files, not per-file headers (as in `tpt-math`), so there are no headers. `LICENSE-MIT` and `LICENSE-APACHE` are copied from `tpt-math`.
- [x] Module skeleton: `ast`, `engine`, `traverse`, `index`, `aggregate`, `source` (plus `spatial` and `json`)
- [x] Local stub layer (`source`): `EntityId`, `EntityTypeId`, `FieldPath`, `Value`, `EntitySource` trait (get by id, iterate by type, relationship lookup)
- [x] Verify `no_std` build (`thumbv7em-none-eabihf` and `wasm32-unknown-unknown`). Build only; nothing runs on target.
- [x] CI script: `scripts/ci.sh` (fmt, clippy `-D warnings`, test with and without features, bench compile, no_std build, wasm test). Passes locally.
- [x] Wire `tpt-testkit` dev-dependency (git, pinned rev, native-only). Used for golden, property, and bench.

## Phase 1: AST & basic engine
- [x] `Query`, `Condition` (Eq, Gt, Contains, And, Or), `Traversal`, `Projection`, `OrderBy`, `Aggregate` types
- [x] Typed Rust builder API
- [x] Add Lt, Gte, Lte, Ne, Not conditions beyond the spec's Eq/Gt/Contains/And/Or (decided: add)
- [x] Mark `Condition` `#[non_exhaustive]` to allow future variants without breaking changes
- [x] Add `Condition::Intersects { field, bounds }` with a local AABB type (evaluated as a plain filter now; `spatial` feature converts to `tpt-math-spatial` AABB). Note: the local `Aabb` is used directly; no conversion exists.
- [x] `Queryable` trait + `QueryError`. Note: `execute` takes `&'a Query` as well as `&'a self`, because rows borrow projections from the query.
- [x] Streaming `QueryResultIterator` (lazy, full-scan source)
- [x] Filter evaluation incl. field-path resolution
- [x] `limit` support (short-circuits iteration)
- [x] Deterministic ordering (stable source order by EntityId) + explicit `order_by` (materializes the result; ties break by id)
- [x] Zero-copy: `Row` borrows the entity and projected values
- [x] Unit tests for flat collections (`tests/flat.rs`)
- **Milestone:** query flat collections of entities ✅

## Phase 2: Traversal & projection
- [x] Relationship map abstraction in source trait (`RelationMap`; O(log N) via ordered maps, no blind scans)
- [x] Edge navigation (parent/child/named edge), e.g. TextRun where parent Paragraph style == Bold (`Condition::Related`)
- [x] Recursive traversal, DFS and BFS, max depth
- [x] Cycle detection (visited set)
- [x] Reject/err on traversals lacking a relationship map (`QueryError::NoRelationMap`)
- [x] Predicate pushdown during traversal (traversal filters run during the walk; source filters run before it)
- [x] `Project` (partial field extraction)
- [x] Nested/hierarchical doc tests (Document > Page > Paragraph > TextRun) (`tests/hierarchy.rs`)
- **Milestone:** query nested hierarchical structures ✅

## Phase 3: Indexing
- [x] `Index` trait + incremental maintenance (insert/update/delete)
- [x] Hash index. Note: backed by an ordered map, so exact lookups are O(log N), not O(1). Switching to a hash map needs `hashbrown` (a new dependency); deferred.
- [x] B-Tree index (range queries; range scans stay within the value kind)
- [x] Spatial R-Tree (feature `spatial`). Note: local `Aabb`, not `tpt-math-spatial`.
- [x] Accelerate `Condition::Intersects` (added to AST in Phase 1) via the R-Tree; no AST change
- [x] Query planner: index discovery, selectivity (exact candidate count), candidate-set fetch, residual filters. Note: selectivity uses the real candidate count, not an estimate.
- [x] Planner: explain/plan output for debugging (`Queryable::explain`, `Plan` implements `Display`)
- [x] Sub-millisecond indexed lookup target validated: `benches/index.rs` (criterion, `--measurement-time 2`, this machine). Indexed point lookup about 0.7–0.85 µs at 10k, 100k, and 1M. Scan 0.75 ms, 13.6 ms, 153 ms. The indexed time is flat across sizes in this run; an earlier quick timer showed 3.2 µs at 1M, which was noisier.
- **Milestone:** large-dataset queries are fast ✅

## Phase 4: Aggregation, parser, release
- [x] Streaming Count, Sum, Min, Max, Avg (O(1) memory)
- [x] Streaming GroupBy (O(K) memory)
- [x] JSON AST (serde, no_std + alloc) serialize/deserialize (feature `json`)
- [x] AST validation with clear errors on parse
- [x] WASM build + smoke test. `tests/wasm.rs` runs under Node via `wasm-bindgen-test-runner` (`.cargo/config.toml`). Needs a `wasm-bindgen-cli` matching the lockfile (0.2.129).
- [x] Docs, examples, API review, changelog. Done: README, `CHANGELOG.md`, `examples/quickstart.rs`, rustdoc builds with no warnings. API review was a pass over the public surface and docs; no breaking changes were needed.
- [ ] Publish v1.0.0. **Not done, by request.** Crate version is 0.1.0.

## Testing & quality
- [x] Golden-file tests: mock CAD assembly + predefined queries, JSON results via `tpt_testkit::golden::assert_text` (`tests/golden.rs`, `tests/fixtures/cad_assembly.expected.json`). Expected values were checked by hand.
- [x] Index invariant property tests (random insert/update/delete vs full scan), via proptest (`tests/index_property.rs`, 64 cases of up to 150 operations). The R-tree churn test in `tests/spatial.rs` still uses a seeded generator.
- [x] Memory limit test: 1M entities, `limit: 10`, peak heap growth under 5 MB (counting global allocator). `tests/memory.rs`. Single test per file, since the counters are process-wide.
- [x] Benchmarks with/without indexes: `benches/index.rs`, criterion via `tpt_testkit::bench::bench_named`, names `lookup/{indexed,scan}/{size}`.
- [x] Complexity test guarding against O(N²) traversal (`walks_do_not_scan_the_whole_collection`: work ratio for 4× nodes must stay under 6×)
- [x] Determinism test (repeat runs identical)

## Later / follow-ups
- [ ] Replace stub layer with real `tpt-uir` / `tpt-model` once they exist
- [ ] Integration examples for tpt-vertex, tpt-doc, tpt-lattice
- [ ] Optional Datalog text syntax (currently out of scope)
