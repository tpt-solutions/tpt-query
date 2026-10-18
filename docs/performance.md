# Performance model

What each stage of a query costs, and what it holds in memory. Use it to predict
the cost of a query before you run it, and to see where `limit` helps.

## Streaming (memory O(1) per row)

These stages pull one item at a time. A `limit` stops them early.

- **Candidates.** A scan walks the entities of one type in id order. An index hit
  iterates the stored id sets directly (exact match) or merges the sets of the
  matching keys in ascending order (range). Neither copies candidates out of the index.
- **Cursor (`after`).** A filter over the ascending candidates. It does not seek,
  so resuming late in a long source still reads the earlier candidates.
- **Residual filters.** Each leftover condition is evaluated per candidate.
- **Traversal.** The walker keeps a frontier and a visited map. Each node's
  neighbours are fetched from the relation map, and nothing scans the collection.
- **Projection, `offset`, and `limit`.** Projection copies references, and `offset`
  and `limit` skip or stop the stream.

## Materialized (memory grows with the result)

These stages hold data until they can emit.

| Stage | Holds | Memory |
|---|---|---|
| `order_by` without `limit` | every matched row | O(matches) |
| `order_by` with `limit` | the best `limit` rows, in a heap | O(limit) |
| `distinct` | every entity id yielded so far | O(distinct results) |
| traversal | the visited and best-depth maps | O(reached nodes) |
| `group_by` | one accumulator per group | O(groups) |
| R-tree query | every hit, sorted | O(hits) |
| live query | the full id set of the last run | O(results) |
| `rows_to_json` / `execute_json` | the whole output string | O(output) |

`Queryable::analyze` and `explain` show which path a query takes. `analyze` also
reports how many rows reached each stage.

## Planning cost

`explain` looks up every top-level conjunct on every index for its type, then picks
the smallest. A hash or B-tree equality lookup costs O(log N). A B-tree range costs
O(keys in the range) for its length count. Planning reads no candidates.

## Measured on this machine

From `benches/index.rs` (criterion, optimized build, indexed point lookup against a
full scan). The indexed time stayed flat across sizes in this run.

| Entities | Indexed lookup | Scan |
|---|---|---|
| 10k | about 0.7 µs | 0.75 ms |
| 100k | about 0.7 µs | 13.6 ms |
| 1M | about 0.85 µs | 153 ms |

These numbers come from one machine. Run `cargo bench --bench index` on yours before
you rely on them.

## Choosing an index

| Query shape | Index | Notes |
|---|---|---|
| `Eq` on a field | `Hash` | Ordered map here, so O(log N) |
| `Gt`, `Gte`, `Lt`, `Lte`, `Between`, or ranges plus equality | `BTree` | Serves equality too |
| `Intersects` on a box field | R-tree (`spatial` feature) | Results are collected, not streamed |
| Everything else (`In`, `Contains`, `StartsWith`, `Not`, `Or`) | none | Evaluated per candidate |

A `BTree` is the safer default when a field is used both ways. `Index advisor`
(`tpt_query::advisor::suggest_indexes`) ranks candidates from a workload.
