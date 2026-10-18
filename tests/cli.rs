//! Runs the `tpt-query` binary against the scene fixture.

#![cfg(feature = "cli")]

use std::process::Command;

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tpt-query"))
        .args([
            "--entities",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scene.json"),
        ])
        .args(args)
        .output()
        .expect("run binary");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn dsl_query_prints_rows_as_json() {
    let (ok, out, _) = run(&["--dsl", "from 1 where mass > 1.0 select name order by mass"]);
    assert!(ok);
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        rows,
        serde_json::json!([
            {"id": 3, "name": "plate"},
            {"id": 1, "name": "bracket"}
        ])
    );
}

#[test]
fn json_query_and_aggregate_work() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cli_aggregate.json"
    );
    let (ok, out, _) = run(&["--json", path]);
    assert!(ok);
    let groups: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(groups.as_array().unwrap().len(), 3);
}

#[test]
fn explain_and_analyze_print_the_plan() {
    let (ok, out, _) = run(&[
        "--dsl",
        "from 1 where material = \"Steel\"",
        "--index",
        "1:material:hash",
        "--explain",
    ]);
    assert!(ok);
    assert!(out.contains("index `1_material`"), "{out}");

    let (ok, out, _) = run(&["--dsl", "from 1 where mass > 1.0", "--analyze"]);
    assert!(ok);
    assert!(out.contains("returned 2"), "{out}");
}

#[test]
fn bad_input_exits_nonzero_with_a_message() {
    let (ok, _, err) = run(&["--dsl", "from 1 where"]);
    assert!(!ok);
    assert!(err.contains("expected a field name"), "{err}");

    let (ok, _, err) = run(&[]);
    assert!(!ok);
    assert!(err.contains("exactly one of"), "{err}");
}
