# tpt-query

An embedded, pure-Rust, `no_std + alloc` declarative query engine for structured
entity graphs. It queries in-memory entity collections directly, with no database
server, no C FFI, and no unsafe code.

## Features

- **Typed builder API**: filters (`eq_to`, `ne_to`, `gt`, `gte`, `lt`, `lte`,
  `contains`, `intersects`, `all`, `any`, `negate`), graph traversal, projection,
  ordering, and limits.
- **Graph traversal** along parent, child, or named edges. Depth-first and
  breadth-first walks, depth limits, and cycle detection. Traversals never scan the
  whole collection; a source without a relation map is rejected.
- **Streaming execution**: results are a lazy iterator of zero-copy rows that
  borrow from the source. A `limit` stops work early.
- **Indexes**: hash and B-tree value indexes, plus an R-tree for bounding-box
  intersection (feature `spatial`). Indexes are maintained on every insert, update,
  and delete. The planner picks the most selective index, and `explain` reports the
  plan.
- **Aggregation**: streaming `Count`, `Sum`, `Min`, `Max`, `Avg`, and `group_by`.
- **JSON AST** (feature `json`) for cross-language clients.
- **Deterministic results**: sources yield ids in ascending order, and traversals
  follow insertion order.

## Quick start

```rust
use tpt_query::{Condition, Edge, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Order, Query, Queryable, Traversal};

const PART: EntityTypeId = EntityTypeId(1);
const FACE: EntityTypeId = EntityTypeId(2);

let mut src = MemorySource::new();
src.create_index("by_material", PART, "material", IndexKind::Hash);
src.insert(Entity::new(EntityId(1), PART).with_field("material", "Titanium"));
src.insert(Entity::new(EntityId(2), FACE).with_field("area", 4.5f64));
src.link_child(EntityId(1), EntityId(2));

// Faces of every titanium part.
let query = Query::new(PART)
    .filter(Condition::eq_to("material", "Titanium"))
    .traverse(Traversal::recursive(Edge::Child, Order::DepthFirst, None).filter(Condition::OfType(FACE)))
    .project("area");

for row in src.execute(&query).unwrap() {
    let row = row.unwrap();
    println!("face {} {:?}", row.id.0, row.projected);
}
println!("{}", src.explain(&query).unwrap());
```

## Features (Cargo)

| Feature   | Adds                                                        |
|-----------|-------------------------------------------------------------|
| `std`     | `std::error::Error` for `QueryError`                        |
| `spatial` | R-tree index (`spatial::RTree`, `spatial::RTreeIndex`)      |
| `serde`   | Serialize/Deserialize on the query AST and values           |
| `json`    | `json::query_from_json` / `query_to_json` (implies `serde`) |

The core builds without any features.

## Development

```sh
bash scripts/ci.sh                                  # fmt, clippy, tests, benches, no_std and wasm checks
cargo bench --bench index                           # indexed lookups vs scans (criterion)
cargo run --example quickstart                      # the quick start above
TPT_UPDATE_GOLDEN=1 cargo test --test golden        # regenerate golden results after an intended change
```

Test tooling comes from [tpt-testkit](https://github.com/tpt-solutions/tpt-testkit),
a dev-dependency only. It provides golden fixtures (`tests/fixtures/`), property
testing via proptest, and criterion benchmarks named `area/format/size`.

The wasm smoke test runs under Node through `wasm-bindgen-test-runner`. Install the
CLI that matches the lockfile: `cargo install -f wasm-bindgen-cli --version <version in Cargo.lock>`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

## Limits

- Hash indexes are backed by an ordered map, so exact lookups are O(log N) rather
  than O(1). The crate avoids a hash-map dependency to stay `no_std`.
- `order_by` materializes the result set. Other operators stream.
- Named edges are stored outgoing only. Removing a target leaves dangling named
  references, which traversals report as `MissingEntity`.
- The R-tree does not condense underfull nodes on removal.
- The data model is a local stub (`EntityId`, `Value`, `EntitySource`). It is meant to
  be replaced by `tpt-uir` / `tpt-model` when those exist.
