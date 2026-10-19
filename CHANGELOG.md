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
- `MemorySource::drop_index`, `QueryError::DuplicateIndex`, `Row::get`, and typed
  `Value` getters (`as_str`, `as_i64`, `as_f64`, `as_bool`).
- Conditions `In`, `Exists`, `StartsWith`, `EndsWith`, and `Between`.
- Paging: `Query::offset` and keyset paging with `Query::after`.
- `json::execute_json` and `json::rows_to_json`, which return rows as a JSON array.
- `Metric` is serializable (feature `serde`).
- `prelude` module, and the `docs/cookbook.md` recipes.
- Rustdoc quick start and a runnable example for the crate and each module, run by
  `cargo test --doc`.
- `Between` is answered from a B-tree index on the field, like the other range
  conditions. Before, it was always evaluated as a filter.
- Examples `doc_outline` and `json_cli`, both run by `scripts/ci.sh`.
- Aggregation in the query: `Query::aggregate` and `Queryable::aggregate`, grouped or
  ungrouped. `json::aggregate_json` returns `[{"key", "value"}]`.
- `docs/query.schema.json`, a hand-written JSON Schema for the query form. A test checks
  its references and that it covers every condition variant.
- `.github/workflows/ci.yml` (runs `scripts/ci.sh`, docs with warnings denied, and
  cargo-audit and cargo-deny). It has not run yet.
- Crate metadata: repository, homepage, documentation, and exclude list. A dry-run
  publish packages and verifies the crate.

- `Query::aggregate` and `Queryable::aggregate`: a reduction as part of the query.
- `Query::distinct`, `Traversal::descend_while`, and `Condition::OfTypeIn` and
  `Condition::ContainsPoint`.
- `Queryable::analyze`: row counts per stage. `Plan::unindexed` lists conditions no
  index answers.
- `tpt_query::schema`: `Schema::validate_against`, a strict field and kind check.
- `tpt_query::advisor::suggest_indexes`, and `tpt_query::live::LiveQuery` (diff-based,
  not incremental).
- `json::execute_json`, `rows_to_json`, `aggregate_json`; `docs/query.schema.json`;
  `docs/performance.md`; `templates/queries/`; examples `custom_source`,
  `spreadsheet_range`, and `cad_viewport`.
- Property tests for the JSON reader (no panics, round trips).
- `dsl` module: a text form of queries. `dsl::parse` and `FromStr` read it, and `Display`
  for `Query` writes it. They round-trip every condition kind.
- `Condition` and `Traversal` gained `OfTypeIn`, `ContainsPoint`, and `descend` (the
  `descend_while` builder). `Query` gained `distinct`.
- `Snapshot` (`MemorySource::snapshot`): a read-only copy of a source. It copies every
  entity and keeps no indexes.
- `RTree::nearest` (k-nearest by box distance) and `RTree::ray` (boxes a ray enters), and
  the `ray_enter` function. Both are standalone, not query conditions yet.
- `Plan::unindexed` and `Queryable::analyze` (`Analysis`): row counts per stage.
- `schema::Schema` and `advisor::suggest_indexes`; the `live::LiveQuery` diff.
- Binary `tpt-query` behind feature `cli`: loads entities from JSON, runs a DSL or JSON
  query, and prints rows, the plan, or the analysis.
- Field paths accept list indexes (`items.0`) and escaped dots (`a\.b`).

### Changed
- `create_index` and `add_index` return `Result`, and reject a duplicate index name.
- `EntitySource::index_lookup` and `Index::lookup` return an `IndexHit` with a lazy
  id iterator and a known length. Indexed queries no longer copy candidates out of
  the index, so `limit` bounds memory on the indexed path. The R-tree search is
  still collected.
- The planner splits top-level `And`s, and it picks the index with the fewest
  candidates across all indexes on the type.
- Values use plain JSON (`"Titanium"`, `5`, `true`) instead of externally tagged
  objects. A bounding box is `{"Aabb": {...}}`.
- Optional query fields (`filters`, `traversals`, `projections`, `limit`, `order_by`)
  may be omitted in JSON.
- `traverse::Walker::new` takes a `descend` argument. Most code is unaffected, since the
  engine is the only caller of the walker.
- `Query`, `Row`, `Plan`, `Access`, `Value`, and `Metric` are `#[non_exhaustive]`. Build
  queries with `Query::new` and the builder methods.
- Queries containing NaN or infinite numbers are rejected by `validate`, and by
  `query_to_json`.

### Fixed
- Field names with a `.` or a backslash are written escaped, and list items are reachable
  by index (`items.0`). A path that used a backslash before now reads as an escape.
- Depth-limited DFS and BFS walks now reach nodes through a shorter path that
  reaches them after a longer one (DFS previously missed nodes on DAGs).
- `Traversal` filters apply to yielded nodes only; the walk continues through
  nodes that fail them.

### Known limitations
- Hash indexes are ordered maps, so exact lookups are O(log N).
- `order_by` runs on the first pull, and it materializes the sorted rows. With a
  `limit` it keeps only the best `limit` rows.
- Named edges are outgoing only; removing a target leaves dangling references that
  traversals report as `MissingEntity`.
- The R-tree does not condense underfull nodes on removal.
- The data model is a local stub pending `tpt-uir` / `tpt-model`.
