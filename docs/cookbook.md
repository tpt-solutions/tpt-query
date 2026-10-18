# tpt-query cookbook

Recipes for the builder API. Each one is covered by a test or runs in an example:
`examples/quickstart.rs`, `examples/doc_outline.rs`, and `examples/json_cli.rs`.
The plan output under each recipe is real: `examples/cookbook.rs` loads three parts
and prints `explain` for the queries (`cargo run --example cookbook`).
Assume the imports below and a `MemorySource` named `src` holding parts (type `PART`).

```rust
use tpt_query::{Condition, Edge, Entity, EntityId, EntityTypeId, IndexKind, MemorySource,
    Order, Query, Queryable, Traversal, Value};
const PART: EntityTypeId = EntityTypeId(1);
```

## 1. Index a field and check the plan

```rust
src.create_index("by_material", PART, "material", IndexKind::Hash).unwrap();
let q = Query::new(PART).filter(Condition::eq_to("material", "Steel"));
println!("{}", src.explain(&q).unwrap());   // access: index `by_material` (n candidates)
```

Use `IndexKind::BTree` for `gt`, `gte`, `lt`, `lte`, and `between`-style ranges.
`explain` counts candidates without reading them.

```text
access: index `by_material` (2 candidates)
residual filters: 0
traversals: 0
ordered: false
limit: none
unindexed: none
```

## 2. Combine conditions

Top-level filters and `Condition::all` are ANDed, and each conjunct can use an index.

```rust
let q = Query::new(PART).filter(Condition::all(vec![
    Condition::eq_to("material", "Steel"),
    Condition::gt("mass", Value::Float(0.1)),
]));
```

The planner picks the index with the fewest candidates. The other conjunct stays a
residual filter. Here `by_material` is chosen, and `mass` is checked per candidate.
A residual is listed under `unindexed` only when no index can answer it. A `gt` on
`mass` would be listed only if `mass` had no index.

```text
access: index `by_material` (2 candidates)
residual filters: 1
traversals: 0
ordered: false
limit: none
unindexed: none
```

## 3. Match one of several values

```rust
let q = Query::new(PART).filter(Condition::In {
    field: "material".into(),
    values: vec![Value::from("Steel"), Value::from("Copper")],
});
```

`In` is evaluated per entity. It does not use an index yet, so `explain` lists it as
unindexed:

```text
access: scan entities of type 1
residual filters: 1
traversals: 0
ordered: false
limit: none
unindexed: in on `material`
```

## 4. Inclusive ranges and string affixes

```rust
Condition::Between { field: "mass".into(), low: Value::Float(0.4), high: Value::Float(8.0) };
Condition::StartsWith { field: "name".into(), value: Value::from("bolt") };
Condition::EndsWith { field: "name".into(), value: Value::from("-s") };
```

`Between` bounds must be the same kind of value. `StartsWith` and `EndsWith` only
match string fields.

## 5. Fields that are missing or null

```rust
// Present and not null:
Condition::Exists { field: "material".into() };
// Missing or null:
Condition::negate(Condition::Exists { field: "material".into() });
```

`Ne` requires the field to exist. `Not(Eq)` matches entities without the field.

## 6. Walk a containment tree

```rust
// Every face under a part, depth first. A non-matching node is still walked through.
let q = Query::new(PART).traverse(
    Traversal::recursive(Edge::Child, Order::DepthFirst, None)
        .filter(Condition::OfType(FACE)),
);
```

Use `Traversal::recursive(Edge::Child, Order::BreadthFirst, Some(2))` for a
two-level limit. Depth limits keep the shortest path, so a node reached early by
a long route is still found.

```text
access: scan entities of type 1
residual filters: 0
traversals: 1
ordered: false
limit: none
unindexed: none
```

## 7. Parts that have a matching descendant

```rust
let q = Query::new(PART).filter(Condition::related(
    Edge::Child,
    None,
    Condition::eq_to("kind", "fillet"),
));
```

`related` is walked per entity, so it is listed as unindexed. Its plan has the scan
and the residual, and no index is involved.

```text
access: scan entities of type 1
residual filters: 1
traversals: 0
ordered: false
limit: none
unindexed: related
```

## 8. Page through results

Offset paging is simple, but the cost grows with the offset:

```rust
let page = Query::new(PART).order_by("mass", true).offset(20).limit(10);
```

Keyset paging keeps the cost flat. Source ids ascend, so resume after the last id seen:

```rust
let page = Query::new(PART).after(last_seen).limit(10);   // not combined with order_by
```

The keyset plan stops at the limit, but its `after` cursor is a filter over ids, so it
still reads from the start rather than seeking. The offset plan sorts, so it reads
every entity and keeps only the rows it needs.

```text
access: scan entities of type 1      # order_by mass, offset 1, limit 1
residual filters: 0
traversals: 0
ordered: true
limit: 1
unindexed: none
```

```text
access: scan entities of type 1      # after id 1, limit 1
residual filters: 0
traversals: 0
ordered: false
limit: 1
unindexed: none
```

## 9. Sort the largest first

```rust
let q = Query::new(PART).order_by("mass", true).limit(5);
```

With a limit, only the best rows are kept, so memory is O(limit).

```text
access: scan entities of type 1
residual filters: 0
traversals: 0
ordered: true
limit: 2
unindexed: none
```

## 10. Get JSON out (feature `json`)

```rust
use tpt_query::json::{execute_json, query_from_json};

let q = query_from_json(r#"{
    "source": 1,
    "filters": [{"Eq": {"field": "material", "value": "Steel"}}],
    "projections": [{"field": "name"}]
}"#)?;
let rows_json = execute_json(&src, &q)?;   // [{"id":2,"name":"bolt"}, ...]
```

Values are plain JSON. A bounding box is `{"Aabb": {"min": [..], "max": [..]}}`.
Run `echo '<query>' | cargo run --example json_cli --features json` to try it.

## 11. Handle a dangling reference

Named edges are stored outgoing only. If a target is removed, a traversal reports
`QueryError::MissingEntity` for that id, and the iterator keeps going:

```rust
for row in src.execute(&q)? {
    match row {
        Ok(row) => println!("{}", row.id.0),
        Err(e) => eprintln!("skipped: {e}"),
    }
}
```

## 12. Aggregate instead of listing rows

```rust
let totals = src.aggregate(
    &Query::new(PART).aggregate(Metric::Sum("mass".into()), Some("material".into())),
)?;   // [(Value::Str("Aluminium"), ..), (Value::Str("Steel"), ..), ...] in key order
```

Without `group_by`, the result is one pair with a `Null` key. Aggregate queries take
no projections, `order_by`, `limit`, `offset`, or `after`, and `execute` refuses them.
Over JSON, set `"aggregate": {"metric": {"Sum": "mass"}, "group_by": "material"}`
and call `json::aggregate_json`. The JSON form is described in `docs/query.schema.json`.

## 13. Time the phases

`analyze_timed` takes a clock in nanoseconds from the caller. The core has no clock.
It times planning and running separately. Stages run lazily in one pass, so it
cannot time each stage on its own.

```rust
use std::time::Instant;
let start = Instant::now();
let clock = || start.elapsed().as_nanos() as u64;
let a = src.analyze_timed(&q, &clock)?;
println!("plan {:?} ns, run {:?} ns", a.plan_nanos, a.run_nanos);
```

The `run` figure includes a second planning pass, because the row stream is built
from the query again. The `tpt-query` CLI prints these with `--analyze`.

```text
read 2, returned 2, plan Some(250) ns, run Some(250) ns
```

The numbers above come from a fixed-step clock in the example, so they are not real
timings.

## 14. Suggest indexes from a workload

`advisor::suggest_indexes` reads a list of queries and proposes indexes, most useful
first. Ranges get a B-tree, equality gets a hash index, and boxes get an R-tree.

```rust
use tpt_query::advisor::suggest_indexes;
let suggestions = suggest_indexes(&workload);
```

```text
EntityTypeId(1) on `mass` for 2 queries: BTree
EntityTypeId(1) on `material` for 1 queries: Hash
```

## 15. Watch a query for changes

`LiveQuery::refresh` reports which ids were added or removed since the last refresh.
With `MemorySource`, a query that depends only on each entity re-checks just the
entities that changed. Other queries re-run in full.

```rust
use tpt_query::live::LiveQuery;
let mut live = LiveQuery::new(Query::new(PART).filter(Condition::eq_to("material", "Steel")));
let first = live.refresh(&src)?;            // everything matching, as added
src.insert(Entity::new(EntityId(200), PART).with_field("material", "Steel"));
let second = live.refresh(&src)?;           // added: [EntityId(200)]
```

```text
first: added [EntityId(1), EntityId(3)]
after an insert: added [EntityId(200)], removed []
```
