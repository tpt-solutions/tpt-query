# tpt-query cookbook

Recipes for the builder API. Each one is covered by a test or runs in an example:
`examples/quickstart.rs`, `examples/doc_outline.rs`, and `examples/json_cli.rs`.
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

## 2. Combine conditions

Top-level filters and `Condition::all` are ANDed, and each conjunct can use an index.

```rust
let q = Query::new(PART).filter(Condition::all(vec![
    Condition::eq_to("material", "Steel"),
    Condition::gt("mass", Value::Float(0.1)),
]));
```

## 3. Match one of several values

```rust
let q = Query::new(PART).filter(Condition::In {
    field: "material".into(),
    values: vec![Value::from("Steel"), Value::from("Copper")],
});
```

`In` is evaluated per entity. It does not use an index yet.

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

## 7. Parts that have a matching descendant

```rust
let q = Query::new(PART).filter(Condition::related(
    Edge::Child,
    None,
    Condition::eq_to("kind", "fillet"),
));
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

## 9. Sort the largest first

```rust
let q = Query::new(PART).order_by("mass", true).limit(5);
```

With a limit, only the best rows are kept, so memory is O(limit).

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
