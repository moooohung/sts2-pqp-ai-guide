//! Generate Rust enums and static tables from the r7 exports in data/
//! (tools/r7_export_tables.py). Output: $OUT_DIR/gen_tables.rs, included by src/tables.rs.
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;

#[path = "../native_obs/build_ids.rs"]
mod native_ids;
mod build_card_preview;

fn ident(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in name.chars() {
        if ch == '_' || ch == ' ' || ch == '-' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'N');
    }
    if matches!(out.as_str(), "Self" | "Super" | "Crate") {
        out.push('_');
    }
    if out.is_empty() { "Empty".into() } else { out }
}

fn emit_enum(code: &mut String, name: &str, members: &[(String, i64)]) {
    writeln!(code, "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\n#[repr(u16)]\npub enum {name} {{").unwrap();
    for (i, (m, _)) in members.iter().enumerate() {
        writeln!(code, "    {} = {i},", ident(m)).unwrap();
    }
    writeln!(code, "}}\nimpl {name} {{\n    pub const COUNT: usize = {};", members.len()).unwrap();
    writeln!(code, "    pub const ALL: [{name}; {}] = [{}];", members.len(),
        members.iter().map(|(m, _)| format!("{name}::{}", ident(m))).collect::<Vec<_>>().join(", ")).unwrap();
    writeln!(code, "    pub const fn name(self) -> &'static str {{ match self {{ {} }} }}",
        members.iter().map(|(m, _)| format!("{name}::{} => {m:?}", ident(m))).collect::<Vec<_>>().join(", ")).unwrap();
    writeln!(code, "    /// r7 enum value (Python `member.value`).\n    pub const fn value(self) -> i64 {{ match self {{ {} }} }}",
        members.iter().map(|(m, v)| format!("{name}::{} => {v}", ident(m))).collect::<Vec<_>>().join(", ")).unwrap();
    writeln!(code, "    pub fn from_name(s: &str) -> Option<{name}> {{ match s {{ {} _ => None }} }}",
        members.iter().map(|(m, _)| format!("{m:?} => Some({name}::{}),", ident(m))).collect::<Vec<_>>().join(" ")).unwrap();
    writeln!(code, "    pub const fn index(self) -> usize {{ self as usize }}\n}}\n").unwrap();
}

/// `CardHook` enum and `CARD_HOOKS[card index]` bitset from data/card_hooks.json (r7
/// cards/registry.py `_CARD_*` dicts), so unwired card-level hooks fail closed.
fn emit_card_hooks(code: &mut String, enums: &Value) {
    println!("cargo:rerun-if-changed=data/card_hooks.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string("data/card_hooks.json").unwrap()).unwrap();
    let hooks = data["hooks"].as_object().unwrap();
    let names: Vec<String> = hooks.keys().cloned().collect();
    assert!(names.len() <= 32);
    let members: Vec<(String, i64)> = names.iter().enumerate().map(|(i, n)| (n.to_uppercase(), i as i64)).collect();
    emit_enum(code, "CardHook", &members);
    let cards: Vec<String> = enums["CardId"]["members"].as_array().unwrap().iter()
        .map(|m| m[0].as_str().unwrap().to_string()).collect();
    let mut bits = vec![0u32; cards.len()];
    for (i, n) in names.iter().enumerate() {
        for id in hooks[n].as_array().unwrap() {
            let idx = cards.iter().position(|c| c == id.as_str().unwrap()).expect("card hook id");
            bits[idx] |= 1 << i;
        }
    }
    writeln!(code, "/// Bit `CardHook::x as u32` set when r7 registers that card-level hook for the card.").unwrap();
    writeln!(code, "pub static CARD_HOOKS: [u32; {}] = {:?};", bits.len(), bits).unwrap();
    for (key, name) in [("_SELF_MUTATING_DAMAGE_CARD_IDS", "SELF_MUTATING_DAMAGE"), ("_SELF_MUTATING_BLOCK_CARD_IDS", "SELF_MUTATING_BLOCK")] {
        let ids: Vec<String> = data["extra"][key].as_array().unwrap().iter()
            .map(|v| format!("CardId::{}", ident(v.as_str().unwrap()))).collect();
        writeln!(code, "pub static {name}: [CardId; {}] = [{}];", ids.len(), ids.join(", ")).unwrap();
    }
}

/// Combat card-generation pools in r7 order (data/card_pools.json, factory.eligible_character_cards).
fn emit_card_pools(code: &mut String) {
    println!("cargo:rerun-if-changed=data/card_pools.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string("data/card_pools.json").unwrap()).unwrap();
    writeln!(code, "/// (pool, multiplayer variant, card type, rarity) -> eligible combat-generated cards, r7 order. `None` = any.").unwrap();
    writeln!(code, "pub static CARD_POOLS: &[(crate::character::CardPool, bool, Option<CardType>, Option<CardRarity>, &[CardId])] = &[").unwrap();
    for (key, ids) in data.as_object().unwrap() {
        let parts: Vec<&str> = key.split('|').collect();
        let opt = |enum_name: &str, v: &str| if v == "*" { "None".to_string() } else { format!("Some({enum_name}::{})", ident(v)) };
        let ids: Vec<String> = ids.as_array().unwrap().iter().map(|v| format!("CardId::{}", ident(v.as_str().unwrap()))).collect();
        let (pool, multiplayer) = match parts[0].strip_suffix("_MP") { Some(pool) => (pool, true), None => (parts[0], false) };
        writeln!(code, "    (crate::character::CardPool::{}, {multiplayer}, {}, {}, &[{}]),", ident(pool), opt("CardType", parts[1]), opt("CardRarity", parts[2]), ids.join(", ")).unwrap();
    }
    writeln!(code, "];").unwrap();
}

fn opt_i(v: &Value) -> String {
    match v.as_i64() { Some(x) => format!("Some({x})"), None => "None".into() }
}

/// Power-class metadata indexed by PowerId, plus a `Hook` enum and a per-power bitset of the
/// hooks that class overrides (dispatch loops skip powers whose bit is clear).
fn emit_powers(code: &mut String, enums: &Value) {
    println!("cargo:rerun-if-changed=data/powers.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string("data/powers.json").unwrap()).unwrap();
    let hooks: Vec<(String, i64)> = data["hook_names"].as_array().unwrap().iter().enumerate()
        .map(|(i, h)| (h.as_str().unwrap().to_string(), i as i64)).collect();
    assert!(hooks.len() <= 128, "hook bitset is u128");
    emit_enum(code, "Hook", &hooks);
    let index: std::collections::HashMap<&str, usize> = hooks.iter().enumerate().map(|(i, (h, _))| (h.as_str(), i)).collect();
    let ids = enums["PowerId"]["members"].as_array().unwrap();
    writeln!(code, "pub static POWER_META: [PowerMeta; {}] = [", ids.len()).unwrap();
    for m in ids {
        let name = m[0].as_str().unwrap();
        match &data["powers"][name] {
            Value::Null => writeln!(code, "    PowerMeta::MISSING,").unwrap(),
            p => {
                let mut bits: u128 = 0;
                for h in p["hooks"].as_array().unwrap() {
                    bits |= 1u128 << index[h.as_str().unwrap()];
                }
                // Optional r7 death methods (getattr, not PowerInstance hooks): before_death=1,
                // after_death=2, on_ally_death=4.
                let extra = p["extra_methods"].as_array().map(|v| v.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>()).unwrap_or_default();
                let death = ["before_death", "after_death", "on_ally_death"].iter().enumerate()
                    .fold(0u8, |m, (i, n)| if extra.contains(n) { m | (1 << i) } else { m });
                writeln!(code, "    PowerMeta {{ exists: true, power_type: PowerType::{}, stack_type: PowerStackType::{}, instanced: {}, allow_negative: {}, temporary: {}, visible: {}, hooks: {bits:#x}, death: {death} }},",
                    ident(p["power_type"].as_str().unwrap()), ident(p["stack_type"].as_str().unwrap()),
                    p["instanced"], p["allow_negative"], p["is_temporary"], p["is_visible"]).unwrap();
            }
        }
    }
    writeln!(code, "];").unwrap();
}

/// Monster state machines with state ids resolved to indices. A8 separates
/// ToughEnemies from DeadlyEnemies; A0/A10 retain their existing band indices.
fn emit_monsters(code: &mut String) {
    println!("cargo:rerun-if-changed=data/monsters.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string("data/monsters.json").unwrap()).unwrap();
    let mut factories: Vec<(&String, &Value)> = data["monsters"].as_object().unwrap().iter()
        .filter(|(_, v)| v.get("a0").is_some()).collect();
    factories.sort_by(|a, b| a.0.cmp(b.0));
    let kinds: Vec<(String, i64)> = factories.iter().enumerate()
        .map(|(i, (f, _))| (f.trim_start_matches("create_").to_uppercase(), i as i64)).collect();
    emit_enum(code, "MonsterKind", &kinds);
    writeln!(code, "pub static MONSTER_DEFS: [[MonsterDef; 3]; {}] = [", factories.len()).unwrap();
    for (_, entry) in &factories {
        let mut sides = Vec::new();
        for asc in ["a0", "a10", "a8"] {
            let a = &entry[asc];
            let states = a["states"].as_object().unwrap();
            let names: Vec<&String> = states.keys().collect();
            let idx = |s: &str| names.iter().position(|n| n.as_str() == s).map(|i| i.to_string()).unwrap_or_else(|| panic!("state {s}"));
            let mut defs = Vec::new();
            for (sid, s) in states {
                let kind = match s["kind"].as_str().unwrap() {
                    "move" => {
                        let intents: Vec<String> = s["intents"].as_array().unwrap().iter().map(|i| format!(
                            "IntentDef {{ kind: IntentType::{}, damage: {}, hits: {}, cards: {} }}",
                            ident(i[0].as_str().unwrap()), i[1].as_i64().unwrap_or(0), i[2], i[3])).collect();
                        format!("StateKind::Move {{ follow_up: {}, must_perform_once: {}, in_logs: {}, intents: &[{}] }}",
                            s["follow_up"].as_str().map(|f| format!("Some({})", idx(f))).unwrap_or("None".into()),
                            s["must_perform_once"], s["in_logs"], intents.join(", "))
                    }
                    "random" => {
                        let branches: Vec<String> = s["branches"].as_array().unwrap().iter().map(|b| format!(
                            "BranchDef {{ target: {}, repeat: MoveRepeatType::{}, max_times: {}, weight: {}, cooldown: {} }}",
                            idx(b[0].as_str().unwrap()), ident(b[1].as_str().unwrap()), b[2],
                            b[3].as_f64().map(|w| format!("Some({w:?})")).unwrap_or("None".into()), b[4])).collect();
                        format!("StateKind::Random {{ branches: &[{}] }}", branches.join(", "))
                    }
                    "conditional" => {
                        let targets: Vec<String> = s["targets"].as_array().unwrap().iter().map(|t| idx(t.as_str().unwrap())).collect();
                        format!("StateKind::Conditional {{ targets: &[{}] }}", targets.join(", "))
                    }
                    other => panic!("state kind {other}"),
                };
                defs.push(format!("StateDef {{ id: {sid:?}, kind: {kind} }}"));
            }
            let powers: Vec<String> = a["powers"].as_array().unwrap().iter()
                .map(|p| format!("(PowerId::{}, {})", ident(p[0].as_str().unwrap()), p[1])).collect();
            sides.push(format!("MonsterDef {{ monster_id: {:?}, hp: ({}, {}), initial: {}, states: &[{}], powers: &[{}] }}",
                a["monster_id"].as_str().unwrap_or(""), a["hp"][0], a["hp"][1], idx(a["initial"].as_str().unwrap()),
                defs.join(", "), powers.join(", ")));
        }
        writeln!(code, "    [{}],", sides.join(", ")).unwrap();
    }
    writeln!(code, "];").unwrap();
}

/// `POWER_MP` (bit 1 `ShouldScaleInMultiplayer`, 2 own `GetScaledAmountForMultiplayer`,
/// 4 `OwnerIsSecondaryEnemy`) and `CARD_MP` (0 any, 1 multiplayer only, 2 singleplayer only)
/// from `data/csharp_meta.json` (tools/csharp_meta, read from the game C#). A C# id with no
/// simulator member fails the build, so a game update cannot drop metadata silently.
fn emit_multiplayer(code: &mut String, enums: &Value) {
    println!("cargo:rerun-if-changed=data/csharp_meta.json");
    let meta: Value = serde_json::from_str(&std::fs::read_to_string("data/csharp_meta.json").unwrap()).unwrap();
    let members = |e: &str| -> Vec<String> {
        enums[e]["members"].as_array().unwrap().iter().map(|m| m[0].as_str().unwrap().to_string()).collect()
    };
    // r7 names a card whose class name clashes with a power `<NAME>_CARD` (TANK_CARD, ...).
    let resolve = |ids: &[String], id: &str, alt: &str| -> usize {
        ids.iter().position(|m| m == id)
            .or_else(|| ids.iter().position(|m| *m == format!("{id}{alt}")))
            .unwrap_or_else(|| panic!("data/csharp_meta.json id {id} has no simulator member"))
    };
    let power_ids = members("PowerId");
    let mut power_bits = vec![0u8; power_ids.len()];
    for (id, p) in meta["powers"].as_object().unwrap() {
        let bits = u8::from(p["scale_mp"].as_bool().unwrap())
            | (u8::from(p["custom_scaling"].as_bool().unwrap()) << 1)
            | (u8::from(p["secondary_enemy"].as_bool().unwrap()) << 2);
        power_bits[resolve(&power_ids, id, "_POWER")] = bits;
    }
    let card_ids = members("CardId");
    let mut card_bits = vec![0u8; card_ids.len()];
    for (id, c) in meta["cards"].as_object().unwrap() {
        card_bits[resolve(&card_ids, id, "_CARD")] = match c["multiplayer"].as_str().unwrap() {
            "MultiplayerOnly" => 1,
            "SingleplayerOnly" => 2,
            other => panic!("unknown CardMultiplayerConstraint {other}"),
        };
    }
    writeln!(code, "pub static POWER_MP: [u8; {}] = {:?};", power_bits.len(), power_bits).unwrap();
    writeln!(code, "pub static CARD_MP: [u8; {}] = {:?};", card_bits.len(), card_bits).unwrap();
}

pub fn main() {
    println!("cargo:rerun-if-changed=../native_obs/build_ids.rs");
    native_ids::generate(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()));
    println!("cargo:rerun-if-changed=data/enums.json");
    println!("cargo:rerun-if-changed=data/cards.json");
    println!("cargo:rerun-if-changed=data/potions_relics.json");
    let enums: Value = serde_json::from_str(&std::fs::read_to_string("data/enums.json").unwrap()).unwrap();
    let cards: Value = serde_json::from_str(&std::fs::read_to_string("data/cards.json").unwrap()).unwrap();
    let extra: Value = serde_json::from_str(&std::fs::read_to_string("data/potions_relics.json").unwrap()).unwrap();
    let mut code = String::from("// @generated by crates/sim/build.rs from data/*.json (r7 exports). Do not edit.\n\n");

    for (name, spec) in enums.as_object().unwrap() {
        if name.starts_with('_') {
            continue;
        }
        let members: Vec<(String, i64)> = spec["members"].as_array().unwrap().iter().enumerate()
            .map(|(i, m)| (m[0].as_str().unwrap().to_string(), m[1].as_i64().unwrap_or(i as i64))).collect();
        emit_enum(&mut code, name, &members);
    }

    // Effect-var keys and keywords seen in card factories.
    let card_map = cards["cards"].as_object().unwrap();
    let mut var_keys = BTreeSet::new();
    let mut keywords = BTreeSet::new();
    for entry in card_map.values() {
        for side in entry.as_object().unwrap().values() {
            for k in side["vars"].as_object().unwrap().keys() {
                var_keys.insert(k.clone());
            }
            for k in side["keywords"].as_array().unwrap() {
                keywords.insert(k.as_str().unwrap().to_string());
            }
        }
    }
    let var_members: Vec<(String, i64)> = var_keys.iter().enumerate().map(|(i, k)| (k.clone(), i as i64)).collect();
    emit_enum(&mut code, "VarKey", &var_members);
    let kw: Vec<String> = keywords.into_iter().collect();
    writeln!(code, "/// Keyword bit positions (r7 keyword strings).\npub const KEYWORD_NAMES: [&str; {}] = {:?};", kw.len(), kw).unwrap();
    for (i, k) in kw.iter().enumerate() {
        writeln!(code, "pub const KW_{}: u16 = 1 << {i};", k.to_uppercase()).unwrap();
    }

    // Card table: index = CardId ordinal, [base, upgraded].
    let card_ids: Vec<String> = enums["CardId"]["members"].as_array().unwrap().iter().map(|m| m[0].as_str().unwrap().to_string()).collect();
    build_card_preview::emit(&std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()), &cards, &card_ids);
    writeln!(code, "\npub static CARD_INFO: [[Option<CardInfo>; 2]; {}] = [", card_ids.len()).unwrap();
    for id in &card_ids {
        let mut sides = Vec::new();
        for side in ["base", "up"] {
            match card_map.get(id).and_then(|e| e.get(side)) {
                None => sides.push("None".to_string()),
                Some(s) => {
                    let kwbits: Vec<String> = s["keywords"].as_array().unwrap().iter()
                        .map(|k| format!("KW_{}", k.as_str().unwrap().to_uppercase())).collect();
                    let tags: Vec<String> = s["tags"].as_array().unwrap().iter()
                        .map(|t| format!("CardTag::{}", ident(t.as_str().unwrap()))).collect();
                    let vars: Vec<String> = s["vars"].as_object().unwrap().iter()
                        .map(|(k, v)| format!("(VarKey::{}, {})", ident(k), v.as_i64().unwrap_or(0))).collect();
                    // Card::vars is ArrayVec<_, MAX_CARD_VARS = 12> (card.rs).
                    assert!(vars.len() <= 12, "card vars exceed MAX_CARD_VARS");
                    assert!(s["enchantments"].as_object().unwrap().len() <= 2, "card enchantments exceed 2");
                    let ench: Vec<String> = s["enchantments"].as_object().unwrap().iter()
                        .map(|(k, v)| format!("(EnchantmentId::{k}, {})", v.as_i64().unwrap_or(0))).collect();
                    sides.push(format!(
                        "Some(CardInfo {{ cost: {}, card_type: CardType::{}, target: TargetType::{}, rarity: CardRarity::{}, damage: {}, block: {}, upgraded: {}, keywords: {}, tags: &[{}], gen_combat: {}, gen_modifiers: {}, enchantments: &[{}], vars: &[{}], x_cost: {}, star_cost: {}, star_x: {}, turn_end_in_hand: {}, replay: {}, single_turn_retain: {}, gains_block: {}, custom_playability: {}, custom_should_play: {}, visual_colorless: {}, entry: {:?}, upgradable: {} }})",
                        s["cost"], ident(s["type"].as_str().unwrap()), ident(s["target"].as_str().unwrap()), ident(s["rarity"].as_str().unwrap()),
                        opt_i(&s["damage"]), opt_i(&s["block"]), s["upgraded"],
                        if kwbits.is_empty() { "0".into() } else { kwbits.join(" | ") }, tags.join(", "),
                        s["gen_combat"], s["gen_modifiers"], ench.join(", "), vars.join(", "),
                        s["x_cost"], s["star_cost"], s["star_x"], s["turn_end_in_hand"], s["replay"], s["single_turn_retain"],
                        s["gains_block"], s["custom_playability"], s["custom_should_play"],
                        s["visual_pool"].as_str() == Some("COLORLESS"),
                        s["entry"].as_str().unwrap(), s["upgradable"]));
                }
            }
        }
        writeln!(code, "    [{}],", sides.join(", ")).unwrap();
    }
    writeln!(code, "];").unwrap();

    // Potions (string ids in r7) and relic metadata.
    let potions = extra["potions"].as_array().unwrap();
    let potion_members: Vec<(String, i64)> = potions.iter().enumerate().map(|(i, p)| (p["id"].as_str().unwrap().to_string(), i as i64)).collect();
    emit_enum(&mut code, "PotionId", &potion_members);
    writeln!(code, "pub static POTION_INFO: [PotionInfo; {}] = [", potions.len()).unwrap();
    for p in potions {
        writeln!(code, "    PotionInfo {{ id: {:?}, rarity: PotionRarity::{}, usage: PotionUsageType::{}, target: PotionTargetType::{}, character_specific: {}, character_pool: {}, gen_combat: {} }},",
            p["id"].as_str().unwrap(), ident(p["rarity"].as_str().unwrap()), ident(p["usage"].as_str().unwrap()), ident(p["target"].as_str().unwrap()),
            p["character_specific"], p["character_pool"].as_str().map(|s| format!("Some({s:?})")).unwrap_or("None".into()), p["gen_combat"]).unwrap();
    }
    writeln!(code, "];").unwrap();
    let relic_ids: Vec<String> = enums["RelicId"]["members"].as_array().unwrap().iter().map(|m| m[0].as_str().unwrap().to_string()).collect();
    let relics = extra["relics"].as_object().unwrap();
    // Relic hooks that only act outside a combat (or before the combat snapshot is taken:
    // after_room_entered fires before _enter_combat, run_manager.py:1142).
    const RUN_ONLY_RELIC_HOOKS: &[&str] = &[
        "after_obtained", "is_allowed", "is_allowed_at_neow", "after_room_entered", "is_used_up", "is_tradable",
        "modify_card_reward_options", "modify_card_reward_options_late", "modify_card_reward_creation_options",
        "modify_rewards", "modify_rewards_late", "after_modifying_rewards", "should_force_potion_reward",
        "should_procure_potion", "should_generate_treasure", "should_gain_gold",
        "modify_rest_site_options", "modify_rest_site_heal_rewards", "modify_rest_site_heal_amount",
        "after_rest_site_heal", "should_disable_remaining_rest_site_options",
        "modify_card_being_added_to_deck", "on_card_added_to_deck",
        "modify_merchant_card_creation_results", "modify_merchant_price", "should_refill_merchant_entry",
        "on_item_purchased", "modify_unknown_map_point_room_types", "modify_generated_map", "modify_generated_map_late",
    ];
    // Bit i of `hook_mask` = the relic overrides `Hook` number i (same order as emit_powers).
    let power_data: Value = serde_json::from_str(&std::fs::read_to_string("data/powers.json").unwrap()).unwrap();
    let hook_index: std::collections::HashMap<&str, u32> = power_data["hook_names"].as_array().unwrap().iter()
        .enumerate().map(|(i, h)| (h.as_str().unwrap(), i as u32)).collect();
    writeln!(code, "pub static RELIC_INFO: [RelicInfo; {}] = [", relic_ids.len()).unwrap();
    for id in &relic_ids {
        let r = &relics[id];
        let combat_hooks: Vec<&str> = r["hooks"].as_array().map(|h| h.iter().filter_map(|x| x.as_str())
            .filter(|h| !RUN_ONLY_RELIC_HOOKS.contains(h)).collect()).unwrap_or_default();
        let mask = combat_hooks.iter().filter_map(|h| hook_index.get(h)).fold(0u128, |m, &i| m | (1u128 << i));
        writeln!(code, "    RelicInfo {{ rarity: RelicRarity::{}, pool: RelicPool::{}, stackable: {}, upon_pickup: {}, spawns_pets: {}, shops: {}, combat_hooks: &{:?}, hook_mask: {mask:#x} }},",
            ident(r["rarity"].as_str().unwrap()), ident(r["pool"].as_str().unwrap()), r["stackable"], r["upon_pickup"], r["spawns_pets"], r["shops"],
            combat_hooks).unwrap();
    }
    writeln!(code, "];").unwrap();

    emit_card_hooks(&mut code, &enums);
    emit_card_pools(&mut code);
    println!("cargo:rerun-if-changed=data/csharp_transform_candidates.json");
    let transforms: Value = serde_json::from_str(&std::fs::read_to_string("data/csharp_transform_candidates.json").unwrap()).unwrap();
    writeln!(code, "pub static CSHARP_TRANSFORMS: &[(bool, &str, &[&str])] = &[").unwrap();
    for mode in ["solo", "multiplayer"] {
        for (source, candidates) in transforms["modes"][mode].as_object().unwrap() {
            let names: Vec<_> = candidates.as_array().unwrap().iter().map(|v| format!("{:?}", v.as_str().unwrap())).collect();
            writeln!(code, "({}, {:?}, &[{}]),", mode == "multiplayer", source, names.join(",")).unwrap();
        }
    }
    writeln!(code, "];").unwrap();
    emit_powers(&mut code, &enums);
    emit_multiplayer(&mut code, &enums);
    emit_monsters(&mut code);

    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("gen_tables.rs");
    std::fs::write(out, code).unwrap();
}
