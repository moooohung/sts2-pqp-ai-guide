//! Compares the advisor's live projection of a recorded shop with the native encoder's observation of
//! the same state, then runs the policy on both. The native side is staged from the original game's
//! own facts (`shop-spec.json`), never from the live snapshot.
use super::*;
use sts2_sim::run::{MerchantFixture, MerchantFixtureCard, MerchantFixtureOffer};
use sts2_sim::tables::{CardId, PotionId, RelicId, card_info};

fn card(name: &str) -> Result<CardId, String> {
    CardId::ALL.into_iter().find(|&id| card_info(id, false).is_some_and(|info| info.entry == name))
        .ok_or_else(|| format!("unknown card {name}"))
}
// The live adapter resolves game entries the same way: the exact name, else the name without separators.
fn normalized(name: &str) -> String { name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect() }
fn relic(name: &str) -> Result<RelicId, String> {
    RelicId::from_name(name).or_else(|| RelicId::ALL.into_iter().find(|id| normalized(&format!("{id:?}")) == normalized(name)))
        .ok_or_else(|| format!("unknown relic {name}"))
}
fn potion(name: &str) -> Result<PotionId, String> {
    PotionId::from_name(name).or_else(|| PotionId::ALL.into_iter().find(|id| normalized(&format!("{id:?}")) == normalized(name)))
        .ok_or_else(|| format!("unknown potion {name}"))
}
fn price(value: &Value) -> Result<i32, String> {
    value["price"].as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(|| "missing price".into())
}
// Every slot is kept, bought ones included: the slot position is the buy index and the encoded offer order.
fn shop_cards(entries: &Value) -> Result<Vec<MerchantFixtureCard>, String> {
    entries.as_array().ok_or("shop cards")?.iter().map(|entry| {
        let sold = entry["sold"] == true;
        Ok(MerchantFixtureCard {
            id: if sold { None } else { Some(card(entry["id"].as_str().ok_or("shop card id")?)?) },
            upgraded: entry["upgrades"].as_i64().unwrap_or(0) > 0,
            price: if sold { 0 } else { price(entry)? }, on_sale: entry["on_sale"] == true })
    }).collect()
}
/// A relic or potion slot. The game empties a bought slot (`id` is null); the simulator's keeps what it held,
/// which the harness recorded as `was`.
fn shop_offers<T>(entries: &Value, name: &str, resolve: fn(&str) -> Result<T, String>) -> Result<Vec<MerchantFixtureOffer<T>>, String> {
    entries.as_array().ok_or_else(|| format!("shop {name}s"))?.iter().map(|entry| {
        let sold = entry["sold"] == true;
        let key = if sold { "was" } else { "id" };
        Ok(MerchantFixtureOffer { id: resolve(entry[key].as_str().ok_or_else(|| format!("shop {name} {key}"))?)?,
            price: if sold { 0 } else { price(entry)? }, sold })
    }).collect()
}
fn coordinate(value: &Value) -> Result<(i32, i32), String> {
    let at = |index: usize| value[index].as_i64().and_then(|n| i32::try_from(n).ok()).ok_or("coordinate");
    Ok((at(0)?, at(1)?))
}

pub fn staged_run(spec: &Value) -> Result<RunManager, String> {
    let seed: u64 = spec["numeric_seed"].as_str().ok_or("numeric_seed")?.parse().map_err(|_| "numeric_seed")?;
    let options = RunOptions { ascension: spec["ascension"].as_i64().ok_or("ascension")? as i32, ..RunOptions::default() };
    let mut run = RunManager::from_numeric_seed(seed, options, Character::Ironclad);
    let shop = &spec["shop"];
    let list = |value: &Value| value.as_array().cloned().ok_or_else(|| "list".to_string());
    run.stage_merchant_fixture(&MerchantFixture {
        act_index: spec["act_index"].as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("act_index")?,
        visited: list(&spec["visited"])?.iter().map(coordinate).collect::<Result<_, _>>()?,
        total_floor: spec["total_floor"].as_i64().ok_or("total_floor")? as i32,
        hp: spec["hp"].as_i64().ok_or("hp")? as i32, max_hp: spec["max_hp"].as_i64().ok_or("max_hp")? as i32,
        gold: spec["gold"].as_i64().ok_or("gold")? as i32,
        max_potion_slots: spec["max_potion_slots"].as_i64().ok_or("max_potion_slots")? as i32,
        potions: list(&spec["potions"])?.iter().map(|p| p.as_str().map(potion).transpose()).collect::<Result<_, _>>()?,
        relics: list(&spec["relics"])?.iter().map(|r| relic(r.as_str().ok_or("relic")?)).collect::<Result<_, _>>()?,
        deck: list(&spec["deck"])?.iter().map(|c| Ok((card(c["id"].as_str().ok_or("deck id")?)?,
            c["upgrades"].as_i64().unwrap_or(0) > 0))).collect::<Result<_, String>>()?,
        cards: shop_cards(&shop["cards"])?, colorless: shop_cards(&shop["colorless"])?,
        relic_offers: shop_offers(&shop["relics"], "relic", relic)?,
        potion_offers: shop_offers(&shop["potions"], "potion", potion)?,
        removal_cost: shop["removal_price"].as_i64().ok_or("removal_price")? as i32,
        removal_used: shop["removal_used"] == true,
        boss: encounter(&spec["boss"]).map_err(|e| format!("boss: {e}"))?,
        second_boss: if spec["second_boss"].is_null() { None } else {
            Some(encounter(&spec["second_boss"]).map_err(|e| format!("second_boss: {e}"))?) },
    })?;
    Ok(run)
}
fn encounter(value: &Value) -> Result<sts2_sim::identity::EncounterId, String> {
    let name = value.as_str().ok_or("missing")?;
    sts2_sim::identity::EncounterId::from_name(name).ok_or_else(|| format!("unknown encounter {name}"))
}

/// The exit status of the comparison: unequal policy inputs are a failure, however the report was written.
pub fn verdict(report: &Value) -> Result<(), String> {
    if report["policy_inputs_identical"] == true { return Ok(()); }
    Err(format!("the policy inputs differ: {}", report["policy_input_differences"]))
}

fn distribution(policy: &CpuPolicy, workspace: &mut CpuWorkspace, view: ObservationView<'_>, rows: &Value) -> Result<Value, String> {
    let decision = NativeDecision::from_view(view)?;
    policy.forward(&decision, workspace);
    let candidates = rows["candidates"].as_array().ok_or("candidates")?;
    let options = decision.slots.iter().zip(workspace.logp.iter()).map(|(&slot, logp)| {
        let candidate = &candidates[slot as usize];
        json!({"slot": slot, "action": candidate["action"], "model": candidate["model"]["name"],
            "price": candidate["numbers"]["gold_cost"], "probability": logp.exp()})
    }).collect::<Vec<_>>();
    Ok(Value::Array(options))
}

/// Feature rows as a multiset: handle numbering depends on emission order, the features do not.
fn feature_counts(rows: &Value) -> std::collections::BTreeMap<String, u64> {
    let mut counts = std::collections::BTreeMap::new();
    for row in rows["rows"].as_array().unwrap() {
        let feature = json!({"kind": row["kind"], "model": row["model"], "detail": row["detail"],
            "numbers": row["numbers"], "flags": row["flags"], "flags_present": row["flags_present"]});
        *counts.entry(feature.to_string()).or_default() += 1;
    }
    counts
}
fn only_in(a: &std::collections::BTreeMap<String, u64>, b: &std::collections::BTreeMap<String, u64>) -> Vec<Value> {
    a.iter().filter_map(|(row, count)| {
        let other = b.get(row).copied().unwrap_or(0);
        (*count > other).then(|| json!({"count": count - other, "row": serde_json::from_str::<Value>(row).unwrap()}))
    }).collect()
}

/// Path and values of the first differences between two policy inputs.
fn json_diff(a: &Value, b: &Value, path: &str, out: &mut Vec<Value>) {
    if out.len() >= 40 || a == b { return; }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => for key in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
            json_diff(x.get(key).unwrap_or(&Value::Null), y.get(key).unwrap_or(&Value::Null), &format!("{path}.{key}"), out);
        },
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => for (i, (p, q)) in x.iter().zip(y).enumerate() {
            json_diff(p, q, &format!("{path}[{i}]"), out);
        },
        _ => out.push(json!({"path": path, "native": a, "live": b})),
    }
}

fn view(obs: &NativeObsV1) -> Result<ObservationView<'_>, String> { obs.view().map_err(|e| format!("{e:?}")) }

/// What the policy samples from. Candidate slot numbers are left out: the native list also holds the
/// offers the player cannot afford (masked), so its positions run higher, and the policy never sees them.
fn policy_input(obs: &NativeObsV1, relational: bool) -> Result<Value, String> {
    let mut input: Value = serde_json::from_str(&NativeDecision::from_view(view(obs)?)?.trajectory_json(relational)
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    input.as_object_mut().ok_or("policy input")?.remove("slots");
    Ok(input)
}

pub struct Encoded { pub native: Box<NativeObsV1>, pub live: Box<NativeObsV1> }

pub fn encode(spec: &Value, live_snapshot: &Value, relational: bool) -> Result<Encoded, String> {
    let run = staged_run(spec)?;
    let mut encoder = sts2_policy::native::encoder_for_policy(relational);
    let mut native = Box::new(NativeObsV1::new());
    encoder.encode_solo(&run, DecisionId { episode: 0, generation: 0, actor: 0 }, &mut native).map_err(|e| format!("{e:?}"))?;
    let snapshot: sts2_native_obs::live::Snapshot = serde_json::from_value(live_snapshot.clone()).map_err(|e| e.to_string())?;
    Ok(Encoded { native, live: snapshot.encode()? })
}

/// Differences between the two observations; `policy_inputs_identical` is the verdict.
pub fn compare(encoded: &Encoded, relational: bool) -> Result<Value, String> {
    let (native_rows, live_rows) = (live::parity_rows(&view(&encoded.native)?), live::parity_rows(&view(&encoded.live)?));
    let (native_input, live_input) = (policy_input(&encoded.native, relational)?, policy_input(&encoded.live, relational)?);
    let mut input_differences = Vec::new();
    json_diff(&native_input, &live_input, "", &mut input_differences);
    let (native_features, live_features) = (feature_counts(&native_rows), feature_counts(&live_rows));
    Ok(json!({
        "policy_inputs_identical": native_input == live_input, "policy_input_differences": input_differences,
        "native_rows": view(&encoded.native)?.state_len, "live_rows": view(&encoded.live)?.state_len,
        "native_candidates": view(&encoded.native)?.candidate_len, "live_candidates": view(&encoded.live)?.candidate_len,
        "rows_only_in_native": only_in(&native_features, &live_features),
        "rows_only_in_live": only_in(&live_features, &native_features),
        "native_raw": native_rows, "live_raw": live_rows,
    }))
}

pub fn report(weights: &[u8], spec: &Value, live_snapshot: &Value) -> Result<Value, String> {
    let (policy, _) = load(weights)?;
    let relational = observation_mode(&policy_fields(weights)?)?;
    let mut workspace = CpuWorkspace::default();
    let encoded = encode(spec, live_snapshot, relational)?;
    let mut report = compare(&encoded, relational)?;
    report["relational_policy"] = relational.into();
    for (name, obs) in [("native_policy", &encoded.native), ("live_policy", &encoded.live)] {
        let rows = live::parity_rows(&view(obs)?);
        report[name] = distribution(&policy, &mut workspace, view(obs)?, &rows)?;
    }
    Ok(report)
}