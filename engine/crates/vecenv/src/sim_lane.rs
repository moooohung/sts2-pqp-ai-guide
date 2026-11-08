//! Native run actions and the captured int1 Episode212 reward contract.
//! These action indices are run-action indices, NOT policy candidate slots.
use sts2_sim::run::{RunAction, RunManager, RunOptions, RunPhase};
use sts2_sim::obs::{ActionSlots, CombatObservationContext, ObsBuffers, ObsError};
use sts2_wire::WireBatch;
pub use crate::choice_context::combat_choice_prompt;
#[cfg(feature = "selector-proof")]
pub use crate::choice_context::combat_choice_prompt_proof;
#[cfg(feature="selector-proof")]
pub use crate::choice_context::combat_choice_prompt_proof_for_player;

/// Pure reward input; all arithmetic remains f64 until publication.
#[derive(Clone, Copy, Debug)]
pub struct RewardState {
    pub potential: f64,
    pub reset_floor: i32,
    pub highest_floor: i32,
}

#[derive(Clone, Copy, Debug)]
pub enum End {
    Ongoing,
    Victory,
    Death,
    /// Episode212 base/step/no-progress limit: failure, not simulator fault.
    Timeout,
    /// Explicit x.truncated (not the timeout-as-failure branch).
    Truncated,
}

/// Captured mode=1, dense=1, objective=1, gamma=1, paid_credit=0.
/// Source: funcs_30.rs::episode_action212; see docs/vecenv_sim.md.
pub fn reward(previous: RewardState, floor: i32, potential: f64, end: End)
    -> (f64, RewardState)
{
    let highest_floor = previous.highest_floor.max(floor);
    let progress = ((highest_floor - previous.reset_floor) as f64 / 51.0).clamp(0.0, 1.0);
    let dense = potential - previous.potential;
    let value = match end {
        End::Ongoing => dense,
        // Preserve the ledger's floating-point operation order.
        End::Victory => ((1.0 - previous.potential) + 1.0) - 1.0,
        End::Death => ((-1.0 - previous.potential) + (-1.0 + 1.5 * progress)) - (-1.0),
        End::Timeout => ((-1.0 - previous.potential) + (-1.0)) - (-1.0),
        End::Truncated => -1.0,
    };
    (value, RewardState { potential, reset_floor: previous.reset_floor, highest_floor })
}

/// Episode212 potential using current enemy max HP, HP and escape flags.
pub fn potential(run: &RunManager) -> f64 {
    if run.phase() == RunPhase::RunOver { return 0.0; }
    let ctx = run.run_ctx();
    let combat = run.combat().filter(|_| run.phase() == RunPhase::Combat);
    let (hp, max_hp) = combat.map_or((ctx.hp, ctx.max_hp),
        |c| (c.player().hp, c.player().max_hp));
    let mut phi = 0.5 * (ctx.total_floor.max(0) as f64 / 51.0).min(1.0);
    if max_hp > 0 { phi += 0.2 * hp.clamp(0, max_hp) as f64 / max_hp as f64; }
    if run.phase() == RunPhase::Combat {
        if let Some(c) = run.combat() {
            let (mut total, mut removed) = (0i64, 0i64);
            for &enemy in &c.enemies {
                let cr = c.cr(enemy);
                if cr.max_hp > 0 {
                    let hp = if cr.hp > 0 && !cr.escaped { cr.hp } else { 0 };
                    total += cr.max_hp as i64;
                    removed += (cr.max_hp - hp.min(cr.max_hp)) as i64;
                }
            }
            if total > 0 { phi += 0.15 * removed as f64 / total as f64; }
        }
    }
    phi
}

#[derive(Debug, PartialEq)]
pub struct Transition {
    pub reward: f32,
    pub done: bool,
    pub fault: Option<&'static str>,
}

/// Solo Ironclad native lane. Wire encoding/candidate reordering is separate.
#[derive(Clone)]
pub struct SimLane {
    run: RunManager,
    actions: Vec<RunAction>,
    ledger: RewardState,
    decisions: usize,
    no_progress: usize,
    fault: Option<&'static str>,
    pub(crate) teacher_bindings: Vec<(usize,usize)>,
}

fn run_fault(run: &RunManager) -> Option<&'static str> {
    // RunManager::resolve_neow_choice clears this decision marker before
    // applying pickup hooks. Do not suppress any actual pickup failure.
    match run.unported {
        Some("EVENT:Neow") if run.phase() == RunPhase::Event
            && run.current_act_index() == 0 && run.run_ctx().total_floor == 1
            && run.current_event_id().is_none() => None,
        reason => reason,
    }
}

impl SimLane {
    pub fn reset(seed: u64, character: &str) -> Self {
        Self::reset_with_options(seed, character, RunOptions::default())
    }

    pub fn reset_with_options(seed: u64, character: &str, options: RunOptions) -> Self {
        let model=match character {
            "SILENT" => sts2_sim::run::mp::Character::Silent,
            "DEFECT" => sts2_sim::run::mp::Character::Defect,
            "REGENT" => sts2_sim::run::mp::Character::Regent,
            "NECROBINDER" => sts2_sim::run::mp::Character::Necrobinder,
            _ => sts2_sim::run::mp::Character::Ironclad,
        };
        let mut run = RunManager::new_for_character(seed, options,model);
        let fault = if !matches!(character,"IRONCLAD"|"SILENT"|"DEFECT"|"REGENT"|"NECROBINDER") {
            Some("native run character is not supported")
        } else { run_fault(&run) };
        Self::from_run_with_fault(run,fault)
    }

    /// Construct a lane from the ordinary run reducer, including C# debug act entry.
    pub fn from_run(run:RunManager)->Self {
        let fault=run_fault(&run);
        Self::from_run_with_fault(run,fault)
    }
    fn from_run_with_fault(mut run:RunManager,fault:Option<&'static str>)->Self {
        let actions = run.available_actions().into_iter().collect();
        let floor = run.run_ctx().total_floor;
        let ledger = RewardState { potential: potential(&run), reset_floor: floor, highest_floor: floor };
        Self { run, actions, ledger, decisions: 0, no_progress: 0, fault, teacher_bindings:vec![] }
    }

    pub fn run(&self) -> &RunManager { &self.run }
    pub fn fault(&self) -> Option<&'static str> { self.fault }
    pub fn actions(&self) -> &[RunAction] { &self.actions }
    pub fn legal_mask(&self) -> Vec<bool> {
        self.actions.iter().map(|a| self.fault.is_none() && a.enabled).collect()
    }

    /// No guessed noncombat rows: an unsupported phase quarantines the lane.
    /// Returned action slots are the encoder's bindings, not raw run indices.
    pub fn encode(
        &mut self, context: &CombatObservationContext<'_>,
    ) -> Result<(WireBatch, ActionSlots), ObsError> {
        self.encode_inner(context, false)
    }

    #[cfg(feature = "selector-proof")]
    pub fn encode_proof(
        &mut self, context: &CombatObservationContext<'_>,
    ) -> Result<(WireBatch, ActionSlots), ObsError> {
        self.encode_inner(context, true)
    }

    fn encode_inner(
        &mut self, context: &CombatObservationContext<'_>, proof: bool,
    ) -> Result<(WireBatch, ActionSlots), ObsError> {
        self.encode_mode(context,proof,false)
    }
    /// Native legal support, using existing wire row schemas and exact run
    /// bindings. This does not alter frozen-policy comparison/default support.
    pub(crate) fn encode_teacher(&mut self,context:&CombatObservationContext<'_>)
        ->Result<(WireBatch,ActionSlots),ObsError> {
        self.encode_mode(context,false,true)
    }
    fn encode_mode(&mut self,context:&CombatObservationContext<'_>,proof:bool,teacher:bool)
        ->Result<(WireBatch,ActionSlots),ObsError> {
        self.teacher_bindings.clear();
        if let Some(reason) = self.fault {
            return Err(ObsError::UnsupportedState(reason));
        }
        let mut w = WireBatch::zeroed(1);
        let mut slots = ActionSlots::new();
        let mut buffers = ObsBuffers::new(
            &mut w.state_kind, &mut w.state_tokens, &mut w.state_values,
            &mut w.state_scales, &mut w.state_tail, &mut w.state_mask,
            &mut w.cand_kind, &mut w.cand_tokens, &mut w.cand_values,
            &mut w.cand_scales, &mut w.cand_tail, &mut w.choice,
            &mut w.model_mask, &mut w.source_applicable, &mut w.target_applicable,
            &mut w.source_known, &mut w.target_known, &mut w.source_bits,
            &mut w.target_bits, &mut w.legal, &mut w.boss,
        )?;
        let phase = self.run.phase();
        if teacher && phase==RunPhase::Combat {
            let combat=self.run.combat().ok_or(ObsError::UnsupportedState("combat phase without combat"))?;
            for (slot,potion) in self.run.run_ctx().potions.iter().enumerate() {
                if let Some(id)=potion.filter(|_|combat.can_discard_potion_for_player(0,slot)) {
                    let mut action=sts2_sim::run::RunAction::new(sts2_sim::run::RunActionKind::DiscardPotion);
                    action.slot_index=Some(slot);
                    action.potion_id=Some(id);
                    self.actions.push(action);
                }
            }
        }
        // Pael's Tooth/Growth keep the event current while their deck selector
        // is open; that screen is a run card choice, not an event option table.
        let encoded = if phase == RunPhase::Event && self.run.current_event_id().is_some()
            && self.run.observation_ordered_run_choice().is_none() {
            sts2_sim::obs::write_run_state(&self.run, context, &mut buffers)
                .and_then(|()| {
                    if proof {
                        #[cfg(feature = "selector-proof")]
                        { return crate::event_semantics::write_event_proof(&self.run, &self.actions, &mut buffers); }
                    }
                    crate::event_semantics::write(&self.run, &self.actions, &mut buffers)
                })
        } else if proof && matches!(phase,
            RunPhase::Event | RunPhase::MapChoice | RunPhase::CardReward |
            RunPhase::RestSite | RunPhase::Shop | RunPhase::Treasure)
        {
            #[cfg(feature = "selector-proof")]
            { sts2_sim::obs::write_selector_proof_obs(
                &self.run, &self.actions, context, &mut buffers, &mut slots).map(|_| ()) }
            #[cfg(not(feature = "selector-proof"))]
            { unreachable!() }
        } else if matches!(phase, RunPhase::CardReward | RunPhase::RestSite | RunPhase::Shop | RunPhase::Treasure) {
            sts2_sim::obs::write_neow_obs(&self.run, &self.actions, context, &mut buffers, &mut slots)
        } else {
            if proof {
                #[cfg(feature = "selector-proof")]
                { self.run.write_combat_proof_observation(context, &mut buffers, &mut slots) }
                #[cfg(not(feature = "selector-proof"))]
                { unreachable!() }
            } else {
                self.run.write_combat_observation(context, &mut buffers, &mut slots)
            }
        };
        if let Err(error) = encoded {
            let reason = match error {
                ObsError::UnsupportedState(reason) => reason,
                _ => "native observation encoding failed",
            };
            self.refuse(reason);
            return Err(error);
        }
        if teacher && phase==RunPhase::Combat {
            for (index,action) in self.actions.iter().enumerate().filter(|(_,a)|
                a.enabled && a.kind==sts2_sim::run::RunActionKind::DiscardPotion) {
                let slot=buffers.push_candidate()?;
                sts2_sim::obs::write_run_potion_candidate(&self.run,action,slot,&mut buffers)?;
                self.teacher_bindings.push((slot,index));
            }
        }
        Ok((w, slots))
    }

    pub(crate) fn refuse(&mut self, reason: &'static str) -> Transition {
        self.fault = Some(reason);
        self.actions.clear();
        Transition { reward: 0.0, done: true, fault: self.fault }
    }

    pub fn step(&mut self, action: usize) -> Transition {
        if let Some(reason) = self.fault { return self.refuse(reason); }
        if self.run.phase() == RunPhase::RunOver {
            return self.refuse("step after episode completion");
        }
        let before = match crate::progress::signature(&self.run) {
            Ok(key)=>key, Err(reason)=>return self.refuse(reason),
        };
        let Some(binding) = self.actions.get(action).copied().filter(|a| a.enabled) else {
            return self.refuse("illegal native run action");
        };
        if let Err(reason) = self.run.take_action(binding) { return self.refuse(reason); }
        self.decisions += 1;
        if let Some(reason) = run_fault(&self.run).or_else(|| self.run.combat().and_then(|c| c.fault)) {
            return self.refuse(reason);
        }
        let after=match crate::progress::signature(&self.run) {
            Ok(key)=>key, Err(reason)=>return self.refuse(reason),
        };
        let limit = crate::progress::update(&before, &after,
            &mut self.no_progress,self.decisions);
        let end = if self.run.observation_full_run_victory() { End::Victory }
            else if self.run.phase() != RunPhase::RunOver { if limit { End::Timeout } else { End::Ongoing } }
            else if self.run.run_ctx().hp <= 0 { End::Death }
            else {
                // A positive-HP RunOver can also be an event/scripted end.
                // Do not assume that every such end is full-run victory.
                return self.refuse("native terminal victory classification is unported");
            };
        let (value, ledger) = reward(self.ledger, self.run.run_ctx().total_floor, potential(&self.run), end);
        self.ledger = ledger;
        self.actions = self.run.available_actions().into_iter().collect();
        if matches!(end, End::Ongoing) && !self.actions.iter().any(|a| a.enabled) {
            return self.refuse("native run has no legal action");
        }
        Transition { reward: value as f32, done: !matches!(end, End::Ongoing), fault: None }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decision_300_with_gameplay_progress_is_not_quarantined() {
        let mut lane = super::SimLane::reset(3000040, "IRONCLAD");
        lane.decisions = 299;
        let result = lane.step(0);
        assert_eq!(result.fault,None);
        assert!(!result.done);
        assert_eq!(lane.no_progress,0);
    }
    use super::*;

    #[test]
    fn potential_ledger_matches_cited_formula() {
        let before = RewardState { potential: 0.25, reset_floor: 1, highest_floor: 1 };
        assert_eq!(reward(before, 2, 0.375, End::Ongoing).0, 0.125);
        assert_eq!(reward(before, 52, 0.0, End::Victory).0, 0.75);
        // Progress=(18-1)/51=1/3; target=-1+1.5/3=-0.5.
        assert_eq!(reward(before, 18, 0.0, End::Death).0, -0.75);
        assert_eq!(reward(before, 18, 0.0, End::Timeout).0, -1.25);
        assert_eq!(reward(before, 18, 0.0, End::Truncated).0, -1.0);
    }

    #[test]
    fn native_neow_decision_dispatches_and_real_faults_are_sticky() {
        let mut a = SimLane::reset(3000040, "IRONCLAD");
        let mut b = SimLane::reset(3000040, "IRONCLAD");
        assert_eq!(a.legal_mask(), b.legal_mask());
        assert_eq!(a.actions(), b.actions());
        assert_eq!(a.fault(), None);
        assert_eq!(a.legal_mask(), vec![true; 3]);
        let slot = a.actions().iter().position(|x| x.label == Some("Fishing Rod")).unwrap();
        assert_eq!(a.step(slot), b.step(slot));
        assert_eq!(a.fault(), None);
        assert_eq!(a.run().phase(), RunPhase::MapChoice);
        assert_eq!(a.run().unported, None);
        assert_eq!(a.actions(), b.actions());
        assert!(a.step(usize::MAX).fault.is_some());
        assert!(a.legal_mask().iter().all(|&x| !x));
        assert_eq!(a.step(0).reward, 0.0);
        assert!(SimLane::reset(100, "SILENT").fault().is_none());
        assert!(SimLane::reset(100, "DEFECT").fault().is_none());
        assert!(SimLane::reset(100, "NECROBINDER").fault().is_none());
        assert!(SimLane::reset(100, "UNKNOWN").fault().is_some());
    }

    #[test]
    fn non_neow_unported_reason_is_never_suppressed() {
        let mut lane = SimLane::reset(3000040, "IRONCLAD");
        lane.run.unported = Some("test pickup fault");
        assert_eq!(run_fault(&lane.run), Some("test pickup fault"));
    }

    #[test]
    fn neow_and_map_encode_with_raw_candidate_order() {
        let context = CombatObservationContext {
            public_act1_boss: [0.0; 6], choice_prompt: "",
        };
        let mut event = SimLane::reset(3000040, "IRONCLAD");
        let (wire, _) = event.encode(&context).unwrap();
        assert_eq!(wire.model_mask.iter().filter(|&&x| x == 1).count(), 3);
        assert_eq!(event.fault(), None);
        let mut map = SimLane::reset(3000040, "IRONCLAD");
        let pick = map.actions().iter().position(|x| x.label == Some("Fishing Rod")).unwrap();
        assert_eq!(map.step(pick).fault, None);
        let (wire, _) = map.encode(&context).unwrap();
        assert_eq!(wire.legal.iter().filter(|&&x| x).count(), map.actions().len());
        assert_eq!(map.fault(), None);
    }

    #[test]
    fn live_combat_wire_has_bound_legal_candidates() {
        use sts2_sim::run::RunActionKind;
        let mut lane = SimLane::reset(3000040, "IRONCLAD");
        let neow = lane.actions().iter().position(|x| x.label == Some("Fishing Rod")).unwrap();
        assert_eq!(lane.step(neow).fault, None);
        let movement = lane.actions().iter().position(|a| a.kind == RunActionKind::Move).unwrap();
        assert_eq!(lane.step(movement).fault, None);
        assert_eq!(lane.run().phase(), RunPhase::Combat);
        // Caller-owned context is explicit. This test checks forwarding, not
        // production public-boss discovery (not yet connected).
        let context = CombatObservationContext {
            public_act1_boss: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], choice_prompt: "",
        };
        let (wire, slots) = lane.encode(&context).unwrap();
        assert_eq!(wire.boss, context.public_act1_boss);
        assert!(wire.state_mask.iter().any(|&x| x == 1));
        assert!(!slots.is_empty());
        assert_eq!(wire.legal.iter().filter(|&&x| x).count(), slots.len());
        assert!((0..slots.len()).all(|i| slots.get(i).unwrap().actor == 0));
    }
}

