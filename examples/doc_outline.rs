//! A document outline over a Document > Page > Paragraph tree: the headings in
//! order, the headings of each document, and keyset paging through the headings.
//!
//! Run with: `cargo run --example doc_outline`

use tpt_query::{
    Condition, Edge, Entity, EntityId, EntityTypeId, MemorySource, Order, Query, Queryable,
    Traversal,
};

const DOC: EntityTypeId = EntityTypeId(1);
const PAGE: EntityTypeId = EntityTypeId(2);
const PARA: EntityTypeId = EntityTypeId(3);

fn main() {
    let mut src = MemorySource::new();
    src.insert(Entity::new(EntityId(1), DOC).with_field("title", "Mounting guide"));
    src.insert(Entity::new(EntityId(2), PAGE).with_field("number", 1i64));
    src.insert(Entity::new(EntityId(3), PAGE).with_field("number", 2i64));
    src.link_child(EntityId(1), EntityId(2));
    src.link_child(EntityId(1), EntityId(3));

    // Ids ascend in document order, so id order is reading order.
    let paragraphs = [
        (4, 2, "Heading", "Overview"),
        (5, 2, "Body", "Fasten the bracket to the plate."),
        (6, 3, "Heading", "Torque"),
        (7, 3, "Body", "Tighten the bolts to 9 Nm."),
    ];
    for (id, page, style, text) in paragraphs {
        src.insert(
            Entity::new(EntityId(id), PARA)
                .with_field("style", style)
                .with_field("text", text),
        );
        src.link_child(EntityId(page), EntityId(id));
    }

    println!("== headings in reading order ==");
    let all_headings = Query::new(PARA)
        .filter(Condition::eq_to("style", "Heading"))
        .project("text");
    for row in src.execute(&all_headings).unwrap() {
        let row = row.unwrap();
        println!(
            "  {}",
            row.get("text").and_then(|v| v.as_str()).unwrap_or("")
        );
    }

    println!("== headings under each document ==");
    let docs = Query::new(DOC).traverse(
        Traversal::recursive(Edge::Child, Order::DepthFirst, None)
            .filter(Condition::eq_to("style", "Heading")),
    );
    for row in src.execute(&docs).unwrap() {
        let row = row.unwrap();
        println!("  paragraph {} is a heading", row.id.0);
    }

    println!("== documents that contain a heading ==");
    let with_heading = Query::new(DOC).filter(Condition::related(
        Edge::Child,
        None,
        Condition::eq_to("style", "Heading"),
    ));
    for row in src.execute(&with_heading).unwrap() {
        let row = row.unwrap();
        println!("  document {}", row.id.0);
    }

    println!("== paging through headings, one per page ==");
    let mut cursor: Option<EntityId> = None;
    loop {
        let mut page = Query::new(PARA)
            .filter(Condition::eq_to("style", "Heading"))
            .limit(1);
        if let Some(after) = cursor {
            page = page.after(after);
        }
        let mut seen = false;
        for row in src.execute(&page).unwrap() {
            let row = row.unwrap();
            println!("  heading {}", row.id.0);
            cursor = Some(row.id);
            seen = true;
        }
        if !seen {
            break;
        }
    }
}
