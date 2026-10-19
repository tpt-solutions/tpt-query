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
- [ ] Replace stub layer with real `tpt-uir` / `tpt-model`. Blocked: `cargo info tpt-uir` (crates.io index, 2026-10-19) reports the crate is not found. Decision: 1.0 does not wait for it. The stub trait stays until the crate exists and its API is checked against `EntitySource`.
- [x] Index-accelerate `Condition::Between`. `ValueIndex::lookup` answers it from the B-tree, with inverted bounds returning no rows. Tests: `tests/index.rs` (`between_uses_the_btree_and_matches_a_scan`, `between_with_inverted_bounds_is_empty_and_does_not_panic`).
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
- [-] `Related` and `Walker`: avoid per-entity `BTreeSet`/`VecDeque` allocation for single-hop and parent walks — decided: keep the visited set. It is what makes a walk safe on any `RelationMap`, including ones with cycles. Dropping it would need a distinct-targets contract, which the trait does not state. Revisit with a benchmark that shows the allocation matters.

### B. API-freeze pass
- [x] Decide `#[non_exhaustive]` for `Query`, `Row`, `Plan`, `Access`, `Value`, `Metric`. Decided: all six, since `Query` gained three fields this round. Construct queries with `Query::new` and the builder methods
- [x] `Row` helpers: `get(&str)`, `to_map()`, typed getters (`as_str`, `as_i64`). `Row` carries a borrow of the query's projections, so `to_map()` names values without copying. Duplicate projection paths: the later one wins (`tests/validation_paths.rs`)
- [x] `prelude` module; more `From` impls for `Value` (`u32`, `i32`, `Vec<Value>`). `usize`: `TryFrom<usize>` (checked, since values above `i64::MAX` do not fit `Value::Int`). `Row::to_map` is done too
- [x] `#[must_use]` on builders, and `Display` for `Query`. `Display` prints the text form from the `dsl` module, and `parse` reads it back.
- [x] Error paths in `InvalidQuery`. Messages are prefixed with the location, e.g. `filters[2].and[0]: ...`, `traversals[0].descend[0]: ...`, `projections[1]: ...`. The location is the leaf condition, not the field name. Also: `Traversal::descend` conditions are now validated, which they were not before (`tests/validation_paths.rs`)
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
- [x] Joins/bindings: each `Row` has `source`, the query's start entity, next to `entity`, the row's own entity. Both are borrowed, so rows stay zero-copy. The source survives every traversal stage. With `distinct`, the first source to reach an entity is kept. Not in the JSON output (`rows_to_json` still writes only the id and projections). Tests: `tests/bindings.rs` Decided: keep source out of `rows_to_json` until a user asks, since it changes the wire format.
- [x] Traversal prune option (`Traversal::descend_while`). A node below the start must meet the conditions before the walk continues from it.
- [~] Multi-type source. Decided: `Query::any()` is not added, because it would need an all-types scan that `EntitySource` does not offer. `Condition::OfTypeIn` is done for filtering a single source.
- [x] Optional `Schema` + `validate_against` (`tpt_query::schema`). Strict: undeclared types and paths are errors, and value kinds must match the field kind.
- [-] Hash index backed by `hashbrown`. Closed as not needed: the B-tree index is O(log N) and already fast (`docs/performance.md`). Revisit if a benchmark shows it matters.
- [x] Composite (multi-field) index: `MemorySource::create_composite_index`. Answers equality on all of its fields in one lookup. The planner compares it with single-field indexes by candidate count, and any other conjunct on those fields stays a residual filter. Entities missing a field are not indexed. Tests: `tests/composite_index.rs`, and the random oracle covers it. `reindex` is declined: index maintenance is incremental and property-tested, so there is no bad state to rebuild from. Add it if a repair path is ever needed.
- [x] `FieldPath` escaping (`a\.b` is one segment `a.b`) and list indexes (`items.0`). Note: a backslash in a field name now escapes, so old paths containing a backslash change meaning.

### D. Adoption assets
- [~] Examples: `doc_outline`, `json_cli`, `custom_source`, `spreadsheet_range`, and `cad_viewport` (feature `spatial`) are done and run in CI. Not done: `wasm_demo`, which needs a browser to run.
- [x] `docs/cookbook.md`: 19 recipes with builder code and JSON. Plan output under each row recipe comes from `examples/cookbook.rs`, which CI runs
- [x] Copy-paste JSON query templates in `templates/queries/` (four). `tests/templates.rs` parses, validates, and round-trips each one. `cargo-generate` template in `templates/cargo-generate/` (`cargo generate --path templates/cargo-generate`). Generated project builds and runs against the local crate; the dependency is `tpt-query = "0.1"` until it is published.
- [~] README restructure: at-a-glance benchmark numbers, the index table, and the features and examples are updated. Decided: no competitor table yet. A fair comparison needs the same data and machine for every library, and recorded versions. Deferred until someone names the comparison set. The `tpt-uir` migration note waits on the crate (see Later / follow-ups).
- [x] Performance model: `docs/performance.md` (what streams, what materializes, planning cost). Crate-level rustdoc guide with doctests per module: done. `src/lib.rs` has a quick start and a module guide, and each module has a runnable example. `cargo test --doc` passes under all, spatial-only, and no features. Found while writing them: `Condition::Between` was not index-accelerated. Fixed since (see Later / follow-ups).
- [x] `tpt-query` CLI: a binary behind feature `cli`. Decided: an in-crate binary, not a separate crate. Loads entities from JSON, runs DSL or JSON queries, and prints rows, the plan, or the analysis.
- [ ] `tpt-query-uir` adapter crate once `tpt-uir` exists

### E. Differentiators
- [x] Query text DSL (`dsl` module): `parse`, `FromStr`, and `Display`. Round-trips every condition kind, and reports error positions.
- [~] `explain analyze` (`Queryable::analyze`): row counts per stage, and `Plan::unindexed` lists conditions no index answers. Fixed: `unindexed` used to list every residual, including conditions an index could answer. Timing: `analyze_timed` takes a caller clock (the core is `no_std`) and reports plan and run time. Per-stage timing is not done, because the stages run lazily in one pass, so each one would need its own timer. The CLI `--analyze` prints the phase timings.
- [x] Index advisor (`tpt_query::advisor::suggest_indexes`): B-tree for fields with ranges, hash for equality, R-tree for boxes.
- [x] Live queries (`tpt_query::live::LiveQuery`). With a change feed (`MemorySource` has one: `EntitySource::changes_since`), a refresh re-checks only the changed ids, for queries whose results depend on each entity alone. Other queries, and sources without a feed, re-run in full. `tests/live_incremental.rs` checks every incremental refresh against a full run over random inserts and removes. Cost: the change feed keeps one entry per id, including removed ids, so memory grows with the number of distinct ids ever removed. Compaction is not done. Decided: compaction deferred until a long-running live query shows memory growth.
- [~] Snapshot read view: `MemorySource::snapshot` returns a `Snapshot`. It copies every entity (O(N)) and has no indexes. Versioned storage is not done. Decided: out of scope for 1.0. Copy-on-snapshot is simple and nothing needs more yet.
- [-] Result diff helper for golden tests. Closed: `tpt_testkit::golden` already renders a unified diff on mismatch, so no helper is added here.
- [~] k-NN and ray queries. Done: `Condition::Ray` (a predicate; routed through the R-tree with `spatial`, so it is index-exact) and `Query::nearest` (the k nearest boxes, ties by id, rejected with order_by, limit, offset, after, or aggregate). Not done: routing `nearest` through the R-tree. The filters make a top-k from the index inexact, so it needs a design for that. The plan shows `nearest: k` and a scan. Decided: keep it a scan until a workload shows it is slow.
- [~] Fuzz targets: `query_from_json` has proptest robustness tests (`tests/json_robustness.rs`: arbitrary, truncated, and mutated input, plus round trips). A cargo-fuzz target is in `fuzz/` (`query_from_json`: parse, then validate and round-trip anything accepted). Run here for 122 s (`cargo +nightly fuzz run query_from_json`, cargo-fuzz 0.13.2, AddressSanitizer): 2,190,301 runs, no crashes or artifacts. On Windows the ASan DLL must be on PATH (`clang_rt.asan_dynamic-x86_64.dll`, in the VS BuildTools `VC/Tools/MSVC/<ver>/bin/Hostx64/x64` folder). Longer runs are not done. The random AST vs brute-force oracle is done (`tests/ast_oracle.rs`): nested And/Or/Not over indexed and scanned fields.

## 1.0 review (2026-10-19)
- [x] API review of the new public surface. `Nearest` and `Changes` are `#[non_exhaustive]`, so fields can be added later. `CompositeIndex::new` is crate-private: `MemorySource::create_composite_index` is the checked way in. The earlier additions (`Row::source`, `Plan::nearest`, `Analysis` timings) are on non-exhaustive types.
- [x] `cargo-deny` policy in `deny.toml`: licenses `MIT` and `Apache-2.0`, and the pinned `tpt-testkit` git source. Passes locally.
- [x] CI wasm-bindgen version lookup fixed: it read two lines before, and now reads only the version.
- [ ] Publish. Not done, by request.
