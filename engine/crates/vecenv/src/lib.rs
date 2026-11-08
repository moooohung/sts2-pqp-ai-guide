//! Vector env for the pure-Rust r8 trainer, generic over the lane simulator
//! (`LaneSim`; default sts2_sim_int1, later the CPU-native sts2_sim).
//!
//! Every lane's r7 wire is written in place into a `WireBatch` (one disjoint
//! slot per lane). A lane whose step refuses (outcome != 0, an illegal action,
//! or no model-legal action afterwards) is quarantined: its episode ends as
//! truncated + refused and it resets on a fresh seed with a new instance (a
//! refusal poisons the instance's allocation cache). User rule 2026-10-23.
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
#[cfg(feature = "native")]
pub use native_backend::NativeBackend as DefaultSim;
#[cfg(feature = "selector-proof")]
pub use native_backend::{ProofBackend, RuleBoundary};
#[cfg(not(feature = "native"))]
compile_error!("select a vecenv backend: native");
use sts2_wire::{WireBatch, WireSlots};

pub mod sim_lane;
mod event_semantics;
#[cfg(feature="selector-proof")]
pub use event_semantics::{write_event_proof,event_option_table_shape,event_option_label};
mod choice_context;
mod progress;
#[cfg(feature = "native")]
mod native_backend;

pub const ASCENSION: i32 = 10;

/// What a vector-env lane needs from a simulator: the `sts2_sim_int1::SimInstance`
/// surface (outcome != 0 = refused; phase/first_reason are diagnostics).
pub trait LaneSim: Send + 'static {
    fn new() -> Self;
    fn default_saved_wongo_points() -> Option<i32> { None }
    fn reset_device(&mut self, seed: u64, ascension: i32);
    fn set_saved_wongo_points(&mut self, points: Option<i32>) -> Result<(), String> {
        if points.is_some() { return Err("saved Wongo points require native backend".into()); }
        Ok(())
    }
    fn step_device(&mut self, action: i32);
    fn phase(&self) -> i32;
    fn outcome(&self) -> i32;
    fn first_reason(&self) -> i32;
    fn reason_text(&self) -> Option<&'static str> { None }
    fn step_reward(&self) -> f32;
    fn is_done(&self) -> bool;
    fn is_terminated(&self) -> bool;
    fn is_truncated(&self) -> bool;
    /// 1 when candidate slot `cand_idx` (0..256) is model-legal.
    fn wire_candidate_model(&self, cand_idx: i32) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn write_obs_and_mask(
        &self,
        state_kind_ids: &mut [u16], state_token_ids: &mut [u16], state_numeric_values: &mut [i32],
        state_numeric_scales: &mut [f32], state_numeric_tail: &mut [f32], state_mask: &mut [i8],
        public_act1_boss: &mut [f32], candidate_kind_ids: &mut [u16], candidate_token_ids: &mut [u16],
        candidate_numeric_values: &mut [i32], candidate_numeric_scales: &mut [f32],
        candidate_numeric_tail: &mut [f32], choice_purpose_ids: &mut [u16], model_mask: &mut [i8],
        source_applicable: &mut [i8], target_applicable: &mut [i8], source_known: &mut [i8],
        target_known: &mut [i8], source_links_bits: &mut [u8], target_links_bits: &mut [u8],
        legal_mask: &mut [bool],
    );
}


/// Per-lane results of one step.
#[derive(Clone, Default)]
pub struct StepResults {
    pub reward: Vec<f32>,
    pub done: Vec<bool>,
    pub terminated: Vec<bool>,
    pub truncated: Vec<bool>,
    /// 0 = fine; > 0 simulator first_reason; -1 illegal action; -2 no legal action.
    pub refusal: Vec<i32>,
    /// Finished episodes only: return, length and seed of the ended episode.
    pub episode_return: Vec<f32>,
    pub episode_len: Vec<i32>,
    pub episode_seed: Vec<u64>,
}

impl StepResults {
    pub fn new(n: usize) -> Self {
        Self {
            reward: vec![0.0; n], done: vec![false; n], terminated: vec![false; n], truncated: vec![false; n],
            refusal: vec![0; n], episode_return: vec![0.0; n], episode_len: vec![0; n], episode_seed: vec![0; n],
        }
    }
}

struct Lane<S> {
    sim: S,
    saved_wongo_points: Option<i32>,
    seed: u64,
    ret: f64,
    len: i32,
    /// This episode's actions, for refusal fixtures ({"seed","actions"}).
    actions: Vec<i32>,
}

fn has_model_action<S: LaneSim>(sim: &S) -> bool {
    (0..256).any(|a| sim.wire_candidate_model(a) == 1)
}

fn exclude_refusal_reward(why: i32, reward: &mut f32, episode_return: &mut f64) {
    if why != 0 { *reward = 0.0; *episode_return = 0.0; }
}

#[cfg(test)]
mod refusal_reward_tests {
    #[test]
    fn empty_mask_clears_current_reward_and_accumulated_return() {
        let (mut reward,mut ret)=(0.75,12.5);
        super::exclude_refusal_reward(-2,&mut reward,&mut ret);
        assert_eq!((reward,ret),(0.0,0.0));
        let (mut reward,mut ret)=(0.75,12.5);
        super::exclude_refusal_reward(0,&mut reward,&mut ret);
        assert_eq!((reward,ret),(0.75,12.5));
    }
}

fn reset_once<S: LaneSim>(lane: &mut Lane<S>, seed: u64) -> Result<(), String> {
    lane.seed = seed;
    lane.ret = 0.0;
    lane.len = 0;
    lane.actions.clear();
    // Poisoned lanes and failed reset attempts replace S with S::new().
    // Reapply the explicit profile input to every replacement before reset.
    lane.sim.set_saved_wongo_points(lane.saved_wongo_points)?;
    lane.sim.reset_device(seed, ASCENSION);
    if lane.sim.outcome() != 0 || !has_model_action(&lane.sim) {
        return Err(format!("reset of seed {seed} refused (outcome={}, reason={}, phase={})",
            lane.sim.outcome(), lane.sim.first_reason(), lane.sim.phase()));
    }
    Ok(())
}

fn reset_lane<S: LaneSim>(lane: &mut Lane<S>, seed: u64) -> (usize, Option<String>) {
    reset_attempts(seed, |candidate| {
        let result = reset_once(lane, candidate);
        if result.is_err() { lane.sim = S::new(); }
        result
    })
}

fn reset_attempts(seed: u64, mut reset: impl FnMut(u64) -> Result<(), String>) -> (usize, Option<String>) {
    for attempt in 0..8 {
        let candidate = seed.wrapping_add((attempt as u64) << 40);
        match reset(candidate) {
            Ok(()) => return (attempt, None),
            Err(error) if attempt == 7 => return (8, Some(error)),
            Err(_) => {}
        }
    }
    unreachable!()
}

#[cfg(test)]
mod reset_tests {
    use super::reset_attempts;
    #[test]
    fn retry_seeds_and_failure_counts_are_bounded_and_deterministic() {
        let mut seen = Vec::new();
        let result = reset_attempts(17, |seed| {
            seen.push(seed);
            if seen.len() < 3 { Err("refused".into()) } else { Ok(()) }
        });
        assert_eq!(seen, [17, 17 + (1u64 << 40), 17 + (2u64 << 40)]);
        assert_eq!(result, (2, None));
        let mut seen = Vec::new();
        let result = reset_attempts(23, |seed| { seen.push(seed); Err("exhausted".into()) });
        assert_eq!(seen, (0..8).map(|i| 23 + (i << 40)).collect::<Vec<u64>>());
        assert_eq!(result, (8, Some("exhausted".into())));
        assert_eq!(reset_attempts(91, |_| Ok(())), (0, None));
    }
}

/// # Safety
/// `i` is this thread's own lane.
unsafe fn write_wire<S: LaneSim>(sim: &S, w: &WireSlots, i: usize) {
    unsafe {
        sim.write_obs_and_mask(
            w.state_kind.lane(i), w.state_tokens.lane(i), w.state_values.lane(i), w.state_scales.lane(i),
            w.state_tail.lane(i), w.state_mask.lane(i), w.boss.lane(i), w.cand_kind.lane(i), w.cand_tokens.lane(i),
            w.cand_values.lane(i), w.cand_scales.lane(i), w.cand_tail.lane(i), w.choice.lane(i), w.model_mask.lane(i),
            w.source_applicable.lane(i), w.target_applicable.lane(i), w.source_known.lane(i), w.target_known.lane(i),
            w.source_bits.lane(i), w.target_bits.lane(i), w.legal.lane(i),
        )
    }
}

/// Opt the process out of Windows power throttling (EcoQoS): a background job
/// otherwise runs on the efficiency cores at half speed (native_bench, 2026-10-25).
#[cfg(windows)]
pub fn no_power_throttling() {
    #[repr(C)]
    struct State { version: u32, control_mask: u32, state_mask: u32 }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn SetProcessInformation(process: *mut std::ffi::c_void, class: i32, info: *const State, size: u32) -> i32;
    }
    let state = State { version: 1, control_mask: 1, state_mask: 0 };
    unsafe { SetProcessInformation(GetCurrentProcess(), 4, &state, std::mem::size_of::<State>() as u32); }
}

#[cfg(not(windows))]
pub fn no_power_throttling() {}

/// Pin worker `index` to a P-core. i7-13700K: logical CPUs 0..15 are the 8 P-cores
/// (2 hyper-threads each), 16..23 the E-cores, which run this simulator at 54%
/// of a P-core (2026-10-25). One thread per physical P-core first (0, 2, ..., 14),
/// then their siblings. Measured 8 threads: 2,615 dec/s pinned vs 2,408 unpinned.
#[cfg(windows)]
pub fn pin_to_p_core(index: usize) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadAffinityMask(thread: *mut std::ffi::c_void, mask: usize) -> usize;
    }
    let cpu = (index % 8) * 2 + (index / 8) % 2;
    unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << cpu); }
}

#[cfg(not(windows))]
pub fn pin_to_p_core(_index: usize) {}

/// A refused episode: replay with `native_replay` ({"seed","actions"}).
pub struct Refusal {
    pub seed: u64,
    pub reason: i32,
    pub reason_text: Option<&'static str>,
    pub actions: Vec<i32>,
}

impl Refusal {
    pub fn fixture_json(&self) -> String {
        let text = self.reason_text.map_or("null".into(), |s| format!("\"{}\"",
            s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")));
        format!("{{\"seed\":{},\"reason\":{},\"reason_text\":{},\"actions\":{:?}}}\n", self.seed, self.reason, text, self.actions)
    }
}

pub struct VecEnv<S: LaneSim = DefaultSim> {
    pub num_envs: usize,
    pool: ThreadPool,
    lanes: Vec<Lane<S>>,
    next_seed: u64,
    /// Refusals since the caller last drained this list.
    pub refusals: Vec<Refusal>,
    /// Cumulative episode resets performed in the worker pool.
    pub inline_resets: usize,
    /// Includes failed initial and inline reset attempts, even on exhaustion.
    pub refused_resets: usize,
    pub refusal_counts: std::collections::BTreeMap<(i32, Option<&'static str>), usize>,
}

impl<S: LaneSim> VecEnv<S> {
    /// Read-only lane inspection for cross-backend state-digest tests.
    pub fn lane_sim(&self, index: usize) -> &S { &self.lanes[index].sim }
    pub fn set_saved_wongo_points(&mut self, points: Option<i32>) -> Result<(), String> {
        if points.is_some_and(|p| p < 0) { return Err("saved Wongo points must be nonnegative".into()); }
        for lane in &mut self.lanes {
            lane.sim.set_saved_wongo_points(points)?;
            lane.saved_wongo_points = points;
        }
        Ok(())
    }

    /// `pin_p_cores`: pin worker i to a P-core (at most 16 workers).
    /// Reset seeds are assigned in lane order, never worker-completion order.
    pub fn new(num_envs: usize, num_threads: usize, seed: u64, pin_p_cores: bool) -> Result<Self, String> {
        if num_envs == 0 || num_threads == 0 {
            return Err("num_envs and num_threads must be positive".into());
        }
        if pin_p_cores && num_threads > 16 {
            return Err("at most 16 workers fit on the 8 P-cores".into());
        }
        no_power_throttling();
        // Generated descriptor frames need the same 64 MiB stacks as native_bench.
        let mut builder = ThreadPoolBuilder::new().num_threads(num_threads).stack_size(64 << 20);
        if pin_p_cores {
            builder = builder.start_handler(pin_to_p_core);
        }
        let pool = builder.build().map_err(|e| e.to_string())?;
        let lanes = pool.install(|| (0..num_envs).into_par_iter()
            .map(|_| Lane { sim: S::new(), saved_wongo_points: S::default_saved_wongo_points(),
                seed: 0, ret: 0.0, len: 0, actions: Vec::new() }).collect());
        Ok(Self {
            num_envs, pool, lanes, next_seed: seed, refusals: Vec::new(), inline_resets: 0, refused_resets: 0,
            refusal_counts: std::collections::BTreeMap::new(),
        })
    }

    fn take_seed(&mut self) -> u64 {
        let s = self.next_seed;
        self.next_seed += 1;
        s
    }

    /// Reset every lane on consecutive seeds and write its wire into `obs`.
    pub fn reset(&mut self, obs: &mut WireBatch) -> Result<(), String> {
        assert_eq!(obs.n, self.num_envs);
        let slots = obs.slots();
        let base = self.next_seed;
        self.next_seed += self.num_envs as u64;
        let (pool, lanes) = (&self.pool, &mut self.lanes);
        let resets: Vec<_> = pool.install(|| lanes.par_iter_mut().enumerate()
            .map(|(i, lane)| {
                let result = reset_lane(lane, base + i as u64);
                if result.1.is_none() { unsafe { write_wire(&lane.sim, &slots, i) }; }
                result
            }).collect());
        let mut error = None;
        for (count, reason) in resets {
            self.refused_resets += count;
            if error.is_none() { error = reason; }
        }
        if let Some(e) = error { return Err(e); }
        Ok(())
    }

    /// Step every lane with its model-mask candidate slot. Ended or refused
    /// lanes reset on seeds assigned in lane order. Refused instances are
    /// dropped: a refusal poisons the allocation cache.
    pub fn step(&mut self, actions: &[i32], obs: &mut WireBatch, out: &mut StepResults) -> Result<usize, String> {
        let n = self.num_envs;
        assert_eq!(actions.len(), n);
        let slots = obs.slots();
        type Row = (f32, bool, bool, bool, i32, f32, i32, u64, Option<Vec<i32>>, Option<&'static str>);
        let (pool, lanes) = (&self.pool, &mut self.lanes);
        let rows: Vec<Row> = pool.install(|| lanes.par_iter_mut().enumerate().map(|(i, lane)| {
            let a = actions[i];
            lane.actions.push(a);
            let mut why = 0i32;
            let mut r = 0.0f32;
            if (0..256).contains(&a) && lane.sim.wire_candidate_model(a) == 1 {
                lane.sim.step_device(a);
                if lane.sim.outcome() != 0 {
                    why = lane.sim.first_reason().max(1);
                } else {
                    r = lane.sim.step_reward();
                }
            } else {
                why = -1;
            }
            lane.ret += r as f64;
            lane.len += 1;
            let ended = why == 0 && lane.sim.is_done();
            if why == 0 && !ended && !has_model_action(&lane.sim) {
                why = -2;
            }
            exclude_refusal_reward(why, &mut r, &mut lane.ret);
            let terminated = ended && lane.sim.is_terminated();
            let truncated = why != 0 || (ended && lane.sim.is_truncated());
            let fixture = (why != 0).then(|| std::mem::take(&mut lane.actions));
            if !(ended || why != 0) {
                unsafe { write_wire(&lane.sim, &slots, i) };
            }
            let text = match why { -1 => Some("illegal action"), -2 => Some("empty model mask"),
                0 => None, _ => lane.sim.reason_text() };
            (r, ended || why != 0, terminated, truncated, why, lane.ret as f32, lane.len, lane.seed, fixture, text)
        }).collect());
        let mut refused = 0;
        let mut ended_lanes = Vec::new();
        for (i, (r, done, term, trunc, why, ret, len, seed, fixture, text)) in rows.into_iter().enumerate() {
            out.reward[i] = r;
            out.done[i] = done;
            out.terminated[i] = term;
            out.truncated[i] = trunc;
            out.refusal[i] = why;
            out.episode_return[i] = ret;
            out.episode_len[i] = len;
            out.episode_seed[i] = seed;
            refused += (why != 0) as usize;
            if let Some(actions) = fixture {
                *self.refusal_counts.entry((why,text)).or_default() += 1;
                self.refusals.push(Refusal { seed, reason: why, reason_text: text, actions });
            }
            if done {
                ended_lanes.push((i, why != 0));
            }
        }
        // Allocate seeds before parallel work. Thread count and reset latency
        // must not affect which episode replaces an ended lane.
        let mut inline = Vec::new();
        for &(i, poisoned) in &ended_lanes {
            if poisoned {
                self.lanes[i].sim = S::new();
            }
            inline.push(i);
        }
        self.inline_resets += inline.len();
        let seeds: Vec<u64> = inline.iter().map(|_| self.take_seed()).collect();
        let (pool, lanes) = (&self.pool, &mut self.lanes);
        let resets: Vec<_> = pool.install(|| lanes.par_iter_mut().enumerate()
            .filter_map(|(i, lane)| {
                let k = inline.iter().position(|&x| x == i)?;
                Some(reset_lane(lane, seeds[k]))
            }).collect());
        let mut error = None;
        for (count, reason) in resets {
            self.refused_resets += count;
            if error.is_none() { error = reason; }
        }
        if let Some(e) = error { return Err(e); }
        for &(i, _) in &ended_lanes {
            unsafe { write_wire(&self.lanes[i].sim, &slots, i) };
        }
        Ok(refused)
    }
}
