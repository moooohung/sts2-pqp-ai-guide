//! Generated tables against the r7 exports they came from.
use serde_json::Value;
use sts2_sim::tables::*;

#[test]
fn enum_counts_and_values_match_export() {
    let enums: Value = serde_json::from_str(include_str!("../data/enums.json")).unwrap();
    assert_eq!(CardId::COUNT, enums["CardId"]["members"].as_array().unwrap().len());
    assert_eq!(PowerId::COUNT, enums["PowerId"]["members"].as_array().unwrap().len());
    assert_eq!(RelicId::COUNT, enums["RelicId"]["members"].as_array().unwrap().len());
    for m in enums["CardId"]["members"].as_array().unwrap() {
        let id = CardId::from_name(m[0].as_str().unwrap()).unwrap();
        assert_eq!(id.value(), m[1].as_i64().unwrap());
        assert_eq!(id.name(), m[0].as_str().unwrap());
    }
}

#[test]
fn every_exported_card_matches() {
    let cards: Value = serde_json::from_str(include_str!("../data/cards.json")).unwrap();
    for (name, entry) in cards["cards"].as_object().unwrap() {
        let id = CardId::from_name(name).unwrap();
        for (side, upgraded) in [("base", false), ("up", true)] {
            let s = &entry[side];
            let info = card_info(id, upgraded).unwrap_or_else(|| panic!("{name} {side}"));
            assert_eq!(info.cost as i64, s["cost"].as_i64().unwrap(), "{name} {side} cost");
            assert_eq!(info.damage.map(i64::from), s["damage"].as_i64(), "{name} {side} damage");
            assert_eq!(info.block.map(i64::from), s["block"].as_i64(), "{name} {side} block");
            assert_eq!(info.card_type.name(), s["type"].as_str().unwrap(), "{name} {side} type");
            let vars: Vec<(String, i64)> = s["vars"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_i64().unwrap_or(0))).collect();
            let got: Vec<(String, i64)> = info.vars.iter().map(|(k, v)| (k.name().to_string(), *v as i64)).collect();
            assert_eq!(got, vars, "{name} {side} vars (order matters)");
        }
    }
    assert!(card_info(CardId::Generic, false).is_none());
    let strike = card_info(CardId::StrikeIronclad, false).unwrap();
    assert_eq!((strike.cost, strike.damage), (1, Some(6)));
}
