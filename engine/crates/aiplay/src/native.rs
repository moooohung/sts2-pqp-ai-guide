//! Browser self-play of the CURRENT native main policy: NativeObsV1 observations,
//! NativePolicy weights run through the trainer's own `CpuPolicy`, solo `RunManager`.
//! C1 v2 weights, C2 v2 trajectories and dashboard v2 replay frames (with the real map).
//! Replay projection and rewards are shared with native training via native_obs.
use burn::{prelude::*, store::{ModuleSnapshot, SafetensorsStore}};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sts2_native_obs::*;
use sts2_policy::native::{CpuPolicy, CpuWorkspace, NativeDecision, NativePolicy, manifest_metadata,relational_manifest_metadata};
use sts2_sim::run::{RunManager, RunOptions, mp::Character};
use sts2_wire::policy_rng::Rng;
#[path="native_driver.rs"]
mod driver;
use driver::PartyGame as Game;
#[path="native_mix.rs"]
mod mix;
pub use mix::TrainingMix;
#[path="live_advisor.rs"]
pub mod live;
#[path="native_search.rs"]
pub mod search;
#[cfg(not(target_arch = "wasm32"))]
#[path="shop_fixture.rs"]
pub mod shop_fixture;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub const POLICY_SCHEMA: &str = "sts2-policy-web-v2";
pub const ACTION_SCHEMA: &str = "sts2-native-obs-v1";
pub const RUN_SCHEMA: &str = "sts2-aiplay-run-v2";
pub const EPISODE_SCHEMA: &str = "sts2-aiplay-episode-v2";
pub const STEP_SCHEMA: &str = "sts2-aiplay-step-v3";
pub const JOINT_POLICY_SCHEMA: &str = "sts2-policy-web-v3";
pub const JOINT_ACTION_SCHEMA: &str = "sts2-native-team-actions-v2";
pub const JOINT_RUN_SCHEMA: &str = "sts2-aiplay-run-v3";
pub const JOINT_EPISODE_SCHEMA: &str = "sts2-aiplay-episode-v3";
pub const JOINT_STEP_SCHEMA: &str = "sts2-aiplay-step-v4";
pub fn is_episode_schema(schema:&str)->bool{schema==EPISODE_SCHEMA||schema==JOINT_EPISODE_SCHEMA}
pub struct Contract {
    pub policy:&'static str,pub action:&'static str,pub run:&'static str,
    pub episode:&'static str,pub step:&'static str,
}
pub fn contract(joint:bool)->Contract {
    if joint {Contract{policy:JOINT_POLICY_SCHEMA,action:JOINT_ACTION_SCHEMA,run:JOINT_RUN_SCHEMA,
        episode:JOINT_EPISODE_SCHEMA,step:JOINT_STEP_SCHEMA}}
    else {Contract{policy:POLICY_SCHEMA,action:ACTION_SCHEMA,run:RUN_SCHEMA,episode:EPISODE_SCHEMA,step:STEP_SCHEMA}}
}
pub const MAX_DECISIONS: usize = 20_000;
pub const MAX_ROW_BYTES: usize = 16 << 20;
/// SHA-256 of the empty byte string: the terminal next observation.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// Build provenance, set by tools/build_aiplay_wasm.ps1 (40-hex Git commits).
pub const SOURCE_COMMIT: &str = match option_env!("STS2_AIPLAY_SOURCE_COMMIT") { Some(c) => c, None => "unknown" };
pub const NATIVE_MAIN_COMMIT: &str = match option_env!("STS2_AIPLAY_NATIVE_MAIN") { Some(c) => c, None => "unknown" };

pub fn sha256(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect() }
fn hex64(s: &str) -> bool { s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }
pub fn identifier(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub use sts2_native_obs::reward::Reward;

/// C1 v2 provenance. `checkpoint_id` is `<lineage>.<update>`; `step` is the run's decision count.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebPolicyMeta {
    pub lineage: String,
    pub update: u64,
    pub step: u64,
    pub checkpoint_id: String,
    pub checkpoint_sha256: String,
    pub hidden: u32,
    pub reward: Reward,
    pub game_version: String,
    /// The checkpoint's training discount (training_config.gamma); decision-time search
    /// discounts with it and refuses weights exported without it.
    #[serde(default)]
    pub gamma: Option<f32>,
}
impl WebPolicyMeta {
    pub fn validate(&self) -> Result<(), String> {
        if self.gamma.is_some_and(|g| !(g > 0. && g <= 1.)) { return Err("invalid native policy discount".into()); }
        if !identifier(&self.lineage, 96) || self.checkpoint_id != format!("{}.{}", self.lineage, self.update)
            || !hex64(&self.checkpoint_sha256) || self.hidden == 0 || self.hidden > 4096
            || self.game_version.is_empty() || self.game_version.len() > 64 || self.game_version.chars().any(char::is_control)
            || self.reward.validate().is_err() {
            return Err("invalid native policy provenance".into());
        }
        Ok(())
    }
}

fn fixed(joint:bool) -> [(&'static str, &'static str); 5] {
    let c=contract(joint);
    [("schema", c.policy), ("obs_spec", if joint {"NativeObsV1+NativeTeamObsV2"}else{"NativeObsV1"}), ("action_schema", c.action),
        ("game_dll_sha256", crate::GAME_DLL_SHA256), ("vanilla_fingerprint", crate::VANILLA_FINGERPRINT)]
}

/// C1 v2: safetensors of the NativePolicy parameters; every metadata value is a string.
pub fn export(model: &NativePolicy, meta: &WebPolicyMeta) -> Result<Vec<u8>, String> {
    export_with_mix(model,meta,None)
}
fn export_with_mix(model:&NativePolicy,meta:&WebPolicyMeta,mix:Option<&TrainingMix>)->Result<Vec<u8>,String>{
    meta.validate()?;
    if model.hidden as u32 != meta.hidden { return Err("model width does not match provenance".into()); }
    let mut store = SafetensorsStore::from_bytes(None);
    if model.is_v2() {
        store=store.metadata("policy_manifest",relational_manifest_metadata(model.is_joint()).to_string());
    }
    if let Some(mix)=mix{mix.validate()?;store=store.metadata("training_mix",serde_json::to_string(mix).map_err(|e|e.to_string())?);}
    for (key, value) in fixed(model.is_joint()) { store = store.metadata(key, value.to_string()); }
    for (key, value) in [("lineage", meta.lineage.clone()), ("update", meta.update.to_string()),
        ("step", meta.step.to_string()), ("checkpoint_id", meta.checkpoint_id.clone()),
        ("checkpoint_sha256", meta.checkpoint_sha256.clone()), ("hidden", meta.hidden.to_string()),
        ("reward", serde_json::to_string(&meta.reward).map_err(|e| e.to_string())?),
        ("training_objective", sts2_native_obs::reward::TRAINING_OBJECTIVE.to_owned()),
        ("game_version", meta.game_version.clone()),
        ("observation_mode",if model.is_v2() {"v2"}else{"v1_compatibility"}.to_owned()),
        ("obs_manifest", sts2_policy::native::observation_manifest_metadata(model.is_v2(),model.is_joint()).to_string())] {
        store = store.metadata(key, value);
    }
    if let Some(gamma) = meta.gamma { store = store.metadata("gamma", gamma.to_string()); }
    model.save_into(&mut store).map_err(|e| format!("{e:?}"))?;
    store.get_bytes().map_err(|e| format!("{e:?}"))
}

fn policy_fields(bytes:&[u8])->Result<Value,String>{
    let n = u64::from_le_bytes(bytes.get(..8).ok_or("missing safetensors length")?.try_into().unwrap());
    if n > 1 << 20 { return Err("oversized safetensors header".into()); }
    let header: Value = serde_json::from_slice(bytes.get(8..8 + n as usize).ok_or("truncated header")?)
        .map_err(|e| e.to_string())?;
    Ok(header["__metadata__"].clone())
}
pub fn metadata(bytes: &[u8]) -> Result<WebPolicyMeta, String> {
    let m = policy_fields(bytes)?;
    let joint=joint_header(&m)?;
    for (key, value) in fixed(joint) {
        if m[key] != value { return Err(format!("incompatible policy metadata: {key}")); }
    }
    let relational=observation_mode(&m)?;
    if m["obs_manifest"] != sts2_policy::native::observation_manifest_metadata(relational,joint).to_string()
        { return Err("native observation manifest mismatch".into()); }
    let text = |k: &str| m[k].as_str().map(str::to_owned).ok_or_else(|| format!("missing metadata: {k}"));
    let number = |k: &str| -> Result<u64, String> { text(k)?.parse().map_err(|_| format!("invalid {k}")) };
    let meta = WebPolicyMeta {
        lineage: text("lineage")?, update: number("update")?, step: number("step")?,
        checkpoint_id: text("checkpoint_id")?, checkpoint_sha256: text("checkpoint_sha256")?,
        hidden: u32::try_from(number("hidden")?).map_err(|_| "invalid hidden")?,
        reward: serde_json::from_str(&text("reward")?).map_err(|e| e.to_string())?,
        game_version: text("game_version")?,
        gamma: match m.get("gamma") { None | Some(Value::Null) => None,
            Some(_) => Some(text("gamma")?.parse().map_err(|_| "invalid gamma")?) },
    };
    meta.validate()?;
    Ok(meta)
}

/// Loads legacy or relational weights into the trainer's shared CPU inference.
pub fn load(bytes: &[u8]) -> Result<(CpuPolicy, WebPolicyMeta), String> {
    let meta = metadata(bytes)?;
    let fields=policy_fields(bytes)?;
    let mut model=init_policy_model(meta.hidden as usize,joint_header(&fields)?,relational_header(&fields)?);
    let mut store = SafetensorsStore::from_bytes(Some(bytes.to_vec()));
    let result = model.load_from(&mut store).map_err(|e| format!("weights: {e:?}"))?;
    if !result.missing.is_empty() || !result.unused.is_empty() || !result.errors.is_empty() {
        return Err(format!("incomplete policy: {result:?}"));
    }
    Ok((CpuPolicy::copy_from(&model), meta))
}

fn init_policy_model(hidden:usize,joint:bool,relational:bool)->NativePolicy {
    let device=Device::flex();
    match (joint,relational) {
        (false,false)=>NativePolicy::init(hidden,&device),
        (true,false)=>NativePolicy::init_joint(hidden,&device),
        (false,true)=>NativePolicy::init_v2(hidden,&device),
        (true,true)=>NativePolicy::init_joint_v2(hidden,&device),
    }
}

/// Exports authenticated v3/v4 legacy or v5/v6 relational native checkpoints.
/// state.json is streamed: only the fields below are kept, the large replay journals are skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_checkpoint(dir: &std::path::Path, lineage: &str, game_version: &str)
    -> Result<(Vec<u8>, WebPolicyMeta), Box<dyn std::error::Error>> {
    use burn::module::Module;
    #[derive(Deserialize)]
    struct Config { reward: Reward, #[serde(default)] gamma: Option<f32>, #[serde(flatten)] mix:TrainingMix }
    #[derive(Deserialize)]
    struct State { schema: String, manifest: Value, hidden: u64, steps: u64, next_update: u64, training_config: Config,
        #[serde(default="sts2_policy::native::legacy_observation_mode")] observation_mode:String }
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or("checkpoint directory name")?;
    let update: u64 = name.strip_prefix("checkpoint_update_").and_then(|n| n.parse().ok())
        .ok_or("not a published native checkpoint directory")?;
    let state: State = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(dir.join("state.json"))?))?;
    let (joint,relational)=sts2_policy::native::authenticate_checkpoint(&json!({
        "schema":state.schema,"manifest":state.manifest,"observation_mode":state.observation_mode}))?;
    if state.next_update != update { return Err("checkpoint directory and state disagree on the update".into()); }
    state.training_config.mix.validate()?;
    let hidden = usize::try_from(state.hidden)?;
    let raw = std::fs::read(dir.join("policy.bpk"))?;
    let model=init_policy_model(hidden,joint,relational).try_load_file(dir.join("policy.bpk"))?;
    let meta = WebPolicyMeta {
        lineage: lineage.into(), update, step: state.steps, checkpoint_id: format!("{lineage}.{update}"),
        checkpoint_sha256: sha256(&raw), hidden: hidden as u32, reward: state.training_config.reward,
        game_version: game_version.into(), gamma: state.training_config.gamma,
    };
    Ok((export_with_mix(&model, &meta,Some(&state.training_config.mix))?, meta))
}

fn character(name: &str) -> Result<Character, String> {
    Ok(match name {
        "IRONCLAD" => Character::Ironclad, "SILENT" => Character::Silent, "DEFECT" => Character::Defect,
        "NECROBINDER" => Character::Necrobinder, "REGENT" => Character::Regent,
        _ => return Err("unsupported character".into()),
    })
}

/// Replays the trusted policy, independently reconstructing every NativeObsV1 input,
/// legal candidate, sampled action, probability, reward and terminal transition.
pub(crate) fn verify_rows(weights: &[u8], seed: u64, names: &[String], ascension: u8,
    rows: &[crate::native_verify::Row]) -> Result<Vec<NativeDecision>, String> {
    let (policy, meta) = load(weights)?;
    let mut workspace = CpuWorkspace::default();
    let characters=names.iter().map(|name|character(name)).collect::<Result<Vec<_>,_>>()?;
    let game = Game::start_mode(seed, &characters, ascension,policy_is_joint(weights)?,
        observation_mode(&policy_fields(weights)?)?)?;
    let current = Some(game.decision_input()?);
    let mut episode = Episode { game, rng: Rng(seed), seed, character: names[0].clone(), characters:names.to_vec(), ascension,
        high: 0, rows: String::new(), decisions: 0, frames: false, acts: vec![], current, end: None, search: None,
        search_failed: false };
    let mut inputs = Vec::new();
    for (t, row) in rows.iter().enumerate() {
        if episode.end.is_some() { return Err("rows after terminal".into()); }
        if inputs.len() < 64 { inputs.push(episode.current.as_ref().ok_or("missing observation")?.0.clone()); }
        let start = episode.rows.len();
        NativeAiPlay::decide(&policy, &mut workspace, &meta, &mut episode, &mut Vec::new(), &mut Map::new())?;
        let expected: crate::native_verify::Row = serde_json::from_str(episode.rows[start..].trim_end()).map_err(|e|e.to_string())?;
        if row.schema != expected.schema || row.t != t as u32 || row.actor != expected.actor
            || row.action.candidate_slot != expected.action.candidate_slot || row.options != expected.options
            || row.action.identity != expected.action.identity
            || row.legal != expected.legal || !row.behavior_logp.is_finite() || !row.value.is_finite()
            || (row.behavior_logp-expected.behavior_logp).abs() > 1e-5 || (row.value-expected.value).abs() > 1e-5
            || row.reward != expected.reward || row.seat_rewards!=expected.seat_rewards || row.done != expected.done
            || row.observation_sha256 != expected.observation_sha256 || row.next_observation_sha256 != expected.next_observation_sha256 {
            return Err(format!("native behavior/replay mismatch at {t}"));
        }
    }
    if !matches!(episode.end, Some(Ok(_))) { return Err("incomplete native episode".into()); }
    Ok(inputs)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Status { Continue, Terminal { won: bool } }

/* ---------------------------------------------------------------- C2 v2 episode */
struct Episode {
    game: Game, rng: Rng, seed: u64, character: String, characters:Vec<String>, ascension: u8, high: u32,
    rows: String, decisions: usize, frames: bool, acts: Vec<u64>,
    current: Option<(NativeDecision, String)>, end: Option<Result<Option<bool>, String>>,
    /// Decision-time search (off unless the start options ask for it). Its games are
    /// never trainable: the searched actions are not the policy's behavior.
    /// Boxed: the encoder workspace is large and the episode is moved by value
    /// (an inline copy overflowed the 2 MiB test-thread stack in debug builds).
    search: Option<Box<(search::SearchConfig, search::Workspace)>>,
    /// A search failed in this game (logged once); those decisions kept the sampled action.
    search_failed: bool,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = warn)]
    fn console_warn(text: &str);
}
/// One diagnostic line: the browser console in the page, stderr natively.
fn log_line(text: &str) {
    #[cfg(target_arch = "wasm32")]
    console_warn(&format!("sts2 aiplay: {text}"));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("sts2 aiplay: {text}");
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options { #[serde(default)] character: String, #[serde(default)] party:Option<Vec<String>>, #[serde(default="default_ascension")] ascension: u8, #[serde(default)] party_size: Option<u8>, #[serde(default)] frames: bool,
    #[serde(default)] search: Option<search::SearchConfig> }
fn default_ascension()->u8{10}

/// One worker's policy and its current episode. The episode keeps the policy it started with.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct NativeAiPlay { policy: CpuPolicy, workspace: CpuWorkspace, meta: WebPolicyMeta, mix:Option<TrainingMix>, episode: Option<Episode>, joint:bool, relational:bool }
#[cfg(feature="memory-profile")]
#[path="native_memory.rs"]
mod memory;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl NativeAiPlay {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(weights: Vec<u8>) -> Result<NativeAiPlay, String> {
        let (policy, meta) = load(&weights)?;
        let mix=policy_fields(&weights)?["training_mix"].as_str().map(|text|serde_json::from_str::<TrainingMix>(text).map_err(|e|e.to_string())).transpose()?;
        if let Some(mix)=&mix{mix.validate()?;}
        let joint=policy_is_joint(&weights)?;
        let relational=observation_mode(&policy_fields(&weights)?)?;
        Ok(Self { policy, workspace: CpuWorkspace::default(), meta,mix, episode: None,joint,relational })
    }
    pub fn meta(&self) -> String { serde_json::to_string(&self.meta).unwrap() }
    #[cfg_attr(target_arch="wasm32",wasm_bindgen(js_name=sampleParty))]
    pub fn sample_party(&self,seed:u64)->Result<String,String>{
        let mix=self.mix.as_ref().ok_or("checkpoint lacks training_mix; republish with the current exporter")?;
        serde_json::to_string(&mix.party(seed)).map_err(|e|e.to_string())
    }

    /// Read-only production parity probe: all legal slots, not just the sampled action.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = policyOutput))]
    pub fn policy_output(&mut self)->Result<String,String> {
        let e=self.episode.as_ref().ok_or("no episode")?;
        let (decision,hash)=e.current.as_ref().ok_or("missing observation")?;
        let value=self.policy.forward(decision,&mut self.workspace);
        if !value.is_finite() || self.workspace.logp.iter().any(|p|!p.is_finite()) {
            return Err("nonfinite policy parity output".into());
        }
        Ok(json!({"observation_sha256":hash,"slots":decision.slots,
            "logp":self.workspace.logp,"value":value}).to_string())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = inputSnapshot))]
    pub fn input_snapshot(&self)->Result<String,String> {
        let e=self.episode.as_ref().ok_or("no episode")?;
        let (decision,_)=e.current.as_ref().ok_or("missing observation")?;
        decision.trajectory_json(self.relational).map_err(|e|e.to_string())
    }

    #[cfg(not(target_arch="wasm32"))]
    pub fn diagnostic_state(&self)->String {
        let Some(e)=&self.episode else {return "null".into()};
        let choice=e.game.combat().and_then(|c|c.pending_choice_for_player(0).map(|p|json!({
            "resolver":format!("{:?}",p.resolver),"min":p.min,"max":p.max,"single":p.single,
            "allow_skip":p.allow_skip,"is_multi":p.is_multi(),"selected":c.choice_selected_for_player(0)})));
        json!({"decisions":e.decisions,"phase":e.game.phase(),"floor":e.game.floor(),
            "choice":choice,"observation_sha256":e.current.as_ref().map(|(_,hash)|hash)}).to_string()
    }

    /// Starts a solo game; returns the replay header (without maps) as JSON.
    pub fn start(&mut self, seed: u64, options: &str) -> Result<String, String> {
        let o: Options = serde_json::from_str(options).map_err(|e| e.to_string())?;
        let party=if let Some(party)=&o.party{party.clone()}else if !o.character.is_empty(){vec![o.character.clone()]}
            else{self.mix.as_ref().ok_or("checkpoint lacks training_mix; republish with the current exporter")?.party(seed)};
        // Ascensions 0-9 are watch-only (finish returns no rows): the policy trains at ascension 10 only.
        if o.ascension > 10 || !(1..=4).contains(&party.len()) || o.party_size.is_some_and(|size|size as usize!=party.len()) { return Err("party/ascension mismatch: AI-play supports ascension 0-10".into()); }
        let characters=party.iter().map(|name|character(name)).collect::<Result<Vec<_>,_>>()?;
        let mut search=o.search;
        if let Some(config)=&search {config.validate()?;}
        // Weights exported before the discount was published, joint ones included: play on
        // with the plain policy.
        let search_unavailable=search.is_some() && self.meta.gamma.is_none();
        if search_unavailable {
            log_line(&format!("decision search off: {} lacks the training discount (gamma); playing with the plain policy",
                self.meta.checkpoint_id));
            search=None;
        }
        if search.is_some() && self.joint {return Err("decision search supports per-seat policies only".into());}
        let mut game = Game::start_mode(seed, &characters, o.ascension,self.joint,self.relational)?;
        if o.frames && self.joint {game.enable_replay_recording();}
        let current = Some(game.decision_input()?);
        self.episode = Some(Episode { game, rng: Rng(seed), seed, character: party[0].clone(), characters:party.clone(), ascension: o.ascension,
            high: 0, rows: String::new(), decisions: 0, frames: o.frames, acts: vec![], current, end: None,
            search: search.map(|config|Box::new((config,search::Workspace::new(self.relational)))), search_failed: false });
        let mut header=json!({"type":"header","v":2,"source":"aiplay","seed":seed.to_string(),"character":party.join("+"),
            "characters":party,"party_size":characters.len(),"ascension":o.ascension,"reason":"aiplay",
            "checkpoint_id":self.meta.checkpoint_id,"checkpoint_step":self.meta.step,"sim_commit":SOURCE_COMMIT});
        if let Some(config)=&search {header["search"]=json!(config);}
        if search_unavailable {header["search_unavailable"]=json!("weights lack the training discount (gamma)");}
        if o.frames {header["between_event_schema"]=json!(sts2_native_obs::replay::between::SCHEMA);}
        if self.joint {header["v"]=json!(3);header["action_contract"]=json!(ACTION_CONTRACT);
            header["queue_event_schema"]=json!("sts2-combat-queue-v1");}
        Ok(header.to_string())
    }

    /// Plays up to `max` decisions: `{frames,maps,decisions,done,fault}` (frames only when requested).
    pub fn advance(&mut self, max: u32) -> Result<String, String> {
        let e = self.episode.as_mut().ok_or("no episode")?;
        let mut frames = Vec::new();
        let mut maps = Map::new();
        let mut played = 0;
        while e.end.is_none() && played < max {
            if let Err(fault) = Self::decide(&self.policy, &mut self.workspace, &self.meta, e, &mut frames, &mut maps) {
                e.end = Some(Err(fault));
            }
            played += 1;
        }
        Ok(json!({"frames":frames,"maps":maps,"decisions":e.decisions,"done":e.end.is_some(),
            "fault":match &e.end { Some(Err(f)) => f.as_str(), _ => "" }}).to_string())
    }

    /// Ends the episode: a completed game gives its C2 rows/META and the replay end row;
    /// a fault (simulator-unsupported scene or decision cap) is never trainable.
    pub fn finish(&mut self) -> Result<String, String> {
        let e = self.episode.take().ok_or("no episode")?;
        let c=contract(self.joint);
        match e.end {
            Some(Ok(won)) => {
                let mut end = e.game.inventory();
                end["type"] = json!("end"); end["floor"] = json!(e.game.floor()); end["len"] = json!(e.decisions);
                end["end"] = json!(if won == Some(true) { "win" } else { "death" });
                let mut result=json!({"decision":"trainable","decisions":e.decisions,"rows":e.rows,"end":end,
                    "meta":{"schema":c.run,"seed":e.seed.to_string(),"character":e.character,"ascension":e.ascension,
                        "party_size":e.characters.len(),"characters":e.characters,"game_version":self.meta.game_version,"game_dll_sha256":crate::GAME_DLL_SHA256,
                        "vanilla_fingerprint":crate::VANILLA_FINGERPRINT,"active_mods":[],
                        "checkpoint_id":self.meta.checkpoint_id,"checkpoint_sha256":self.meta.checkpoint_sha256,
                        "policy_schema":c.policy,"action_schema":c.action,"sim_commit":SOURCE_COMMIT,
                        "decision":"trainable"}});
                if result["meta"]["party_size"]==1{
                    result["meta"].as_object_mut().unwrap().remove("characters");
                }
                if let Some(state)=&e.search {result["meta"]["search"]=json!(state.0);}
                if e.ascension!=10 || e.search.is_some() {
                    result["decision"]=json!("watch_only"); result["meta"]["decision"]=json!("watch_only");
                    result.as_object_mut().unwrap().remove("rows");
                }
                Ok(result.to_string())
            },
            Some(Err(fault)) => Ok(json!({"decision":"fault","fault":fault,"decisions":e.decisions}).to_string()),
            None => Err("episode is still running".into()),
        }
    }

    /// Headless farm episode: start, play to the end, finish.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = playEpisode))]
    pub fn play_episode(&mut self, seed: u64, options: &str) -> Result<String, String> {
        self.start(seed, options)?;
        self.advance(u32::MAX)?;
        self.finish()
    }
}

impl NativeAiPlay {
    /// One decision, sampled like trainer native_run (PCG uniform, inverse CDF over legal slots),
    /// except that the reward-reopen guard's blocked slots are left out.
    fn decide(policy: &CpuPolicy, workspace: &mut CpuWorkspace, meta: &WebPolicyMeta, e: &mut Episode,
        frames: &mut Vec<Value>, maps: &mut Map<String, Value>) -> Result<(), String> {
        if e.decisions >= MAX_DECISIONS { return Err("decision cap".into()); }
        let (decision, observation) = e.current.take().ok_or("missing observation")?;
        let (options, legal) = { let view = e.game.observation()?;
            (view.candidate_len, decision.slots.len()) };
        let value = policy.forward(&decision, workspace);
        let sum: f64 = workspace.logp.iter().map(|p| (*p as f64).exp()).sum();
        if !value.is_finite() || (sum - 1.).abs() > 1e-5 { return Err("invalid native policy distribution/value".into()); }
        // Under the reward-reopen guard the behavior distribution is the policy's,
        // renormalized over the positions it leaves; that probability is the one recorded.
        let blocked = e.game.reopen_guard(&decision.slots);
        let kept = |p: &usize| blocked.as_ref().is_none_or(|b| !b[*p]);
        let mass: f64 = if blocked.is_some() { (0..legal).filter(kept).map(|p| (workspace.logp[p] as f64).exp()).sum() } else { 1. };
        let u = e.rng.uniform() * mass;
        let mut cumulative = 0.;
        let mut position = (0..legal).filter(kept).last().ok_or("no legal option")?;
        for p in (0..legal).filter(kept) {
            cumulative += (workspace.logp[p] as f64).exp();
            if u < cumulative { position = p; break; }
        }
        if let Some(state) = &mut e.search && let Some(root) = e.game.search_root() {
            let (config, ws) = &mut **state;
            // Search only ever replaces the sampled action; a failed search leaves it.
            match search::search(policy, &meta.reward, meta.gamma, &root, config, ws) {
                Ok(outcome) if outcome.reason == "searched" => {
                    position = decision.slots.iter().position(|slot| *slot == outcome.slot).ok_or("searched slot is not legal")?;
                },
                Ok(_) => {},
                Err(error) => if !e.search_failed {
                    e.search_failed = true;
                    log_line(&format!("decision search failed ({error}); this game continues with the plain policy where it fails"));
                },
            }
        }
        let slot = decision.slots[position] as usize;
        let identity=e.game.action_identity(slot)?;
        let actor=identity.actor;
        let logp = workspace.logp[position] - mass.ln() as f32;
        if e.frames {
            let act = e.game.act();
            if !e.acts.contains(&act) { e.acts.push(act); maps.insert(act.to_string(), e.game.map()); }
            frames.push(e.game.frame(slot, e.decisions)?);
        }
        let before=e.game.reward_state();
        let cursor=e.game.queue_cursor();
        let between=if e.frames {Some(e.game.begin_between(e.decisions))}else{None};
        let status = e.game.step(slot)?;
        let terminal = match status { Status::Terminal { won } => Some(won), Status::Continue => None };
        let transition=meta.reward.transition(&before,&e.game.reward_state(),terminal);
        let reward=transition.team_reward;
        let seat_rewards=&transition.seat_rewards[..transition.seats];
        e.high=e.high.max(e.game.floor());
        if let Some(frame)=frames.last_mut() {
            frame["r"]=json!(reward);
            if e.game.is_joint(){frame["queue_events"]=e.game.queue_events_since(cursor)?;
                frame["after"]=e.game.replay_snapshot(actor)?;}
            if let Some((capture,origin))=between {
                drop(capture);
                frame["between"]=e.game.between(origin,frame,terminal.is_some())?;
            }
        }
        let next = if terminal.is_some() { EMPTY_SHA256.to_string() } else {
            let (next, hash) = e.game.decision_input()?;
            e.current = Some((next, hash.clone()));
            hash
        };
        let action=if e.game.is_joint(){json!({"candidate_slot":slot,"identity":identity})}else{json!({"candidate_slot":slot})};
        let row = json!({"schema":contract(e.game.is_joint()).step,"t":e.decisions,"actor":actor,"action":action,
            "options":options,"legal":legal,"behavior_logp":logp,"value":value,"reward":reward,"seat_rewards":seat_rewards,"done":terminal.is_some(),
            "observation_sha256":observation,"next_observation_sha256":next});
        e.rows.push_str(&row.to_string()); e.rows.push('\n');
        if e.rows.len() > MAX_ROW_BYTES { return Err("trajectory exceeds byte cap".into()); }
        e.decisions += 1;
        if terminal.is_some() { e.end = Some(Ok(terminal)); }
        Ok(())
    }
}

/// A recorded human run (gzipped or plain `sts2-human-run-v2`) as dashboard replay data:
/// `{header,maps,frames,end,complete,failure}` (sts2_human_replay::view::replay).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = humanReplay))]
pub fn human_replay(recording: &[u8]) -> String { sts2_human_replay::view::replay(recording).to_string() }

/// Package provenance shown on the page and written to the package manifest.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = buildInfo))]
pub fn build_info() -> String {
    json!({"source_commit":SOURCE_COMMIT,"native_main_commit":NATIVE_MAIN_COMMIT,"policy_schema":POLICY_SCHEMA,
        "action_schema":ACTION_SCHEMA,"obs_manifest":manifest_metadata(),"characters":mix::characters(),"maxPlayers":MAX_SEATS,
        "obs_manifests":[sts2_policy::native::observation_manifest_metadata(false,false),
            sts2_policy::native::observation_manifest_metadata(true,false)],
        "supported_policy_schemas":[POLICY_SCHEMA,JOINT_POLICY_SCHEMA],
        "supported_policy_architectures":["legacy","relational-v2"],
        "supported_action_schemas":[ACTION_SCHEMA,JOINT_ACTION_SCHEMA]}).to_string()
}

fn joint_header(m:&Value)->Result<bool,String>{
    if m["schema"]==JOINT_POLICY_SCHEMA {Ok(true)}
    else if m["schema"]==POLICY_SCHEMA {Ok(false)}else{Err("unknown native policy schema".into())}
}
fn relational_header(m:&Value)->Result<bool,String> {
    let Some(value)=m.get("policy_manifest") else {return Ok(false);};
    let text=value.as_str().ok_or("invalid native policy manifest")?;
    let manifest:Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
    if manifest!=relational_manifest_metadata(joint_header(m)?) {
        return Err("native policy manifest mismatch".into());
    }
    Ok(true)
}
pub fn policy_is_joint(bytes:&[u8])->Result<bool,String>{joint_header(&policy_fields(bytes)?)}
pub fn policy_is_relational(bytes:&[u8])->Result<bool,String>{observation_mode(&policy_fields(bytes)?)}
fn observation_mode(m:&Value)->Result<bool,String> {
    let relational=relational_header(m)?;
    let mode=m.get("observation_mode").map(|v|v.as_str().ok_or("invalid observation mode")).transpose()?;
    sts2_policy::native::authenticate_observation_mode(mode,relational)?;
    Ok(relational)
}

#[cfg(test)]
#[path="native_joint_tests.rs"]
mod joint_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(hidden: usize) -> Vec<u8> { weights_with(hidden, Some(0.999)) }
    fn weights_with(hidden: usize, gamma: Option<f32>) -> Vec<u8> {
        let model = NativePolicy::init(hidden, &Device::flex());
        let meta = WebPolicyMeta { lineage: "test-run".into(), update: 7, step: 4096, checkpoint_id: "test-run.7".into(),
            checkpoint_sha256: sha256(b"policy.bpk"), hidden: hidden as u32,
            reward: Reward::default(), game_version: "test".into(), gamma };
        export(&model, &meta).unwrap()
    }

    #[test]
    fn weights_without_gamma_play_the_plain_policy_when_search_is_asked_for() {
        let mut ai = NativeAiPlay::new(weights_with(8, None)).unwrap();
        let options = r#"{"character":"IRONCLAD","ascension":10,"search":{"samples":2,"node_budget":64}}"#;
        let header: Value = serde_json::from_str(&ai.start(403, options).unwrap()).unwrap();
        assert!(header.get("search").is_none(), "{header}");
        assert!(header["search_unavailable"].as_str().is_some_and(|s| s.contains("gamma")));
        ai.advance(u32::MAX).unwrap();
        let searched_request = ai.finish().unwrap();
        let plain = ai.play_episode(403, r#"{"character":"IRONCLAD","ascension":10}"#).unwrap();
        assert_eq!(searched_request, plain, "the game is the plain policy's, trainable as such");
    }

    #[test]
    fn joint_weights_without_gamma_also_play_the_plain_policy_when_search_is_asked_for() {
        let model = NativePolicy::init_joint(8, &Device::flex());
        let joint = |gamma| export(&model, &WebPolicyMeta { lineage: "joint-test".into(), update: 0, step: 0,
            checkpoint_id: "joint-test.0".into(), checkpoint_sha256: sha256(b"joint"), hidden: 8,
            reward: Reward::default(), game_version: "test".into(), gamma }).unwrap();
        let options = r#"{"party":["IRONCLAD","SILENT"],"ascension":10,"search":{"samples":2,"node_budget":64}}"#;
        let header: Value = serde_json::from_str(&NativeAiPlay::new(joint(None)).unwrap().start(404, options).unwrap()).unwrap();
        assert!(header.get("search").is_none(), "{header}");
        assert!(header["search_unavailable"].as_str().is_some_and(|s| s.contains("gamma")), "{header}");
        assert!(NativeAiPlay::new(joint(Some(0.999))).unwrap().start(404, options).is_err(),
            "joint weights with a discount still refuse search");
    }

    #[test]
    fn every_registry_character_and_duplicate_party_has_owned_frames() {
        let bytes=weights(8);
        let catalog=mix::characters();
        assert_eq!(catalog.len(),Namespace::Character.registry().unwrap().count);
        for item in catalog{
            for seats in 1..=4{
                let mut ai=NativeAiPlay::new(bytes.clone()).unwrap();
                let party=vec![item["id"].as_str().unwrap();seats];
                ai.start(20261030000000,&json!({"party":party,"frames":true}).to_string()).unwrap();
                let chunk:Value=serde_json::from_str(&ai.advance(16).unwrap()).unwrap();
                assert_eq!(chunk["fault"],"","{item}/{seats}: {chunk}");
                let mut actors=std::collections::BTreeSet::new();
                for frame in chunk["frames"].as_array().unwrap(){
                    assert_eq!(frame["players"].as_array().unwrap().len(),seats);
                    actors.insert(frame["actor"].as_u64().unwrap());
                    assert!(frame["actor"].as_u64().unwrap()<seats as u64);
                }
                assert_eq!(actors.len(),seats,"initial choices must not starve seats");
            }
        }
    }

    #[test]
    fn default_party_uses_checkpoint_distribution_and_rejects_empty_explicit_party(){
        let mix=TrainingMix{character_weights:Namespace::Character.registry().unwrap().names.iter().map(|n|((*n).to_owned(),1.)).collect(),
            party_weights:vec![0.,1.,0.,0.],ascension_min:10,ascension_weights:vec![1.]};
        let meta=metadata(&weights(8)).unwrap();
        let model=NativePolicy::init(8,&Device::flex());
        let mut ai=NativeAiPlay::new(export_with_mix(&model,&meta,Some(&mix)).unwrap()).unwrap();
        let mut seen=std::collections::BTreeSet::new();let mut duplicates=false;
        for seed in 0..128{
            let party:Vec<String>=serde_json::from_str(&ai.sample_party(seed).unwrap()).unwrap();
            assert_eq!(party.len(),2);
            duplicates|=party[0]==party[1];seen.extend(party);
        }
        assert_eq!(seen.len(),Namespace::Character.registry().unwrap().count);assert!(duplicates);
        let header:Value=serde_json::from_str(&ai.start(33,"{}").unwrap()).unwrap();
        assert_eq!(header["characters"].as_array().unwrap().len(),2);assert_eq!(header["ascension"],10);
        // Any ascension 0-10 starts (below 10 is watch-only); the training mix stays ascension 10.
        for ascension in 0..=10 {
            assert!(ai.start(33,&json!({"ascension":ascension}).to_string()).is_ok());
        }
        assert!(ai.start(33,&json!({"ascension":11}).to_string()).is_err());
        let mut mixed=mix.clone();
        mixed.ascension_min=0;mixed.ascension_weights=vec![1.;11];
        assert!(export_with_mix(&model,&meta,Some(&mixed)).is_err());
        assert!(ai.start(33,r#"{"party":[]}"#).is_err());
    }

    #[test]
    fn policy_probe_preserves_sampling_and_returns_a_normalized_legal_distribution() {
        let weights=weights(8);
        let mut probed=NativeAiPlay::new(weights.clone()).unwrap();
        let mut plain=NativeAiPlay::new(weights).unwrap();
        let options=r#"{"character":"IRONCLAD","ascension":10,"party_size":1}"#;
        probed.start(400,options).unwrap();
        let output=probed.policy_output().unwrap();
        assert_eq!(output,probed.policy_output().unwrap());
        let output:Value=serde_json::from_str(&output).unwrap();
        let logp=output["logp"].as_array().unwrap();
        assert!(!logp.is_empty());
        assert_eq!(logp.len(),output["slots"].as_array().unwrap().len());
        assert!((logp.iter().map(|p|p.as_f64().unwrap().exp()).sum::<f64>()-1.).abs()<1e-5);
        probed.advance(u32::MAX).unwrap();
        assert_eq!(probed.finish().unwrap(),plain.play_episode(400,options).unwrap());
    }

    #[test]
    fn noncombat_replay_frames_never_publish_retained_combat_state() {
        let mut ai=NativeAiPlay::new(weights(8)).unwrap();
        let (mut noncombat,mut combat)=(0,0);
        for seed in 400..410 {
            ai.start(seed,r#"{"character":"IRONCLAD","ascension":10,"party_size":1,"frames":true}"#).unwrap();
            loop {
                let chunk:Value=serde_json::from_str(&ai.advance(64).unwrap()).unwrap();
                for frame in chunk["frames"].as_array().unwrap() {
                    if frame["ph"]=="COMBAT" {
                        combat+=1;assert!(frame.get("turn").is_some());
                    } else {
                        noncombat+=1;
                        for key in ["turn","foes","hand","piles","pw","en","men","blk"] {
                            assert!(frame.get(key).is_none(),"{key}: {frame}");
                        }
                    }
                }
                if chunk["done"]==true {ai.finish().unwrap();break;}
            }
        }
        assert!(noncombat>0 && combat>0);
    }

    #[test]
    fn native_verifier_accepts_complete_policy_games_and_rejects_tampering() {
        let weights=weights(8);
        let mut ai=NativeAiPlay::new(weights.clone()).unwrap();
        let end:Value=serde_json::from_str(&ai.play_episode(400,
            r#"{"character":"IRONCLAD","ascension":10,"party_size":1}"#).unwrap()).unwrap();
        assert_eq!(end["decision"],"trainable");
        let mut meta=end["meta"].clone();meta["sim_commit"]=json!("a".repeat(40));
        let text=format!("{}\n{}",json!({"schema":EPISODE_SCHEMA,"meta":meta}),end["rows"].as_str().unwrap());
        let verified=crate::native_verify::replay(&text,&weights,&"a".repeat(40)).unwrap();
        assert!(!verified.inputs.is_empty());
        assert!(verified.rows.last().unwrap().done);
        let bad=text.replacen("\"candidate_slot\":", "\"extra\":true,\"candidate_slot\":",1);
        assert!(crate::native_verify::replay(&bad,&weights,&"a".repeat(40)).is_err());
        let mut lines:Vec<Value>=text.lines().map(|line|serde_json::from_str(line).unwrap()).collect();
        lines[1]["behavior_logp"]=json!(0.1);
        let bad=lines.iter().map(|line|format!("{line}\n")).collect::<String>();
        assert!(crate::native_verify::replay(&bad,&weights,&"a".repeat(40)).err().unwrap().contains("mismatch"));
    }


    #[test]
    fn c1_v2_roundtrip_and_rejects_other_schemas_or_lineage_ids() {
        let bytes = weights(8);
        let meta = metadata(&bytes).unwrap();
        assert_eq!((meta.step, meta.update, meta.hidden), (4096, 7, 8));
        assert!(NativeAiPlay::new(bytes.clone()).is_ok());
        let header = 8 + u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
        for (from, to) in [(POLICY_SCHEMA, "sts2-policy-web-v1"), ("test-run.7", "test-run.8")] {
            let mut edited = String::from_utf8_lossy(&bytes[..header]).replace(from, to).into_bytes();
            edited.extend_from_slice(&bytes[header..]);
            assert!(metadata(&edited).is_err(), "{from} -> {to}");
        }
    }

    /// Diagnostic: completed vs faulted games for a seed range (random policy weights).
    #[test]
    #[ignore = "diagnostic: cargo test -p sts2_aiplay --features native -- --ignored --nocapture outcomes"]
    fn outcomes() {
        let mut ai = NativeAiPlay::new(weights(8)).unwrap();
        let mut faults = std::collections::BTreeMap::<String, usize>::new();
        let (mut complete, mut decisions) = (0, 0);
        for seed in 0..40u64 {
            let character = ["IRONCLAD", "SILENT", "DEFECT", "NECROBINDER", "REGENT"][seed as usize % 5];
            let end: Value = serde_json::from_str(&ai.play_episode(seed,
                &format!(r#"{{"character":"{character}","ascension":10,"party_size":1}}"#)).unwrap()).unwrap();
            decisions += end["decisions"].as_u64().unwrap();
            if end["decision"] == "trainable" { complete += 1; }
            else { *faults.entry(end["fault"].as_str().unwrap().chars().take(90).collect()).or_default() += 1; }
        }
        eprintln!("complete={complete}/40 decisions={decisions} faults={faults:#?}");
    }

    #[test]
    fn search_is_off_by_default_and_searched_games_are_watch_only() {
        let mut ai = NativeAiPlay::new(weights(8)).unwrap();
        let plain: Value = serde_json::from_str(&ai.start(402, r#"{"character":"IRONCLAD"}"#).unwrap()).unwrap();
        assert!(plain.get("search").is_none());
        let options = r#"{"character":"IRONCLAD","search":{"samples":2,"node_budget":48}}"#;
        let header: Value = serde_json::from_str(&ai.start(402, options).unwrap()).unwrap();
        assert_eq!(header["search"]["samples"], 2);
        ai.advance(u32::MAX).unwrap();
        let end = ai.finish().unwrap();
        assert_eq!(ai.play_episode(402, options).unwrap(), end, "searched play is reproducible");
        let end: Value = serde_json::from_str(&end).unwrap();
        if end["decision"] == "fault" { return; }
        assert_eq!((&end["decision"], &end["meta"]["search"]["node_budget"]), (&json!("watch_only"), &json!(48)));
        assert!(end.get("rows").is_none(), "searched actions are not the policy's behavior");
        assert!(ai.start(402, r#"{"character":"IRONCLAD","search":{"samples":0}}"#).is_err());
    }

    #[test]
    fn lower_ascension_games_are_watch_only() {
        let mut ai = NativeAiPlay::new(weights(8)).unwrap();
        assert!(ai.start(401, r#"{"character":"IRONCLAD","ascension":11}"#).is_err());
        let header: Value = serde_json::from_str(&ai.start(401, r#"{"character":"IRONCLAD","ascension":0,"frames":true}"#).unwrap()).unwrap();
        assert_eq!(header["ascension"], 0);
        ai.advance(u32::MAX).unwrap();
        let end: Value = serde_json::from_str(&ai.finish().unwrap()).unwrap();
        if end["decision"] == "fault" { return; }
        assert_eq!((&end["decision"], &end["meta"]["decision"], &end["meta"]["ascension"]), (&json!("watch_only"), &json!("watch_only"), &json!(0)));
        assert!(end.get("rows").is_none(), "a watch-only game has nothing to train on");
    }

    #[test]
    fn episode_rows_are_a_complete_hash_chained_c2_v2_trajectory_or_a_fault() {
        let mut ai = NativeAiPlay::new(weights(8)).unwrap();
        let options = r#"{"character":"IRONCLAD","ascension":10,"party_size":1,"frames":true}"#;
        let header: Value = serde_json::from_str(&ai.start(400, options).unwrap()).unwrap();
        assert_eq!(header["v"], 2);
        let step: Value = serde_json::from_str(&ai.advance(5).unwrap()).unwrap();
        assert_eq!(step["frames"].as_array().unwrap().len(), 5);
        assert!(step["maps"].as_object().unwrap().values().next().is_some_and(|m| m.as_array().is_some_and(|m| !m.is_empty())),
            "the real act map is projected");
        ai.advance(u32::MAX).unwrap();
        let end: Value = serde_json::from_str(&ai.finish().unwrap()).unwrap();
        if end["decision"] == "fault" { assert!(end["fault"].as_str().is_some_and(|f| !f.is_empty())); return; }
        let rows: Vec<Value> = end["rows"].as_str().unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len() as u64, end["decisions"].as_u64().unwrap());
        for (t, row) in rows.iter().enumerate() {
            assert_eq!(row["t"], t);
            assert!(row["action"]["candidate_slot"].as_u64().unwrap() < row["options"].as_u64().unwrap());
            let logp = row["behavior_logp"].as_f64().unwrap();
            assert!(logp <= 0. && logp.exp() > 0.);
            assert_eq!(row["done"], t + 1 == rows.len());
            let next = row["next_observation_sha256"].as_str().unwrap();
            if t + 1 < rows.len() { assert_eq!(next, rows[t + 1]["observation_sha256"]); } else { assert_eq!(next, EMPTY_SHA256); }
        }
        // Headless replay of the same seed and weights is the same trajectory.
        let again: Value = serde_json::from_str(&ai.play_episode(400, &options.replace(",\"frames\":true", "")).unwrap()).unwrap();
        assert_eq!(again["rows"], end["rows"]);
    }
}
