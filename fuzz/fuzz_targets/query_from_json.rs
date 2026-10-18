//! Feeds arbitrary bytes to the JSON query parser and checks that anything it
//! accepts validates and survives a round trip. Run with `cargo fuzz run query_from_json`
//! from the repository root (needs `cargo install cargo-fuzz` and nightly).

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_query::json::{query_from_json, query_to_json};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(query) = query_from_json(text) {
        let json = query_to_json(&query).expect("an accepted query serializes");
        let again = query_from_json(&json).expect("serialized query parses again");
        assert_eq!(query, again);
    }
});
