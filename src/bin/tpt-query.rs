//! Command-line runner: load entities from a JSON file, run a query, print the result.
//!
//! ```text
//! tpt-query --entities scene.json --dsl 'from 1 where mass > 2 select name'
//! tpt-query --entities scene.json --json query.json --explain
//! tpt-query --entities scene.json --dsl 'from 1' --index 1:material:hash --analyze
//! ```
//!
//! Entities file: `{"entities": [{"id": 1, "type": 1, "fields": {...},
//! "parent": 0, "named": {"label": [2, 3]}}]}`. `parent` and `named` are optional.
//! Values use the plain JSON form (see the `json` module). Build with `--features cli`.

use std::{collections::BTreeMap, process::ExitCode};

use serde::Deserialize;
use tpt_query::{
    Entity, EntityId, EntityTypeId, FieldPath, IndexKind, MemorySource, Query, Queryable, Value,
    json::{aggregate_json, execute_json, query_from_json},
};

#[derive(Deserialize)]
struct EntitiesFile {
    entities: Vec<EntityDoc>,
}

#[derive(Deserialize)]
struct EntityDoc {
    id: u64,
    #[serde(rename = "type")]
    ty: u32,
    #[serde(default)]
    fields: BTreeMap<String, Value>,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    named: BTreeMap<String, Vec<u64>>,
}

#[derive(Default)]
struct Options {
    entities: Option<String>,
    dsl: Option<String>,
    json: Option<String>,
    indexes: Vec<String>,
    explain: bool,
    analyze: bool,
}

const USAGE: &str = "usage: tpt-query --entities FILE (--dsl TEXT | --json FILE) \
[--index TYPE:FIELD:hash|btree]... [--explain] [--analyze]";

fn parse_args() -> Result<Options, String> {
    let mut o = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--entities" => o.entities = Some(value("--entities")?),
            "--dsl" => o.dsl = Some(value("--dsl")?),
            "--json" => o.json = Some(value("--json")?),
            "--index" => o.indexes.push(value("--index")?),
            "--explain" => o.explain = true,
            "--analyze" => o.analyze = true,
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    if o.entities.is_none() {
        return Err(format!("--entities is required\n{USAGE}"));
    }
    if o.dsl.is_some() == o.json.is_some() {
        return Err(format!("give exactly one of --dsl or --json\n{USAGE}"));
    }
    Ok(o)
}

fn load(path: &str) -> Result<MemorySource, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let file: EntitiesFile = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let mut src = MemorySource::new();
    for doc in &file.entities {
        let mut entity = Entity::new(EntityId(doc.id), EntityTypeId(doc.ty));
        entity.fields = doc.fields.clone();
        src.insert(entity);
    }
    for doc in &file.entities {
        if let Some(p) = doc.parent
            && !src.link_child(EntityId(p), EntityId(doc.id))
        {
            return Err(format!("entity {}: parent {p} is not in the file", doc.id));
        }
        for (label, targets) in &doc.named {
            for &t in targets {
                if !src.link_named(EntityId(doc.id), label, EntityId(t)) {
                    return Err(format!(
                        "entity {}: edge target {t} is not in the file",
                        doc.id
                    ));
                }
            }
        }
    }
    Ok(src)
}

fn add_index(src: &mut MemorySource, spec: &str) -> Result<(), String> {
    let parts: Vec<&str> = spec.split(':').collect();
    let [ty, field, kind] = parts.as_slice() else {
        return Err(format!(
            "index `{spec}` must be TYPE:FIELD:hash or TYPE:FIELD:btree"
        ));
    };
    let ty: u32 = ty
        .parse()
        .map_err(|_| format!("index `{spec}`: bad type"))?;
    let kind = match *kind {
        "hash" => IndexKind::Hash,
        "btree" => IndexKind::BTree,
        other => return Err(format!("index `{spec}`: unknown kind `{other}`")),
    };
    let name = format!("{ty}_{field}");
    let field: FieldPath = FieldPath::parse(field);
    src.create_index(&name, EntityTypeId(ty), field, kind)
        .map_err(|e| e.to_string())
}

fn run(o: Options) -> Result<(), String> {
    let mut src = load(o.entities.as_deref().unwrap_or_default())?;
    for spec in &o.indexes {
        add_index(&mut src, spec)?;
    }

    let query: Query = match (&o.dsl, &o.json) {
        (Some(text), _) => tpt_query::dsl::parse(text).map_err(|e| e.to_string())?,
        (_, Some(path)) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            query_from_json(&text).map_err(|e| e.to_string())?
        }
        _ => unreachable!("parse_args requires one query source"),
    };

    if o.explain {
        println!("{}", src.explain(&query).map_err(|e| e.to_string())?);
    }
    if o.analyze {
        let start = std::time::Instant::now();
        let clock = || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let a = src
            .analyze_timed(&query, &clock)
            .map_err(|e| e.to_string())?;
        println!(
            "read {}, passed filters {}, after traversal {}, returned {}",
            a.read, a.passed_filters, a.after_traversal, a.returned
        );
        if let (Some(plan), Some(run)) = (a.plan_nanos, a.run_nanos) {
            println!("plan {plan} ns, run {run} ns");
        }
        return Ok(());
    }
    let out = if query.aggregate.is_some() {
        aggregate_json(&src, &query).map_err(|e| e.to_string())?
    } else {
        execute_json(&src, &query).map_err(|e| e.to_string())?
    };
    println!("{out}");
    Ok(())
}

fn main() -> ExitCode {
    let result = parse_args().and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}
