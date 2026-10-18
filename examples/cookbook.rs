//! Prints the plan for the cookbook's row recipes, so `docs/cookbook.md` shows
//! real `explain` output. Run with: `cargo run --example cookbook`

use tpt_query::advisor::suggest_indexes;
use tpt_query::live::LiveQuery;
use tpt_query::{
    Condition, Edge, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Order, Query,
    Queryable, Traversal, Value,
};

const PART: EntityTypeId = EntityTypeId(1);
const FACE: EntityTypeId = EntityTypeId(2);

fn main() {
    let mut src = MemorySource::new();
    src.create_index("by_material", PART, "material", IndexKind::Hash)
        .unwrap();
    src.create_index("by_mass", PART, "mass", IndexKind::BTree)
        .unwrap();
    for (i, (material, mass)) in [("Steel", 0.5), ("Copper", 2.0), ("Steel", 9.0)]
        .into_iter()
        .enumerate()
    {
        let id = EntityId(i as u64 + 1);
        src.insert(
            Entity::new(id, PART)
                .with_field("material", material)
                .with_field("mass", Value::Float(mass))
                .with_field("kind", "fillet"),
        );
        let face = EntityId(100 + i as u64);
        src.insert(Entity::new(face, FACE).with_field("area", 1.0f64));
        src.link_child(id, face);
    }

    let recipes: [(&str, Query); 8] = [
        (
            "1. Index a field",
            Query::new(PART).filter(Condition::eq_to("material", "Steel")),
        ),
        (
            "2. Combine conditions",
            Query::new(PART).filter(Condition::all(vec![
                Condition::eq_to("material", "Steel"),
                Condition::gt("mass", Value::Float(0.1)),
            ])),
        ),
        (
            "6. Walk a containment tree",
            Query::new(PART).traverse(
                Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                    .filter(Condition::OfType(FACE)),
            ),
        ),
        (
            "7. Parts with a matching descendant",
            Query::new(PART).filter(Condition::related(
                Edge::Child,
                None,
                Condition::eq_to("kind", "fillet"),
            )),
        ),
        (
            "8. Offset paging",
            Query::new(PART).order_by("mass", true).offset(1).limit(1),
        ),
        (
            "8. Keyset paging",
            Query::new(PART).after(EntityId(1)).limit(1),
        ),
        (
            "9. Largest first",
            Query::new(PART).order_by("mass", true).limit(2),
        ),
        (
            "3. In, without an index",
            Query::new(PART).filter(Condition::In {
                field: "material".into(),
                values: vec![Value::from("Steel"), Value::from("Copper")],
            }),
        ),
    ];

    for (name, query) in &recipes {
        println!("## {name}\n{}\n", src.explain(query).unwrap());
    }

    // 13. Time the phases. This example's clock is a fixed step per call.
    let steel = Query::new(PART).filter(Condition::eq_to("material", "Steel"));
    let ticks = std::cell::Cell::new(0u64);
    let clock = || {
        ticks.set(ticks.get() + 250);
        ticks.get()
    };
    let timed = src.analyze_timed(&steel, &clock).unwrap();
    println!(
        "## 13. Time the phases\nread {}, returned {}, plan {:?} ns, run {:?} ns\n",
        timed.read, timed.returned, timed.plan_nanos, timed.run_nanos
    );

    // 14. Ask the advisor which indexes a workload would use.
    let workload = [
        Query::new(PART).filter(Condition::eq_to("material", "Steel")),
        Query::new(PART).filter(Condition::gt("mass", Value::Float(1.0))),
        Query::new(PART).filter(Condition::gt("mass", Value::Float(0.2))),
    ];
    println!("## 14. Suggest indexes");
    for s in suggest_indexes(&workload) {
        println!(
            "{:?} on `{}` for {} queries: {:?}",
            s.ty, s.field, s.queries, s.kind
        );
    }
    println!();

    // 15. Watch a query for changes.
    let mut live = LiveQuery::new(steel);
    let first = live.refresh(&src).unwrap();
    src.insert(Entity::new(EntityId(200), PART).with_field("material", "Steel"));
    let second = live.refresh(&src).unwrap();
    println!(
        "## 15. Watch a query\nfirst: added {:?}\nafter an insert: added {:?}, removed {:?}",
        first.added, second.added, second.removed
    );
}
