//! A spreadsheet-style range: cells with a row and a column. A B-tree index on the row
//! answers a range of rows, and an aggregate totals each column over that range.
//!
//! Run with: `cargo run --example spreadsheet_range`

use tpt_query::{
    Condition, Entity, EntityId, EntityTypeId, IndexKind, MemorySource, Metric, Query, Queryable,
    Value,
};

const CELL: EntityTypeId = EntityTypeId(1);

fn main() {
    let mut sheet = MemorySource::new();
    sheet
        .create_index("by_row", CELL, "row", IndexKind::BTree)
        .unwrap();

    // Rows 1..=5, columns A and B. Ids are assigned in reading order.
    let mut id = 0u64;
    for row in 1..=5i64 {
        for (col, value) in [("A", row * 10), ("B", row * 100)] {
            id += 1;
            sheet.insert(
                Entity::new(EntityId(id), CELL)
                    .with_field("row", Value::Int(row))
                    .with_field("col", col)
                    .with_field("value", Value::Int(value)),
            );
        }
    }

    // Rows 2 through 4, totalled per column.
    let range = Condition::Between {
        field: "row".into(),
        low: Value::Int(2),
        high: Value::Int(4),
    };
    let totals = sheet
        .aggregate(
            &Query::new(CELL)
                .filter(range.clone())
                .aggregate(Metric::Sum("value".into()), Some("col".into())),
        )
        .unwrap();
    for (col, total) in totals {
        println!("column {} total {:?}", col.as_str().unwrap_or("?"), total);
    }

    println!(
        "{}",
        sheet.explain(&Query::new(CELL).filter(range)).unwrap()
    );
}
