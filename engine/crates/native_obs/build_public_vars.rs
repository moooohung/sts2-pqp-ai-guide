//! Literal CanonicalVars from the original capture; expressions stay unresolved.
use std::{collections::{BTreeMap, BTreeSet}, fmt::Write, path::Path};
use serde_json::Value;

pub struct Catalog {
    pub names: Vec<String>,
    pub models: Vec<(String, usize, Vec<(String, String)>)>,
}

pub fn load(sim: &Path, native: &BTreeMap<String, Vec<String>>, original: &Value) -> Catalog {
    let mut names: BTreeSet<String> = [
        "CalculatedDamage", "CalculatedBlock", "CalculatedHits", "CalculatedForge",
        "CalculatedCards", "CalculatedShivs", "CalculatedDoom", "CalculatedFocus",
        "CalculatedChannels", "EnergyX", "StarX", "HpCost", "MaxHpCost", "GoldCost",
        "Heal", "MaxHpGain", "GoldGain", "CardsGain", "CardsRemove", "CardsUpgrade",
        "CardsDuplicate", "RelicsGain", "CursesGain", "StrengthGain", "Combats",
        "LiftsLeft", "PotionsGain", "PotionsCost", "RelicsCost", "CardChoices", "RelicChoices",
        "CardsTransform", "CardsDowngrade", "CardsEnchant", "EnchantmentAmount", "CardEnergyCost",
        "Continue", "Combat", "VictoryTurnLimit", "DivinationsGain", "DivinationsCost", "Locked",
        "CardType", "WinRun", "AbandonRun", "CardsGenerate", "GeneratedCardEnergyCost", "ImprovementPower",
    ].into_iter().map(String::from).collect();
    let mut models = Vec::new();
    for (kind, namespace) in [("card", "CardId"), ("potion", "PotionId"),
        ("relic", "RelicId"), ("event", "EventId")] {
        let path = sim.join(format!("data/v2/{kind}.json"));
        println!("cargo:rerun-if-changed={}", path.display());
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for model in data["models"].as_array().unwrap() {
            let class = model["class"].as_str().unwrap().rsplit('.').next().unwrap();
            let members = model["members"].as_array().unwrap();
            let canonical = members.iter().filter_map(|m| m["csharp"].as_str())
                .find(|m| m.contains("CanonicalVars")).unwrap_or("");
            let mut values = Vec::new();
            for ctor in canonical.split("new ").skip(1) {
                let Some((ty, rest)) = ctor.split_once('(') else { continue; };
                if ty.contains("ReadOnly") || ty == "StringVar" { continue; }
                let Some((args, _)) = rest.split_once(')') else { continue; };
                let args: Vec<_> = args.split(',').map(str::trim).collect();
                let (name, value) = if let Some(name) = args.first().filter(|a| a.starts_with('"')) {
                    (name.trim_matches('"').to_string(), args.get(1).copied())
                } else if let Some(power) = ty.strip_prefix("PowerVar<").and_then(|s| s.strip_suffix('>')) {
                    (power.to_string(), args.first().copied())
                } else if let Some(name) = ty.strip_suffix("Var") {
                    (name.to_string(), args.first().copied())
                } else { continue; };
                names.insert(name.clone());
                let Some(value) = value else { continue; };
                let literal = value.trim_end_matches('m');
                let encoded = if let Ok(n) = literal.parse::<i32>() {
                    format!("PublicAmount::Integer({n})")
                } else if literal.contains('.') && literal.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-')) {
                    let decimal: f64 = literal.parse().unwrap();
                    let float = decimal as f32;
                    assert_eq!(f64::from(float), decimal, "non-exact public fraction {class}:{name}");
                    format!("PublicAmount::Float({float:?})")
                } else { continue; };
                values.push((name, encoded));
            }
            if kind == "card" { continue; }
            let index = if kind == "event" {
                original["events"].as_array().unwrap().iter().position(|e| e["name"] == class)
            } else {
                native[namespace].iter().position(|id| id.eq_ignore_ascii_case(class))
            };
            if let Some(index) = index {
                if !values.is_empty() { models.push((namespace.to_string(), index, values)); }
            }
        }
    }
    Catalog { names: names.into_iter().collect(), models }
}

pub fn emit(out: &Path, catalog: &Catalog) {
    let mut code = String::from("pub(super) fn public_variable(name: &str) -> Option<ContentId> { match name {\n");
    for (index, name) in catalog.names.iter().enumerate() {
        writeln!(code, "{name:?} => ContentId::from_index(Namespace::PublicVariable, {index}),").unwrap();
    }
    code.push_str("_ => None, } }\nfn model_parameters(model: ContentId) -> &'static [(&'static str, PublicAmount)] { match (model.namespace, model.value) {\n");
    for (namespace, index, values) in &catalog.models {
        writeln!(code, "(Namespace::{namespace}, {}) => &[{}],", index + 1,
            values.iter().map(|(name, value)| format!("({name:?}, {value})")).collect::<Vec<_>>().join(",")).unwrap();
    }
    code.push_str("_ => &[], } }\n");
    std::fs::write(out.join("public_vars.rs"), code).unwrap();
}
