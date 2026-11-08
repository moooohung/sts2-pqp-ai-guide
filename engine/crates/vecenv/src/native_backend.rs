use crate::{LaneSim, sim_lane::{SimLane, Transition}};
use sts2_sim::{obs::{ActionKind, ActionSlots, CombatObservationContext}, run::{RunActionKind, RunPhase}};
use sts2_wire::WireBatch;

fn enemy_target_index(combat: &sts2_sim::combat::Combat, target: Option<sts2_sim::card::CId>)
    -> Option<Option<usize>>
{
    match target {
        None => Some(None),
        Some(id) => combat.enemies.iter().position(|&enemy| enemy == id).map(Some),
    }
}

/// Select with `--no-default-features --features native`. Unsupported reset
/// phases return a refusal, never an empty observation passed to the policy.
pub struct NativeBackend {
    lane: SimLane,
    wire: WireBatch,
    slots: ActionSlots,
    last: Transition,
    opened_rewards: std::collections::HashSet<u64>,
    selector_source: Option<String>,
    proof_mode: bool,
    saved_wongo_points: Option<i32>,
}

#[cfg(feature = "selector-proof")]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuleBoundary {
    pub kind: &'static str,
    pub source: String,
}

impl NativeBackend {
    #[cfg(any(test,feature="selector-proof"))]
    pub fn recorded_choice_wire_slot(&self, index:Option<usize>) -> Option<usize> {
        (0..self.slots.len()).find(|&slot|self.slots.get(slot).is_some_and(|binding|
            match index {
                Some(index)=>binding.kind==ActionKind::Choose && binding.choice_index as usize==index,
                None=>binding.kind==ActionKind::ConfirmChoice,
            }))
    }

    /// Latest successful decision (never publish it after a refusal).
    pub fn observation(&self) -> &WireBatch { &self.wire }
    pub fn fault_reason(&self) -> Option<&'static str> { self.last.fault }
    pub fn run(&self) -> &sts2_sim::run::RunManager { self.lane.run() }
    pub fn actions(&self) -> &[sts2_sim::run::RunAction] { self.lane.actions() }
    pub fn selector_source(&self) -> Option<&str> { self.selector_source.as_deref() }

    fn blocked_reward_reopen(&self,action:&sts2_sim::run::RunAction)->bool {
        action.kind==RunActionKind::OpenReward
            && action.reward_index.and_then(|i|self.run().observation_reward_card_identity(i))
                .is_some_and(|id|self.opened_rewards.contains(&id))
    }
    /// Legal native commands excluded by the frozen teacher's history mask.
    /// Retain their exact bindings in transport, with teacher probability zero.
    pub fn masked_reward_reopens(&self)->Vec<sts2_sim::run::RunAction> {
        self.actions().iter().enumerate().filter(|(slot,action)|
            action.enabled && self.wire.model_mask.get(*slot)==Some(&0)
                && self.blocked_reward_reopen(action)).map(|(_,action)|*action).collect()
    }
    pub fn observe_native_action(&mut self,action:&sts2_sim::run::RunAction) {
        if action.kind==RunActionKind::OpenReward {
            if let Some(id)=action.reward_index.and_then(|i|self.run().observation_reward_card_identity(i)) {
                self.opened_rewards.insert(id);
            }
        }
    }

    /// Rebuild the read-only projection after any native decision. Encoding
    /// faults belong to this snapshot, never to the authoritative game.
    pub fn sync_run(&mut self, run: sts2_sim::run::RunManager) {
        self.selector_source=run.observation_selector_source().map(str::to_owned);
        self.lane=SimLane::from_run(run);
        self.last=Transition{reward:0.,done:false,fault:self.lane.fault()};
        self.wire=WireBatch::zeroed(1);
        self.slots.clear();
        self.refresh_mode(true);
    }

    /// Exact run-action index used by step_device, without mutating the lane.
    pub fn candidate_run_index(&self, action: i32) -> Option<usize> {
        let slot = usize::try_from(action).ok()?;
        if self.fault_reason().is_some() || self.wire.model_mask.get(slot) != Some(&1) {
            return None;
        }
        if let Some((_,index))=self.lane.teacher_bindings.iter().find(|(candidate,_)|*candidate==slot) {
            return Some(*index);
        }
        if self.lane.run().phase() != RunPhase::Combat {
            return self.lane.actions().get(slot).filter(|a| a.enabled).map(|_| slot);
        }
        let b = self.slots.get(slot)?;
        let combat = self.lane.run().combat()?;
        let random_enemy = b.kind == ActionKind::PlayCard
            && combat.players[b.actor].hand.get(b.hand_index as usize)
                .is_some_and(|&k| combat.card_target_type(k) == sts2_sim::tables::TargetType::RandomEnemy);
        let target = if random_enemy || (b.kind == ActionKind::UsePotion && b.target == Some(b.actor)) {
            None
        } else { enemy_target_index(combat, b.target)? };
        if b.actor != 0 { return None; }
        self.lane.actions().iter().position(|a| a.enabled && match b.kind {
            ActionKind::EndTurn => a.kind == RunActionKind::EndTurn,
            ActionKind::PlayCard => a.kind == RunActionKind::PlayCard
                && a.hand_index == Some(b.hand_index as usize) && a.target_index == target,
            ActionKind::UsePotion => a.kind == RunActionKind::UsePotion
                && a.slot_index == Some(b.potion_slot as usize) && a.target_index == target,
            ActionKind::Choose => a.kind == RunActionKind::Choose && a.index == Some(b.choice_index as usize),
            ActionKind::ConfirmChoice => a.kind == RunActionKind::ConfirmChoice,
            ActionKind::Empty => false,
        })
    }

    pub fn reset_with_options(&mut self, seed: u64, options: sts2_sim::run::RunOptions) {
        self.reset_with_options_mode(seed, options, false);
    }

    pub fn reset_for_character(&mut self, seed: u64, character: &str) {
        self.reset_character_with_options_mode(seed, character,
            sts2_sim::run::RunOptions::default(), false);
    }

    #[cfg(feature = "selector-proof")]
    pub fn reset_proof_with_options(&mut self, seed: u64, options: sts2_sim::run::RunOptions) {
        self.reset_with_options_mode(seed, options, true);
    }

    fn reset_with_options_mode(&mut self, seed: u64, options: sts2_sim::run::RunOptions, proof_mode: bool) {
        self.reset_character_with_options_mode(seed,"IRONCLAD",options,proof_mode);
    }

    fn reset_character_with_options_mode(&mut self, seed:u64, character:&str,
        options:sts2_sim::run::RunOptions, proof_mode:bool) {
        let ascension = options.ascension;
        // Frozen structured_env.py:1742 normalizes an explicit reset seed
        // before run_env.py performs its Gym/NumPy run-seed draw. Keep the
        // raw RunManager Gym adapter unchanged; this is the wrapper boundary.
        let candidate_seed=seed % sts2_sim::rng::INT_MAX as u64;
        self.lane = SimLane::reset_with_options(candidate_seed, character, options);
        self.wire = WireBatch::zeroed(1);
        self.slots.clear();
        self.opened_rewards.clear();
        self.selector_source = None;
        self.proof_mode = proof_mode;
        self.last = Transition { reward: 0.0, done: false, fault: self.lane.fault() };
        if ascension != 10 { self.last = self.lane.refuse("native backend requires ascension 10"); }
        self.refresh();
    }

    fn refresh(&mut self) {
        self.refresh_mode(false)
    }
    fn refresh_mode(&mut self,teacher:bool) {
        if self.last.fault.is_some() { return; }
        let run = self.lane.run();
        if !matches!(run.phase(), RunPhase::Combat | RunPhase::Event | RunPhase::MapChoice | RunPhase::CardReward | RunPhase::RestSite | RunPhase::Shop | RunPhase::Treasure) {
            self.last = self.lane.refuse(run.phase().name());
            return;
        }
        let prompt_result = run.combat().filter(|_| run.phase() == RunPhase::Combat).map(|combat| {
            if self.proof_mode {
                #[cfg(feature = "selector-proof")]
                { return Ok(crate::sim_lane::combat_choice_prompt_proof(combat)); }
            }
            crate::sim_lane::combat_choice_prompt(combat)
        }).transpose();
        let prompt = match prompt_result {
            Ok(prompt) => prompt.unwrap_or(""),
            Err(reason) => { self.last = self.lane.refuse(reason); return; }
        };
        // Frozen public_boss_observation.py::public_boss_feature.
        let bosses = ["CEREMONIAL_BEAST_BOSS", "LAGAVULIN_MATRIARCH_BOSS",
            "SOUL_FYSH_BOSS", "THE_KIN_BOSS", "VANTOM_BOSS", "WATERFALL_GIANT_BOSS"];
        let mut feature = [0.0; 6];
        if let Some(index) = bosses.iter().position(|&boss| Some(boss) == run.public_act1_boss()) {
            feature[index] = 1.0;
        }
        let context = CombatObservationContext {
            public_act1_boss: feature, choice_prompt: prompt,
        };
        let encoded = if teacher {
            self.lane.encode_teacher(&context)
        } else if self.proof_mode {
            #[cfg(feature = "selector-proof")]
            { self.lane.encode_proof(&context) }
            #[cfg(not(feature = "selector-proof"))]
            { unreachable!() }
        } else {
            self.lane.encode(&context)
        };
        match encoded {
            Ok((mut wire, slots)) => {
                // structured_env.py:1320-1327 counts explicit opens, NOT
                // modal dismissals. The first reopen remains model-eligible.
                for (slot, action) in self.lane.actions().iter().enumerate() {
                    if self.blocked_reward_reopen(action)
                    { wire.model_mask[slot] = 0; }
                }
                self.wire = wire; self.slots = slots;
            }
            Err(_) => {
                self.last = Transition { reward: 0.0, done: true, fault: self.lane.fault() };
            }
        }
    }

    #[cfg(feature = "selector-proof")]
    pub fn rule_boundaries(&self) -> Vec<RuleBoundary> {
        use sts2_sim::{run::RunActionKind as K, tables::PowerId};
        let mut out = std::collections::BTreeSet::new();
        let run = self.lane.run();
        if let Some(source) = run.observation_selector_source() {
            let inventory = self.lane.actions().iter().any(|a|
                matches!(a.kind, K::UsePotion | K::DiscardPotion));
            out.insert(RuleBoundary {
                kind: if inventory { "run_selector_inventory" } else { "run_selector" },
                source: source.to_owned(),
            });
        }
        if let Some(id) = run.current_event_id().filter(|id| matches!(*id,
            "SAPPHIRE_SEED" | "JUNGLE_MAZE_ADVENTURE" | "TABLET_OF_TRUTH" |
            "WHISPERING_HOLLOW" | "LUMINOUS_CHOIR" | "SLIPPERY_BRIDGE" |
            "PUNCH_OFF" | "ENDLESS_CONVEYOR" | "THE_FUTURE_OF_POTIONS"))
        {
            out.insert(RuleBoundary { kind: "event_option_table", source: format!("event:{id}") });
        }
        if run.phase() == RunPhase::CardReward {
            for source in run.observation_extra_reward_sources() {
                out.insert(RuleBoundary { kind: "extra_reward", source: source.to_owned() });
            }
        }
        if let Some(combat) = run.combat().filter(|_| run.phase() == RunPhase::Combat) {
            if let Some(source) = crate::choice_context::combat_choice_source(combat) {
                out.insert(RuleBoundary { kind: "combat_choice_prompt", source });
            }
            for owner in combat.all_creatures() {
                for p in combat.cr(owner).power_list() {
                    let power = &combat.powers[p as usize];
                    if matches!(power.id,
                        PowerId::Automation | PowerId::Cacophony | PowerId::Feral |
                        PowerId::HardenedShell | PowerId::Monologue | PowerId::Orbit |
                        PowerId::PaleBlueDot | PowerId::Panache | PowerId::Sloth |
                        PowerId::Slow | PowerId::TagTeam | PowerId::Tender)
                    {
                        out.insert(RuleBoundary {
                            kind: "public_power_counter",
                            source: format!("power:{}", power.id.name()),
                        });
                    }
                }
            }
        }
        out.into_iter().collect()
    }
}

impl LaneSim for NativeBackend {
    fn default_saved_wongo_points() -> Option<i32> {
        sts2_sim::run::RunOptions::default().saved_wongo_points
    }
    fn new() -> Self {
        Self { lane: SimLane::reset(0, "IRONCLAD"), wire: WireBatch::zeroed(1),
            slots: ActionSlots::new(), last: Transition { reward: 0.0, done: false, fault: None },
            opened_rewards: std::collections::HashSet::new(), selector_source: None, proof_mode: false,
            saved_wongo_points: Self::default_saved_wongo_points() }
    }
    fn set_saved_wongo_points(&mut self, points: Option<i32>) -> Result<(), String> {
        self.saved_wongo_points = points;
        Ok(())
    }
    fn reset_device(&mut self, seed: u64, ascension: i32) {
        self.reset_with_options(seed, sts2_sim::run::RunOptions {
            ascension, saved_wongo_points: self.saved_wongo_points, ..Default::default() });
    }
    fn step_device(&mut self, action: i32) {
        if self.last.done { return; }
        if matches!(self.lane.run().phase(), RunPhase::Event | RunPhase::MapChoice | RunPhase::CardReward | RunPhase::RestSite | RunPhase::Shop | RunPhase::Treasure) {
            let slot = usize::try_from(action).ok();
            if self.lane.run().observation_selector_context().is_none() {
                self.selector_source = slot.and_then(|i| self.lane.actions().get(i)).and_then(|a| {
                    if a.kind == RunActionKind::EventChoice {
                        self.lane.run().current_event_id().map(|id| format!("event:{id}"))
                            .or_else(|| a.option_id.and_then(|id| self.lane.run().observation_neow_relic(id))
                                .map(|id| format!("relic:{}",id.name())))
                    } else if let Some(id) = a.relic_id {
                        Some(format!("relic:{}",id.name()))
                    } else if let Some(id) = a.card_id {
                        Some(format!("card:{}",id.name()))
                    } else { None }
                });
            }
            if let Some(binding) = slot.and_then(|i| self.lane.actions().get(i))
                .filter(|a| a.kind == RunActionKind::OpenReward) {
                if let Some(id) = binding.reward_index.and_then(|i| self.lane.run().observation_reward_card_identity(i)) {
                    self.opened_rewards.insert(id);
                }
            }
            self.last = match slot.filter(|&i| self.wire.model_mask.get(i) == Some(&1)) {
                Some(index) => self.lane.step(index),
                None => self.lane.refuse("illegal event candidate"),
            };
            if !self.last.done { self.refresh(); }
            return;
        }
        let raw = self.candidate_run_index(action);
        self.last = match raw {
            Some(index) => self.lane.step(index),
            None => self.lane.refuse("unrepresentable native candidate binding"),
        };
        self.slots.clear();
        if !self.last.done { self.refresh(); }
    }
    fn phase(&self) -> i32 { self.lane.run().phase() as i32 }
    fn outcome(&self) -> i32 { i32::from(self.last.fault.is_some()) }
    fn first_reason(&self) -> i32 {
        // Stable FNV-1a reason code; retain the full string to disambiguate
        // even a hash collision. Zero is reserved for no fault.
        self.last.fault.map_or(0, |s| {
            let hash=s.bytes().fold(2166136261u32, |h,b| (h ^ u32::from(b)).wrapping_mul(16777619));
            (hash & 0x7fff_ffff).max(1) as i32
        })
    }
    fn reason_text(&self) -> Option<&'static str> { self.last.fault }
    fn step_reward(&self) -> f32 { self.last.reward }
    fn is_done(&self) -> bool { self.last.done }
    fn is_terminated(&self) -> bool { self.last.done && self.last.fault.is_none() }
    fn is_truncated(&self) -> bool { self.last.fault.is_some() }
    fn wire_candidate_model(&self, slot: i32) -> i32 {
        if self.last.fault.is_some() { return 0; }
        usize::try_from(slot).ok().and_then(|i| self.wire.model_mask.get(i)).copied().unwrap_or(0) as i32
    }
    fn write_obs_and_mask(
        &self, state_kind_ids: &mut [u16], state_token_ids: &mut [u16], state_numeric_values: &mut [i32],
        state_numeric_scales: &mut [f32], state_numeric_tail: &mut [f32], state_mask: &mut [i8],
        public_act1_boss: &mut [f32], candidate_kind_ids: &mut [u16], candidate_token_ids: &mut [u16],
        candidate_numeric_values: &mut [i32], candidate_numeric_scales: &mut [f32],
        candidate_numeric_tail: &mut [f32], choice_purpose_ids: &mut [u16], model_mask: &mut [i8],
        source_applicable: &mut [i8], target_applicable: &mut [i8], source_known: &mut [i8],
        target_known: &mut [i8], source_links_bits: &mut [u8], target_links_bits: &mut [u8],
        legal_mask: &mut [bool],
    ) {
        assert!(self.last.fault.is_none());
        macro_rules! copy { ($($dst:ident => $src:ident),*) => { $(
            $dst.copy_from_slice(&self.wire.$src);
        )* }; }
        copy!(state_kind_ids => state_kind, state_token_ids => state_tokens,
            state_numeric_values => state_values, state_numeric_scales => state_scales,
            state_numeric_tail => state_tail, state_mask => state_mask,
            public_act1_boss => boss, candidate_kind_ids => cand_kind,
            candidate_token_ids => cand_tokens, candidate_numeric_values => cand_values,
            candidate_numeric_scales => cand_scales, candidate_numeric_tail => cand_tail,
            choice_purpose_ids => choice, model_mask => model_mask,
            source_applicable => source_applicable, target_applicable => target_applicable,
            source_known => source_known, target_known => target_known,
            source_links_bits => source_bits, target_links_bits => target_bits, legal_mask => legal);
    }
}

#[cfg(test)]
mod wongo_profile_tests {
    use super::*;

    #[test]
    fn poisoned_lane_replacement_keeps_explicit_saved_points_for_next_episode() {
        let mut env = crate::VecEnv::<NativeBackend>::new(1, 1, 4100000, false).unwrap();
        assert_eq!(env.lanes[0].saved_wongo_points,Some(0));
        assert_eq!(env.lanes[0].sim.saved_wongo_points,Some(0));
        env.set_saved_wongo_points(Some(0)).unwrap();
        // Same reconstruction used after a refusal and after a reset failure.
        env.lanes[0].sim = NativeBackend::new();
        crate::reset_once(&mut env.lanes[0], 4100000).unwrap();
        assert_eq!(env.lanes[0].sim.saved_wongo_points, Some(0));
        env.set_saved_wongo_points(Some(1984)).unwrap();
        env.lanes[0].sim = NativeBackend::new();
        crate::reset_once(&mut env.lanes[0], 4100001).unwrap();
        assert_eq!(env.lanes[0].sim.saved_wongo_points, Some(1984));
    }
}

#[cfg(feature = "selector-proof")]
pub struct ProofBackend(NativeBackend);

#[cfg(feature = "selector-proof")]
impl ProofBackend {
    pub fn reset_proof_character_with_options(&mut self, seed:u64, character:&str,
        options:sts2_sim::run::RunOptions) {
        self.0.reset_character_with_options_mode(seed,character,options,true);
    }
    pub fn observation(&self) -> &WireBatch { self.0.observation() }
    pub fn run(&self) -> &sts2_sim::run::RunManager { self.0.run() }
    pub fn actions(&self) -> &[sts2_sim::run::RunAction] { self.0.actions() }
    /// Read-only policy binding for connected original-DLL input plans.
    pub fn candidate_binding(&self, slot:usize) -> Option<sts2_sim::obs::ActionSlot> {
        self.0.slots.get(slot)
    }
    pub fn reset_proof_with_options(&mut self, seed:u64, options:sts2_sim::run::RunOptions) {
        self.0.reset_proof_with_options(seed,options);
    }
    pub fn rule_boundaries(&self) -> Vec<RuleBoundary> { self.0.rule_boundaries() }
    pub fn fault_reason(&self) -> Option<&'static str> { self.0.fault_reason() }
    pub fn reset_proof_inputs(&mut self, seed:u64, ascension:i32, int1_training_reward_pools:bool) {
        self.0.reset_proof_with_options(seed, sts2_sim::run::RunOptions {
            ascension, int1_training_reward_pools, ..Default::default()
        });
    }
}

#[cfg(feature = "selector-proof")]
impl LaneSim for ProofBackend {
    fn new() -> Self { Self(NativeBackend::new()) }
    fn reset_device(&mut self, seed: u64, ascension: i32) {
        self.0.reset_proof_with_options(seed,
            sts2_sim::run::RunOptions { ascension, ..Default::default() });
    }
    fn step_device(&mut self, action: i32) { self.0.step_device(action); }
    fn phase(&self) -> i32 { self.0.phase() }
    fn outcome(&self) -> i32 { self.0.outcome() }
    fn first_reason(&self) -> i32 { self.0.first_reason() }
    fn reason_text(&self) -> Option<&'static str> { self.0.reason_text() }
    fn step_reward(&self) -> f32 { self.0.step_reward() }
    fn is_done(&self) -> bool { self.0.is_done() }
    fn is_terminated(&self) -> bool { self.0.is_terminated() }
    fn is_truncated(&self) -> bool { self.0.is_truncated() }
    fn wire_candidate_model(&self, cand_idx: i32) -> i32 { self.0.wire_candidate_model(cand_idx) }
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
    ) {
        self.0.write_obs_and_mask(state_kind_ids,state_token_ids,state_numeric_values,
            state_numeric_scales,state_numeric_tail,state_mask,public_act1_boss,candidate_kind_ids,
            candidate_token_ids,candidate_numeric_values,candidate_numeric_scales,candidate_numeric_tail,
            choice_purpose_ids,model_mask,source_applicable,target_applicable,source_known,target_known,
            source_links_bits,target_links_bits,legal_mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}};
    #[test]
    fn model_candidates_bind_through_run_actions() {
        for lane in 0..16u64 {
            let mut sim=NativeBackend::new();
            let mut seed=100+lane;
            let mut rng=42+lane;
            sim.reset_device(seed,10);
            for decision in 0..12500 {
                let actions:Vec<_>=(0..256).filter(|&i| sim.wire_candidate_model(i)==1).collect();
                assert!(!actions.is_empty());
                rng^=rng<<13; rng^=rng>>7; rng^=rng<<17;
                let slot=actions[rng as usize%actions.len()];
                let binding=sim.slots.get(slot as usize);
                let run_actions=sim.actions().to_vec();
                sim.step_device(slot);
                assert_ne!(sim.fault_reason(),Some("unrepresentable native candidate binding"),
                    "seed={seed} decision={decision} binding={binding:?} actions={run_actions:?}");
                if sim.is_done() { seed+=16; sim.reset_device(seed,10); }
            }
        }
    }
    #[test]
    fn potion_enemy_binding_never_confuses_osty_id_with_enemy_index() {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.summon_osty(0, 20).unwrap();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(7));
        let enemy = c.enemies[0];
        let osty = c.osty(0).unwrap();
        assert_ne!(enemy, osty);
        assert_eq!(enemy_target_index(&c, Some(enemy)), Some(Some(0)));
        assert_eq!(enemy_target_index(&c, None), Some(None));
        assert_eq!(enemy_target_index(&c, Some(osty)), None, "pet targeting must not silently hit an enemy");
    }
}
