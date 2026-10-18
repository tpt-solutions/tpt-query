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
- [x] Predicate pushdown during traversal. Corrected: traversal filters apply to yielded nodes only, so a node that fails them is still walked through (see `tests/planner_and_validation.rs`)
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

## Review follow-ups (2026-10-18)
Source: review plan in `~/.claude/plans/review-platform-for-bugs-peaceful-anchor.md`. Items are listed in suggested order of work.

### A. Correctness pass (before 1.0)
- [x] **DFS + `max_depth` misses nodes on DAGs.** Confirmed by `tests/hierarchy.rs::depth_limited_walks_reach_nodes_through_shorter_paths_in_a_dag` (currently failing for DepthFirst). Fix in `src/traverse.rs`: track best depth per node and re-expand on a shallower arrival.
- [x] Fix the `Traversal.filters` doc ("pruned during the walk") and the Phase 2 "predicate pushdown" claim: filters only apply to yielded nodes
- [x] Index candidate lists are fully materialized, so `limit` does not bound memory on indexed queries. Add an index-path memory test (separate file, global allocator) and make candidates lazy
- [x] `explain()` runs the real index lookup. Avoid fetching all candidates (count only)
- [x] `order_by` runs eagerly inside `execute()` and a row error aborts `execute`. Make it lazy, and use a top-k heap when combined with `limit`
- [x] Reject non-finite floats (NaN, ±inf) in boxes and values on validate/parse. `Aabb::EMPTY` does not survive a JSON round trip
- [x] `create_index`: reject duplicate index names; add `drop_index`
- [x] Planner: pick the most selective index among all indexes on a type (not first match); consider indexable conditions inside top-level `And`
- [x] Document `Ne` vs `Not(Eq)` (missing field) and the Int/Float cross-kind comparison trap
- [ ] `Related` and `Walker`: avoid per-entity `BTreeSet`/`VecDeque` allocation for single-hop and parent walks — skipped: the visited set gives cycle safety on any `RelationMap`. Dropping it needs a contract that targets are distinct, which is an API decision

### B. API-freeze pass
- [x] Decide `#[non_exhaustive]` for `Query`, `Row`, `Plan`, `Access`, `Value`, `Metric`. Decided: all six, since `Query` gained three fields this round. Construct queries with `Query::new` and the builder methods
- [ ] `Row` helpers: `get(&str)`, `to_map()`, typed getters (`as_str`, `as_i64`) — partial: `get` and typed getters (`as_str`, `as_i64`, `as_f64`, `as_bool`) done; `to_map()` not done (needs row to carry projection names)
- [x] `prelude` module; more `From` impls for `Value` (`u32`, `i32`, `Vec<Value>`). Not done: `usize` (needs a checked conversion, which is a choice), and `Row::to_map`, which needs the row to carry projection names
- [x] `#[must_use]` on builders, and `Display` for `Query`. `Display` prints the text form from the `dsl` module, and `parse` reads it back.
- [ ] Error paths in `InvalidQuery` (e.g. `filters[2].and[0].field`). Deferred: messages name the field, but not where it sits in the query. Needs a path-tracking validator.
- [x] `Cargo.toml`: `repository`, `documentation`, `homepage`, `exclude`. `cargo publish --dry-run` packages and verifies the crate
- [~] GitHub Actions workflow written in `.github/workflows/ci.yml`: fmt, clippy, tests, no_std, wasm, `cargo doc` with warnings denied, `cargo deny`, and `cargo audit`. Not yet run on GitHub. `cargo semver-checks` is not added (no published baseline yet)
- [ ] Scheduled benchmark run that fails on a >20% regression. Deferred: a shared CI runner is too noisy for a 20% gate. Needs a dedicated machine.
- [ ] `release-plz` or `cargo-release` for the 1.0 publish. Deferred until the 1.0 publish decision, which is excluded for now.

### C. Features that unlock adoption
- [x] Aggregation and `group_by` in the AST and JSON. Decided: a query-level `aggregate` field, run with `Queryable::aggregate`, which returns `(key, value)` pairs. Row execution keeps its own shape and refuses aggregate queries. `json::aggregate_json` gives the JSON form
- [x] Result serialization: `json::rows_to_json` / `json::execute_json`
- [x] Friendlier JSON values and a published JSON Schema. Decided: a hand-written `docs/query.schema.json` with no new dependency. `tests/schema.rs` checks its references and that every condition variant has a branch
- [x] Pagination: `offset` and a keyset cursor (`after`). The cursor is a filter over ascending ids, so it does not seek; a seekable source trait method would fix that
- [x] More conditions: `In`, `Exists`, `Between`, `StartsWith`/`EndsWith`, `OfTypeIn`, and `ContainsPoint` (point containment, boundaries included) are done. Decided: `IsNull` is not added (`Not(Exists)` covers it), and there is no numeric coercion. Numeric kinds stay strict, and `Schema` reports Int/Float mix-ups.
- [ ] Joins/bindings: expose source and target entities in a row. Deferred: needs the traversal start carried through the row stream. Design first. `distinct` is done (`Query::distinct`).
- [x] Traversal prune option (`Traversal::descend_while`). A node below the start must meet the conditions before the walk continues from it.
- [~] Multi-type source. Decided: `Query::any()` is not added, because it would need an all-types scan that `EntitySource` does not offer. `Condition::OfTypeIn` is done for filtering a single source.
- [x] Optional `Schema` + `validate_against` (`tpt_query::schema`). Strict: undeclared types and paths are errors, and value kinds must match the field kind.
- [-] Hash index backed by `hashbrown`. Closed as not needed: the B-tree index is O(log N) and already fast (`docs/performance.md`). Revisit if a benchmark shows it matters.
- [ ] Composite (multi-field) index, `reindex`. Deferred: `Index` answers one field, so this needs conjunctions across fields.
- [x] `FieldPath` escaping (`a\.b` is one segment `a.b`) and list indexes (`items.0`). Note: a backslash in a field name now escapes, so old paths containing a backslash change meaning.

### D. Adoption assets
- [~] Examples: `doc_outline`, `json_cli`, `custom_source`, `spreadsheet_range`, and `cad_viewport` (feature `spatial`) are done and run in CI. Not done: `wasm_demo`, which needs a browser to run.
- [x] `docs/cookbook.md`: 11 recipes with builder code and JSON. Not done: `explain` output in each recipe, and the target of ~15
- [x] Copy-paste JSON query templates in `templates/queries/` (four). `tests/templates.rs` parses, validates, and round-trips each one. `cargo-generate` template: not done.
- [~] README restructure: at-a-glance benchmark numbers, the index table, and the features and examples are updated. Not done: a comparison table (needs competitor numbers) and the `tpt-uir` migration note.
- [x] Performance model: `docs/performance.md` (what streams, what materializes, planning cost). Crate-level rustdoc guide with doctests per module: not done.
- [x] `tpt-query` CLI: a binary behind feature `cli`. Decided: an in-crate binary, not a separate crate. Loads entities from JSON, runs DSL or JSON queries, and prints rows, the plan, or the analysis.
- [ ] `tpt-query-uir` adapter crate once `tpt-uir` exists

### E. Differentiators
- [x] Query text DSL (`dsl` module): `parse`, `FromStr`, and `Display`. Round-trips every condition kind, and reports error positions.
- [x] `explain analyze` (`Queryable::analyze`): row counts per stage, and `Plan::unindexed` lists conditions no index answers. Timing per stage is not done: the core is `no_std`, and timing needs a clock the caller supplies.
- [x] Index advisor (`tpt_query::advisor::suggest_indexes`): B-tree for fields with ranges, hash for equality, R-tree for boxes.
- [~] Live queries (`tpt_query::live::LiveQuery`). Diff-based: each refresh re-runs the query. Not incremental. Incremental deltas on insert, update, and remove are not done.
- [~] Snapshot read view: `MemorySource::snapshot` returns a `Snapshot`. It copies every entity (O(N)) and has no indexes. Versioned storage is not done.
- [ ] Result diff helper for golden tests. Deferred: `LiveQuery` is the id-level diff, and golden tests diff JSON text.
- [~] k-NN and ray queries on the R-tree: `RTree::nearest` and `RTree::ray`, standalone. Not yet a query condition. That needs the planner to route them through the index.
- [~] Fuzz targets: `query_from_json` has proptest robustness tests (`tests/json_robustness.rs`: arbitrary, truncated, and mutated input, plus round trips). No cargo-fuzz target yet. The random AST vs full-scan oracle is not done.
