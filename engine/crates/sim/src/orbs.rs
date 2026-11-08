//! Defect orbs (C# `Core/Models/Orbs/*.cs`, `Core/Entities/Orbs/OrbQueue.cs`, `Commands/OrbCmd.cs`).
//!
//! The queue is fixed-size inline storage on the combat (no allocation). Every player has a
//! queue; Ironclad's starts at 0 slots, and `channel` grants the first slot like
//! `OrbCmd.Channel` does for characters with `BaseOrbSlotCount == 0` (r7 lacks this rule and
//! drops the orb; see `tests/csharp_divergences.txt`).

use crate::card::CId;
use crate::combat::{Combat, Props};
use crate::tables::PowerId;
use arrayvec::ArrayVec;

pub const MAX_ORB_SLOTS: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrbKind { Lightning, Frost, Dark, Plasma, Glass }

impl OrbKind {
    /// r7 `list(OrbType)` order, used by random channels.
    pub const ALL: [OrbKind; 5] = [OrbKind::Lightning, OrbKind::Frost, OrbKind::Dark, OrbKind::Plasma, OrbKind::Glass];

    pub fn name(self) -> &'static str {
        match self {
            OrbKind::Lightning => "LIGHTNING",
            OrbKind::Frost => "FROST",
            OrbKind::Dark => "DARK",
            OrbKind::Plasma => "PLASMA",
            OrbKind::Glass => "GLASS",
        }
    }
}

/// One channeled orb. `val` is the orb's own mutable field: Dark `_evokeVal` (starts 6),
/// Glass `_passiveVal` (starts 4); unused by the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orb {
    pub kind: OrbKind,
    pub val: i32,
    pub instance_id: u64,
}

impl Orb {
    pub fn new(kind: OrbKind) -> Orb {
        let val = match kind { OrbKind::Dark => 6, OrbKind::Glass => 4, _ => 0 };
        Orb { kind, val, instance_id: 0 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OrbQueue {
    pub capacity: u8,
    /// The character's `BaseOrbSlotCount` (0 for Ironclad).
    pub base_slots: u8,
    pub orbs: ArrayVec<Orb, { MAX_ORB_SLOTS as usize }>,
}

impl OrbQueue {
    /// Combat setup (`PlayerCombatState`: `OrbQueue.Clear(); AddCapacity(player.BaseOrbSlotCount)`).
    pub fn open_base_slots(&mut self, base: i32) {
        let n = base.clamp(0, MAX_ORB_SLOTS as i32) as u8;
        self.base_slots = n;
        self.capacity = n;
        self.orbs.clear();
    }
}

impl Combat {
    /// Focus-modified value (`FocusPower.ModifyOrbValue`: `max(value + focus, 0)`).
    fn orb_focus(&self, owner: CId, base: i32) -> i32 {
        (base + self.power_amount(owner, PowerId::Focus)).max(0)
    }

    /// The orb's passive value for its owner (`OrbModel.PassiveVal` through `Hook.ModifyOrbValue`).
    pub fn orb_passive_value(&self, owner: CId, orb: Orb) -> i32 {
        match orb.kind {
            OrbKind::Lightning => self.lightning_value(owner, 3),
            OrbKind::Frost => self.orb_focus(owner, 2),
            OrbKind::Dark => self.orb_focus(owner, 6),
            OrbKind::Plasma => 1,
            OrbKind::Glass => self.orb_focus(owner, orb.val),
        }
    }

    pub fn orb_evoke_value(&self, owner: CId, orb: Orb) -> i32 {
        match orb.kind {
            OrbKind::Lightning => self.lightning_value(owner, 8),
            OrbKind::Frost => self.orb_focus(owner, 5),
            OrbKind::Dark => orb.val.max(0),
            OrbKind::Plasma => 2,
            OrbKind::Glass => self.orb_passive_value(owner, orb) * 2,
        }
    }

    fn lightning_value(&self, owner: CId, base: i32) -> i32 {
        // CombatState.IterateHookListeners: Owner powers before Owner relics.
        // InfusedCore.ModifyOrbValue adds one AFTER Focus's nonnegative clamp.
        let bonus = self.run.relics.iter().filter(|r| r.owner == owner
            && r.enabled && self.players[owner].hooks_active
            && r.id == crate::tables::RelicId::InfusedCore).count() as i32;
        self.orb_focus(owner, base) + bonus
    }

    /// `OrbCmd.AddSlots`.
    pub fn add_orb_slots(&mut self, owner: CId, amount: i32) {
        if self.is_over_or_ending() || amount <= 0 {
            return;
        }
        let q = &mut self.players[owner].orbs;
        q.capacity = (q.capacity as i32 + amount).min(MAX_ORB_SLOTS as i32) as u8;
    }

    /// `OrbCmd.RemoveSlots`: `OrbQueue.RemoveCapacity` drops the overflow from the back
    /// (Entities/Orbs/OrbQueue.cs; r7's Bulk Up keeps the orbs instead).
    pub fn remove_orb_slots(&mut self, owner: CId, amount: i32) {
        if self.is_over_or_ending() || amount <= 0 {
            return;
        }
        let q = &mut self.players[owner].orbs;
        q.capacity = (q.capacity as i32 - amount).max(0) as u8;
        q.orbs.truncate(q.capacity as usize);
    }

    /// `OrbCmd.Channel`.
    pub fn channel_orb(&mut self, owner: CId, kind: OrbKind) {
        if self.is_over_or_ending() {
            return;
        }
        let q = &self.players[owner].orbs;
        if q.base_slots == 0 && q.capacity == 0 {
            self.add_orb_slots(owner, 1);
        }
        if self.players[owner].orbs.capacity == 0 {
            return;
        }
        let q = &self.players[owner].orbs;
        if q.orbs.len() >= q.capacity as usize {
            self.evoke_front_orb(owner);
        }
        if self.players[owner].orbs.orbs.len() < self.players[owner].orbs.capacity as usize {
            let mut orb = Orb::new(kind);
            if self.orb_recording {
                self.next_orb_instance += 1;
                orb.instance_id = self.next_orb_instance;
            }
            self.players[owner].orbs.orbs.push(orb);
            let _activation = self.record_orb_activation(owner, orb, "channel", false);
            self.hist.orbs_channeled += 1;
            if kind == OrbKind::Lightning {
                self.hist.lightning_channeled += 1;
                self.hist.public_lightning_channels[usize::from(owner)] += 1;
            }
            self.relics_after_orb_channeled(owner);
        }
    }

    /// r7 `channel_random_orb`: one `combat_orbs` draw over `OrbKind::ALL`.
    pub fn channel_random_orb(&mut self, owner: CId) {
        let i = self.run.rngs.combat_orbs.choice_index(OrbKind::ALL.len());
        self.channel_orb(owner, OrbKind::ALL[i]);
    }

    pub fn evoke_front_orb(&mut self, owner: CId) {
        if self.is_over_or_ending() || self.players[owner].orbs.orbs.is_empty() {
            return;
        }
        let orb = self.players[owner].orbs.orbs.remove(0);
        self.evoke_orb(owner, orb, true);
    }

    /// `OrbCmd.EvokeNext(dequeue: false)`: the front orb's evoke and hooks; it stays queued.
    pub fn evoke_front_orb_keep(&mut self, owner: CId) {
        if self.is_over_or_ending() {
            return;
        }
        if let Some(&orb) = self.players[owner].orbs.orbs.first() {
            self.evoke_orb(owner, orb, false);
        }
    }

    /// One passive of the orb at queue position `i` (Darkness, Loop).
    pub fn orb_passive_at(&mut self, owner: CId, i: usize) {
        if i < self.players[owner].orbs.orbs.len() {
            self.orb_passive(owner, i);
        }
    }

    pub fn evoke_last_orb(&mut self, owner: CId) {
        if self.is_over_or_ending() {
            return;
        }
        if let Some(orb) = self.players[owner].orbs.orbs.pop() {
            self.evoke_orb(owner, orb, true);
        }
    }

    pub fn evoke_all_orbs(&mut self, owner: CId) {
        while !self.players[owner].orbs.orbs.is_empty() && !self.is_over_or_ending() {
            self.evoke_front_orb(owner);
        }
    }

    /// Orb `Evoke` body, then `Hook.AfterOrbEvoked` (Thunder).
    fn evoke_orb(&mut self, owner: CId, orb: Orb, dequeued: bool) -> ArrayVec<CId, 8> {
        let value = self.orb_evoke_value(owner, orb);
        let mut targets: ArrayVec<CId, 8> = ArrayVec::new();
        let activation = self.record_orb_activation(owner, orb, "evoke", dequeued);
        match orb.kind {
            OrbKind::Lightning => {
                if value > 0 && let Some(t) = self.orb_random_target() {
                    activation.target(t);
                    self.apply_damage(t, value, Props::UNPOWERED, Some(owner));
                    targets.push(t);
                }
            }
            OrbKind::Frost => {
                activation.target(owner);
                if value > 0 {
                    self.power_gain_unpowered_block(owner, value);
                }
                // C# FrostOrb.Evoke: Hibernate shares block with every other player.
                if self.find_power(owner, PowerId::Hibernate).is_some() {
                    for player in 0..self.players.len() as CId {
                        if player != owner {
                            activation.target(player);
                            if value > 0 {
                                self.power_gain_unpowered_block(player, value);
                            }
                        }
                    }
                    // FrostOrb.Evoke returns CombatState.Players in seat order,
                    // although its GainBlock commands run for the owner first.
                    targets.extend(0..self.players.len() as CId);
                } else {
                    targets.push(owner);
                }
            }
            OrbKind::Dark => {
                let hittable = self.hittable_enemies();
                // `MinBy(CurrentHp)`: the first enemy with the lowest HP.
                if value > 0 && let Some(&t) = hittable.iter().min_by_key(|&&e| self.cr(e).hp) {
                    activation.target(t);
                    self.apply_damage(t, value, Props::UNPOWERED, Some(owner));
                    targets.push(t);
                }
            }
            OrbKind::Plasma => {
                activation.target(owner);
                self.players[owner].energy += value;
                targets.push(owner);
            }
            OrbKind::Glass => {
                if value > 0 {
                    let selected = self.hittable_enemies();
                    activation.targets(&selected);
                    for t in selected {
                        self.apply_damage(t, value, Props::UNPOWERED, Some(owner));
                        targets.push(t);
                    }
                }
            }
        }
        self.after_orb_evoked(owner, orb.kind, &targets);
        targets
    }

    fn orb_random_target(&mut self) -> Option<CId> {
        let hittable = self.hittable_enemies();
        if hittable.is_empty() {
            return None;
        }
        Some(hittable[self.run.rngs.combat_targets.choice_index(hittable.len())])
    }

    /// Orb `Passive` body.
    fn orb_passive(&mut self, owner: CId, i: usize) {
        // GoldPlatedCables.ModifyOrbPassiveTriggerCounts / OrbModel.TriggerPassive:
        // the front orb triggers once extra, for every passive entry point.
        let extra = if i == 0 { self.run.relics.iter().filter(|r| r.owner == owner
            && r.enabled && self.players[owner].hooks_active
            && r.id == crate::tables::RelicId::GoldPlatedCables).count() } else { 0 };
        for _ in 0..1 + extra {
            // The preceding passive can end combat and clear the owner's queue.
            if self.players[owner].orbs.orbs.get(i).is_none() { return; }
            self.orb_passive_once(owner, i);
        }
    }

    fn orb_passive_once(&mut self, owner: CId, i: usize) {
        let orb = self.players[owner].orbs.orbs[i];
        let value = self.orb_passive_value(owner, orb);
        let activation = self.record_orb_activation(owner, orb, "passive", false);
        match orb.kind {
            OrbKind::Lightning => {
                if value > 0 && let Some(t) = self.orb_random_target() {
                    activation.target(t);
                    self.apply_damage(t, value, Props::UNPOWERED, Some(owner));
                }
            }
            OrbKind::Frost => {
                activation.target(owner);
                if value > 0 {
                    self.power_gain_unpowered_block(owner, value);
                    // C# FrostOrb.Passive: Hibernate shares the focused passive value.
                    if self.find_power(owner, PowerId::Hibernate).is_some() {
                        for player in 0..self.players.len() as CId {
                            if player != owner {
                                activation.target(player);
                                self.power_gain_unpowered_block(player, value);
                            }
                        }
                    }
                }
            }
            OrbKind::Dark => {
                if let Some(o) = self.players[owner].orbs.orbs.get_mut(i) {
                    o.val += value;
                }
            }
            OrbKind::Plasma => {
                activation.target(owner);
                self.players[owner].energy += value;
            }
            // GlassOrb.Passive: decays only when the (focused) passive value is positive.
            OrbKind::Glass => {
                if value > 0 {
                    if let Some(o) = self.players[owner].orbs.orbs.get_mut(i) {
                        o.val = (o.val - 1).max(0);
                    }
                    let selected = self.hittable_enemies();
                    activation.targets(&selected);
                    for t in selected {
                        self.apply_damage(t, value, Props::UNPOWERED, Some(owner));
                    }
                }
            }
        }
    }

    /// `BeforeTurnEndOrbTrigger` (all but Plasma) or `AfterTurnStartOrbTrigger` (Plasma), over a
    /// snapshot of the owner's queue like r7's `list(player.orbs)`.
    pub fn trigger_orb_passives(&mut self, owner: CId, turn_start: bool) {
        let n = self.players[owner].orbs.orbs.len();
        for i in 0..n {
            if self.players[owner].orbs.orbs.len() != n {
                // A passive changed the queue; positions no longer name the snapshot's orbs.
                self.unported("orb queue changed during passives");
                return;
            }
            if (self.players[owner].orbs.orbs[i].kind == OrbKind::Plasma) == turn_start {
                self.orb_passive(owner, i);
            }
        }
    }

    /// `trigger_first_orb_passive` (LoopPower).
    pub fn trigger_first_orb_passive(&mut self, owner: CId) {
        if self.is_over_or_ending() || self.players[owner].orbs.orbs.is_empty() {
            return;
        }
        self.orb_passive(owner, 0);
    }

    pub fn count_orbs(&self, owner: CId, kind: OrbKind) -> i32 {
        self.players[owner].orbs.orbs.iter().filter(|o| o.kind == kind).count() as i32
    }

    pub fn count_distinct_orb_kinds(&self, owner: CId) -> i32 {
        OrbKind::ALL.iter().filter(|&&k| self.count_orbs(owner, k) > 0).count() as i32
    }

    /// `Hook.AfterOrbEvoked`: the evoking player's ThunderPower deals its amount to every living
    /// Lightning target.
    fn after_orb_evoked(&mut self, owner: CId, kind: OrbKind, targets: &[CId]) {
        let _hook = self.orb_trace("after_orb_evoked", None);
        if kind != OrbKind::Lightning {
            return;
        }
        let amount = self.power_amount(owner, PowerId::Thunder);
        if amount <= 0 {
            return;
        }
        for &t in targets {
            if self.cr(t).is_alive() {
                self.deal_damage(Some(owner), t, amount, Props::UNPOWERED);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{RunManager, RunOptions};

    #[test]
    fn unrecorded_combat_does_not_join_another_replay_capture() {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut combat = Combat::new(run, 7);
        combat.setup_encounter("INKLETS_NORMAL", &mut crate::rng::Rng::new(7));
        combat.channel_orb(0, OrbKind::Lightning);
        let mut expected = combat.clone();
        let recorder = crate::between::Recorder::default();
        let capture = recorder.begin(1);
        combat.evoke_front_orb(0);
        combat.channel_orb(0, OrbKind::Dark);
        combat.evoke_front_orb(0);
        drop(capture);
        expected.evoke_front_orb(0);
        expected.channel_orb(0, OrbKind::Dark);
        expected.evoke_front_orb(0);
        assert_eq!(combat.digest_json(), expected.digest_json());
        assert_eq!(format!("{:?}", combat.run.rngs), format!("{:?}", expected.run.rngs));
        assert!(recorder.orb_activations().is_empty());
        assert!(recorder.orb_traces().is_empty());
        assert!(recorder.orb_entities().is_empty());
    }

    #[test]
    fn zero_value_frost_passive_keeps_its_owner_target() {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 7);
        c.setup_encounter("INKLETS_NORMAL", &mut crate::rng::Rng::new(7));
        c.apply_power_to(0, PowerId::Focus, -100, Some(0));
        c.channel_orb(0, OrbKind::Frost);
        let before_rng = c.run.rngs.combat_targets.clone();
        let recorder = crate::between::Recorder::default();
        recorder.record_combat(&mut c);
        let capture = recorder.begin(1);
        c.orb_passive_at(0, 0);
        drop(capture);
        let events = recorder.orb_activations();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].phase, "passive");
        assert_eq!(events[0].targets, [0]);
        assert_eq!(c.cr(0).block, 0);
        assert_eq!(c.run.rngs.combat_targets, before_rng);
    }

    #[test]
    fn recorded_orbs_preserve_targets_and_simulation_rng() {
        for (seed, encounter) in [(7, "INKLETS_NORMAL"), (11, "SLIMES_WEAK")] {
        let run = RunManager::new(seed, RunOptions::default()).run_ctx().clone();
        let mut recorded = Combat::new(run, seed);
        recorded.add_player(80, 80, 3, 0, &[]);
        recorded.setup_encounter(encounter, &mut crate::rng::Rng::new(seed));
        let enemies = recorded.enemies.clone();
        assert_eq!(enemies.len(), 3);
        for (i, &enemy) in enemies.iter().enumerate() {
            recorded.cr_mut(enemy).hp = [25, 20, 30][i];
            recorded.cr_mut(enemy).block = 50;
        }
        let mut plain = recorded.clone();
        let recorder = crate::between::Recorder::default();
        recorder.record_combat(&mut recorded);
        let capture = recorder.begin(9);
        recorded.channel_orb(1, OrbKind::Lightning);
        recorded.evoke_front_orb_keep(1);
        let lightning_target = enemies.iter().copied()
            .find(|&enemy| recorded.cr(enemy).block == 42).unwrap();
        recorded.evoke_front_orb(1);
        recorded.channel_orb(1, OrbKind::Dark);
        recorded.evoke_front_orb(1);
        drop(capture);
        plain.channel_orb(1, OrbKind::Lightning);
        plain.evoke_front_orb_keep(1);
        plain.evoke_front_orb(1);
        plain.channel_orb(1, OrbKind::Dark);
        plain.evoke_front_orb(1);
        assert_eq!(recorded.digest_json(), plain.digest_json());
        assert_eq!(format!("{:?}", recorded.creatures), format!("{:?}", plain.creatures));
        assert_eq!(format!("{:?}", recorded.players), format!("{:?}", plain.players));
        assert_eq!(format!("{:?}", recorded.powers), format!("{:?}", plain.powers));
        assert_eq!(format!("{:?}", recorded.cards), format!("{:?}", plain.cards));
        assert_eq!(format!("{:?}", recorded.hist), format!("{:?}", plain.hist));
        assert_eq!(format!("{:?}", recorded.run), format!("{:?}", plain.run));
        assert_eq!(recorded.rng, plain.rng);
        let events = recorder.orb_activations();
        let evokes: Vec<_> = events.iter().filter(|event| event.phase == "evoke").collect();
        assert_eq!(evokes.len(), 3);
        assert_eq!(evokes[0].owner_id, 1);
        assert_eq!(evokes[0].targets, [lightning_target]);
        assert_eq!(evokes[0].activation_index, 0);
        assert!(!evokes[0].dequeued);
        assert_eq!(evokes[1].orb_id, evokes[0].orb_id);
        assert_eq!(evokes[1].activation_index, 1);
        assert!(evokes[1].dequeued);
        assert_eq!(evokes[2].orb_kind, "DARK");
        assert_eq!(evokes[2].targets, [enemies[1]]);
        assert_eq!(recorded.cr(enemies[1]).hp, 20);
        }
    }

    #[test]
    fn recorded_thunder_hook_remains_open_across_damage_continuation() {
        use crate::choice::{ChoiceSrc, PendingChoice, Resolver};
        use crate::combat::PileKind;
        use crate::tables::CardId;
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 7);
        c.setup_encounter("TUNNELER_WEAK", &mut crate::rng::Rng::new(7));
        let enemy = c.enemies[0];
        c.apply_power_to(enemy, PowerId::HardenedShell, 100, Some(enemy));
        c.apply_power_to(0, PowerId::Thunder, 3, Some(0));
        c.channel_orb(0, OrbKind::Lightning);
        let card = c.new_card(0, CardId::Headbutt, false).unwrap();
        c.pending_choice = Some(PendingChoice {
            player: 0, resolver: Resolver { src: ChoiceSrc::Card(CardId::Headbutt),
                k: card, target: None, step: 0, arg: 0 },
            source: PileKind::Discard, allow_skip: false, min: 1, max: 1, single: true,
        });
        let recorder = crate::between::Recorder::default();
        recorder.record_combat(&mut c);
        let capture = recorder.begin(9);
        c.evoke_front_orb(0);
        drop(capture);
        let activation = recorder.orb_activations().into_iter()
            .find(|event| event.phase == "evoke").unwrap();
        let first = recorder.orb_traces();
        assert!(!c.pending_damage_walks.is_empty());
        assert!(first.iter().any(|event| event.operation == "after_orb_evoked"
            && event.transition == "suspend" && event.activation_id == activation.activation_id));
        assert!(!first.iter().any(|event| event.operation == "after_orb_evoked"
            && event.transition == "end"));
        assert_eq!(c.pending_damage_walks.len(), 2);
        c.pending_choice = None;
        let capture = recorder.begin(10);
        let (_, mut thunder_walk) = c.pending_damage_walks.pop().unwrap();
        assert!(c.resume_damage_received_walk(&mut thunder_walk));
        assert!(recorder.orb_traces().iter().any(|event| event.operation == "after_orb_evoked"
            && event.transition == "end" && event.activation_id == activation.activation_id));
        while let Some((_, mut walk)) = c.pending_damage_walks.pop() {
            assert!(c.resume_damage_received_walk(&mut walk));
        }
        drop(capture);
        let resumed = recorder.orb_traces();
        assert!(resumed.iter().any(|event| event.operation == "after_orb_evoked"
            && event.transition == "end" && event.activation_id == activation.activation_id
            && event.action_id == 9));
    }

    #[test]
    fn combat_end_cancels_suspended_orb_trace() {
        use crate::choice::{ChoiceSrc, PendingChoice, Resolver};
        use crate::combat::PileKind;
        use crate::tables::CardId;
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 7);
        c.setup_encounter("TUNNELER_WEAK", &mut crate::rng::Rng::new(7));
        let enemy = c.enemies[0];
        c.apply_power_to(enemy, PowerId::HardenedShell, 100, Some(enemy));
        c.apply_power_to(0, PowerId::Thunder, 3, Some(0));
        c.channel_orb(0, OrbKind::Lightning);
        let card = c.new_card(0, CardId::Headbutt, false).unwrap();
        c.pending_choice = Some(PendingChoice {
            player: 0, resolver: Resolver { src: ChoiceSrc::Card(CardId::Headbutt),
                k: card, target: None, step: 0, arg: 0 },
            source: PileKind::Discard, allow_skip: false, min: 1, max: 1, single: true,
        });
        let recorder = crate::between::Recorder::default();
        recorder.record_combat(&mut c);
        let capture = recorder.begin(0);
        c.evoke_front_orb(0);
        assert!(!c.pending_damage_walks.is_empty());
        c.cr_mut(0).hp = 0;
        c.check_combat_end();
        drop(capture);
        assert!(c.is_over);
        assert!(c.pending_damage_walks.is_empty());
        assert!(recorder.orb_traces().iter().any(|event|
            event.operation == "after_orb_evoked" && event.transition == "cancel"));
    }

    #[test]
    fn nested_orb_suboperation_waits_for_card_choice() {
        use crate::choice::{ChoiceSrc, Resolver};
        use crate::combat::PileKind;
        use crate::tables::CardId;
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 7);
        c.setup_encounter("TUNNELER_WEAK", &mut crate::rng::Rng::new(7));
        c.channel_orb(0, OrbKind::Lightning);
        let card = c.new_card(0, CardId::Hologram, false).unwrap();
        let first = c.new_card(0, CardId::StrikeDefect, false).unwrap();
        let second = c.new_card(0, CardId::DefendDefect, false).unwrap();
        c.players[0].discard.extend([first, second]);
        let recorder = crate::between::Recorder::default();
        recorder.record_combat(&mut c);
        let orb = c.players[0].orbs.orbs[0];
        let capture = recorder.begin(4);
        {
            let _activation = c.record_orb_activation(0, orb, "evoke", false);
            let _hook = c.orb_trace("after_orb_evoked", None);
            let _damage = c.orb_trace("damage", Some(c.enemies[0]));
            c.request_card_choice_for_player(0, &[first, second], PileKind::Discard,
                Resolver { src: ChoiceSrc::Card(CardId::Hologram), k: card,
                    target: None, step: 0, arg: 0 }, false);
        }
        drop(capture);
        assert!(recorder.orb_traces().iter().any(|e|
            e.operation == "damage" && e.transition == "suspend"));
        let capture = recorder.begin(5);
        assert!(c.resolve_pending_choice(Some(0)));
        drop(capture);
        assert!(c.players[0].hand.contains(&first));
        assert!(recorder.orb_traces().iter().any(|e|
            e.operation == "damage" && e.transition == "end" && e.action_id == 4));
    }

    #[test]
    fn hibernate_skips_dead_and_finished_block_and_returns_seat_order() {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(80, 80, 3, 0, &[]);
        c.add_player(80, 80, 3, 0, &[]);
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut crate::rng::Rng::new(7));
        c.apply_power_to(1, PowerId::Hibernate, 1, Some(1));
        c.channel_orb(1, OrbKind::Frost);
        c.creatures[2].hp = 0;
        c.orb_passive_at(1, 0);
        assert_eq!([c.cr(0).block, c.cr(1).block, c.cr(2).block], [2, 2, 0]);
        let orb = c.players[1].orbs.orbs[0];
        let targets = c.evoke_orb(1, orb, false);
        assert_eq!(targets.as_slice(), &[0, 1, 2]);
        assert_eq!([c.cr(0).block, c.cr(1).block, c.cr(2).block], [7, 7, 0]);
        c.is_over = true;
        c.orb_passive_at(1, 0);
        c.evoke_orb(1, orb, false);
        assert_eq!([c.cr(0).block, c.cr(1).block, c.cr(2).block], [7, 7, 0]);
    }
}
