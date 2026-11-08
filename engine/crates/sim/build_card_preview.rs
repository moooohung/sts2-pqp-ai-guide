//! Display-variable descriptors from the captured original C# CanonicalVars.
use serde_json::Value;
use std::fmt::Write;

fn normalized(name: &str) -> String {
    name.chars().filter(|&c| c != '_').flat_map(char::to_lowercase).collect()
}

fn variable_key(card: &Value, name: &str, entry: &str) -> String {
    let full = normalized(&name.replace("Calculation", "Calc"));
    let short = full.strip_suffix("power").unwrap_or(&full);
    let vars = card["base"]["vars"].as_object().expect("native variable object");
    let keys: Vec<_> = vars.keys().filter(|key| {
        let key = normalized(key); key == full || key == short
    }).collect();
    let key = keys.iter().find(|key| normalized(key) == full)
        .or_else(|| keys.first()).unwrap_or_else(|| panic!("unknown preview variable: {entry}:{name}"));
    for state in ["base", "up"] {
        let values = card[state]["vars"].as_object().expect("native upgrade variable object");
        let value = values.get(*key).unwrap_or_else(|| panic!("missing {state} preview variable: {entry}:{name}"));
        for alias in &keys {
            assert_eq!(values.get(*alias), Some(value), "conflicting preview aliases: {entry}:{name}");
        }
    }
    super::ident(key)
}

fn preview_source(card: &Value, name: &str, entry: &str) -> String {
    if matches!(name, "Damage" | "Block") {
        for state in ["base", "up"] {
            let field = name.to_ascii_lowercase();
            assert!(card[state][&field].is_number() || card[state]["vars"][&field].is_number(),
                "missing {state} display source: {entry}:{name}");
        }
        return format!("Source::{name}");
    }
    format!("Source::Var(VarKey::{})", variable_key(card, name, entry))
}

pub fn emit(out: &std::path::Path, cards: &Value, ids: &[String]) {
    println!("cargo:rerun-if-changed=data/v2/card.json");
    let source: Value = serde_json::from_str(&std::fs::read_to_string("data/v2/card.json").unwrap()).unwrap();
    let mut code = String::from("// Generated from data/v2/card.json; no modifier rules live here.\n");
    code.push_str("fn definitions(id: CardId) -> &'static [Definition] { match id {\n");
    for id in ids {
        let entry = cards["cards"][id]["base"]["entry"].as_str().unwrap_or(id);
        let Some(model) = source["models"].as_array().unwrap().iter().find(|m| m["entry"] == entry) else { continue; };
        let members = model["members"].as_array().unwrap();
        let canonical = members.iter().filter_map(|m| m["csharp"].as_str())
            .find(|s| s.contains("CanonicalVars")).unwrap_or("");
        let constructors: Vec<_> = canonical.split("new ").skip(1).filter_map(|s| {
            let (kind, rest) = s.split_once('(')?;
            let (args, suffix) = rest.split_once(')')?;
            Some((kind, args, suffix))
        }).collect();
        let mut vars = Vec::new();
        for &(kind, args, suffix) in &constructors {
            let power = kind.strip_prefix("PowerVar<").and_then(|s| s.strip_suffix('>'));
            let (variant, default_name) = match kind {
                "DamageVar" => ("Damage", "Damage"),
                "BlockVar" => ("Block", "Block"),
                "OstyDamageVar" => ("OstyDamage", "OstyDamage"),
                "ExtraDamageVar" => ("ExtraDamage", "ExtraDamage"),
                "CalculatedDamageVar" => (if suffix.starts_with(".FromOsty()") { "CalculatedOstyDamage" } else { "CalculatedDamage" }, "CalculatedDamage"),
                "CalculatedBlockVar" => ("CalculatedBlock", "CalculatedBlock"),
                _ if power.is_some() => ("Power", power.unwrap()),
                _ => continue,
            };
            let args: Vec<_> = args.split(',').map(str::trim).collect();
            let name = if args[0].starts_with('"') { args[0].trim_matches('"') } else { default_name };
            let card = &cards["cards"][id];
            let value_source = preview_source(card, if kind.starts_with("Calculated") { "CalculationBase" } else { name }, entry);
            let props = if variant == "Power" { 0 } else if kind == "ExtraDamageVar" { 8 } else {
                let arg = args.last().unwrap();
                arg.split('|').map(|s| match s.trim() {
                    "ValueProp.Move" => 8, "ValueProp.Unpowered" => 4,
                    "ValueProp.Unblockable" => 2, other => panic!("unlowered preview props: {entry} {other}"),
                }).fold(0, |a, b| a | b)
            };
            let extra_kind = if variant == "CalculatedBlock" { "CalculationExtraVar" } else { "ExtraDamageVar" };
            let extra = if kind.starts_with("Calculated") && constructors.iter().any(|c| c.0 == extra_kind) {
                let name = extra_kind.strip_suffix("Var").unwrap();
                format!("Some(VarKey::{})", variable_key(card, name, entry))
            } else { "None".to_owned() };
            let variant = if variant == "Power" {
                format!("Power(PowerId::{})", power.unwrap().strip_suffix("Power").expect("original PowerVar class"))
            } else { variant.to_owned() };
            vars.push(format!("Definition {{ name:{name:?}, kind:Kind::{variant}, source:{value_source}, extra:{extra}, props:Props({props}) }}"));
        }
        if !vars.is_empty() {
            writeln!(code, "CardId::{} => &[{}],", super::ident(id), vars.join(",")).unwrap();
        }
    }
    code.push_str("_ => &[], } }\n");
    std::fs::write(out.join("gen_card_preview.rs"), code).unwrap();
}
