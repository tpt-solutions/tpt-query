//! Property tests for the JSON reader: arbitrary, truncated, and mutated input must
//! never panic, and any query that parses must survive a round trip.

#![cfg(all(feature = "json", not(target_arch = "wasm32")))]

use proptest::prelude::*;
use tpt_query::json::{query_from_json, query_to_json};

const VALID: &str = r#"{"source":1,"filters":[{"Between":{"field":"mass","low":1,"high":9}}],"traversals":[{"edge":"Child","order":"DepthFirst","max_depth":2,"filters":[]}],"projections":[{"field":"name"}],"order_by":{"field":"mass","descending":true},"limit":5,"offset":1,"aggregate":null}"#;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics(s in ".*") {
        let _ = query_from_json(&s);
    }

    #[test]
    fn truncated_queries_never_panic(cut in 0..VALID.len()) {
        let _ = query_from_json(&VALID[..cut]);
    }

    #[test]
    fn mutated_queries_never_panic(at in 0..VALID.len(), ch in any::<char>()) {
        let mut chars: Vec<char> = VALID.chars().collect();
        chars[at] = ch;
        let mutated: String = chars.into_iter().collect();
        let _ = query_from_json(&mutated);
    }

    #[test]
    fn parsed_queries_round_trip(at in 0..VALID.len(), ch in any::<char>()) {
        let mut chars: Vec<char> = VALID.chars().collect();
        chars[at] = ch;
        let mutated: String = chars.into_iter().collect();
        if let Ok(q) = query_from_json(&mutated) {
            let text = query_to_json(&q).expect("a parsed query serializes");
            prop_assert_eq!(query_from_json(&text).expect("round trip parses"), q);
        }
    }
}
