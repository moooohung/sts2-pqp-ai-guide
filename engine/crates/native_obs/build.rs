use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fmt::Write, fs, path::Path};
use syn::Item;

// One generator determines the engine's discriminants and the observation manifest.
// Its only output is generated build data; no second MonsterKind or legacy vocabulary.
#[path = "../sim/build.rs"]
mod engine_tables;
#[path = "src/content_hash.rs"]
mod content_hash;
mod build_public_vars;

fn read_json(path: &Path) -> Value {
    println!("cargo:rerun-if-changed={}", path.display());
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn read_source(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}
fn enums(source: &str) -> BTreeMap<String, Vec<String>> {
    syn::parse_file(source).unwrap().items.into_iter().filter_map(|item| {
        if let Item::Enum(e) = item {
            Some((e.ident.to_string(), e.variants.iter().map(|v| v.ident.to_string()).collect()))
        } else { None }
    }).collect()
}
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() { out.push('_'); }
        out.push(ch.to_ascii_lowercase());
    }
    out
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn emit_enum(code: &mut String, name: &str, members: &[String], absent: bool) {
    assert!(!members.is_empty() && members.len() < u16::MAX as usize);
    let unique: BTreeSet<_> = members.iter().collect();
    assert_eq!(unique.len(), members.len(), "duplicate identifier in {name}");
    for member in members { syn::parse_str::<syn::Ident>(member).expect("valid generated Rust identifier"); }
    writeln!(code, "#[repr(u16)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum {name} {{").unwrap();
    if absent { writeln!(code, "Absent = 0,").unwrap(); }
    for (index, member) in members.iter().enumerate() {
        writeln!(code, "{member} = {},", index + usize::from(absent)).unwrap();
    }
    writeln!(code, "}}\nimpl {name} {{ pub const COUNT: usize = {};", members.len()).unwrap();
    writeln!(code, "pub const ALL: [Self; Self::COUNT] = [{}];",
        members.iter().map(|n| format!("Self::{n}")).collect::<Vec<_>>().join(",")).unwrap();
    if !absent {
        writeln!(code, "pub const fn index(self) -> usize {{ self as usize }}
            pub const fn wire_id(self) -> u16 {{ self as u16 + 1 }}
            pub fn from_index(index: usize) -> Option<Self> {{ Self::ALL.get(index).copied() }}").unwrap();
    }
    writeln!(code, "}}").unwrap();
    if absent { writeln!(code, "impl Default for {name} {{ fn default() -> Self {{ Self::Absent }} }}").unwrap(); }
}
fn main() {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&root);
    let sim = root.join("../sim").canonicalize().unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);
    println!("cargo:rerun-if-changed={}", sim.join("data").display());
    println!("cargo:rerun-if-changed={}", sim.join("build.rs").display());
    println!("cargo:rerun-if-changed=build_ids.rs");
    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&sim).unwrap();
    engine_tables::main();
    std::env::set_current_dir(original_cwd).unwrap();
    let native_tables = fs::read_to_string(out.join("gen_tables.rs")).unwrap();
    let native = enums(&native_tables);
    let schema = read_json(&root.join("schema.json"));
    let original = read_json(&root.join("data/original_ids.json"));
    let event_capture = read_json(&sim.join("data/v2/event.json"));
    assert_eq!(original["capture"], event_capture["capture"], "original DLL capture changed");
    assert_eq!(original["events"].as_array().unwrap().len(),
        event_capture["models"].as_array().unwrap().len(), "event identity census changed");
    let mut registries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in schema["native_tables"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        registries.insert(name.into(), native.get(name).unwrap_or_else(|| panic!("missing native enum {name}")).clone());
    }
    let mut source_enums = BTreeMap::new();
    for (path, selected) in [
        ("src/card.rs", &["CardAffliction", "CostExpiry"][..]),
        ("src/combat.rs", &["PileKind"][..]),
        ("src/creature.rs", &["Side"][..]),
        ("src/choice.rs", &["ChoiceSrc"][..]),
        ("src/run/manager.rs", &["RunPhase", "RunActionKind", "RunDeckChoiceKind", "RewardItemKind"][..]),
        ("src/run/rest.rs", &["RestOptionKind"][..]),
        ("src/run/event_preview.rs", &["PublicEventTiming"][..]),
        ("src/run/events/mod.rs", &["EventEffect", "RunChoiceEffect", "ChoiceResume"][..]),
        ("src/run/mp/driver.rs", &["MpPhase"][..]),
        ("src/run/mp/events.rs", &["EventCardAction"][..]),
        ("src/character.rs", &["Character"][..]),
    ] {
        let source = read_source(&sim.join(path));
        let found = enums(&source);
        for name in selected {
            let members = found.get(*name).unwrap_or_else(|| panic!("missing {path}::{name}")).clone();
            source_enums.insert(name.to_string(), members);
        }
    }
    for name in ["CardAffliction", "CostExpiry", "PileKind", "Side", "ChoiceSrc",
        "RunPhase", "RunActionKind", "RestOptionKind", "ChoiceResume", "MpPhase", "Character", "PublicEventTiming"] {
        registries.insert(name.into(), source_enums[name].clone());
    }
    let generated = enums(&fs::read_to_string(out.join("gen_native_ids.rs")).unwrap());
    for (name, members) in &generated { assert!(registries.insert(name.clone(), members.clone()).is_none()); }
    let public_vars = build_public_vars::load(&sim, &native, &original);
    registries.insert("PublicVariable".into(), public_vars.names.clone());
    build_public_vars::emit(out, &public_vars);
    for (name, key) in [("PublicModalKind", "public_modal_kinds"), ("PublicModalAction", "public_modal_actions")] {
        registries.insert(name.into(), schema[key].as_array().unwrap().iter()
            .map(|v| v.as_str().unwrap().to_owned()).collect());
    }

    let row_names: Vec<_> = schema["rows"].as_array().unwrap().iter()
        .map(|r| r["name"].as_str().unwrap().to_owned()).collect();
    let action_names: Vec<_> = schema["action_kinds"].as_array().unwrap().iter()
        .map(|r| r.as_str().unwrap().to_owned()).collect();
    for action in &source_enums["RunActionKind"] { assert!(action_names.contains(action), "unrepresented action {action}"); }
    registries.insert("RowKind".into(), row_names.clone());
    registries.insert("ActionKind".into(), action_names.clone());
    let flags: Vec<_> = schema["flags"].as_array().unwrap().iter()
        .map(|v| v.as_str().unwrap().to_owned()).collect();
    assert!(flags.len() <= 64);
    let relations: Vec<_> = schema["relations"].as_array().unwrap().iter()
        .map(|v| v.as_str().unwrap().to_owned()).collect();
    registries.insert("Relation".into(), relations.clone());

    let binding = read_source(&root.join("src/binding.rs"));
    let layout = read_source(&root.join("src/layout.rs"));
    let schema_payload = json!({"layout": schema, "binding": binding, "row_types": layout,
        "namespaces": registries.keys().collect::<Vec<_>>()});
    let schema_hash: [u8; 32] = Sha256::digest(serde_json::to_vec(&schema_payload).unwrap()).into();
    let width = |name: &str| -> usize { match name {
        "CardId" => 32, "RelicId" | "PowerId" | "MonsterKind" => 24,
        "PotionId" | "EventId" | "RowKind" | "ActionKind" => 16, _ => 8
    }};
    let mut manifest = json!({"schema_version": schema["version"], "schema_hash": hex(&schema_hash),
        "original_dll_capture": original["capture"],
        "capacity_status": schema["capacity_status"], "schema": schema,
        "registries": registries.iter().map(|(name, members)| json!({
            "name": name, "count": members.len(), "embedding_rows": members.len()+1,
            "embedding_width": width(name), "ids": members.iter().enumerate()
                .map(|(index, name)| json!({"index": index, "wire": index+1, "name": name})).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "original_sources": original["source_sha256"]});
    let content_hash = content_hash::content_hash(&manifest);
    manifest["content_hash"] = json!(hex(&content_hash));
    let mut code = String::from("// @generated; IDs are native ordinal + 1, never a legacy token vocabulary.\n");
    writeln!(code, "pub const SCHEMA_VERSION: u32 = {};", schema["version"]).unwrap();
    writeln!(code, "pub const SCHEMA_HASH: [u8;32] = {schema_hash:?};\npub const CONTENT_HASH: [u8;32] = {content_hash:?};").unwrap();
    let _team = read_json(&root.join("team_schema.json"));
    let team_hash = content_hash::team_schema_hash(schema_hash);
    writeln!(code, "pub const TEAM_SCHEMA_HASH: [u8;32] = {team_hash:?};").unwrap();
    writeln!(code, "pub const ORIGINAL_DLL_CAPTURE: &str = {:?};", original["capture"].as_str().unwrap()).unwrap();
    writeln!(code, "pub const CAPACITIES_VALIDATED: bool = {};",
        schema["capacity_status"] == "validated_measured_envelope").unwrap();
    for (key, name) in [("state_capacity", "STATE_CAPACITY"), ("candidate_capacity", "CANDIDATE_CAPACITY"),
        ("edge_capacity", "EDGE_CAPACITY"), ("selection_capacity", "SELECTION_CAPACITY"),
        ("numeric_lanes", "NUMERIC_LANES"), ("id_lanes", "ID_LANES"), ("max_seats", "MAX_SEATS")] {
        let value = schema[key].as_u64().unwrap();
        assert!(value > 0 && value <= u16::MAX as u64);
        writeln!(code, "pub const {name}: usize = {value};").unwrap();
    }
    assert_eq!(schema["numeric_lanes"], 16);
    let namespaces = registries.keys().cloned().collect::<Vec<_>>();
    emit_enum(&mut code, "Namespace", &namespaces, true);
    emit_enum(&mut code, "RowKind", &row_names, true);
    emit_enum(&mut code, "ActionKind", &action_names, true);
    emit_enum(&mut code, "Relation", &relations, false);
    emit_enum(&mut code, "Flag", &flags, false);
    writeln!(code, "impl Flag {{ pub const fn bit(self) -> u64 {{ 1u64 << self as u16 }} }}").unwrap();
    let valid_flags = (1u64 << flags.len()) - 1;
    writeln!(code, "pub const VALID_FLAGS: u64 = {valid_flags};").unwrap();
    for (name, members) in &generated {
        assert!(!members.is_empty());
        writeln!(code, "pub use sts2_sim::identity::{name};").unwrap();
        writeln!(code, "impl From<{name}> for ContentId {{ fn from(id: {name}) -> Self {{
            Self {{ namespace: Namespace::{name}, value: id.wire_id() }}
        }} }}").unwrap();
    }
    writeln!(code, "pub const REGISTRIES: &[Registry] = &[").unwrap();
    for (name, members) in &registries {
        writeln!(code, "Registry {{ namespace: Namespace::{name}, name: {name:?}, names: &{members:?},
            count: {}, embedding_width: {} }},", members.len(), width(name)).unwrap();
    }
    writeln!(code, "];\npub mod counts {{").unwrap();
    for (name, members) in &registries {
        writeln!(code, "pub const {}: usize = {};", snake(name).to_uppercase(), members.len()).unwrap();
    }
    writeln!(code, "}}").unwrap();
    let field = |f: &Value| -> String {
        let kind = match f["type"].as_str().unwrap() { "i32" => "Integer", "f32" => "Float", "number" => "Number", other => panic!("unknown field type {other}") };
        format!("NumericField {{ name: {:?}, kind: NumericKind::{kind} }}", f["name"].as_str().unwrap())
    };
    writeln!(code, "pub const ROW_FIELDS: &[&[NumericField]] = &[&[],").unwrap();
    for row in schema["rows"].as_array().unwrap() {
        let fields = row["numeric"].as_array().unwrap();
        assert!(fields.len() <= 16);
        writeln!(code, "&[{}],", fields.iter().map(&field).collect::<Vec<_>>().join(",")).unwrap();
    }
    writeln!(code, "];\npub const CANDIDATE_FIELDS: &[NumericField] = &[{}];",
        schema["candidate_numeric"].as_array().unwrap().iter().map(&field).collect::<Vec<_>>().join(",")).unwrap();
    writeln!(code, "pub mod row_fields {{").unwrap();
    for row in schema["rows"].as_array().unwrap() {
        writeln!(code, "pub mod {} {{", snake(row["name"].as_str().unwrap())).unwrap();
        for (lane, f) in row["numeric"].as_array().unwrap().iter().enumerate() {
            writeln!(code, "pub const {}: usize = {lane};", f["name"].as_str().unwrap().to_ascii_uppercase()).unwrap();
        }
        writeln!(code, "}}").unwrap();
    }
    writeln!(code, "}}").unwrap();
    fs::write(out.join("manifest.rs"), code).unwrap();
    fs::write(out.join("content_manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}
