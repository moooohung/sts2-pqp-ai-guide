//! Trusted NativePolicy behavior verification. No client observation bytes or rewards are trusted.
use serde::Deserialize;
use sts2_policy::native::NativeDecision;
use crate::native::*;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub candidate_slot: u16,
    #[serde(default)]
    pub identity: Option<sts2_native_obs::ActionIdentity>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub schema: String, pub t: u32, pub actor: u8, pub action: Action,
    pub options: u16, pub legal: u16, pub behavior_logp: f32, pub value: f32,
    pub reward: f32, pub done: bool, pub observation_sha256: String,
    pub seat_rewards: Vec<f32>,
    pub next_observation_sha256: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub schema: String, pub seed: String, pub character: String, pub ascension: u8,
    #[serde(default)]
    pub characters: Vec<String>,
    pub party_size: u8, pub game_version: String, pub game_dll_sha256: String,
    pub vanilla_fingerprint: String, pub active_mods: Vec<String>, pub checkpoint_id: String,
    pub checkpoint_sha256: String, pub policy_schema: String, pub action_schema: String,
    pub sim_commit: String, pub decision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope { pub schema: String, pub meta: Meta }
pub struct Verified {
    pub meta: Meta, pub rows: Vec<Row>,
    /// At most 64 authenticated policy inputs; full episode rewards compute returns.
    pub inputs: Vec<NativeDecision>,
}
pub fn replay(text: &str, weights: &[u8], sim_commit: &str) -> Result<Verified, String> {
    if text.len() > MAX_ROW_BYTES || !text.ends_with('\n') { return Err("trajectory size/final LF".into()); }
    let mut lines = text.lines();
    let envelope: Envelope = serde_json::from_str(lines.next().ok_or("missing envelope")?).map_err(|e|e.to_string())?;
    let m = envelope.meta;
    let policy = metadata(weights)?;
    let c=contract(policy_is_joint(weights)?);
    let seed = m.seed.parse::<u64>().map_err(|_|"invalid seed")?;
    if envelope.schema != c.episode || m.schema != c.run || m.seed != seed.to_string()
        || m.checkpoint_id != policy.checkpoint_id || m.checkpoint_sha256 != policy.checkpoint_sha256
        || m.policy_schema != c.policy || m.action_schema != c.action
        || m.game_version != policy.game_version || m.game_dll_sha256 != crate::GAME_DLL_SHA256
        || m.vanilla_fingerprint != crate::VANILLA_FINGERPRINT || !m.active_mods.is_empty()
        || m.decision != "trainable" || !(1..=4).contains(&m.party_size) || m.ascension > 10
        || m.sim_commit != sim_commit || sim_commit.len() != 40
        || !sim_commit.bytes().all(|b|b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err("untrusted native episode identity".into());
    }
    let rows: Vec<Row> = lines.map(|line|serde_json::from_str(line).map_err(|e|e.to_string())).collect::<Result<_,_>>()?;
    if rows.is_empty() || rows.len() > MAX_DECISIONS { return Err("episode decision count".into()); }
    let party=if m.characters.is_empty() && m.party_size==1{vec![m.character.clone()]}else{m.characters.clone()};
    if party.len()!=m.party_size as usize || party.first()!=Some(&m.character){return Err("party identity mismatch".into())}
    let inputs = verify_rows(weights, seed, &party, m.ascension, &rows)?;
    Ok(Verified { meta: m, rows, inputs })
}
