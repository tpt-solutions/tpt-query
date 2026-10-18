//! Every file in `templates/queries/` must parse, validate, and round-trip.

#![cfg(all(feature = "json", not(target_arch = "wasm32")))]

use tpt_query::json::{query_from_json, query_to_json};

#[test]
fn every_template_parses_validates_and_round_trips() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/queries");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("templates directory") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read template");
        let q = query_from_json(&text)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
        let again =
            query_from_json(&query_to_json(&q).expect("serialize")).expect("round trip parses");
        assert_eq!(again, q, "{}", path.display());
        count += 1;
    }
    assert!(
        count >= 4,
        "expected at least four templates, found {count}"
    );
}
