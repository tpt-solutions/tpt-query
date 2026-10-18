//! Runs a JSON query from stdin against a small built-in catalogue and prints the
//! rows as JSON. Shows the wire format a non-Rust client would send and receive.
//!
//! Run with:
//! `echo '{"source":1,"filters":[{"Eq":{"field":"material","value":"Steel"}}],"projections":[{"field":"name"}]}' | cargo run --example json_cli --features json`

use std::io::Read;

use tpt_query::{
    Entity, EntityId, EntityTypeId, MemorySource,
    json::{execute_json, query_from_json},
};

const PART: EntityTypeId = EntityTypeId(1);

fn main() {
    let mut src = MemorySource::new();
    for (id, name, material, mass) in [
        (1u64, "bracket", "Titanium", 12.5f64),
        (2, "bolt", "Steel", 0.4),
        (3, "plate", "Aluminium", 3.25),
        (4, "washer", "Steel", 0.05),
    ] {
        src.insert(
            Entity::new(EntityId(id), PART)
                .with_field("name", name)
                .with_field("material", material)
                .with_field("mass", mass),
        );
    }

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .expect("read stdin");

    match query_from_json(&input).and_then(|q| execute_json(&src, &q)) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
