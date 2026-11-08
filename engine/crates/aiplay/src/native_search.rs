//! Decision-time search for play and advice only; training never calls it.
//!
//! Within a combat, every root candidate the policy rates highly is tried on the
//! same determinized samples of the hidden state (`determinize_hidden`: draw pile
//! order and every RNG stream re-sampled from the public state and a seed). Each
//! line is played by the greedy policy (teammates included, under `reopen_guard`) until the acting
//! seat's next decision in a later round (so the enemy turn and the next draw are
//! simulated), the combat's end, or the run's end, and scored with the training
//! reward contract, discounted with the checkpoint's training gamma per acting-seat
//! decision, plus the critic: Q = sum_k gamma^k r_k + gamma^K V(leaf), where the leaf
//! is always an acting-seat decision. A line cut by `max_rollout` bootstraps from the
//! last acting-seat decision it reached, or is refused when it reached none. The
//! chosen action is the best mean Q when it beats the policy's own first choice by
//! `margin`; otherwise the policy's choice stands. Out of combat the policy decides.
//! A search that cannot sample the hidden state (a teammate is resolving a card
//! choice) or whose line hits any simulator fault is refused as a whole (Err): every
//! caller then keeps the policy's own choice.
//!
//! The result depends only on the public state, the decision id and `seed`. The only
//! budget is the node budget: every policy evaluation, the root's included, spends
//! one node, so the result never depends on machine speed.
use serde::{Deserialize, Serialize};
use sts2_native_obs::reward::{Reward, State, party_objective_complete, solo_objective_complete};
use sts2_native_obs::{DecisionId, DecisionStatus, NativeEncoder, NativeObsV1};
use sts2_policy::native::{CpuPolicy, CpuWorkspace, NativeDecision};
use sts2_sim::run::{NativeRewardContent, RunActionKind, RunManager, RunPhase, mp::{MpPhase, MpRun}};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SearchConfig {
    /// Determinized samples of the hidden state, shared by every candidate.
    pub samples: u32,
    /// Most root candidates searched, highest policy probability first.
    pub max_actions: u32,
    /// Candidates are added in probability order until this mass is covered (at least two).
    pub prior_mass: f32,
    /// Policy evaluations one decision may spend: the root's, rollout steps and leaf values.
    pub node_budget: u32,
    /// Decisions one line may take before it is cut at its last acting-seat decision.
    pub max_rollout: u32,
    /// Q advantage over the policy's first choice required to replace it.
    pub margin: f32,
    pub seed: u64,
}
impl Default for SearchConfig {
    fn default() -> Self {
        // Sized for the mod: p95 75 ms per combat decision on one native core under load
        // (Ironclad A10, 200 paired games); 4 samples/160 nodes cost 2.4x for no measured gain.
        Self { samples: 2, max_actions: 4, prior_mass: 0.95, node_budget: 64, max_rollout: 24,
            margin: 0.002, seed: 0 }
    }
}
impl SearchConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.samples == 0 || self.samples > 64 || self.max_actions < 2 || self.max_actions > 32
            || !(0. ..=1.).contains(&self.prior_mass) || self.node_budget == 0 || self.node_budget > 1_000_000
            || self.max_rollout == 0 || self.max_rollout > 1000 || !self.margin.is_finite() || self.margin < 0. {
            return Err("invalid search config".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum SimRun { Solo(RunManager), Party(MpRun) }
impl SimRun {
    fn determinize(&mut self, seed: u64, viewer: usize) -> Result<(), String> {
        match self { Self::Solo(g) => g.determinize_hidden(seed), Self::Party(g) => g.determinize_hidden(seed, viewer) }
            .map_err(str::to_owned)
    }
    fn combat_round(&self) -> Option<i32> {
        let (combat, fighting) = match self {
            Self::Solo(g) => (g.combat(), g.phase() == RunPhase::Combat),
            Self::Party(g) => (g.combat(), g.phase == MpPhase::Combat),
        };
        combat.filter(|c| fighting && !c.is_over).map(|c| c.round)
    }
    pub fn in_combat(&self) -> bool { self.combat_round().is_some() }
    fn reward_state(&self) -> State {
        match self { Self::Solo(g) => State::solo(g), Self::Party(g) => State::party(g) }
    }
}

/// A decision of the acting seat. `net_base + seat` is a party seat's network id
/// (the solo driver's id is always 1).
#[derive(Clone, Debug)]
pub struct Root { pub run: SimRun, pub decision: DecisionId, pub seats: usize, pub net_base: u64 }
impl Root {
    fn net(&self, seat: u8) -> u64 { if self.seats == 1 { 1 } else { self.net_base + u64::from(seat) } }
}

/// Heap-held parts: the encoder and observation are large, and debug builds copy them
/// through the 2 MiB test-thread stack.
pub struct Workspace { encoder: Box<NativeEncoder>, obs: Box<NativeObsV1>, cpu: CpuWorkspace }
impl Workspace {
    pub fn new(relational: bool) -> Self {
        Self { encoder: Box::new(sts2_policy::native::encoder_for_policy(relational)), obs: Box::new(NativeObsV1::new()),
            cpu: CpuWorkspace::default() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate { pub position: usize, pub slot: i32, pub prior: f32, pub q: Option<f64> }

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    /// Index into the decision's legal slots.
    pub position: usize,
    pub slot: i32,
    /// The greedy policy's own choice on the same observation.
    pub policy_position: usize,
    /// Why the outcome is (or is not) a searched one: searched, not_combat,
    /// single_option or no_complete_sample.
    pub reason: &'static str,
    pub candidates: Vec<Candidate>,
    pub samples: u32,
    /// Samples refused because a line reached no acting-seat decision within `max_rollout`.
    pub incomplete: u32,
    /// Policy evaluations spent, the root's included.
    pub nodes: u32,
}

enum Status { Continue, Terminal { won: bool } }

struct Line<'a> { root: &'a Root, run: SimRun, decision: DecisionId }
impl Line<'_> {
    /// Same order as the AI-play driver: automatic multiplayer advancement, then the
    /// first actionable seat starting at the current actor.
    fn refresh(&mut self, ws: &mut Workspace) -> Result<Status, String> {
        let err = |e: &dyn std::fmt::Debug| format!("{e:?}");
        if let SimRun::Party(g) = &mut self.run {
            NativeEncoder::advance_multiplayer(g, &mut self.decision).map_err(|e| err(&e))?;
        }
        if match &self.run { SimRun::Solo(g) => solo_objective_complete(g), SimRun::Party(g) => party_objective_complete(g) } {
            return Ok(Status::Terminal { won: true });
        }
        for offset in 0..self.root.seats {
            let actor = ((self.decision.actor as usize + offset) % self.root.seats) as u8;
            let decision = DecisionId { actor, ..self.decision };
            match &self.run {
                SimRun::Solo(g) => ws.encoder.encode_solo(g, decision, &mut ws.obs),
                SimRun::Party(g) => ws.encoder.encode_multiplayer(g, decision, self.root.net(actor), &mut ws.obs),
            }.map_err(|e| err(&e))?;
            match ws.obs.view().map_err(|e| err(&e))?.status {
                DecisionStatus::Actionable => { self.decision.actor = actor; return Ok(Status::Continue); },
                DecisionStatus::Terminal => return Ok(Status::Terminal { won: match &self.run {
                    SimRun::Solo(g) => g.observation_full_run_victory(), SimRun::Party(g) => party_objective_complete(g) } }),
                DecisionStatus::GameplayFault => return Err("gameplay fault".into()),
                DecisionStatus::Waiting => {},
            }
        }
        Err("all seats waiting".into())
    }
    fn step(&mut self, slot: usize, ws: &mut Workspace) -> Result<Status, String> {
        let actor = self.decision.actor;
        match &mut self.run {
            SimRun::Solo(g) => ws.encoder.dispatch_solo(g, &mut self.decision, &mut ws.obs, slot),
            SimRun::Party(g) => ws.encoder.dispatch_multiplayer(g, &mut self.decision, self.root.net(actor), &mut ws.obs, slot),
        }.map_err(|e| format!("{e:?}"))?;
        self.decision.actor = ((actor as usize + 1) % self.root.seats) as u8;
        self.refresh(ws)
    }
}

struct Budget { nodes: u32, limit: u32 }

enum LineValue { Value(f64), OutOfBudget, Incomplete }

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
fn greedy(logp: &[f32]) -> usize { (1..logp.len()).fold(0, |a, b| if logp[b] > logp[a] { b } else { a }) }

/// Reward-reopen guard for a policy acting on its own (search rollouts, the website's AI
/// play, the A/B harness; the mod's AI teammate applies the same rule in
/// `AiSeatDecisionLoop`). Within one rewards screen a seat that closed a card reward
/// without taking a card (C# `CardReward.OnSelect` returned false and `NRewardButton`
/// re-enabled the button) does not open it again: reopening is legal but leads back to
/// the same screen, so a chooser preferring it cycles forever (sim
/// `run::reward_cycle_tests`). Only the solo simulator has the reopenable container.
/// Returns the blocked positions of `slots`, or None when nothing (or everything) is blocked.
pub fn reopen_guard(g: &RunManager, obs: &NativeObsV1, slots: &[i32]) -> Option<Vec<bool>> {
    let closed = |backing: usize| g.native_reward(backing)
        .is_some_and(|r| matches!(r.content, NativeRewardContent::Cards { closed: true, .. }));
    if !g.native_reward_indices().any(closed) { return None; }
    let blocked: Vec<bool> = slots.iter().map(|&slot| NativeEncoder::solo_run_action(g, obs, slot as usize)
        .is_ok_and(|a| a.kind == RunActionKind::OpenReward
            && a.reward_index.and_then(|d| g.native_reward_indices().nth(d)).is_some_and(closed))).collect();
    (blocked.contains(&true) && blocked.contains(&false)).then_some(blocked)
}
/// The policy's most probable position among those the guard leaves (ties: the lower one).
pub fn guarded_greedy(logp: &[f32], blocked: Option<&[bool]>) -> usize {
    match blocked {
        None => greedy(logp),
        Some(b) => (0..logp.len()).filter(|&i| !b[i]).reduce(|a, i| if logp[i] > logp[a] { i } else { a }).unwrap_or(0),
    }
}

struct Searcher<'a> { policy: &'a CpuPolicy, reward: &'a Reward, gamma: f64, config: &'a SearchConfig, seat: u8, round: i32 }
impl Searcher<'_> {
    /// The one place the policy is evaluated: each evaluation spends a node first.
    /// Ok(None): the budget is spent.
    fn forward(&self, ws: &mut Workspace, budget: &mut Budget) -> Result<Option<(NativeDecision, f32)>, String> {
        if budget.nodes >= budget.limit { return Ok(None); }
        budget.nodes += 1;
        #[cfg(test)]
        tests::EVALUATIONS.with(|n| n.set(n.get() + 1));
        let decision = NativeDecision::from_view(ws.obs.view().map_err(|e| format!("{e:?}"))?)?;
        let value = self.policy.forward(&decision, &mut ws.cpu);
        if !value.is_finite() || ws.cpu.logp.iter().any(|p| !p.is_finite()) { return Err("nonfinite policy output".into()); }
        Ok(Some((decision, value)))
    }
    /// Plays `slot` on `line` and continues with the greedy policy to the leaf. Rewards
    /// are discounted once per acting-seat decision, as in training's per-seat returns.
    /// Err: the sampled line hit a simulator fault.
    fn line_value(&self, line: &mut Line, slot: usize, ws: &mut Workspace, budget: &mut Budget)
        -> Result<LineValue, String> {
        let seat = usize::from(self.seat);
        let (mut total, mut discount) = (0f64, 1f64);
        // Q bootstrapped at the latest acting-seat decision, for a line cut by max_rollout.
        let mut cut: Option<f64> = None;
        let mut slot = slot;
        for _ in 0..self.config.max_rollout {
            let before = line.run.reward_state();
            let status = line.step(slot, ws)?;
            let after = line.run.reward_state();
            let terminal = match status { Status::Terminal { won } => Some(won), Status::Continue => None };
            total += discount * f64::from(self.reward.transition(&before, &after, terminal).seat_rewards[seat]);
            if terminal.is_some() { return Ok(LineValue::Value(total)); }
            let ours = line.decision.actor == self.seat;
            if ours { discount *= self.gamma; }
            let Some((decision, value)) = self.forward(ws, budget)? else { return Ok(LineValue::OutOfBudget) };
            if ours {
                cut = Some(total + discount * f64::from(value));
                if line.run.combat_round() != Some(self.round) { return Ok(LineValue::Value(cut.unwrap())); }
            }
            let blocked = match &line.run { SimRun::Solo(g) => reopen_guard(g, &ws.obs, &decision.slots), SimRun::Party(_) => None };
            slot = decision.slots[guarded_greedy(&ws.cpu.logp, blocked.as_deref())] as usize;
        }
        Ok(cut.map_or(LineValue::Incomplete, LineValue::Value))
    }
}

/// `search`'s signature; the advisor's and the bridge's tests substitute a faulty search.
pub type SearchFn = fn(&CpuPolicy, &Reward, Option<f32>, &Root, &SearchConfig, &mut Workspace) -> Result<Outcome, String>;

/// Searches `root`'s decision; `root.decision.actor` must be actionable. `gamma` is the
/// checkpoint's training discount (`WebPolicyMeta::gamma`); weights without one are refused.
pub fn search(policy: &CpuPolicy, reward: &Reward, gamma: Option<f32>, root: &Root, config: &SearchConfig, ws: &mut Workspace)
    -> Result<Outcome, String> {
    config.validate()?;
    let gamma = gamma.filter(|g| *g > 0. && *g <= 1.).ok_or("search needs the checkpoint's training discount (gamma)")?;
    let mut budget = Budget { nodes: 0, limit: config.node_budget };
    let base = mix(config.seed ^ mix(root.decision.generation) ^ (u64::from(root.decision.actor) << 56));
    let sample_seed = |s: u32| mix(base.wrapping_add(u64::from(s)));
    let viewer = usize::from(root.decision.actor);
    // Priors come from a determinized copy: the observation must not depend on hidden order.
    let mut first = root.run.clone();
    first.determinize(sample_seed(0), viewer)?;
    let mut line = Line { root, run: first.clone(), decision: root.decision };
    match line.refresh(ws)? { Status::Continue => {}, Status::Terminal { .. } => return Err("terminal root".into()) }
    if line.decision != root.decision { return Err("root actor is not actionable".into()); }
    let searcher = Searcher { policy, reward, gamma: f64::from(gamma), config, seat: root.decision.actor,
        round: root.run.combat_round().unwrap_or(-1) };
    let (decision, _) = searcher.forward(ws, &mut budget)?.ok_or("node budget below one")?;
    let logp = ws.cpu.logp.clone();
    let policy_position = greedy(&logp);
    let mut order: Vec<usize> = (0..logp.len()).collect();
    order.sort_by(|&a, &b| logp[b].total_cmp(&logp[a]).then(a.cmp(&b)));
    let mut candidates = Vec::new();
    let mut mass = 0f32;
    for &position in &order {
        if candidates.len() >= config.max_actions as usize || (candidates.len() >= 2 && mass >= config.prior_mass) { break; }
        let prior = logp[position].exp();
        mass += prior;
        candidates.push(Candidate { position, slot: decision.slots[position], prior, q: None });
    }
    let mut outcome = Outcome { position: policy_position, slot: decision.slots[policy_position], policy_position,
        reason: "searched", candidates, samples: 0, incomplete: 0, nodes: 0 };
    let finish = |mut outcome: Outcome, reason, budget: &Budget| {
        outcome.reason = reason; outcome.nodes = budget.nodes; Ok(outcome)
    };
    if root.run.combat_round().is_none() { return finish(outcome, "not_combat", &budget); }
    if outcome.candidates.len() < 2 { return finish(outcome, "single_option", &budget); }
    let mut sums = vec![0f64; outcome.candidates.len()];
    'samples: for s in 0..config.samples {
        let sample = if s == 0 { first.clone() } else {
            let mut run = root.run.clone(); run.determinize(sample_seed(s), viewer)?; run
        };
        let mut values = Vec::with_capacity(sums.len());
        for candidate in &outcome.candidates {
            let mut line = Line { root, run: sample.clone(), decision: root.decision };
            let value = line.refresh(ws).and_then(|_| searcher.line_value(&mut line, candidate.slot as usize, ws, &mut budget));
            #[cfg(test)]
            let value = if faults::fault(config.seed, s) { Err(faults::INJECTED.into()) } else { value };
            match value {
                Ok(LineValue::Value(value)) => values.push(value),
                Ok(LineValue::OutOfBudget) => break 'samples,
                Ok(LineValue::Incomplete) => { outcome.incomplete += 1; continue 'samples; },
                // A faulting simulator may have played any of the lines wrong, not just this one.
                Err(error) => return Err(format!("simulator fault in a search line: {error}")),
            }
        }
        for (sum, value) in sums.iter_mut().zip(values) { *sum += value; }
        outcome.samples += 1;
    }
    if outcome.samples == 0 { return finish(outcome, "no_complete_sample", &budget); }
    for (candidate, sum) in outcome.candidates.iter_mut().zip(&sums) { candidate.q = Some(sum / f64::from(outcome.samples)); }
    // Candidates are in prior order, so ties keep the more probable action.
    let best = (1..sums.len()).fold(0, |a, b| if sums[b] > sums[a] { b } else { a });
    let policy_index = outcome.candidates.iter().position(|c| c.position == policy_position).unwrap();
    if (sums[best] - sums[policy_index]) / f64::from(outcome.samples) > f64::from(config.margin) {
        outcome.position = outcome.candidates[best].position;
        outcome.slot = outcome.candidates[best].slot;
    }
    finish(outcome, "searched", &budget)
}

/// A simulator fault injected into search lines, for the fallback test here. Every fault
/// injection is `#[cfg(test)]` code of the crate it tests (the advisor's and the bridge's tests
/// substitute the whole search through `SearchFn`), so no build of a library can contain one.
/// Each injected fault names `STS2_SEARCH_FAULT_INJECTION`; the mod's PackagingCheck refuses a
/// bridge DLL that contains it.
#[cfg(test)]
pub(crate) mod faults {
    pub const FAULT_SEED: u64 = 0x5eed_0000_dead_0003;
    pub const INJECTED: &str = "STS2_SEARCH_FAULT_INJECTION: injected simulator fault";
    /// FAULT_SEED: every line of the first sample faults; the later samples play normally.
    pub(super) fn fault(seed: u64, sample: u32) -> bool { seed == FAULT_SEED && sample == 0 }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests {
    use super::*;
    use super::faults::FAULT_SEED;
    use burn::prelude::*;
    use sts2_native_obs::reward::Contract;
    use sts2_policy::native::NativePolicy;
    use sts2_sim::run::{RunOptions, mp::Character};

    fn policy() -> CpuPolicy { CpuPolicy::copy_from(&NativePolicy::init_v2(8, &Device::flex())) }
    const REWARD: Reward = Reward { contract: Contract::R7TerminalDepthMenuCostV1, shaping_gamma: 1. };

    /// The first combat decision with several legal options, reached by the greedy policy.
    /// Random test weights: some seeds' greedy play never reaches such a decision.
    fn combat_root(policy: &CpuPolicy, seed: u64) -> Root {
        (seed..seed + 50).find_map(|seed| combat_root_at(policy, seed)).expect("no combat decision reached")
    }
    fn combat_root_at(policy: &CpuPolicy, seed: u64) -> Option<Root> {
        let mut g = RunManager::new_for_character(seed, RunOptions { ascension: 10, saved_wongo_points: Some(0),
            ..RunOptions::default() }, Character::Ironclad);
        let mut ws = Workspace::new(true);
        let mut decision = DecisionId { episode: seed, generation: 0, actor: 0 };
        for _ in 0..500 {
            ws.encoder.encode_solo(&g, decision, &mut ws.obs).ok()?;
            let d = NativeDecision::from_view(ws.obs.view().ok()?).ok()?;
            if g.phase() == RunPhase::Combat && d.slots.len() >= 3 && g.combat().unwrap().players[0].draw.len() >= 3 {
                return Some(Root { run: SimRun::Solo(g), decision, seats: 1, net_base: 0 });
            }
            policy.forward(&d, &mut ws.cpu);
            ws.encoder.dispatch_solo(&mut g, &mut decision, &mut ws.obs, d.slots[greedy(&ws.cpu.logp)] as usize).ok()?;
        }
        None
    }
    const GAMMA: Option<f32> = Some(0.99);
    thread_local! { pub(super) static EVALUATIONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

    fn run(policy: &CpuPolicy, root: &Root, config: &SearchConfig) -> String {
        serde_json::to_string(&search(policy, &REWARD, GAMMA, root, config, &mut Workspace::new(true)).unwrap()).unwrap()
    }

    #[test]
    fn hidden_future_never_changes_the_search() {
        let policy = policy();
        let root = combat_root(&policy, 20261107);
        let config = SearchConfig { samples: 3, ..SearchConfig::default() };
        let expected = run(&policy, &root, &config);
        let outcome: serde_json::Value = serde_json::from_str(&expected).unwrap();
        assert_eq!(outcome["reason"], "searched", "{outcome}");
        // Only state the player cannot see: draw order and every RNG stream's future.
        let mut edited = root.clone();
        let SimRun::Solo(g) = &mut edited.run else { unreachable!() };
        let combat = g.combat_mut().unwrap();
        let before = combat.players[0].draw.clone();
        combat.players[0].draw.rotate_left(1);
        assert_ne!(before, combat.players[0].draw, "the edit must reorder the draw pile");
        let rngs = &mut combat.run.rngs;
        for rng in [&mut rngs.shuffle, &mut rngs.monster_ai, &mut rngs.rewards, &mut rngs.combat_targets,
            &mut rngs.combat_card_generation, &mut rngs.niche, &mut combat.rng] {
            for _ in 0..5 { rng.next_unsigned_long_raw(); }
        }
        for creature in &mut combat.creatures {
            if let Some(ai) = &mut creature.ai { ai.rng = sts2_sim::rng::Rng::new(424242); }
        }
        assert_eq!(run(&policy, &edited, &config), expected);
        // Run-level hidden state (relic bag order, map and reward streams) as well.
        let mut resampled = root.clone();
        resampled.run.determinize(99, 0).unwrap();
        assert_eq!(run(&policy, &resampled, &config), expected);
    }

    #[test]
    fn fixed_seed_is_deterministic_and_the_seed_is_an_input() {
        let policy = policy();
        let root = combat_root(&policy, 20261108);
        let config = SearchConfig::default();
        let first = run(&policy, &root, &config);
        assert_eq!(run(&policy, &root, &config), first);
        let mut ws = Workspace::new(true);
        for _ in 0..2 { search(&policy, &REWARD, GAMMA, &root, &config, &mut ws).unwrap(); }
        assert_eq!(serde_json::to_string(&search(&policy, &REWARD, GAMMA, &root, &config, &mut ws).unwrap()).unwrap(), first,
            "a reused workspace carries no state between searches");
        let reseeded: Vec<String> = (1..6).map(|seed| run(&policy, &root, &SearchConfig { seed, ..config })).collect();
        assert!(reseeded.iter().any(|other| *other != first), "samples must depend on the search seed");
    }

    #[test]
    fn node_budget_counts_every_evaluation_and_is_honored() {
        let policy = policy();
        let root = combat_root(&policy, 20261109);
        for node_budget in [1, 2, 5, 17, 60, 400] {
            let before = EVALUATIONS.with(std::cell::Cell::get);
            let outcome = search(&policy, &REWARD, GAMMA, &root, &SearchConfig { node_budget, ..SearchConfig::default() },
                &mut Workspace::new(true)).unwrap();
            assert_eq!(outcome.nodes, EVALUATIONS.with(std::cell::Cell::get) - before, "every evaluation, the root's included");
            assert!(outcome.nodes <= node_budget, "{} > {node_budget}", outcome.nodes);
            if outcome.samples == 0 {
                assert_eq!((outcome.reason, outcome.position), ("no_complete_sample", outcome.policy_position));
            }
        }
        // The root alone fills a budget of one.
        let outcome = search(&policy, &REWARD, GAMMA, &root, &SearchConfig { node_budget: 1, ..SearchConfig::default() },
            &mut Workspace::new(true)).unwrap();
        assert_eq!((outcome.nodes, outcome.samples), (1, 0));
    }

    #[test]
    fn values_use_the_training_discount_and_cut_lines_bootstrap_from_the_acting_seat() {
        let policy = policy();
        let root = combat_root(&policy, 20261110);
        assert!(search(&policy, &REWARD, None, &root, &SearchConfig::default(), &mut Workspace::new(true)).is_err(),
            "weights without a training discount are refused");
        // max_rollout 1: every line is cut after one step at the acting seat's next decision.
        let config = SearchConfig { max_rollout: 1, node_budget: 1000, ..SearchConfig::default() };
        let q = |gamma: f32| {
            let outcome = search(&policy, &REWARD, Some(gamma), &root, &config, &mut Workspace::new(true)).unwrap();
            assert_eq!((outcome.reason, outcome.incomplete), ("searched", 0));
            outcome.candidates.iter().map(|c| c.q.unwrap()).collect::<Vec<_>>()
        };
        assert_ne!(q(1.), q(0.5), "the leaf value is discounted");
    }

    /// A two-seat party at the first combat decision of seat 0 with several options, while
    /// seat 1 holds cards in hand and in the draw pile and still plays this turn.
    fn party_root(policy: &CpuPolicy, seed: u64) -> Option<Root> {
        use sts2_sim::run::mp::PlayerSpec;
        let specs: Vec<PlayerSpec> = (0..2).map(|i| PlayerSpec { net_id: 100 + i, character: Character::Ironclad }).collect();
        let mut g = MpRun::new(&seed.to_string(), &specs, 10).ok()?;
        g.enable_legacy_replay_adapter();
        let shell = Root { run: SimRun::Party(g), decision: DecisionId { episode: seed, generation: 0, actor: 0 }, seats: 2, net_base: 100 };
        let mut ws = Workspace::new(true);
        let mut line = Line { root: &shell, run: shell.run.clone(), decision: shell.decision };
        let Status::Continue = line.refresh(&mut ws).ok()? else { return None };
        for _ in 0..1500 {
            let d = NativeDecision::from_view(ws.obs.view().ok()?).ok()?;
            if line.decision.actor == 0 && line.run.in_combat() && d.slots.len() >= 3
                && let SimRun::Party(g) = &line.run
                && g.combat().is_some_and(|c| c.players[1].hand.len() >= 2 && c.players[1].draw.len() >= 2
                    && !c.is_player_ready_to_end_turn(1)) {
                return Some(Root { run: line.run.clone(), decision: line.decision, seats: 2, net_base: 100 });
            }
            policy.forward(&d, &mut ws.cpu);
            let Status::Continue = line.step(d.slots[greedy(&ws.cpu.logp)] as usize, &mut ws).ok()? else { return None };
        }
        None
    }

    #[test]
    fn a_teammates_hand_never_changes_the_acting_seats_search() {
        let policy = policy();
        let root = (20261112..20261142).find_map(|seed| party_root(&policy, seed)).expect("no party combat decision");
        let config = SearchConfig { samples: 3, ..SearchConfig::default() };
        let expected = run(&policy, &root, &config);
        let outcome: serde_json::Value = serde_json::from_str(&expected).unwrap();
        assert_eq!(outcome["reason"], "searched", "{outcome}");
        // Which of seat 1's cards are in its hand is hidden from seat 0 (only the count shows).
        let mut edited = root.clone();
        let SimRun::Party(g) = &mut edited.run else { unreachable!() };
        let teammate = &mut g.combat_mut().unwrap().players[1];
        let (held, drawn) = (teammate.hand[0], teammate.draw[0]);
        (teammate.hand[0], teammate.draw[0]) = (drawn, held);
        assert_eq!(run(&policy, &edited, &config), expected);
    }

    #[test]
    fn reopen_guard_blocks_only_a_card_reward_closed_on_this_screen() {
        let policy = policy();
        let SimRun::Solo(mut g) = combat_root(&policy, 20261111).run else { unreachable!() };
        g.debug_win_combat().unwrap();
        let mut ws = Workspace::new(true);
        let id = DecisionId { episode: 1, generation: 0, actor: 0 };
        let slots = |g: &RunManager, ws: &mut Workspace| {
            ws.encoder.encode_solo(g, id, &mut ws.obs).unwrap();
            NativeDecision::from_view(ws.obs.view().unwrap()).unwrap().slots
        };
        let take = |g: &mut RunManager, kind: RunActionKind| {
            let action = *g.available_actions().iter().find(|a| a.kind == kind).unwrap_or_else(|| panic!("no {kind:?}"));
            g.take_action(action).unwrap();
        };
        if !g.available_actions().iter().any(|a| a.kind == RunActionKind::PickCard) { take(&mut g, RunActionKind::OpenReward); }
        let s = slots(&g, &mut ws);
        assert!(reopen_guard(&g, &ws.obs, &s).is_none(), "nothing closed yet");
        take(&mut g, RunActionKind::Skip);
        let s = slots(&g, &mut ws);
        let blocked = reopen_guard(&g, &ws.obs, &s).expect("the closed card reward is blocked");
        let kinds: Vec<RunActionKind> = s.iter().map(|&x| NativeEncoder::solo_run_action(&g, &ws.obs, x as usize).unwrap().kind).collect();
        assert_eq!(blocked.iter().filter(|b| **b).count(), 1, "{kinds:?}");
        let at = blocked.iter().position(|b| *b).unwrap();
        assert_eq!(kinds[at], RunActionKind::OpenReward);
        assert!(kinds.contains(&RunActionKind::SkipRewards), "proceeding stays available");
        let mut logp = vec![-5f32; s.len()];
        logp[at] = 0.;
        assert_eq!(greedy(&logp), at);
        assert_ne!(guarded_greedy(&logp, Some(&blocked)), at, "the policy's favourite reopen is not taken");
        // The guard only steers a chooser; the rules still allow reopening.
        let mut fresh = g.clone();
        take(&mut fresh, RunActionKind::OpenReward);
        let s = slots(&fresh, &mut ws);
        assert!(reopen_guard(&fresh, &ws.obs, &s).is_none(), "an open card reward is not blocked");
    }

    #[test]
    fn a_simulator_fault_in_any_line_refuses_the_whole_search() {
        let policy = policy();
        let root = combat_root(&policy, 20261107);
        let config = SearchConfig { samples: 3, node_budget: 400, ..SearchConfig::default() };
        let clean = search(&policy, &REWARD, GAMMA, &root, &config, &mut Workspace::new(true)).unwrap();
        assert_eq!(clean.reason, "searched");
        // The other samples complete, but the faulting one's lines may hide a wrong state.
        let faulty = search(&policy, &REWARD, GAMMA, &root, &SearchConfig { seed: FAULT_SEED, ..config }, &mut Workspace::new(true));
        assert!(faulty.as_ref().is_err_and(|e| e.contains("simulator fault")), "{faulty:?}");
    }

    #[test]
    fn invalid_configs_are_refused() {
        for config in [SearchConfig { samples: 0, ..SearchConfig::default() },
            SearchConfig { max_actions: 1, ..SearchConfig::default() },
            SearchConfig { node_budget: 0, ..SearchConfig::default() },
            SearchConfig { margin: f32::NAN, ..SearchConfig::default() }] {
            assert!(config.validate().is_err());
        }
        assert!(serde_json::from_str::<SearchConfig>(r#"{"samples":2,"unknown":1}"#).is_err());
        assert!(serde_json::from_str::<SearchConfig>(r#"{"deadline_ms":30}"#).is_err(), "no wall-clock budget");
        assert_eq!(serde_json::from_str::<SearchConfig>("{}").unwrap(), SearchConfig::default());
    }
}
