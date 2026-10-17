//! Golden-file tests: a mock CAD assembly and a fixed set of queries whose JSON
//! results are stored in `tests/fixtures/cad_assembly.expected.json` (via tpt-testkit).
//!
//! Regenerate after an intentional change with: `TPT_UPDATE_GOLDEN=1 cargo test --test golden`

#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::{FACE, PART};
use serde_json::{Value as Json, json};
use tpt_query::{
    Aabb, Condition, Edge, Entity, EntityId, EntityTypeId, MemorySource, Order, Query, Queryable,
    Traversal, Value,
};

const ASSEMBLY: EntityTypeId = EntityTypeId(10);

fn assembly() -> MemorySource {
    let mut s = MemorySource::new();
    s.insert(Entity::new(EntityId(1), ASSEMBLY).with_field("name", "bracket-assembly"));

    let parts = [
        (2u64, "bracket", "Titanium", 12.5f64),
        (3, "bolt", "Steel", 0.4),
        (4, "plate", "Aluminium", 3.25),
        (5, "washer", "Steel", 0.05),
    ];
    for (id, name, material, mass) in parts {
        s.insert(
            Entity::new(EntityId(id), PART)
                .with_field("name", name)
                .with_field("material", material)
                .with_field("mass", mass),
        );
        assert!(s.link_child(EntityId(1), EntityId(id)));
    }

    // Faces hang off their part. Each face carries an area and a bounding box.
    let faces = [
        (10u64, 2u64, 6.0f64),
        (11, 2, 2.5),
        (12, 2, 9.25),
        (13, 3, 0.1),
        (14, 4, 5.75),
        (15, 5, 0.01),
    ];
    for (id, part, area) in faces {
        let bbox = Aabb::new([0.0; 3], [area.sqrt(); 3]);
        s.insert(
            Entity::new(EntityId(id), FACE)
                .with_field("area", area)
                .with_field("bbox", bbox),
        );
        assert!(s.link_child(EntityId(part), EntityId(id)));
    }
    s
}

fn to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::Float(f) => json!(f),
        Value::Str(s) => json!(s),
        other => panic!("unsupported golden value: {other:?}"),
    }
}

fn run_case(src: &MemorySource, name: &str, query: &Query) -> Json {
    let rows = src.execute(query).expect("execute");
    let mut collected: Vec<Json> = Vec::new();
    for r in rows {
        let r = r.expect("row");
        collected.push(json!({
            "id": r.id.0,
            "projected": r.projected.iter().map(|v| v.map_or(Json::Null, to_json)).collect::<Vec<_>>(),
        }));
    }
    json!({ "query": name, "rows": collected })
}

fn cases() -> Json {
    let s = assembly();
    let titanium = Query::new(PART)
        .filter(Condition::eq_to("material", "Titanium"))
        .project("name")
        .project("mass");
    let heavy = Query::new(PART)
        .filter(Condition::gt("mass", 1.0))
        .project("name")
        .order_by("mass", true);
    let bracket_faces = Query::new(PART)
        .filter(Condition::eq_to("name", "bracket"))
        .traverse(Traversal::step(Edge::Child).filter(Condition::OfType(FACE)))
        .project("area");
    let large_faces = Query::new(ASSEMBLY)
        .traverse(
            Traversal::recursive(Edge::Child, Order::DepthFirst, None)
                .filter(Condition::OfType(FACE))
                .filter(Condition::gt("area", 5.0)),
        )
        .project("area");
    let steel_part_with_small_face = Query::new(PART)
        .filter(Condition::eq_to("material", "Steel"))
        .filter(Condition::related(
            Edge::Child,
            Some(1),
            Condition::lt("area", 1.0),
        ))
        .project("name");

    let results = vec![
        run_case(&s, "titanium_parts", &titanium),
        run_case(&s, "heavy_parts_desc", &heavy),
        run_case(&s, "bracket_faces", &bracket_faces),
        run_case(&s, "large_faces_under_assembly", &large_faces),
        run_case(
            &s,
            "steel_parts_with_small_face",
            &steel_part_with_small_face,
        ),
    ];
    Json::Array(results)
}

#[test]
fn cad_assembly_results_match_golden_file() {
    let actual = serde_json::to_string_pretty(&cases()).expect("serialize")
        + "
";
    tpt_testkit::golden::assert_text(
        env!("CARGO_MANIFEST_DIR"),
        "cad_assembly.expected.json",
        actual,
    );
}

#[test]
fn golden_helper_builds_json_for_rows() {
    // Keeps the row-to-JSON helper honest even when the golden file is regenerated.
    let s = assembly();
    let q = Query::new(PART)
        .filter(Condition::eq_to("name", "bolt"))
        .project("mass");
    let json = run_case(&s, "bolt", &q);
    assert_eq!(json["rows"][0]["id"], 3);
    assert_eq!(json["rows"][0]["projected"][0], json!(0.4));
}
