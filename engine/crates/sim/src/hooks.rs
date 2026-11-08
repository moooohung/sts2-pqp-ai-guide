//! r7 `core/hooks.py` dispatch: powers (all creatures, insertion order) then relics.
//!
//! Python calls every hook on every listener and most bodies are the PowerInstance no-op
//! defaults. Here a power is only called when its class overrides the hook (`PowerMeta.hooks`,
//! generated from r7), so a loop over powers that do not listen costs one bit test each.
//! A listening `(power, hook)` pair without a Rust body marks the combat unported.

use crate::card::{CardAffliction, CId, KRef};
use crate::combat::{Combat, EndReward, MAX_HAND_SIZE, PLAYER, PileKind, PilePos, Props};
use crate::creature::{PRef, Side};
use crate::powers::*;
use crate::tables::*;
use arrayvec::ArrayVec;
use smallvec::SmallVec;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod smog_tests;
#[cfg(test)]
mod turn_end_tests;
#[cfg(test)]
mod ending_auto_pre_play_tests;
#[cfg(test)]
mod owner_review_tests;
#[cfg(test)]
mod relic_group_two_tests;
#[cfg(test)]
mod relic_group_three_tests;
#[cfg(test)]
mod painful_stabs_death_tests;
#[cfg(test)]
mod relic_group_four_tests;
#[cfg(test)]
mod relic_group_five_tests;
#[cfg(test)]
mod relic_group_six_tests;
#[cfg(test)]
mod potion_relic_owner_tests;
#[cfg(test)]
mod relic_group_seven_tests;
#[cfg(test)]
mod relic_group_eight_tests;
#[cfg(test)]
mod relic_group_nine_tests;
#[cfg(test)]
mod relic_wide01_a_tests;
#[cfg(test)]
mod relic_wide02_a_tests;
#[cfg(test)]
mod relic_wide03_a_tests;
#[cfg(test)]
mod relic_review_a_tests;
#[cfg(test)]
mod relic_wide04_a_tests;
#[cfg(test)]
mod relic_dispatch_review_a_tests;
#[cfg(test)]
mod relic_stomp_capture_a_tests;
#[cfg(test)]
mod relic_wide05_a_tests;
#[cfg(test)]
mod relic_unlock_review_a_tests;
#[cfg(test)]
mod relic_wide06_a_tests;
#[cfg(test)]
mod relic_wide07_a_tests;
#[cfg(test)]
mod relic_wide08_a_tests;
#[cfg(test)]
mod relic_wide09_a_tests;
#[cfg(test)]
mod relic_wide10_a_tests;
#[cfg(test)]
mod relic_wide11_a_tests;
#[cfg(test)]
mod relic_sz01_a_tests;
#[cfg(test)]
mod relic_sz02_a_tests;
#[cfg(test)]
mod relic_batch_one_b_tests;
#[cfg(test)]
mod relic_batch_two_b_tests;
#[cfg(test)]
mod relic_batch_one_h_tests;
#[cfg(test)]
mod relic_batch_two_h_tests;
#[cfg(test)]
mod relic_batch_three_h_tests;
#[cfg(test)]
mod relic_batch_four_h_tests;
#[cfg(test)]
mod relic_batch_five_h_tests;
#[cfg(test)]
mod relic_batch_one_c_tests;
#[cfg(test)]
mod relic_batch_three_b_tests;
#[cfg(test)]
mod relic_batch_four_b_tests;
#[cfg(test)]
mod relic_batch_five_b_tests;
#[cfg(test)]
mod relic_batch_six_b_tests;
#[cfg(test)]
mod relic_batch_seven_b_tests;
#[cfg(test)]
mod relic_batch_eight_b_tests;
#[cfg(test)]
mod relic_batch_nine_b_tests;
#[cfg(test)]
mod relic_batch_ten_b_tests;
#[cfg(test)]
mod relic_batch_eleven_b_tests;
#[cfg(test)]
mod relic_batch_twelve_b_tests;
#[cfg(test)]
mod relic_batch_thirteen_b_tests;
#[cfg(test)]
mod relic_discard_review_b_tests;

#[derive(Clone, Copy, Debug)]
pub(crate) enum TurnStartListener {
    Power(CId, PRef),
    Relic(usize),
    Monster(CId),
    Card(KRef),
    Affliction(KRef, CardAffliction),
    Enchantment(KRef, crate::identity::EnchantmentId),
}

#[derive(Clone, Debug)]
pub(crate) struct AfterPlayWalk {
    listeners: Vec<TurnStartListener>,
    next: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct DamageReceivedWalk {
    walk: AfterPlayWalk,
    ev: Ev,
    orb_context: Option<crate::between::OrbContext>,
    orb_pending: bool,
    orb_spans: Vec<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExhaustWalk {
    listeners: Vec<TurnStartListener>, next: usize,
    card: KRef, ethereal: bool, joss_modulo: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct TurnStartWalk {
    owner: CId, tier: usize, loaded: bool,
    listeners: Vec<TurnStartListener>, next: usize,
    hooks: &'static [Hook],
    pub(crate) resume_stage: crate::combat::TurnStage,
}

#[derive(Clone, Debug)]
pub(crate) struct TurnEndWalk {
    auto_post_owner: CId,
    tier: usize,
    loaded: bool,
    listeners: Vec<TurnStartListener>,
    next: usize,
    stampede: Option<(PRef, CId, i32)>,
    /// A parked task finishes only its current seat (tier 0) or callback (tiers 1-3).
    detached: bool,
    resume_stage: crate::combat::TurnStage,
}

/// Arguments of fire-and-forget hooks.
#[derive(Clone, Copy, Debug)]
pub enum Ev {
    None,
    Side(Side),
    Creature(CId),
    HpChanged { creature: CId, delta: i32 },
    PileChanged { card: KRef, old_pile: PileKind, cloned_by: Option<crate::piles::CloneSource> },
    Card(KRef),
    BlockGained {
        creature: CId,
        amount: i32,
    },
    PowerAmount {
        target: CId,
        id: PowerId,
        amount: i32,
        applier: Option<CId>,
        source: Option<KRef>,
        instance: Option<PRef>,
    },
    /// before/after_damage_received and after_damage_given. `amount` is r7 `damage` (the hit
    /// for before_damage_received, unblocked/HP loss for the after-hooks); `total_damage` is the
    /// post-modifier hit before block; `was_block_broken` feeds relic listeners.
    Damage {
        target: CId,
        dealer: Option<CId>,
        card_source: Option<KRef>,
        amount: i32,
        total_damage: i32,
        blocked_damage: i32,
        props: Props,
        was_block_broken: bool,
    },
    /// after_card_generated_for_combat(card, added_by_player).
    Generated { card: KRef, by_player: bool, creator: Option<CId> },
    /// on_stars_gained / on_stars_spent(owner, stars).
    Stars { owner: CId, amount: i32 },
    /// after_energy_spent(card, amount).
    EnergySpent { card: KRef, amount: i32 },
    /// after_forge(amount, forger, source card).
    Forge { forger: CId, amount: i32, source: Option<KRef> },
    /// C# Hook.AfterSummon(summoner, modified amount).
    Summon { owner: CId, amount: i32 },
    /// after_attack `AttackContext` fields needed by power listeners.
    Attack {
        attacker: Option<CId>,
        target: Option<CId>,
        props: Props,
        card: Option<KRef>,
    },
}

#[cfg(test)]
mod application_order_tests;

impl Combat {
    pub(crate) fn start_after_play_walk(&self) -> AfterPlayWalk {
        self.start_listener_walk(Hook::AfterCardPlayed)
    }

    fn start_listener_walk(&self, hook: Hook) -> AfterPlayWalk {
        let mut listeners = Vec::new();
        for owner in self.all_creatures() {
            listeners.extend(self.listeners_of(owner, hook)
                .into_iter().map(|p| TurnStartListener::Power(owner, p)));
            if self.players.is_player(owner) {
                listeners.extend(self.run.relics.iter().enumerate().filter(|(_, relic)|
                    relic.owner == owner && relic.enabled && self.players[owner].hooks_active
                        && relic_hook_mask(relic.id) & (1u128 << hook as u32) != 0)
                    .map(|(r, _)| TurnStartListener::Relic(r)));
            }
        }
        AfterPlayWalk { listeners, next: 0 }
    }

    pub(crate) fn resume_after_play_walk(&mut self, walk: &mut AfterPlayWalk, card: KRef) -> bool {
        self.resume_listener_walk(walk, Hook::AfterCardPlayed, Ev::Card(card))
    }

    fn resume_listener_walk(&mut self, walk: &mut AfterPlayWalk, hook: Hook, ev: Ev) -> bool {
        // Hooks/Hook.cs:195-209 awaits each listener, then its caller.
        while let Some(listener) = walk.listeners.get(walk.next).copied() {
            walk.next += 1;
            match listener {
                TurnStartListener::Power(owner, p) => {
                    if self.cr(owner).power_list().contains(&p) {
                        self.power_event(p, owner, hook, ev);
                    }
                }
                TurnStartListener::Relic(r) => {
                    if self.run.relics[r].enabled && self.players[self.run.relics[r].owner].hooks_active {
                        self.relic_event(r, hook, ev);
                    }
                }
                _ => unreachable!(),
            }
            if self.pending_choice.is_some() || self.fault.is_some() { return false; }
        }
        true
    }

    pub(crate) fn resume_damage_received_walk(&mut self, frame: &mut DamageReceivedWalk) -> bool {
        let _context = self.restore_orb_context(if frame.orb_pending { frame.orb_context } else { None },
            if frame.orb_pending { &frame.orb_spans } else { &[] });
        let completed = self.resume_listener_walk(&mut frame.walk, Hook::AfterDamageReceived, frame.ev);
        if !completed && !frame.orb_pending {
            frame.orb_spans = self.suspend_orb_context(frame.orb_context);
            frame.orb_pending = true;
        } else if completed && frame.orb_pending {
            self.finish_orb_context(&frame.orb_spans, "end");
            frame.orb_pending = false;
        }
        completed
    }

    pub(crate) fn cancel_damage_received_walk(&mut self, frame: DamageReceivedWalk) {
        if frame.orb_pending { self.finish_orb_context(&frame.orb_spans, "cancel"); }
    }

    /// C# Hook.ModifySummonAmount. The audited C# has no overrides; unknown exported
    /// listeners still fail closed rather than silently bypassing a future modifier.
    pub(crate) fn modify_summon_amount(&mut self, _owner: CId, amount: i32, _source: Option<KRef>) -> i32 {
        for c in self.all_creatures() {
            if !self.listeners_of(c, Hook::ModifySummonAmount).is_empty() {
                self.unported("modify_summon_amount");
            }
        }
        for r in &self.run.relics {
            if relic_hook_mask(r.id) & (1u128 << Hook::ModifySummonAmount as u32) != 0 {
                self.unported("relic modify_summon_amount");
                break;
            }
        }
        amount
    }

    /// powers/*.py `before_damage_received(owner, target, dealer, damage, props)`.
    fn power_before_damage_received(&mut self, p: PRef, owner: CId, target: CId, dealer: Option<CId>,
                                    _damage: i32, props: Props, card_source: Option<KRef>) {
        match self.powers[p as usize].id {
            // powers/damage_reactions.py ThornsPower: unpowered damage back at the attacker,
            // with the active card source cleared while it resolves.
            PowerId::Thorns => {
                if target == owner && let Some(d) = dealer {
                    // ThornsPower.BeforeDamageReceived explicitly includes Omnislice's
                    // unpowered splash, not just powered attacks.
                    if props.is_powered_attack()
                        || card_source.is_some_and(|k| self.cards[k as usize].id == CardId::Omnislice) {
                        let amount = self.powers[p as usize].amount;
                        self.record_public_power(p,"trigger",Some(amount),true,true);
                        let prev = self.active_card.take();
                        self.deal_damage(Some(owner), d, amount, Props::UNPOWERED);
                        self.active_card = prev;
                    }
                }
            }
            _ => self.unported("power before_damage_received"),
        }
    }
}

/// Hooks `relic_event` handles for relic `id`: its r7 combat hooks, plus Permafrost's reset,
/// which r7 does in the run-level `after_room_entered` and the native combat does at
/// `BeforeCombatStart` (relics.rs Permafrost arm).
#[inline]
fn relic_hook_mask(id: RelicId) -> u128 {
    let mask = RELIC_INFO[id.index()].hook_mask;
    // BoomingConch.cs AfterSideTurnStart is missing from the frozen r7 metadata.
    if id == RelicId::BoomingConch {
        return mask | (1u128 << Hook::AfterSideTurnStart as u32);
    }
    // BoneTea.AfterPlayerTurnStart upgrades the drawn hand, not the empty
    // pre-draw hand at r7 AfterSideTurnStart.
    // C# GamblingChip.AfterPlayerTurnStart shares this hook, so inventory
    // listener order determines whether the selected/refilled hand is upgraded.
    // r7 relics/shop_event.py:1146-1169 uses AfterSideTurnStart instead;
    // relics/rare.py:309-328 keeps GamblingChip in AfterPlayerTurnStart.
    if id == RelicId::BoneTea {
        return 1u128 << Hook::AfterPlayerTurnStart as u32;
    }
    // Metronome.AfterRoomEntered(CombatRoom) resets before any orb channels.
    // AfterOrbChanneled uses the owner-bearing channel_orb dispatch.
    if id == RelicId::Metronome {
        return mask | (1u128 << Hook::BeforeCombatStart as u32);
    }
    // Models/Relics/Pantograph.cs:27 heals in BeforeCombatStart; its C#
    // AfterRoomEntered callback changes only visual status.
    if id == RelicId::Pantograph {
        return 1u128 << Hook::BeforeCombatStart as u32;
    }
    // Models/Relics/PhialHolster.cs:26 / RoyalStamp.cs:32 / OldCoin.cs:25:
    // AfterObtained is not replayed for an equipped combat snapshot.
    // PaelsWing.cs:73,83 modifies reward alternatives only.
    // Planisphere.cs:23 heals on run-room entry, not combat start/end.
    if matches!(id, RelicId::PhialHolster | RelicId::RoyalStamp
        | RelicId::OldCoin | RelicId::PaelsWing | RelicId::Planisphere) {
        return 0;
    }
    // Models/Relics/PrayerWheel.cs:14 modifies run rewards only.
    // Orrery.cs:19 / Pear.cs:17 / PreciseScissors.cs:18 /
    // PaelsHorn.cs:17 / PreservedFog.cs:20 only implement AfterObtained.
    // PaelsFlesh.cs:50-91 callbacks update UI, never grant energy directly;
    // ModifyMaxEnergy (relics.rs) is its only gameplay effect.
    if matches!(id, RelicId::PrayerWheel | RelicId::Orrery | RelicId::Pear
        | RelicId::PreciseScissors | RelicId::PaelsHorn | RelicId::PreservedFog
        | RelicId::PaelsFlesh) {
        return 0;
    }
    // Models/Relics/Pomander.cs:17 / NutritiousSoup.cs:21 /
    // NeowsBones.cs:38: AfterObtained only, already represented in entry state.
    if matches!(id, RelicId::Pomander | RelicId::NutritiousSoup | RelicId::NeowsBones) {
        return 0;
    }
    // Models/Relics/NutritiousOyster.cs:17 only overrides AfterObtained.
    // Acquisition max HP is handled by the run layer, never combat callbacks.
    // Likewise Shovel.cs:18 is rest-only; SignetRing.cs:15 and SeaGlass.cs:74
    // are acquisition-only, with no combat listener.
    if matches!(id, RelicId::NutritiousOyster | RelicId::PrecariousShears
        | RelicId::PaelsGrowth | RelicId::Whetstone | RelicId::PandorasBox
        | RelicId::NeowsTalisman | RelicId::PotionBelt
        | RelicId::Shovel | RelicId::SignetRing | RelicId::SeaGlass
        | RelicId::RegalPillow | RelicId::WingCharm | RelicId::PaelsClaw
        | RelicId::TriBoomerang | RelicId::WongoCustomerAppreciationBadge
        | RelicId::TouchOfOrobas | RelicId::NewLeaf | RelicId::WhiteBeastStatue
        | RelicId::TheCourier | RelicId::SilverCrucible | RelicId::SmallCapsule
        | RelicId::NeowsSacrifice | RelicId::Strawberry | RelicId::PunchDagger
        | RelicId::ToxicEgg | RelicId::SereTalon | RelicId::TanxsWhistle
        | RelicId::NeowsTorment) {
        // Batch b9: acquisition only (NeowsSacrifice.cs:29, Strawberry.cs:17,
        // PunchDagger.cs:24, SereTalon.cs:31, TanxsWhistle.cs:17,
        // NeowsTorment.cs:19). ToxicEgg.cs:21 modifies rewards/deck additions,
        // never cards generated into combat piles.
        // Batch b8: WongoCustomerAppreciationBadge.cs:5 has no hooks.
        // TouchOfOrobas.cs:149 / NewLeaf.cs:22 / SmallCapsule.cs:13 are pickups;
        // WhiteBeastStatue.cs:17 / SilverCrucible.cs:94 are rewards/treasure;
        // TheCourier.cs:19 is merchant-only.
        // RegalPillow.cs:19 / WingCharm.cs:24 are rest/reward-only;
        // PaelsClaw.cs:32 / TriBoomerang.cs:23 are acquisition-only.
        // Models/Relics/<Name>.cs: AfterObtained (and PaelsGrowth rest options)
        // only. Imported combat state already contains acquisition effects.
        return 0;
    }
    // Models/Relics/Regalite.cs:37,47: generation, not entering combat.
    if id == RelicId::Regalite {
        return (1u128 << Hook::AfterCardGeneratedForCombat as u32)
            | (1u128 << Hook::BeforeSideTurnStart as u32)
            | (1u128 << Hook::AfterCombatEnd as u32);
    }
    // FuneraryMask.cs creates Souls in BeforeHandDraw, not r7's side-start hook.
    if id == RelicId::FuneraryMask {
        return (mask & !(1u128 << Hook::AfterSideTurnStart as u32))
            | (1u128 << Hook::BeforeHandDraw as u32);
    }
    // Original FurCoat also affects newly added enemies. Bookmark is dispatched
    // with the retained-card snapshot at AfterFlush, never r7's AfterTurnEnd.
    if id == RelicId::FurCoat {
        return mask | (1u128 << Hook::AfterCreatureAddedToCombat as u32);
    }
    if id == RelicId::Bookmark {
        return mask & !(1u128 << Hook::AfterTurnEnd as u32);
    }
    // C# CombatManager.RunAutoPrePlayPhase: after all setup, side hooks and orbs.
    if id == RelicId::HistoryCourse {
        return (mask & !(1u128 << Hook::AfterPlayerTurnStartEarly as u32))
            | (1u128 << Hook::AfterAutoPrePlayPhaseEntered as u32);
    }
    // UnsettlingLamp.BeforePowerAmountChanged selects the first eligible source
    // before ModifyPowerAmountGivenMultiplicative, including Artifact filtering.
    if id == RelicId::UnsettlingLamp {
        return mask | (1u128 << Hook::BeforePowerAmountChanged as u32);
    }
    // Models/Relics/PollinousCore.cs:88 increments for the owner's hand draw,
    // not BeforeSideTurnStart as in r7.
    if id == RelicId::PollinousCore {
        return (mask & !(1u128 << Hook::BeforeSideTurnStart as u32))
            | (1u128 << Hook::BeforeHandDraw as u32);
    }
    // LetterOpener.BeforeCombatStart resets SkillsPlayedThisTurn in C#.
    if matches!(id, RelicId::Permafrost | RelicId::LetterOpener | RelicId::VelvetChoker) {
        mask | (1u128 << Hook::BeforeCombatStart as u32)
    } else { mask }
}

impl Combat {
    /// Snapshot of `(owner, power)` listeners for `hook`, taken creature by creature like
    /// r7's `_iter_power_listeners` generator (each creature's list is copied when reached).
    #[inline]
    fn listeners_of(&self, c: CId, hook: Hook) -> SmallVec<[PRef; 32]> {
        let bit = 1u128 << hook as u32;
        let cr = self.cr(c);
        let mut out = SmallVec::new();
        // C# CombatState.IsModelValidForHooks after Player.DeactivateHooks.
        if self.players.is_player(c) && !self.players[c].hooks_active {
            return out;
        }
        if cr.hook_mask & bit == 0 {
            return out;
        }
        // CombatState.IterateHookListeners walks Creature.Powers in application order,
        // interleaving instanced repeats with other power classes.
        for p in cr.power_list() {
            if power_meta(self.powers[p as usize].id).hooks & bit != 0 {
                out.push(p);
            }
        }
        out
    }

    /// Generic fire-and-forget hook over powers then relics; `owner_filter` mirrors the
    /// `if owner is drawing_owner` checks of the r7 dispatcher.
    pub fn fire(&mut self, hook: Hook, ev: Ev, owner_filter: Option<CId>) {
        if hook == Hook::AfterDamageReceived && owner_filter.is_none() {
            // Hook.cs:306-322 awaits each listener before continuing the hit.
            let mut frame = DamageReceivedWalk { walk: self.start_listener_walk(hook), ev,
                orb_context: self.current_orb_context(), orb_pending: false, orb_spans: Vec::new() };
            if !self.resume_damage_received_walk(&mut frame) {
                self.pending_damage_walks.push((self.pending_play.len(), frame));
            }
            return;
        }
        // Hook.cs checks once at this tier's entry. A started listener loop
        // still finishes its snapshot if an earlier callback makes combat ending.
        if hook == Hook::AfterAutoPrePlayPhaseEntered && self.is_over_or_ending() { return; }
        // AttackCommand.Execute records CreatureAttacked before dispatching AfterAttack.
        if hook == Hook::AfterAttack && let Ev::Attack { attacker: Some(actor), .. } = ev {
            self.hist.attacked_this_turn.push(actor);
        }
        // Game order (CombatState.IterateHookListeners, CombatState.cs:272): per creature, its
        // powers, then for the player its relics.
        for c in self.all_creatures() {
            if owner_filter.is_some_and(|o| o != c) {
                continue;
            }
            for p in self.listeners_of(c, hook) {
                self.power_event(p, c, hook, ev);
                if self.fault.is_some() { return; }
            }
            if self.players.is_player(c) {
                self.fire_relics_for_owner(hook, ev, Some(c));
                if self.fault.is_some() { return; }
            }
        }
        if hook == Hook::AfterAttack && let Ev::Attack { attacker, .. } = ev {
            self.fire_card_after_attack(attacker);
        }
    }

    pub fn fire_relics(&mut self, hook: Hook, ev: Ev) {
        self.fire_relics_for_owner(hook, ev, None);
    }

    fn fire_relics_for_owner(&mut self, hook: Hook, ev: Ev, owner: Option<CId>) {
        let bit = 1u128 << hook as u32;
        let listeners:Vec<_>=self.run.relics.iter().enumerate().filter(|(_,relic)|
            relic.enabled && self.players[relic.owner].hooks_active
                && owner.is_none_or(|o|relic.owner==o) && relic_hook_mask(relic.id)&bit!=0)
            .map(|(r,_)|r).collect();
        for r in listeners {
            let relic = &self.run.relics[r];
            // Hook.AfterCombatEnd iterates a materialized listener list.
            // ToyBox melting a later relic must not cancel that relic's reset.
            if hook == Hook::AfterCombatEnd || (relic.enabled && self.players[relic.owner].hooks_active) {
                if hook == Hook::AfterAutoPrePlayPhaseEntered
                    && matches!(ev, Ev::Creature(player) if player != relic.owner) {
                    continue; // concrete HistoryCourse callback is an immediate no-op
                }
                if matches!(hook, Hook::AfterPlayerTurnStartEarly | Hook::AfterPlayerTurnStart | Hook::AfterPlayerTurnStartLate | Hook::AfterAutoPrePlayPhaseEntered)
                    && self.pending_choice.is_some()
                {
                    self.unported("AfterPlayerTurnStart listener continuation while choice is pending");
                    return;
                }
                self.relic_event(r, hook, ev);
            }
        }
    }

    /// Power bodies of fire-and-forget hooks.
    fn power_event(&mut self, p: PRef, owner: CId, hook: Hook, ev: Ev) {
        if matches!(ev, Ev::Side(Side::Player)) && self.players.is_player(owner)
            && !self.is_player_taking_turn(owner)
            && matches!(hook, Hook::BeforeSideTurnStart | Hook::AfterSideTurnStart |
                Hook::BeforeTurnEndVeryEarly | Hook::BeforeTurnEndEarly | Hook::BeforeTurnEnd |
                Hook::AfterTurnEnd | Hook::AfterTurnEndLate)
        {
            return;
        }
        if hook == Hook::AfterAutoPrePlayPhaseEntered
            && matches!(ev, Ev::Creature(player) if player != owner) {
            return; // Mayhem is owner-scoped
        }
        // C# awaits each AfterPlayerTurnStart callback. Until the resumable
        // listener/seat scheduler is ported, never execute a later callback
        // against an unresolved selector (even a non-choice callback).
        if matches!(hook, Hook::AfterPlayerTurnStartEarly | Hook::AfterPlayerTurnStart | Hook::AfterPlayerTurnStartLate | Hook::AfterAutoPrePlayPhaseEntered)
            && self.pending_choice.is_some() {
            self.unported("AfterPlayerTurnStart listener continuation while choice is pending");
            return;
        }
        // C# CombatState.IsModelValidForHooks is rechecked for each callback:
        // an earlier callback in the snapshot can kill and deactivate this player.
        if self.players.is_player(owner) && !self.players[owner].hooks_active {
            return;
        }
        match hook {
            Hook::AfterAutoPrePlayPhaseEntered => {
                if self.powers[p as usize].id == PowerId::Mayhem {
                    if matches!(ev, Ev::Creature(player) if player == owner) {
                        self.auto_play_from_draw(owner, self.powers[p as usize].amount.max(0) as usize, false);
                    }
                } else { self.unported("power auto-pre-play phase"); }
            }
            Hook::BeforeSideTurnStart => {
                let Ev::Side(side) = ev else {
                    self.unported("before_side_turn_start args");
                    return;
                };
                self.power_before_side_turn_start(p, owner, side);
            }
            Hook::BeforeTurnEndVeryEarly => {
                let Ev::Side(side) = ev else {
                    self.unported("before_turn_end_very_early args");
                    return;
                };
                self.power_before_turn_end_very_early(p, owner, side);
            }
            Hook::BeforeTurnEndEarly => {
                let Ev::Side(side) = ev else {
                    self.unported("before_turn_end_early args");
                    return;
                };
                self.power_before_turn_end_early(p, owner, side);
            }
            Hook::BeforeTurnEnd => {
                let Ev::Side(side) = ev else {
                    self.unported("before_turn_end args");
                    return;
                };
                self.power_before_turn_end(p, owner, side);
            }
            Hook::AfterSideTurnStart => {
                let Ev::Side(side) = ev else {
                    self.unported("after_side_turn_start args");
                    return;
                };
                self.power_after_side_turn_start(p, owner, side);
            }
            Hook::AfterTurnEnd => {
                let Ev::Side(side) = ev else {
                    self.unported("after_turn_end args");
                    return;
                };
                self.power_after_turn_end(p, owner, side);
            }
            Hook::AfterTurnEndLate => {
                let Ev::Side(side) = ev else {
                    self.unported("after_turn_end_late args");
                    return;
                };
                self.power_after_turn_end_late(p, owner, side);
            }
            Hook::AfterCardPlayed => {
                let Ev::Card(k) = ev else {
                    self.unported("after_card_played args");
                    return;
                };
                self.power_after_card_played(p, owner, k);
            }
            Hook::AfterCardEnteredCombat => {
                let Ev::Card(k) = ev else {
                    self.unported("after_card_entered_combat args");
                    return;
                };
                self.power_after_card_entered_combat(p, owner, k);
            }
            Hook::OnCardDrawn => {
                let Ev::Card(k) = ev else {
                    self.unported("on_card_drawn args");
                    return;
                };
                self.power_on_card_drawn(p, owner, k);
            }
            Hook::BeforeCardPlayed => {
                let Ev::Card(k) = ev else {
                    self.unported("before_card_played args");
                    return;
                };
                self.power_before_card_played(p, owner, k);
            }
            Hook::BeforePotionUsed => {
                self.power_before_potion_used(p, owner);
            }
            Hook::AfterCardExhausted => {
                let Ev::Card(k) = ev else {
                    self.unported("after_card_exhausted args");
                    return;
                };
                self.power_after_card_exhausted(p, owner, k, false);
            }
            Hook::AfterEnergyReset => {
                let Ev::Creature(reset_owner) = ev else {
                    self.unported("after_energy_reset args");
                    return;
                };
                self.power_after_energy_reset(p, owner, reset_owner);
            }
            Hook::AfterPowerAmountChanged => {
                let Ev::PowerAmount {
                    target,
                    id,
                    amount,
                    applier,
                    source,
                    ..
                } = ev
                else {
                    self.unported("after_power_amount_changed args");
                    return;
                };
                self.power_after_power_amount_changed(
                    p, owner, target, id, amount, applier, source,
                );
            }
            Hook::BeforeCombatStart => {
                self.power_before_combat_start(p, owner);
            }
            Hook::AfterCurrentHpChanged => {
                let Ev::HpChanged { creature, delta } = ev else {
                    self.unported("after_current_hp_changed args");
                    return;
                };
                if self.powers[p as usize].id == PowerId::NecroMastery {
                    // Models/Powers/NecroMasteryPower.AfterCurrentHpChanged uses actual
                    // negative HP delta, including direct kills, not attack damage/overkill.
                    if delta < 0 && self.osty(owner) == Some(creature) {
                        let amount = -delta * self.powers[p as usize].amount;
                        let props = Props::UNBLOCKABLE.or(Props::UNPOWERED);
                        for e in self.hittable_enemies() {
                            let damage = self.modify_damage(amount, Some(owner), e, props, None);
                            self.apply_damage(e, damage, props, Some(owner));
                        }
                    }
                } else {
                    self.unported("after_current_hp_changed");
                }
            }
            Hook::AfterCombatEnd => {
                self.power_after_combat_end(p, owner);
            }
            Hook::AfterBlockGained => {
                let Ev::BlockGained { creature, amount } = ev else {
                    self.unported("after_block_gained args");
                    return;
                };
                self.power_after_block_gained(p, owner, creature, amount);
            }
            Hook::AfterBlockCleared => {
                let Ev::Creature(creature) = ev else {
                    self.unported("after_block_cleared args");
                    return;
                };
                self.power_after_block_cleared(p, owner, creature);
            }
            Hook::BeforeDamageReceived => {
                let Ev::Damage { target, dealer, amount: damage, props, card_source, .. } = ev else {
                    self.unported("before_damage_received args");
                    return;
                };
                self.power_before_damage_received(p, owner, target, dealer, damage, props, card_source);
            }
            Hook::AfterDamageReceived => {
                let Ev::Damage { target, dealer, amount: damage, total_damage, blocked_damage, props, .. } = ev else {
                    self.unported("after_damage_received args");
                    return;
                };
                self.power_after_damage_received(
                    p, owner, target, dealer, damage, total_damage, blocked_damage, props,
                );
            }
            Hook::AfterDamageGiven => {
                let Ev::Damage { target, dealer, amount: damage, total_damage, props, .. } = ev else {
                    self.unported("after_damage_given args");
                    return;
                };
                self.power_after_damage_given(
                    p, owner, target, dealer, damage, total_damage, props,
                );
            }
            Hook::BeforeAttack => {
                let Ev::Attack { attacker, target, props, card } = ev else {
                    self.unported("before_attack args");
                    return;
                };
                self.power_before_attack(p, owner, attacker, target, props, card);
            }
            Hook::AfterAttack => {
                let Ev::Attack { attacker, target, props, card } = ev else {
                    self.unported("after_attack args");
                    return;
                };
                self.power_after_attack(p, owner, attacker, target, props, card);
            }
            Hook::AfterPlayerTurnStart => {
                let Ev::Creature(turn_owner) = ev else {
                    self.unported("after_player_turn_start args");
                    return;
                };
                self.power_after_player_turn_start(p, owner, turn_owner);
            }
            Hook::BeforeHandDraw => {
                let Ev::Creature(turn_owner) = ev else {
                    self.unported("before_hand_draw args");
                    return;
                };
                self.power_before_hand_draw(p, owner, turn_owner);
            }
            // c2: core/hooks.py fire_after_card_generated_for_combat (powers)
            Hook::AfterCardGeneratedForCombat => {
                let Ev::Generated { card, by_player, creator } = ev else {
                    self.unported("after_card_generated_for_combat args");
                    return;
                };
                if !self.necro_after_card_generated(p, owner, card, creator)
                    && !self.defect_after_card_generated(p, owner, card, by_player)
                    && !self.regent_after_card_generated(p, owner, card, by_player)
                {
                    self.unported("power after_card_generated_for_combat");
                }
            }
            Hook::OnStarsGained | Hook::OnStarsSpent | Hook::AfterForge | Hook::AfterEnergySpent => {
                if !self.regent_power_event(p, owner, hook, ev) {
                    self.unported("regent power hook");
                }
            }
            _ => self.unported("power hook"),
        }
    }

    fn power_before_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::ChainsOfBinding => {
                if side == self.cr(owner).side {
                    self.set_power_private_int(owner, PowerId::ChainsOfBinding, 0, 0);
                    self.set_power_private_int(owner, PowerId::ChainsOfBinding, 1, 0);
                }
            }
            PowerId::Plating => {
                if side == Side::Player && self.cr(owner).side == Side::Enemy && self.round == 1 {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            // Frozen r7 powers/monster.py HardenedShellPower.before_side_turn_start:
            // reset the private damage-taken tracker for every side turn.
            PowerId::HardenedShell => {
                self.powers[p as usize].vars[0] = 0;
                self.cr_mut(owner).hp_display_infinite = false;
            }
            // powers/damage_modifiers.py AggressionPower.before_side_turn_start
            PowerId::Aggression => {
                if side == self.cr(owner).side {
                    let mut candidates: Vec<KRef> = self.players[owner]
                        .discard
                        .iter()
                        .copied()
                        .filter(|&k| {
                            self.cards[k as usize].owner == owner
                                && self.cards[k as usize].card_type == CardType::Attack
                        })
                        .collect();
                    self.run.rngs.combat_card_selection.shuffle(&mut candidates);
                    for k in candidates.into_iter().take(amount.max(0) as usize) {
                        let pl = &mut self.players[owner];
                        if let Some(pos) = pl.discard.iter().position(|&q| q == k) {
                            pl.discard.remove(pos);
                        }
                        if pl.hand.len() < MAX_HAND_SIZE {
                            pl.hand.push(k);
                        } else {
                            pl.discard.push(k);
                        }
                        if self.card_pile(k) != PileKind::Discard {
                            self.fire_after_card_changed_piles(k, PileKind::Discard, None);
                        }
                        let card = &self.cards[k as usize];
                        if !card.upgraded
                            && card_info(card.id, card.upgraded).is_some_and(|i| i.upgradable)
                        {
                            self.upgrade_card(k);
                        }
                    }
                }
            }
            // C# SlothPower resets only when the owner participates in turn start.
            PowerId::Sloth => {
                if side == self.cr(owner).side { self.powers[p as usize].vars[0]=0; }
            }
            // C# VoidFormPower.BeforeSideTurnStart: reset the cards-played counter.
            PowerId::VoidForm => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].vars[0] = 0;
                }
            }
            _ => {
                if !self.chars_before_side_turn_start(p, owner, side) && !self.defect_before_side_turn_start(p, owner, side) {
                    self.unported("before_side_turn_start");
                }
            }
        }
    }

    fn power_before_turn_end_very_early(&mut self, p: PRef, owner: CId, side: Side) {
        match self.powers[p as usize].id {
            PowerId::Asleep => {
                if side == self.cr(owner).side && self.powers[p as usize].amount <= 1 {
                    if let Some(plating) = self.find_power(owner, PowerId::Plating) {
                        self.remove_power(owner, plating);
                    }
                }
            }
            _ => self.unported("before_turn_end_very_early"),
        }
    }

    fn power_before_turn_end_early(&mut self, p: PRef, owner: CId, side: Side) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::Regen => {
                // RegenPower.BeforeSideTurnEndEarly precedes orb passives.
                if side == self.cr(owner).side && self.cr(owner).is_alive() {
                    self.heal(owner,amount);
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount<=0 {self.remove_power(owner,p);}
                }
            }
            PowerId::Plating => {
                if side == self.cr(owner).side {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            _ => self.unported("before_turn_end_early"),
        }
    }

    fn power_before_turn_end(&mut self, p: PRef, owner: CId, side: Side) {
        match self.powers[p as usize].id {
            // DoomPower.BeforeSideTurnEnd: enemy Doom triggers BEFORE other side-end effects.
            PowerId::Doom => {
                if side == Side::Enemy {
                    self.doom_side_end(owner, side);
                }
            }
            PowerId::ChainsOfBinding => {
                // powers/remaining_a.py:493-508.
                self.set_power_private_int(owner, PowerId::ChainsOfBinding, 0, 0);
                self.set_power_private_int(owner, PowerId::ChainsOfBinding, 1, 0);
                for k in self.pile_cards(owner) {
                    if self.cards[k as usize].owner == owner
                        && self.cards[k as usize].cv.affliction == Some(CardAffliction::Bound)
                    {
                        self.cards[k as usize].cv.affliction = None;
                        self.cards[k as usize].cv.affliction_amount = 0;
                        self.cards[k as usize].bound = false;
                    }
                }
            }
            PowerId::TheBomb => {
                // fire: powers/turn_effects.py:661-674
                if side != self.cr(owner).side {
                    return;
                }
                if self.powers[p as usize].amount > 1 {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    return;
                }
                let damage = self.powers[p as usize].payload;
                let enemies = self.hittable_enemies();
                for enemy in enemies {
                    self.deal_damage(Some(owner), enemy, damage, Props::UNPOWERED);
                }
                if self.cr(owner).powers.contains(&p) || self.cr(owner).extra.contains(&p) {
                    self.remove_power(owner, p);
                }
            }
            PowerId::Stampede => {
                if side != self.cr(owner).side {
                    return;
                }
                self.stampede_autoplays(p, owner, 0);
            }
            _ => {
                if !self.defect_before_turn_end(p, owner, side) {
                    self.unported("before_turn_end");
                }
            }
        }
    }

    fn power_after_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::DemonForm => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        amount,
                        None,
                        None,
                        false,
                        false,
                    );
                }
            }
            PowerId::DrawCardsNextTurn => {
                if side == self.cr(owner).side && amount != 0 {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::EnergyNextTurn => {
                if side == self.cr(owner).side && self.cr(owner).is_player && amount != 0 {
                    if amount > 0 && !self.is_over_or_ending() {
                        self.players[owner].energy += amount;
                    }
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Plating => {
                if side == self.cr(owner).side && self.round != 1 {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-self.plating_decrement(owner));
                }
            }
            PowerId::Rampart => {
                // RampartPower.cs:23 excludes every player-only extra turn.
                if side == Side::Player && self.extra_turn_players == 0 {
                    let enemies = self.enemies.clone();
                    for enemy in enemies {
                        if self.cr(enemy).is_alive()
                            && self.cr(enemy).monster_id == "TURRET_OPERATOR"
                        {
                            self.power_gain_unpowered_block(enemy, amount);
                        }
                    }
                }
            }
            // SlowPower.AfterSideTurnStart resets only when Owner participates;
            // player-only extra turns must not reset an enemy's accumulated Slow.
            PowerId::Slow => {
                if self.cr(owner).side == side { self.powers[p as usize].vars[0] = 0; }
            }
            PowerId::Clarity => {
                if side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            PowerId::BlockNextTurn => {
                if side == self.cr(owner).side && self.cr(owner).block == 0 {
                    self.power_gain_unpowered_block(owner, amount);
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Radiance => {
                if side == self.cr(owner).side
                    && self.cr(owner).is_player
                    && self.power_private_int(owner, PowerId::Radiance, 0) != self.round as i32
                {
                    if !self.is_over_or_ending() {self.players[owner].energy += 1;}
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    self.powers[p as usize].vars[0]=self.round as i32;
                    if self.powers[p as usize].amount <= 0
                        && self.cr(owner).power_list().contains(&p)
                    {
                        self.remove_power(owner, p);
                    }
                }
            }
            PowerId::Sandpit => {
                // C# SandpitPower.AfterSideTurnStartLate: every Sandpit instance loses one at
                // the start of the enemy side. PowerCmd.Decrement removes that exact instance
                // immediately at zero (including instances held in Creature::extra).
                if side == Side::Enemy {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].should_remove()
                        && self.cr(owner).power_list().contains(&p)
                    {
                        self.remove_power(owner, p);
                    }
                }
            }
            _ => {
                if !self.chars_after_side_turn_start(p, owner, side)
                    && !self.defect_after_side_turn_start(p, owner, side)
                    && !self.regent_after_side_turn_start(p, owner, side)
                {
                    self.unported("after_side_turn_start");
                }
            }
        }
    }

    fn power_after_turn_end(&mut self, p: PRef, owner: CId, side: Side) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::Hellraiser => {
                if side == self.cr(owner).side {self.powers[p as usize].vars[0]=0;}
            }
            PowerId::MagicBomb => {
                // MagicBombPower.cs:21-28 is instanced and removes this exact
                // bomb after its owner's side ends, only while its caster lives.
                if side == self.cr(owner).side
                    && self.powers[p as usize].applier.is_some_and(|a| self.cr(a).is_alive()) {
                    self.deal_damage(Some(owner), owner, amount, Props::UNPOWERED);
                    self.remove_power(owner, p);
                }
            }
            PowerId::EscapeArtist => {
                if side == self.cr(owner).side && amount > 1 {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            // powers/common.py ShrinkPower.after_turn_end
            PowerId::Shrink => {
                if amount >= 0 && side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            PowerId::FeedingFrenzy => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        -amount,
                        None,
                        None,
                        false,
                        false,
                    );
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Hatch => {
                if side == Side::Enemy {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            // C# HighVoltagePower.AfterSideTurnEnd: Amount Strength at its owner's side turn end.
            PowerId::HighVoltage => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(owner, PowerId::Strength, amount, Some(owner), None, false, false);
                }
            }
            PowerId::BattlewornDummyTimeLimit => {
                // powers/remaining_a.py:278-288.
                if side == self.cr(owner).side {
                    if amount > 1 {
                        self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    } else {
                        self.escape_monster(owner);
                        self.powers[p as usize].amount = 0;
                    }
                }
            }
            PowerId::NoDraw => {
                self.powers[p as usize].amount = 0;
            }
            PowerId::OneTwoPunch => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Rage => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Ritual => {
                if side == self.cr(owner).side {
                    if self.powers[p as usize].skip_next_tick {
                        self.powers[p as usize].skip_next_tick = false;
                    } else {
                        self.creature_apply_power(
                            owner,
                            PowerId::Strength,
                            amount,
                            Some(owner),
                            None,
                            false,
                            false,
                        );
                    }
                }
            }
            PowerId::Rebound => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::SetupStrike => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        -amount,
                        None,
                        None,
                        false,
                        false,
                    );
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Territorial => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        amount,
                        None,
                        None,
                        false,
                        false,
                    );
                }
            }
            PowerId::FlexPotion => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner, PowerId::Strength, -amount, Some(owner), None, false, false,
                    );
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Skittish => {
                if side != self.cr(owner).side {
                    self.powers[p as usize].skip_next_tick = false;
                }
            }
            PowerId::FlameBarrier => {
                if side != self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::RetainHand => {
                if side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            PowerId::Smoggy => {
                // SmoggyPower.cs:46-59 clears only Smog, for participating Owner.
                if side == self.cr(owner).side {
                    self.clear_power_affliction(owner, CardAffliction::Smog, false);
                }
            }
            PowerId::SpeedPotion => {
                if side == self.cr(owner).side {
                    self.creature_apply_power(
                        owner, PowerId::Dexterity, -amount, Some(owner), None, false, false,
                    );
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Duplication => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Constrict => {
                if side == self.cr(owner).side {
                    self.deal_damage(Some(owner), owner, amount, Props::UNPOWERED);
                }
            }
            PowerId::Demise => {
                if side == self.cr(owner).side {
                    self.deal_damage(
                        None,
                        owner,
                        amount,
                        Props::UNBLOCKABLE.or(Props::UNPOWERED),
                    );
                }
            }
            PowerId::Grapple => {
                self.powers[p as usize].amount = 0;
            }
            PowerId::Nemesis => {
                if side == self.cr(owner).side {
                    let apply = self.power_private_int(owner, PowerId::Nemesis, 0) == 0;
                    self.set_power_private_int(owner, PowerId::Nemesis, 0, i32::from(apply));
                    if apply {
                        if let Some(intangible) = self.creature_apply_power(
                            owner,
                            PowerId::Intangible,
                            1,
                            Some(owner),
                            None,
                            false,
                            false,
                        ) {
                            self.powers[intangible as usize].skip_next_tick = true;
                        }
                    } else if let Some(intangible) = self.find_power(owner, PowerId::Intangible) {
                        self.remove_power(owner, intangible);
                    }
                }
            }
            PowerId::Asleep => {
                if side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0 {
                        if let Some(plating) = self.find_power(owner, PowerId::Plating) {
                            self.remove_power(owner, plating);
                        }
                        self.remove_power(owner, p);
                        if self.cr(owner).monster_id == "LAGAVULIN_MATRIARCH" {
                            if !self.power_set_enemy_state(owner, "SLASH_MOVE") {
                                self.unported("asleep natural wake state");
                            }
                        } else {
                            self.stun_enemy(owner);
                        }
                    }
                }
            }
            PowerId::Slumber => {
                if side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                        if self.cr(owner).monster_id == "SLUMBERING_BEETLE" {
                            if let Some(plating) = self.find_power(owner, PowerId::Plating) {
                                self.remove_power(owner, plating);
                            }
                            if !self.power_set_enemy_state(owner, "ROLL_OUT_MOVE") {
                                self.unported("slumber natural wake state");
                            }
                        }
                    }
                }
            }
            PowerId::Juggling => {
                if side == self.cr(owner).side {
                    self.set_power_private_int(owner, PowerId::Juggling, 0, 0);
                }
            }
            PowerId::DarkEmbrace => {
                if side == Side::Player {
                    let exhausted = self.power_private_int(owner, PowerId::DarkEmbrace, 0);
                    if exhausted > 0 {
                        let draw = self.powers[p as usize].amount * exhausted;
                        self.set_power_private_int(owner, PowerId::DarkEmbrace, 0, 0);
                        self.draw_cards(owner, draw, false);
                    }
                }
            }
            PowerId::Tender => {
                if side == Side::Player {
                    let cards_played = self.power_private_int(owner, PowerId::Tender, 0);
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        cards_played,
                        None,
                        None,
                        false,
                        false,
                    );
                    self.creature_apply_power(
                        owner,
                        PowerId::Dexterity,
                        cards_played,
                        None,
                        None,
                        false,
                        false,
                    );
                    self.set_power_private_int(owner, PowerId::Tender, 0, 0);
                }
            }
            PowerId::Strangle => {
                // C# StranglePower.AfterSideTurnEnd: only the owner's participants expire.
                if side == self.cr(owner).side {
                    self.remove_power(owner, p);
                }
            }
            PowerId::Tainted => {
                // powers/tainted.py: after the enemy side ends, Tainted expires.
                if side == Side::Enemy && self.cr(owner).powers.contains(&p) {
                    self.remove_power(owner, p);
                }
            }
            PowerId::NoBlock => {
                // powers/block_modifiers.py:71-89.
                if side == Side::Enemy {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            PowerId::Tangled => {
                // powers/remaining_c.py:1421-1432.
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                    self.clear_power_affliction(owner, CardAffliction::Entangled, false);
                }
            }
            PowerId::Ringing => {
                // powers/remaining_c.py:304-310.
                if side == self.cr(owner).side {
                    self.powers[p as usize].amount = 0;
                    self.clear_power_affliction(owner, CardAffliction::Ringing, false);
                }
            }
            _ => {
                if !self.chars_after_turn_end(p, owner, side)
                    && !self.defect_after_turn_end(p, owner, side)
                    && !self.regent_after_turn_end(p, owner, side)
                {
                    self.unported("after_turn_end");
                }
            }
        }
    }

    fn smog_afflict_card(&mut self, owner:CId, k:KRef) {
        let card=&mut self.cards[k as usize];
        if card.owner==owner && card.card_type==CardType::Skill
            && card.cv.affliction.is_none() && !card.bound {
            card.cv.affliction=Some(CardAffliction::Smog);
            card.cv.affliction_amount=1; // CardCmd.Afflict<Smog>(card, 1m).
        }
    }

    fn vital_spark_afflict_skill(&mut self, k: KRef, amount: i32, stack: bool) {
        if self.cards[k as usize].card_type != CardType::Skill {
            return;
        }
        let cv = &mut self.cards[k as usize].cv;
        match cv.affliction {
            Some(CardAffliction::Tainted) if stack => cv.affliction_amount += amount,
            Some(_) => {}
            None => {
                cv.affliction = Some(CardAffliction::Tainted);
                cv.affliction_amount = amount;
            }
        }
    }

    fn hex_afflict_card(&mut self, k: KRef) {
        // powers/remaining_b.py:415-428 + cards/base.py:402-412.
        if self.cards[k as usize].cv.affliction.is_some() || self.cards[k as usize].bound {
            return;
        }
        let added_ethereal = !self.cards[k as usize].is_ethereal();
        let card = &mut self.cards[k as usize];
        card.cv.affliction = Some(CardAffliction::Hexed);
        card.cv.affliction_amount = 0;
        if added_ethereal {
            card.keywords |= KW_ETHEREAL;
            card.cv.affliction_added_ethereal = true;
        }
    }

    fn tangled_afflict_card(&mut self, owner: CId, k: KRef) {
        // powers/remaining_c.py:1359-1377.
        let card = &mut self.cards[k as usize];
        if card.owner != owner
            || card.card_type != CardType::Attack
            || card.cv.affliction.is_some()
            || card.bound
        {
            return;
        }
        card.cv.affliction = Some(CardAffliction::Entangled);
        card.cv.affliction_amount = 1; // TangledPower.cs:30,38 Afflict<Entangled>(1m).
    }

    fn ringing_afflict_card(&mut self, owner: CId, k: KRef) {
        // RingingPower.cs AfterApplied / AfterCardEnteredCombat: Afflict<Ringing>(1m).
        let card = &mut self.cards[k as usize];
        if card.owner != owner || card.cv.affliction.is_some() || card.bound {
            return;
        }
        card.cv.affliction = Some(CardAffliction::Ringing);
        card.cv.affliction_amount = 1;
    }

    fn clear_power_affliction(
        &mut self,
        owner: CId,
        affliction: CardAffliction,
        clear_hex_ethereal: bool,
    ) {
        for k in self.pile_cards(owner) {
            if self.cards[k as usize].owner != owner {
                continue;
            }
            if clear_hex_ethereal && self.cards[k as usize].cv.affliction_added_ethereal {
                self.cards[k as usize].keywords &= !KW_ETHEREAL;
                self.cards[k as usize].cv.affliction_added_ethereal = false;
            }
            if self.cards[k as usize].cv.affliction == Some(affliction) {
                self.cards[k as usize].cv.affliction = None;
                self.cards[k as usize].cv.affliction_amount = 0;
                if affliction == CardAffliction::Bound {
                    self.cards[k as usize].bound = false;
                }
            }
        }
    }

    fn power_on_card_drawn(&mut self, p: PRef, owner: CId, k: KRef) {
        match self.powers[p as usize].id {
            // C# ConfusedPower.AfterCardDrawn / NextEnergyCost: canonical, not current cost.
            PowerId::Confused => {
                let card = &self.cards[k as usize];
                if card.owner == owner
                    && card_info(card.id, false).is_some_and(|info| info.cost >= 0 || info.x_cost)
                {
                    let cost = self.run.rngs.combat_energy_costs.next_int(0, 3) as i32;
                    // SetThisCombat appends an absolute modifier after earlier discounts.
                    // Writing only Card.cost would leave the ordered cost model unchanged.
                    self.cards[k as usize].set_energy_cost(cost, crate::card::CostExpiry::Combat, false);
                }
            }
            PowerId::Automation => {
                // powers/remaining_a.py:233-254.
                if self.cards[k as usize].owner != owner {
                    return;
                }
                let energy = self.automation_on_draw(owner);
                if energy <= 0 {
                    return;
                }
                if !self.players.is_player(owner) {
                    self.unported("automation non-player energy");
                } else if self.cr(owner).is_alive() && !self.is_over_or_ending() {
                    self.players[owner].energy += energy;
                }
            }
            PowerId::ChainsOfBinding => {
                // powers/remaining_a.py:482-492.
                if self.cards[k as usize].owner != owner || self.side != self.cr(owner).side {
                    return;
                }
                let count = self.power_private_int(owner, PowerId::ChainsOfBinding, 0);
                if count >= self.powers[p as usize].amount {
                    return;
                }
                let card = &mut self.cards[k as usize];
                if card.cv.affliction.is_some() || card.bound {
                    return;
                }
                card.cv.affliction = Some(CardAffliction::Bound);
                card.cv.affliction_amount = 0;
                card.bound = true;
                self.set_power_private_int(owner, PowerId::ChainsOfBinding, 0, count + 1);
            }
            _ => {
                if !self.chars_on_card_drawn(p, owner, k) && !self.defect_on_card_drawn(p, owner, k) {
                    self.unported("on_card_drawn");
                }
            }
        }
    }

    fn power_before_combat_start(&mut self, p: PRef, _owner: CId) {
        match self.powers[p as usize].id {
            PowerId::Galvanic => {
                for k in self.all_players_pile_cards() {
                    self.galvanic_afflict_card(k);
                }
            }
            PowerId::VitalSpark => {
                // powers/remaining_c.py VitalSparkPower.before_combat_start.
                let amount = self.powers[p as usize].amount;
                for k in self.all_players_pile_cards() {
                    self.vital_spark_afflict_skill(k, amount, true);
                }
            }
            _ => self.unported("before_combat_start"),
        }
    }

    fn power_before_card_played(&mut self, p: PRef, owner: CId, k: KRef) {
        match self.powers[p as usize].id {
            PowerId::ChainsOfBinding => {
                // powers/remaining_a.py:517-527.
                if !self.cards[k as usize].cv.is_clone
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].bound
                {
                    self.set_power_private_int(owner, PowerId::ChainsOfBinding, 1, 1);
                }
            }
            // powers/monster.py:1180-1211 SurroundedPower.before_card_played.
            PowerId::Surrounded => {
                if self.cards[k as usize].owner != owner {
                    return;
                }
                let target = self.pending_play.last().and_then(|frame| frame.target);
                self.power_surrounded_face_target(owner, target);
            }
            PowerId::FreeAttack => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                    && self.powers[p as usize].amount > 0
                {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            // C# SlothPower.BeforeCardPlayed: the power's own lifetime counter.
            PowerId::Sloth => {
                if self.cards[k as usize].owner == owner { self.powers[p as usize].vars[0]+=1; }
            }
            PowerId::Rupture => {
                if self.cards[k as usize].owner == owner && self.side == self.cr(owner).side {
                    self.set_power_private_int(owner, PowerId::Rupture, 0, k as i32 + 1);
                    self.set_power_private_int(owner, PowerId::Rupture, 1, 0);
                }
            }
            PowerId::Strangle => {
                // C# StranglePower.BeforeCardPlayed: snapshot per card and per applier.
                if self.powers[p as usize].applier.is_some_and(|a|
                    self.players.is_player(a) && self.cards[k as usize].owner == a)
                {
                    if self.strangle_cards.iter().any(|&(power, card, _)| power == p && card == k) {
                        self.unported("Strangle duplicate in-flight card");
                    } else {
                        self.strangle_cards.push((p, k, self.powers[p as usize].amount));
                    }
                }
            }
            _ => {
                if !self.chars_before_card_played(p, owner, k)
                    && !self.defect_before_card_played(p, owner, k)
                    && !self.regent_before_card_played(p, owner, k)
                {
                    self.unported("before_card_played");
                }
            }
        }
    }

    fn power_before_potion_used(&mut self, p: PRef, owner: CId) {
        match self.powers[p as usize].id {
            // powers/monster.py:1213-1220 SurroundedPower.before_potion_used. The potion
            // fire site exposes its resolved target through Combat::active_target.
            PowerId::Surrounded => self.power_surrounded_face_target(owner, self.active_target),
            _ => self.unported("before_potion_used"),
        }
    }

    fn power_surrounded_face_target(&mut self, owner: CId, target: Option<CId>) {
        let Some(target) = target else { return };
        if self.has_power(target, PowerId::BackAttackLeft) {
            self.set_power_private_int(owner, PowerId::Surrounded, 0, 1);
        } else if self.has_power(target, PowerId::BackAttackRight) {
            self.set_power_private_int(owner, PowerId::Surrounded, 0, 0);
        }
    }

    fn power_after_turn_end_late(&mut self, p: PRef, owner: CId, side: Side) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::Disintegration => {
                if side == self.cr(owner).side {
                    let props = Props::UNPOWERED;
                    let damage = self.calculate_damage(amount, Some(owner), owner, props);
                    self.apply_damage(owner, damage, props, Some(owner));
                    self.flush_pending_attack();
                }
            }
            _ => self.unported("after_turn_end_late"),
        }
    }

    fn power_after_card_played(&mut self, p: PRef, owner: CId, k: KRef) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::Galvanic => {
                if self.cards[k as usize].cv.affliction == Some(CardAffliction::Galvanized) {
                    let target = self.cards[k as usize].owner;
                    self.deal_damage(None, target, amount, Props::UNPOWERED.or(Props::MOVE));
                }
            }
            PowerId::Enrage => {
                if self.cards[k as usize].card_type == CardType::Skill {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        amount,
                        None,
                        None,
                        false,
                        false,
                    );
                }
            }
            PowerId::Rage => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            PowerId::Slow => self.powers[p as usize].vars[0] += 1,
            PowerId::Smoggy => {
                // SmoggyPower.cs:22-36: afflict every un-afflicted owner Skill.
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill {
                    for card in self.pile_cards(owner) { self.smog_afflict_card(owner, card); }
                }
            }
            PowerId::CurlUp => {
                let tracked = self.power_private_int(owner, PowerId::CurlUp, 0);
                if tracked == k as i32 + 1 {
                    self.set_power_private_int(owner, PowerId::CurlUp, 0, 0);
                    self.power_gain_unpowered_block(owner, amount);
                    self.remove_power(owner, p);
                }
            }
            PowerId::Juggling => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    let count = self.power_private_int(owner, PowerId::Juggling, 0) + 1;
                    self.set_power_private_int(owner, PowerId::Juggling, 0, count);
                    if count == 3 {
                        let copies = self.powers[p as usize].amount.max(0);
                        for _ in 0..copies {
                            let iid = self.run.new_card_id();
                            let clone = self.cards[k as usize].clone_with_id(iid);
                            let g = self.cards.len() as KRef;
                            self.card_hook_mask |= CARD_HOOKS[clone.id.index()];
                            self.cards.push(clone);
                            self.add_generated_card(g, PileKind::Hand, PilePos::None, true);
                        }
                    }
                }
            }
            PowerId::Rupture => {
                if self.cards[k as usize].owner == owner {
                    let tracked = self.power_private_int(owner, PowerId::Rupture, 0);
                    if tracked == k as i32 + 1 {
                        let pending = self.power_private_int(owner, PowerId::Rupture, 1);
                        self.set_power_private_int(owner, PowerId::Rupture, 0, 0);
                        self.set_power_private_int(owner, PowerId::Rupture, 1, 0);
                        if pending > 0 {
                            self.creature_apply_power(
                                owner,
                                PowerId::Strength,
                                pending,
                                Some(owner),
                                None,
                                false,
                                false,
                            );
                        }
                    }
                }
            }
            PowerId::Tender => {
                if self.cards[k as usize].owner == owner
                    && self.pending_play.last().is_some_and(|frame| frame.remaining == 0)
                {
                    let cards_played = self.power_private_int(owner, PowerId::Tender, 0) + 1;
                    self.set_power_private_int(owner, PowerId::Tender, 0, cards_played);
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        -1,
                        None,
                        None,
                        false,
                        false,
                    );
                    self.creature_apply_power(
                        owner,
                        PowerId::Dexterity,
                        -1,
                        None,
                        None,
                        false,
                        false,
                    );
                }
            }
            PowerId::Strangle => {
                // C# StranglePower.AfterCardPlayed removes the snapshot before damage.
                if let Some(i) = self.strangle_cards.iter().position(|&(power, card, _)| power == p && card == k) {
                    let (_, _, recorded) = self.strangle_cards.remove(i);
                    self.deal_damage(None, owner, recorded, Props::UNBLOCKABLE.or(Props::UNPOWERED));
                }
            }
            // powers/remaining_c.py VitalSparkPower.after_card_played.
            PowerId::VitalSpark => {
                if self.cards[k as usize].cv.affliction == Some(CardAffliction::Tainted) {
                    let target = self.cards[k as usize].owner;
                    self.apply_power_to(target, PowerId::Tainted, amount, None);
                }
            }
            PowerId::WitheringPresence => {
                // powers/monster.py:1676-1691; C# WitheringPresencePower.AfterCardPlayed: only a
                // card of this instance's Target player counts; the Wither goes to that player.
                let player = self.cards[k as usize].owner;
                if self.powers[p as usize].target != Some(player) {
                    return;
                }
                let mut left = self.powers[p as usize].vars[0];
                if left <= 0 {
                    left = 6;
                }
                left -= 1;
                if left <= 0 {
                    if let Some(g) = self.new_card(player, CardId::Wither, false) {
                        self.add_generated_card(g, PileKind::Hand, PilePos::None, false);
                    }
                    left = 6;
                }
                self.powers[p as usize].vars[0] = left;
            }
            _ => {
                if !self.chars_after_card_played(p, owner, k)
                    && !self.defect_after_card_played(p, owner, k)
                    && !self.regent_after_card_played(p, owner, k)
                {
                    self.unported("after_card_played");
                }
            }
        }
    }

    fn power_after_card_entered_combat(&mut self, p: PRef, _owner: CId, k: KRef) {
        match self.powers[p as usize].id {
            PowerId::Galvanic => self.galvanic_afflict_card(k),
            PowerId::Smoggy => {
                // SmoggyPower.cs:38-44 uses plays STARTED this turn, including
                // the currently resolving Skill that generated the new card.
                if self.hist.play_start_cards.iter().any(|&played|
                    self.cards[played as usize].owner == _owner
                    && self.cards[played as usize].card_type == CardType::Skill) {
                    self.smog_afflict_card(_owner,k);
                }
            }
            PowerId::Hex => {
                let owner = self.powers[p as usize].owner;
                if self.cards[k as usize].owner == owner {
                    self.hex_afflict_card(k);
                }
            }
            PowerId::Tangled => {
                let owner = self.powers[p as usize].owner;
                self.tangled_afflict_card(owner, k);
            }
            PowerId::Ringing => {
                let owner = self.powers[p as usize].owner;
                self.ringing_afflict_card(owner, k);
            }
            // powers/remaining_c.py VitalSparkPower.after_card_entered_combat.
            PowerId::VitalSpark => {
                if self.cards[k as usize].cv.affliction.is_none()
                    && self.cards[k as usize].card_type == CardType::Skill
                {
                    self.cards[k as usize].cv.affliction = Some(CardAffliction::Tainted);
                    self.cards[k as usize].cv.affliction_amount = self.powers[p as usize].amount;
                }
            }
            _ => self.unported("after_card_entered_combat"),
        }
    }

    fn power_after_card_exhausted(
        &mut self,
        p: PRef,
        owner: CId,
        k: KRef,
        exhausted_by_ethereal: bool,
    ) {
        match self.powers[p as usize].id {
            PowerId::FeelNoPain => {
                let amount = self.powers[p as usize].amount;
                self.power_gain_unpowered_block(owner, amount);
            }
            PowerId::DarkEmbrace => {
                if self.cards[k as usize].owner != owner {
                    return;
                }
                if exhausted_by_ethereal {
                    let count = self.power_private_int(owner, PowerId::DarkEmbrace, 0) + 1;
                    self.set_power_private_int(owner, PowerId::DarkEmbrace, 0, count);
                } else {
                    let amount = self.powers[p as usize].amount;
                    self.draw_cards(owner, amount, false);
                }
            }
            _ => self.unported("after_card_exhausted"),
        }
    }

    /// Card listeners fire in flow.rs first, followed by power listeners and relics.
    /// exhausted_by_ethereal mirrors r7's temporary card combat-var marker.
    pub(crate) fn fire_power_relic_after_card_exhausted(
        &mut self,
        k: KRef,
        exhausted_by_ethereal: bool,
    ) {
        // Hook.IterateCombatHookListeners rejects NEW snapshots while
        // IsOverOrEnding && !IsStarting. CombatManager.StartCombat clears
        // IsStarting before BeforeCombatStart; combat_started is set at that
        // same boundary here. Never recheck inside a suspended snapshot.
        if self.combat_started && self.is_over_or_ending() { return; }
        let mut listeners = Vec::new();
        for owner in self.all_creatures() {
            listeners.extend(self.listeners_of(owner, Hook::AfterCardExhausted)
                .into_iter().map(|p| TurnStartListener::Power(owner, p)));
            if self.players.is_player(owner) {
                listeners.extend(self.run.relics.iter().enumerate().filter(|(_, r)|
                    r.owner == owner && r.enabled && self.players[owner].hooks_active
                    && relic_hook_mask(r.id) & (1u128 << Hook::AfterCardExhausted as u32) != 0)
                    .map(|(r, _)| TurnStartListener::Relic(r)));
            }
        }
        let mut walk = ExhaustWalk { listeners, next: 0, card: k,
            ethereal: exhausted_by_ethereal, joss_modulo: None };
        if !self.resume_exhaust_walk(&mut walk) {
            self.pending_exhaust_walks.push((self.pending_play.len(), walk));
        }
    }

    pub(crate) fn resume_exhaust_walk(&mut self, walk: &mut ExhaustWalk) -> bool {
        // Hook.cs:164-174 awaits listeners in order. JossPaper.cs:125-130
        // reduces its counter only after the suspended Draw has completed.
        if let Some(r) = walk.joss_modulo.take() { self.run.relics[r].counter %= 5; }
        while let Some(listener) = walk.listeners.get(walk.next).copied() {
            walk.next += 1;
            match listener {
                TurnStartListener::Power(owner, p) => {
                    if self.cr(owner).power_list().contains(&p) {
                        self.power_after_card_exhausted(p, owner, walk.card, walk.ethereal);
                    }
                }
                TurnStartListener::Relic(r) => {
                    if !self.run.relics[r].enabled { continue; }
                    if self.run.relics[r].id == RelicId::JossPaper {
                        if self.relic_joss_after_exhaust(r, walk.card, walk.ethereal) {
                            if self.pending_choice.is_some() { walk.joss_modulo = Some(r); }
                            else { self.run.relics[r].counter %= 5; }
                        }
                    } else {
                        self.relic_event(r, Hook::AfterCardExhausted, Ev::Card(walk.card));
                    }
                }
                _ => unreachable!(),
            }
            if self.pending_choice.is_some() || self.fault.is_some() { return false; }
        }
        true
    }

    fn power_after_energy_reset(&mut self, p: PRef, owner: CId, reset_owner: CId) {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            PowerId::EnergyNextTurn => {
                if reset_owner == owner && self.cr(owner).is_player && amount != 0 {
                    // PlayerCmd.GainEnergy gains nothing once CombatManager.IsEnding (e.g. a
                    // Lightning Rod evoke earlier in this AfterEnergyReset killed the last enemy).
                    if amount > 0 && !self.is_over_or_ending() {
                        self.players[owner].energy += amount;
                    }
                    self.powers[p as usize].amount = 0;
                }
            }
            PowerId::Radiance => {
                if reset_owner == owner && self.cr(owner).is_player {
                    if !self.is_over_or_ending() {self.players[owner].energy += 1;}
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    // The listener snapshot survives combat-end power removal.
                    self.powers[p as usize].vars[0]=self.round as i32;
                }
            }
            _ => {
                if !self.defect_after_energy_reset(p, owner) && !self.regent_after_energy_reset(p, owner) {
                    self.unported("after_energy_reset");
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn power_after_power_amount_changed(
        &mut self,
        p: PRef,
        owner: CId,
        target: CId,
        changed_id: PowerId,
        delta: i32,
        applier: Option<CId>,
        source: Option<KRef>,
    ) {
        match self.powers[p as usize].id {
            PowerId::Dampen => {
                if owner == target
                    && changed_id == PowerId::Dampen
                    && delta > 0
                    && self.powers[p as usize].payload == 0
                {
                    self.powers[p as usize].payload = 1;
                    let cards = self.pile_cards(owner);
                    for k in cards {
                        if self.cards[k as usize].upgraded {
                            if self.downgrade_card_for_dampen(k)
                                && self.has_power(owner, PowerId::Hex)
                            {
                                self.cards[k as usize].keywords |= KW_ETHEREAL;
                            }
                        }
                    }
                }
            }
            PowerId::FeedingFrenzy => {
                if target == owner
                    && changed_id == PowerId::FeedingFrenzy
                    && delta != 0
                    && !self.consume_ignore_next_instance(p)
                {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        delta,
                        applier,
                        source,
                        false,
                        false,
                    );
                }
            }
            PowerId::Ritual => {
                if target == owner
                    && changed_id == PowerId::Ritual
                    && delta > 0
                    && self.cr(owner).side == Side::Enemy
                    && self.powers[p as usize].amount == delta
                {
                    self.powers[p as usize].skip_next_tick = true;
                }
            }
            PowerId::SetupStrike => {
                if target == owner
                    && changed_id == PowerId::SetupStrike
                    && delta != 0
                    // TemporaryStrengthPower.cs:124: initial application was
                    // already handled by BeforeApplied, before attachment.
                    && delta != self.powers[p as usize].amount
                    && !self.consume_ignore_next_instance(p)
                {
                    self.creature_apply_power(
                        owner,
                        PowerId::Strength,
                        delta,
                        applier,
                        source,
                        false,
                        false,
                    );
                }
            }
            PowerId::Vicious => {
                if delta > 0 && applier == Some(owner) && changed_id == PowerId::Vulnerable {
                    let amount = self.powers[p as usize].amount;
                    self.draw_cards(owner, amount, false);
                }
            }
            PowerId::FlexPotion => {
                if target == owner
                    && changed_id == PowerId::FlexPotion
                    && delta != 0
                    // TemporaryStrengthPower.BeforeApplied already applied the
                    // first stack's Strength before this power was attached.
                    && delta != self.powers[p as usize].amount
                    && !self.consume_ignore_next_instance(p)
                {
                    self.creature_apply_power(
                        owner, PowerId::Strength, delta, applier, source, false, false,
                    );
                }
            }
            PowerId::SpeedPotion => {
                if target == owner
                    && changed_id == PowerId::SpeedPotion
                    && delta != 0
                    && !self.consume_ignore_next_instance(p)
                {
                    self.creature_apply_power(
                        owner, PowerId::Dexterity, delta, applier, source, false, false,
                    );
                }
            }
            // C# TankPower.AfterApplied is handled only on fresh application in Combat.
            // GuardedPower.AfterApplied only sets the localized applier name.
            PowerId::Tank | PowerId::Guarded => {}
            // Rust stores Grapple's applier on the primary power. In solo play every stack comes
            // from the same Ironclad, so no per-applier instance list is needed.
            PowerId::Grapple => {}
            PowerId::PossessSpeed => {
                if applier == Some(owner)
                    && changed_id == PowerId::Dexterity
                    && delta < 0
                    && self.cr(target).is_player
                {
                    self.record_stolen_power(p, target, -delta);
                }
            }
            PowerId::PossessStrength => {
                if applier == Some(owner)
                    && changed_id == PowerId::Strength
                    && delta < 0
                    && self.cr(target).is_player
                {
                    self.record_stolen_power(p, target, -delta);
                }
            }
            PowerId::MagicBomb => {} // No C# AfterPowerAmountChanged override.
            PowerId::Illusion => {
                // powers/monster.py:657-670 IllusionPower.after_power_amount_changed.
                if target == owner
                    && changed_id == PowerId::Illusion
                    && self.powers[p as usize].amount > 0
                    && !self.has_power(owner, PowerId::Minion)
                {
                    self.creature_apply_power(
                        owner, PowerId::Minion, 1, applier, source, false, false,
                    );
                }
            }
            PowerId::Automation => {
                // powers/remaining_a.py:218-232. Every positive application becomes an
                // independent 10-card countdown; r7 leaves the visible amount at that delta.
                if target == owner && changed_id == PowerId::Automation && delta > 0 {
                    // C# AutomationPower.Data starts at 10; frozen r7's
                    // representative cards_left survives _add_instance.
                    if self.powers[p as usize].vars[0] == 0 {
                        self.powers[p as usize].vars[0] = 10;
                    }
                    self.automation_add_instance(owner, delta);
                    self.powers[p as usize].amount = delta;
                }
            }
            PowerId::Hex => {
                // powers/remaining_b.py:397-413.
                if target == owner
                    && changed_id == PowerId::Hex
                    && delta > 0
                    && self.power_private_int(owner, PowerId::Hex, 0) == 0
                {
                    self.set_power_private_int(owner, PowerId::Hex, 0, 1);
                    for k in self.pile_cards(owner) {
                        if self.cards[k as usize].owner == owner {
                            self.hex_afflict_card(k);
                        }
                    }
                }
            }
            PowerId::Tangled => {
                // powers/remaining_c.py:1384-1400.
                if target == owner
                    && changed_id == PowerId::Tangled
                    && delta > 0
                    && self.power_private_int(owner, PowerId::Tangled, 0) == 0
                {
                    self.set_power_private_int(owner, PowerId::Tangled, 0, 1);
                    for k in self.pile_cards(owner) {
                        self.tangled_afflict_card(owner, k);
                    }
                }
            }
            PowerId::Ringing => {
                // powers/remaining_c.py:284-295.
                if target == owner && changed_id == PowerId::Ringing && delta > 0 {
                    for k in self.pile_cards(owner) {
                        self.ringing_afflict_card(owner, k);
                    }
                }
            }
            PowerId::Juggling => {
                if target == owner && changed_id == PowerId::Juggling && delta > 0 {
                    let attacks = self
                        .hist
                        .played_this_turn
                        .iter()
                        .filter(|&&(actor, played)| {
                            actor == owner
                                && self.cards[played as usize].card_type == CardType::Attack
                        })
                        .count() as i32;
                    self.set_power_private_int(owner, PowerId::Juggling, 0, attacks);
                }
            }
            PowerId::VitalSpark => {
                // powers/remaining_c.py VitalSparkPower.after_power_amount_changed.
                if target == owner && changed_id == PowerId::VitalSpark {
                    let amount = self.powers[p as usize].amount;
                    for k in self.all_players_pile_cards() {
                        if self.cards[k as usize].cv.affliction == Some(CardAffliction::Tainted) {
                            self.cards[k as usize].cv.affliction_amount = amount;
                        }
                    }
                }
            }
            _ => {
                if !self.chars_after_power_amount_changed(p, owner, target, changed_id, delta, applier, source)
                    && !self.defect_after_power_amount_changed(p, owner, target, changed_id, delta, applier, source)
                {
                    self.unported("after_power_amount_changed");
                }
            }
        }
    }

    fn power_after_damage_received(
        &mut self,
        p: PRef,
        owner: CId,
        target: CId,
        dealer: Option<CId>,
        damage: i32,
        _total_damage: i32,
        blocked_damage: i32,
        props: Props,
    ) {
        match self.powers[p as usize].id {
            // C# TheGambitPower.AfterDamageReceived: remove before normal (preventable) Kill.
            PowerId::TheGambit => {
                if target == owner && damage > 0 && props.is_powered_attack() {
                    self.remove_power(owner, p);
                    self.kill_creature(owner);
                }
            }
            // C# ReflectPower.AfterDamageReceived: reflect blocked damage, not HP lost.
            PowerId::Reflect => {
                if target == owner && props.is_powered_attack() && let Some(dealer) = dealer {
                    if blocked_damage > 0 {
                        self.deal_damage(Some(owner), dealer, blocked_damage, Props::UNPOWERED);
                    }
                }
            }
            PowerId::FlameBarrier => {
                if target == owner && let Some(d) = dealer && props.is_powered_attack() {
                    let amount = self.powers[p as usize].amount;
                    self.deal_damage(Some(owner), d, amount, Props::UNPOWERED);
                }
            }
            PowerId::Inferno => {
                if target == owner && damage > 0 && self.side == self.cr(owner).side {
                    let amount = self.powers[p as usize].amount;
                    let enemies = self.hittable_enemies();
                    for enemy in enemies {
                        self.deal_damage(Some(owner), enemy, amount, Props::UNPOWERED);
                    }
                }
            }
            // Frozen r7 powers/monster.py HardenedShellPower.after_damage_received:
            // track actual HP loss, including damage received after its cap modifier ran.
            PowerId::HardenedShell => {
                if target == owner && damage > 0 {
                    self.powers[p as usize].vars[0] += damage;
                    if self.powers[p as usize].vars[0] >= self.powers[p as usize].amount {
                        self.cr_mut(owner).hp_display_infinite = true;
                    }
                }
            }
            PowerId::CurlUp => {
                if target == owner
                    && props.is_powered_attack()
                    && dealer.is_some()
                    && self.power_private_int(owner, PowerId::CurlUp, 0) == 0
                    && let Some(k) = self.active_card
                {
                    self.set_power_private_int(owner, PowerId::CurlUp, 0, k as i32 + 1);
                }
            }
            // SlipperyPower.AfterDamageReceived: only hp actually lost (`UnblockedDamage >= 1`).
            PowerId::Slippery => {
                if target == owner && damage >= 1 {
                    self.powers[p as usize].amount -= 1;
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            PowerId::Shriek => {
                if target == owner
                    && damage > 0
                    && self.cr(owner).hp <= self.powers[p as usize].amount
                {
                    self.remove_power(owner, p);
                    if self.cr(owner).monster_id == "TERROR_EEL" {
                        if !self.power_stun_enemy_with_follow_up(owner, "TERROR_MOVE", crate::monster::StunMove::Nothing) {
                            self.unported("shriek stun state");
                        }
                    } else {
                        self.stun_enemy(owner);
                    }
                }
            }
            PowerId::Plow => {
                if target == owner
                    && damage > 0
                    && self.cr(owner).hp <= self.powers[p as usize].amount
                {
                    if let Some(strength) = self.find_power(owner, PowerId::Strength) {
                        self.remove_power(owner, strength);
                    }
                    self.remove_power(owner, p);
                    if !self.power_stun_enemy_with_follow_up(owner, "BEAST_CRY_MOVE", crate::monster::StunMove::Nothing) {
                        self.stun_enemy(owner);
                    }
                }
            }
            PowerId::Flutter => {
                if target == owner && damage > 0 && props.is_powered_attack() {
                    self.powers[p as usize].amount -= 1;
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                        // C# FlutterPower.AfterDamageReceived supplies
                        // StateLog.Last().GetNextState(), not Last().Id.
                        let next = self.cr(owner).ai.as_ref().and_then(|ai|
                            ai.log.last().map(|&last| ai.follow_up(last)));
                        if let Some(next) = next {
                            self.stun_enemy_with_move(owner, crate::monster::StunMove::Nothing, Some(next));
                        } else {
                            self.unported("Flutter requires logged move follow-up");
                        }
                    }
                }
            }
            PowerId::Asleep => {
                if target == owner && damage > 0 {
                    if let Some(plating) = self.find_power(owner, PowerId::Plating) {
                        self.remove_power(owner, plating);
                    }
                    self.remove_power(owner, p);
                    if self.cr(owner).monster_id == "LAGAVULIN_MATRIARCH" {
                        if !self.power_stun_enemy_with_follow_up(owner, "SLASH_MOVE", crate::monster::StunMove::Nothing) {
                            self.unported("asleep damage wake stun");
                        }
                    } else {
                        self.stun_enemy(owner);
                    }
                }
            }
            PowerId::Slumber => {
                if target == owner && damage > 0 {
                    self.powers[p as usize].amount -= 1;
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                        if self.cr(owner).monster_id == "SLUMBERING_BEETLE" {
                            if !self.power_stun_enemy_with_follow_up(
                                owner, "ROLL_OUT_MOVE", crate::monster::StunMove::BeetleWakeUp,
                            ) {
                                self.unported("slumber damage wake stun");
                            }
                        }
                    }
                }
            }
            PowerId::PersonalHive => {
                // C# PersonalHivePower: Dazed for the dealing player (Osty's owner for Osty).
                let player = dealer.and_then(|d| if self.players.is_player(d) { Some(d) } else { self.cr(d).pet_owner });
                if let Some(player) = player
                    && target == owner
                    && props.is_powered_attack()
                {
                    let amount = self.powers[p as usize].amount.max(0);
                    for _ in 0..amount {
                        let _ = self.create_card_in_pile(
                            player,
                            CardId::Dazed,
                            false,
                            PileKind::Draw,
                            PilePos::Random,
                        );
                    }
                }
            }
            PowerId::Rupture => {
                if target == owner && damage > 0 && self.side == self.cr(owner).side {
                    let amount = self.powers[p as usize].amount;
                    let tracked = self.power_private_int(owner, PowerId::Rupture, 0);
                    if self.active_card.is_some_and(|k| tracked == k as i32 + 1) {
                        let pending = self.power_private_int(owner, PowerId::Rupture, 1) + amount;
                        self.set_power_private_int(owner, PowerId::Rupture, 1, pending);
                    } else {
                        self.creature_apply_power(
                            owner,
                            PowerId::Strength,
                            amount,
                            Some(owner),
                            None,
                            false,
                            false,
                        );
                    }
                }
            }
            _ => self.unported("after_damage_received"),
        }
    }

    fn power_after_damage_given(
        &mut self,
        p: PRef,
        owner: CId,
        target: CId,
        dealer: Option<CId>,
        damage: i32,
        total_damage: i32,
        props: Props,
    ) {
        match self.powers[p as usize].id {
            PowerId::Imbalanced => {
                // fire: core/damage.py:201 + powers/remaining_b.py:533-538.
                let fully_blocked = !props.has(Props::UNBLOCKABLE)
                    && (self.last_damage.blocked > 0 || self.cr(target).block > 0)
                    && damage == 0;
                if dealer == Some(owner) && fully_blocked {
                    if self.cr(owner).monster_id == "BOWLBUG_ROCK" {
                        self.note_bowlbug_rock_fully_blocked(owner);
                    } else {
                        self.stun_enemy(owner);
                    }
                }
            }
            PowerId::PaperCuts => {
                if dealer == Some(owner)
                    && self.cr(target).is_player
                    && props.is_powered_attack()
                    && damage > 0
                {
                    let amount = self.powers[p as usize].amount.max(0);
                    let old_hp = self.cr(target).hp;
                    let new_max = (self.cr(target).max_hp - amount).max(1);
                    let new_hp = self.cr(target).hp.min(new_max);
                    self.cr_mut(target).max_hp = new_max;
                    self.cr_mut(target).hp = new_hp;
                    if target == PLAYER {
                        self.run.max_hp = new_max;
                        self.run.hp = self.run.hp.min(new_max);
                    }
                    if new_hp != old_hp && !self.is_over {
                        // r7 core/creature.py:350-357 lose_max_hp.
                        self.fire(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: target, delta: new_hp - old_hp }, None);
                    }
                }
            }
            _ => {
                if !self.chars_after_damage_given(p, owner, target, dealer, damage, total_damage, props) {
                    self.unported("after_damage_given");
                }
            }
        }
    }

    fn power_after_player_turn_start(&mut self, p: PRef, owner: CId, turn_owner: CId) {
        if owner != turn_owner {
            return;
        }
        match self.powers[p as usize].id {
            PowerId::Entropy => {
                // C# EntropyPower.AfterPlayerTurnStart: select Amount from this player's hand.
                let hand = self.players[owner].hand.to_vec();
                let amount = self.powers[p as usize].amount.max(0) as u16;
                self.request_multi_card_choice_for_player(owner, &hand, crate::combat::PileKind::Hand,
                    crate::choice::Resolver { src: crate::choice::ChoiceSrc::Card(CardId::Entropy),
                        k: 0, target: Some(owner), step: 0, arg: 0 },
                    amount, Some(amount), false);
            }
            // C# RollingBoulderPower.AfterPlayerTurnStart / DoDamage: each instance grows by 5.
            PowerId::RollingBoulder => {
                let amount = self.powers[p as usize].amount;
                self.power_damage_all_enemies(owner, amount);
                self.powers[p as usize].amount += 5;
            }
            PowerId::Inferno => {
                let damage = self.power_self_damage(p);
                if damage > 0 {
                    self.deal_damage(
                        Some(owner),
                        owner,
                        damage,
                        Props::UNBLOCKABLE.or(Props::UNPOWERED),
                    );
                }
            }
            PowerId::CrimsonMantle => {
                if !self.cr(owner).is_player {
                    return;
                }
                let damage = self.power_self_damage(p);
                if damage > 0 {
                    self.deal_damage(
                        Some(owner),
                        owner,
                        damage,
                        Props::UNBLOCKABLE.or(Props::UNPOWERED),
                    );
                }
                let amount = self.powers[p as usize].amount;
                self.power_gain_unpowered_block(owner, amount);
            }
            _ => {
                if !self.chars_after_player_turn_start(p, owner) && !self.defect_after_player_turn_start(p, owner) {
                    self.unported("after_player_turn_start");
                }
            }
        }
    }

    fn power_before_attack(
        &mut self,
        p: PRef,
        owner: CId,
        attacker: Option<CId>,
        _target: Option<CId>,
        props: Props,
        card: Option<KRef>,
    ) {
        match self.powers[p as usize].id {
            // GigantificationPower.BeforeAttack binds the first qualifying command.
            // Active stack depth identifies it until the matching AfterAttack pops it.
            PowerId::Gigantification => {
                if props.is_powered_attack() && card.is_some_and(|k|
                    self.cards[k as usize].owner == owner && self.cards[k as usize].card_type == CardType::Attack)
                    && self.powers[p as usize].vars[0] == 0
                {
                    self.powers[p as usize].vars[0] = self.attack_stack.len() as i32;
                    self.powers[p as usize].vars[1] = card.unwrap() as i32 + 1;
                }
            }
            PowerId::Vigor => {
                if attacker != Some(owner)
                    || !props.is_powered_attack()
                    || self.power_private_int(owner, PowerId::Vigor, 1) != 0
                {
                    return;
                }
                // r7 tracks the AttackCommand object. Attack-stack depth is the exact command
                // identity needed here: nested attacks have a different depth and cannot consume
                // or receive this Vigor instance.
                self.set_power_private_int(
                    owner,
                    PowerId::Vigor,
                    1,
                    self.attack_stack.len() as i32,
                );
                self.set_power_private_int(
                    owner,
                    PowerId::Vigor,
                    2,
                    self.powers[p as usize].amount,
                );
            }
            _ => self.unported("before_attack"),
        }
    }

    fn power_after_attack(
        &mut self,
        p: PRef,
        owner: CId,
        attacker: Option<CId>,
        target: Option<CId>,
        props: Props,
        card: Option<KRef>,
    ) {
        match self.powers[p as usize].id {
            // GigantificationPower.AfterAttack: only the bound command consumes one,
            // not every hit, nested command, or every point of Amount.
            PowerId::Gigantification => {
                if self.powers[p as usize].vars[0] != 0
                    && self.powers[p as usize].vars[0] == self.attack_stack.len() as i32 + 1
                {
                    self.powers[p as usize].vars[0] = 0;
                    self.powers[p as usize].vars[1] = 0;
                    self.modify_power_instance(p, -1, None, None);
                }
            }
            PowerId::Suck => {
                if attacker != Some(owner) || !props.is_powered_attack() {
                    return;
                }
                let qualifying = self.power_private_int(owner, PowerId::Suck, 1);
                self.set_power_private_int(owner, PowerId::Suck, 1, 0);
                // PowerCmd.Apply does nothing once the combat is over (every player died).
                if qualifying > 0 && !self.is_over && !self.is_combat_ending_live() {
                    let amount = self.powers[p as usize].amount * qualifying;
                    self.creature_apply_power(
                        owner, PowerId::Strength, amount, Some(owner), None, false, false,
                    );
                }
            }
            PowerId::Skittish => {
                let was_hit = self.power_private_int(owner, PowerId::Skittish, 1) != 0;
                self.set_power_private_int(owner, PowerId::Skittish, 1, 0);
                if self.powers[p as usize].skip_next_tick || !props.has(Props::MOVE) || card.is_none() {
                    return;
                }
                if was_hit {
                    self.powers[p as usize].skip_next_tick = true;
                    let amount = self.powers[p as usize].amount;
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            PowerId::PainfulStabs => {
                if attacker != Some(owner) || !props.is_powered_attack() {
                    return;
                }
                // PainfulStabsPower.cs:34-63 groups ALL results in first-receiver
                // encounter order, then adds Amount Wounds per unblocked hit.
                let mut receivers: Vec<(CId, i32)> = Vec::new();
                for &(receiver, unblocked) in &self.completed_attack_results {
                    if !self.players.is_player(receiver) { continue; }
                    if let Some((_, hits)) = receivers.iter_mut().find(|(c, _)| *c == receiver) {
                        *hits += i32::from(unblocked > 0);
                    } else {
                        receivers.push((receiver, i32::from(unblocked > 0)));
                    }
                }
                for (receiver, hits) in receivers {
                    // CardPileCmd.AddToCombatAndPreview:1065-1069 returns BEFORE
                    // allocating cards when this attack killed the recipient.
                    if self.cr(receiver).is_dead() { continue; }
                    for _ in 0..(hits * self.powers[p as usize].amount).max(0) {
                        let _ = self.create_card_in_pile(receiver, CardId::Wound, false,
                            PileKind::Discard, PilePos::Bottom);
                    }
                }
            }
            PowerId::Vigor => {
                if attacker != Some(owner) || !props.is_powered_attack() {
                    return;
                }
                let tracked_depth = self.power_private_int(owner, PowerId::Vigor, 1);
                if tracked_depth == 0 || tracked_depth != self.attack_stack.len() as i32 + 1 {
                    return;
                }
                if self.is_over_or_ending() {
                    return;
                }
                let amount_when_started = self.power_private_int(owner, PowerId::Vigor, 2);
                self.set_power_private_int(owner, PowerId::Vigor, 1, 0);
                self.set_power_private_int(owner, PowerId::Vigor, 2, 0);
                if amount_when_started != 0 {
                    self.creature_apply_power(
                        owner,
                        PowerId::Vigor,
                        -amount_when_started,
                        Some(owner),
                        None,
                        false,
                        false,
                    );
                }
            }
            _ => self.unported("after_attack"),
        }
    }

    /// Packet fire-site companion for r7 `AttackContext.results` / `damage_result_groups`.
    /// The Rust damage engine has no redirection result expansion yet, so each recorded
    /// `apply_damage` result is one command result group for the current attack.
    pub(crate) fn power_record_attack_damage_result(
        &mut self,
        dealer: Option<CId>,
        target: CId,
        props: Props,
        unblocked: i32,
    ) {
        let Some(dealer) = dealer else {
            return;
        };
        if let Some(attack) = self.attack_stack.last_mut() {
            attack.results.push((target, unblocked));
        }
        if props.is_powered_attack()
            && unblocked > 0
            && self.cr(target).side != self.cr(dealer).side
            && let Some(s) = self.find_power(dealer, PowerId::Suck)
        {
            let pw = &mut self.powers[s as usize];
            if pw.vars[2] != 2 {
                pw.vars[1] += 1;
                if pw.vars[2] == 1 {
                    pw.vars[2] = 2;
                }
            }
        }
        if self.active_card.is_some()
            && props.has(Props::MOVE)
            && unblocked != 0
            && self.find_power(target, PowerId::Skittish).is_some()
        {
            self.set_power_private_int(target, PowerId::Skittish, 1, 1);
        }
    }

    fn power_after_block_gained(&mut self, p: PRef, owner: CId, creature: CId, amount: i32) {
        match self.powers[p as usize].id {
            PowerId::BeaconOfHope => {
                // C# BeaconOfHopePower.AfterBlockGained: share half the block during
                // the owner's side, with a per-instance recursion guard.
                if creature != owner || amount < 2 || self.side != self.cr(owner).side
                    || self.powers[p as usize].vars[0] != 0
                {
                    return;
                }
                self.powers[p as usize].vars[0] = 1;
                for player in 0..self.players.len() as CId {
                    if player != owner && self.cr(player).is_alive() {
                        self.power_gain_unpowered_block(player, amount / 2);
                    }
                }
                self.powers[p as usize].vars[0] = 0;
            }
            PowerId::Juggernaut => {
                if creature != owner
                    || amount <= 0
                    || self.cr(owner).is_dead()
                    || self.is_over_or_ending()
                {
                    return;
                }
                let enemies = self.hittable_enemies();
                if enemies.is_empty() {
                    return;
                }
                let index = self.run.rngs.combat_targets.choice_index(enemies.len());
                let target = enemies[index];
                let props = Props::UNPOWERED;
                let damage = self.calculate_damage(
                    self.powers[p as usize].amount,
                    Some(owner),
                    target,
                    props,
                );
                self.apply_damage(target, damage, props, Some(owner));
                self.flush_pending_attack();
            }
            PowerId::Grapple => {
                if amount > 0 && self.powers[p as usize].applier == Some(creature) {
                    let damage = self.powers[p as usize].amount;
                    self.deal_damage(Some(owner), owner, damage, Props::UNPOWERED);
                }
            }
            _ => self.unported("after_block_gained"),
        }
    }

    fn power_after_block_cleared(&mut self, p: PRef, owner: CId, creature: CId) {
        match self.powers[p as usize].id {
            // C# SelfFormingClayPower.AfterBlockCleared.
            PowerId::SelfFormingClay => {
                if creature == owner {
                    let amount = self.powers[p as usize].amount;
                    self.power_gain_unpowered_block(owner, amount);
                    self.remove_power(owner, p);
                }
            }
            PowerId::BlockNextTurn => {
                if creature == owner {
                    let amount = self.powers[p as usize].amount;
                    self.power_gain_unpowered_block(owner, amount);
                    self.powers[p as usize].amount = 0;
                    if self.cr(owner).power_list().contains(&p) {
                        self.remove_power(owner, p);
                    }
                }
            }
            PowerId::ToricToughness => {
                // powers/remaining_c.py:1622-1635. Each instance carries its own
                // SetBlock value in payload and decrements independently.
                if creature == owner {
                    let block = self.powers[p as usize].payload;
                    if block > 0 {
                        self.power_gain_unpowered_block(owner, block);
                    }
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0
                        && self.cr(owner).power_list().contains(&p)
                    {
                        self.remove_power(owner, p);
                    }
                }
            }
            _ => self.unported("after_block_cleared"),
        }
    }

    fn power_stun_enemy_with_follow_up(&mut self, owner: CId, state_id: &str, body: crate::monster::StunMove) -> bool {
        let follow_up = {
            let Some(ai) = self.cr(owner).ai.as_ref() else {
                return false;
            };
            let Some(index) = ai.def().states.iter().position(|s| s.id == state_id) else {
                return false;
            };
            index as u8
        };
        self.stun_enemy_with_move(owner, body, Some(follow_up))
    }

    fn power_set_enemy_state(&mut self, owner: CId, state_id: &str) -> bool {
        let state = {
            let Some(ai) = self.cr(owner).ai.as_ref() else {
                return false;
            };
            let Some(index) = ai.def().states.iter().position(|s| s.id == state_id) else {
                return false;
            };
            index as u8
        };
        if let Some(ai) = self.cr_mut(owner).ai.as_mut() {
            // C# MonsterModel.SetMoveImmediate cannot replace an unperformed
            // must-perform move (including an injected STUNNED).
            if !ai.can_transition_away(ai.current) {
                return true;
            }
            // C# RollMove after a performed injected stun first selects its
            // explicit SNORE/SLEEP follow-up, even if WakeUpMove just ran.
            // Rust's early roll already made precisely that selection.
            if ai.rolled_stun_follow_up {
                return true;
            }
            // Natural Asleep/Slumber wake: Rust rolled the sleeping move before
            // AfterSideTurnEnd. C# rolls after WakeUpMove at player StartTurn,
            // so replace that premature log entry, not append a second roll.
            ai.log.pop();
            ai.current = state;
            ai.next_unset = false;
            ai.log_selected_move(state);
            return true;
        }
        false
    }

    #[inline]
    pub(crate) fn consume_ignore_next_instance(&mut self, p: PRef) -> bool {
        let ignored = self.powers[p as usize].ignore_next_instance;
        self.powers[p as usize].ignore_next_instance = false;
        ignored
    }

    pub(crate) fn power_gain_unpowered_block(&mut self, owner: CId, amount: i32) -> i32 {
        let _block = self.orb_trace("gain_block", Some(owner));
        // C# CreatureCmd.GainBlock, including FrostOrb's Hibernate recipients.
        if self.is_over_or_ending() || self.cr(owner).is_dead() {
            return 0;
        }
        let before = self.cr(owner).block;
        self.gain_recorded_block(owner,amount);
        let gained = self.cr(owner).block - before;
        if gained > 0 {
            self.fire(
                Hook::AfterBlockGained,
                Ev::BlockGained {
                    creature: owner,
                    amount: gained,
                },
                None,
            );
        }
        gained
    }

    // ───────────────────────────── value pipelines ─────────────────────────────

    /// `hooks.modify_damage` (Hook.ModifyDamageInternal).
    pub fn modify_damage(
        &mut self,
        base: i32,
        dealer: Option<CId>,
        target: CId,
        props: Props,
        card: Option<KRef>,
    ) -> i32 {
        let (amount, fault) = self.query_damage(base, dealer, Some(target), props, card, self.active_card, false);
        self.apply_query_fault(fault);
        amount
    }

    /// DamageVar/OstyDamageVar.UpdatePreview: same hooks, explicit card source,
    /// no CardPlay command, no RNG or mutation of active_card/fault.
    pub fn preview_damage(&self, base: i32, dealer: Option<CId>, target: Option<CId>,
        props: Props, card: Option<KRef>) -> Result<i32, &'static str> {
        let (amount, fault) = self.query_damage(base, dealer, target, props, card, card, true);
        match fault { Some(reason) => Err(reason), None => Ok(amount) }
    }

    /// Original AttackIntent.GetSingleDamage uses the local actor as its target.
    /// Dynamic DamageCalc delegates share their existing gameplay accessors.
    pub fn public_intent(&self, enemy: CId, actor: CId, index: usize) -> Result<Option<IntentDef>, &'static str> {
        let Some(ai) = self.cr(enemy).ai.as_ref() else { return Ok(None); };
        let Some(next) = ai.peek_move() else { return Ok(None); };
        let Some(mut intent) = ai.intents(next).get(index).copied() else { return Ok(None); };
        if matches!(intent.kind, IntentType::Attack | IntentType::MultiAttack | IntentType::DeathBlow) {
            intent.damage = match ai.kind {
                MonsterKind::TheForgotten if next == crate::identity::NATIVE_FORGOTTEN_DREAD[usize::from(ai.band)] => self.forgotten_dread_damage(enemy),
                MonsterKind::WaterfallGiant if next == crate::identity::NATIVE_WATERFALL_PRESSURE[usize::from(ai.band)] => self.waterfall_pressure_gun_damage(enemy),
                MonsterKind::WaterfallGiant if next == crate::identity::NATIVE_WATERFALL_ERUPTION[usize::from(ai.band)] => self.waterfall_eruption_damage(enemy),
                _ => intent.damage,
            };
            if ai.kind == MonsterKind::TestSubject && next == crate::identity::NATIVE_TEST_SUBJECT_CLAW[usize::from(ai.band)] {
                intent.hits = self.test_subject_claw_hits(enemy);
            }
            intent.damage = self.preview_damage(intent.damage, Some(enemy), Some(actor), Props::MOVE, None)?;
        }
        Ok(Some(intent))
    }

    fn query_damage(&self, base: i32, dealer: Option<CId>, target: Option<CId>, props: Props,
        card: Option<KRef>, source_card: Option<KRef>, preview: bool) -> (i32, Option<&'static str>) {
        let mut fault = None;
        // CreatureCmd.Damage continues its already-issued target batch when
        // Thorns kills the attacker. AttackCommand gates NEW hits separately.
        let mut damage = base as f64;
        if let Some(k) = card {
            // fire: core/hooks.py:202
            // Hook.ModifyDamage: both enchantment modifiers precede global
            // additive powers (Strength) and global multipliers (Vulnerable).
            damage = self.enchanted_damage(base, props, k);
        }
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageAdditive) {
                damage += self.power_damage_additive(p, c, dealer, target, props, source_card, preview, &mut fault) as f64;
            }
        }
        // relic fire: core/hooks.py:162
        for r in 0..self.run.relics.len() {
            damage += self.query_relic_damage_additive(r, self.run.relics[r].owner, dealer, target.unwrap_or(PLAYER), props, card, &mut fault) as f64;
        }
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageMultiplicative) {
                damage *= self.power_damage_multiplicative(p, c, dealer, target, props, source_card, &mut fault);
            }
        }
        // relic fire: core/hooks.py:162
        for r in 0..self.run.relics.len() {
            damage *= self.query_relic_damage_multiplicative(r, self.run.relics[r].owner, dealer, target, props, card, &mut fault);
        }
        let mut cap = f64::INFINITY;
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageCap) {
                cap = cap.min(self.power_damage_cap(p, c, dealer, target, props, &mut fault));
            }
        }
        if damage > cap {
            damage = cap;
        }
        ((damage.floor() as i32).max(0), fault)
    }

    /// `cards/ironclad.py:_calculate_untargeted_card_damage` / r7 `modify_damage`
    /// with `target=None`. Target-side listeners are neutral, while dealer/card/relic
    /// modifiers still see the exhausted card as the active source.
    pub(crate) fn modify_untargeted_card_damage(
        &mut self,
        base: i32,
        dealer: CId,
        card: KRef,
    ) -> i32 {
        if self.cr(dealer).is_dead() && !self.cr(dealer).is_player {
            return 0;
        }
        // Preserve the gameplay path's explicit null-target listener contract.
        // C# Thrash.OnPlay passes cardSource without a CardPlay command.
        let previous_card = self.active_card.replace(card);
        let props = Props::MOVE;
        let mut damage = base as f64;
        damage += crate::card::enchant::damage_additive(&self.cards[card as usize], props) as f64;
        damage *= crate::card::enchant::damage_multiplicative(&self.cards[card as usize], props);

        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageAdditive) {
                match self.powers[p as usize].id {
                    PowerId::Strength | PowerId::Vigor | PowerId::Accuracy
                    | PowerId::PhantomBlades | PowerId::Calcify => {
                        let mut fault = None;
                        damage += self.power_damage_additive(p, c, Some(dealer), Some(PLAYER), props,
                            Some(card), false, &mut fault) as f64;
                        if let Some(reason) = fault { self.unported(reason); }
                    }
                    PowerId::OneForAll => {
                        if self.cards[card as usize].owner == c
                            && !self.cards[card as usize].x_cost
                            && self.modified_card_cost(c, card) == 0
                        {
                            damage += self.powers[p as usize].amount as f64;
                        }
                    }
                    PowerId::Tainted => {}
                    _ => self.unported("untargeted damage additive"),
                }
            }
        }
        for r in 0..self.run.relics.len() {
            damage += self.relic_damage_additive(r, self.run.relics[r].owner, Some(dealer), PLAYER, props, Some(card)) as f64;
        }
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageMultiplicative) {
                match self.powers[p as usize].id {
                    PowerId::Weak | PowerId::Shrink | PowerId::DoubleDamage
                    | PowerId::Lethality | PowerId::Gigantification => {
                        let mut fault = None;
                        damage *= self.power_damage_multiplicative(p, c, Some(dealer), None, props,
                            Some(card), &mut fault);
                        if let Some(reason) = fault { self.unported(reason); }
                    }
                    PowerId::Vulnerable | PowerId::Colossus | PowerId::Soar | PowerId::Slow
                    | PowerId::Flutter | PowerId::Tank | PowerId::Guarded | PowerId::Covered
                    | PowerId::Intercept | PowerId::Surrounded | PowerId::Tracking
                    | PowerId::Conqueror | PowerId::Flanking | PowerId::Knockdown | PowerId::Hang => {}
                    _ => self.unported("untargeted damage multiplicative"),
                }
            }
        }
        for r in 0..self.run.relics.len() {
            damage *= self.relic_damage_multiplicative(r, self.run.relics[r].owner, Some(dealer), None, props, Some(card));
        }
        // All currently ported caps are target-side and therefore neutral for target=None.
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyDamageCap) {
                match self.powers[p as usize].id {
                    PowerId::HardToKill | PowerId::Intangible => {}
                    _ => self.unported("untargeted damage cap"),
                }
            }
        }
        self.active_card = previous_card;
        (damage.floor() as i32).max(0)
    }

    fn power_damage_additive(
        &self,
        p: PRef,
        owner: CId,
        dealer: Option<CId>,
        target: Option<CId>,
        props: Props,
        source_card: Option<KRef>,
        preview: bool,
        fault: &mut Option<&'static str>,
    ) -> i32 {
        let pw = &self.powers[p as usize];
        match pw.id {
            PowerId::Strength => {
                if dealer == Some(owner) && props.is_powered_attack() {
                    pw.amount
                } else {
                    0
                }
            }
            PowerId::Vigor => {
                let tracked_depth = self.power_private_int(owner, PowerId::Vigor, 1);
                if dealer == Some(owner)
                    && props.is_powered_attack()
                    // C# VigorPower.ModifyDamageAdditive also applies when
                    // commandToModify is null (intent/read-only previews).
                    && (tracked_depth == 0 || tracked_depth == self.attack_stack.len() as i32)
                {
                    self.powers[p as usize].amount
                } else {
                    0
                }
            }
            PowerId::Tainted => {
                // powers/tainted.py TaintedPower.modify_damage_additive.
                if target == Some(owner) && props.is_powered_attack() {
                    pw.amount
                } else {
                    0
                }
            }
            _ => {
                if let Some(v) = self.chars_damage_additive(p, owner, dealer, target.unwrap_or(PLAYER), props, source_card)
                    .or_else(|| self.defect_damage_additive(p, owner, dealer, props, source_card, preview, fault))
                {
                    return v;
                }
                fault.get_or_insert("damage additive");
                0
            }
        }
    }

    fn power_damage_multiplicative(
        &self,
        p: PRef,
        owner: CId,
        dealer: Option<CId>,
        target: Option<CId>,
        props: Props,
        source_card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> f64 {
        let id = self.powers[p as usize].id;
        match id {
            PowerId::Vulnerable => {
                if target == Some(owner) && props.is_powered_attack() {
                    let mut m = VULNERABLE_MULTIPLIER;
                    // VulnerablePower.ModifyDamageMultiplicative calls PaperPhrog,
                    // then dealer (or PetOwner) Cruelty, then Debilitate. PaperPhrog
                    // belongs to the dealer's Player, not an attacking pet's owner.
                    // VulnerablePower.ModifyDamageMultiplicative looks up
                    // dealer.Player.GetRelic<PaperPhrog>(), not any party relic.
                    if let Some(dealer) = dealer
                        && self.players.is_player(dealer) && target != Some(dealer)
                        && self.query_relic_owner_has_enabled(dealer, RelicId::PaperPhrog, fault)
                    {
                        m += 0.25;
                    }
                    if let Some(cr) = dealer.and_then(|d| self.find_power(d, PowerId::Cruelty)
                        .or_else(|| self.cr(d).pet_owner.and_then(|o| self.find_power(o, PowerId::Cruelty))))
                    {
                        m += self.powers[cr as usize].amount as f64 / 100.0;
                    }
                    if self.has_power(owner, PowerId::Debilitate) {
                        m += m - 1.0;
                    }
                    m
                } else {
                    1.0
                }
            }
            PowerId::Weak => {
                if dealer == Some(owner) && props.is_powered_attack() {
                    // Keep these C# decimal percentages integral until the final
                    // division: 60% with Debilitate is 20%, not 19.999...%.
                    let mut percent = WEAK_MULTIPLIER * 100.0;
                    // WeakPower.cs:36 -> PaperKrane.cs:16: target.Player's relic,
                    // not the weakened dealer's relic or a pet owner's inventory.
                    if let Some(target) = target
                        && self.players.is_player(target)
                        && self.query_relic_owner_has_enabled(target, RelicId::PaperKrane, fault) {
                        percent -= 15.0;
                    }
                    if self.has_power(owner, PowerId::Debilitate) {
                        // Models/Powers/DebilitatePower.cs:39.
                        percent -= 100.0 - percent;
                    }
                    percent / 100.0
                } else {
                    1.0
                }
            }
            // powers/common.py ShrinkPower.modify_damage_multiplicative
            PowerId::Shrink => {
                if dealer == Some(owner) && props.is_powered_attack() { 0.7 } else { 1.0 }
            }
            PowerId::Colossus => {
                if target == Some(owner)
                    && props.is_powered_attack()
                    && dealer.is_some_and(|d| self.has_power(d, PowerId::Vulnerable))
                {
                    0.5
                } else {
                    1.0
                }
            }
            PowerId::Soar => {
                if target == Some(owner) && props.is_powered_attack() {
                    0.5
                } else {
                    1.0
                }
            }
            PowerId::Slow => {
                if target == Some(owner) && props.is_powered_attack() {
                    1.0 + 0.1 * self.powers[p as usize].vars[0] as f64
                } else {
                    1.0
                }
            }
            PowerId::Flutter => {
                if target == Some(owner) && props.is_powered_attack() { 0.5 } else { 1.0 }
            }
            PowerId::Tank => {
                if target == Some(owner) && props.is_powered_attack() { 1.5 } else { 1.0 }
            }
            // C# GuardedPower.ModifyDamageMultiplicative.
            PowerId::Guarded => {
                if target == Some(owner) && props.is_powered_attack() { 0.5 } else { 1.0 }
            }
            // C# CoveredPower/InterceptPower.ModifyDamageMultiplicative.
            PowerId::Covered => {
                if target == Some(owner) && props.is_powered_attack() { 0.0 } else { 1.0 }
            }
            PowerId::Intercept => {
                if target == Some(owner) && props.is_powered_attack() {
                    (self.powers[p as usize].vars[0].count_ones() + 1) as f64
                } else { 1.0 }
            }
            // powers/monster.py:1188-1206 SurroundedPower.modify_damage_multiplicative.
            PowerId::Surrounded => {
                if target != Some(owner) {
                    return 1.0;
                }
                let Some(dealer) = dealer else { return 1.0 };
                let facing_left = self.power_private_int(owner, PowerId::Surrounded, 0) != 0;
                let from_behind = if facing_left {
                    self.has_power(dealer, PowerId::BackAttackRight)
                } else {
                    self.has_power(dealer, PowerId::BackAttackLeft)
                };
                if from_behind { 1.5 } else { 1.0 }
            }
            _ => {
                if let Some(v) = self.chars_damage_multiplicative(p, owner, dealer, target, props, source_card) {
                    return v;
                }
                if target.is_none() && id == PowerId::Conqueror { return 1.0; }
                if let Some(target) = target
                    && let Some(v) = self.regent_damage_multiplicative(p, owner, target, props, source_card) {
                    return v;
                }
                fault.get_or_insert("damage multiplicative");
                1.0
            }
        }
    }

    fn power_damage_cap(
        &self,
        p: PRef,
        owner: CId,
        dealer: Option<CId>,
        target: Option<CId>,
        _props: Props,
        fault: &mut Option<&'static str>,
    ) -> f64 {
        match self.powers[p as usize].id {
            PowerId::HardToKill => {
                if target == Some(owner) {
                    self.powers[p as usize].amount as f64
                } else {
                    f64::INFINITY
                }
            }
            PowerId::Intangible => {
                if target != Some(owner) {
                    f64::INFINITY
                } else {
                    // IntangiblePower.cs:42 caps damage at 1 BEFORE block.
                    // TheBoot.cs:26 raises only final nonzero HP loss later.
                    1.0
                }
            }
            _ => {
                fault.get_or_insert("damage cap");
                f64::INFINITY
            }
        }
    }

    /// `hooks.modify_block` (Hook.ModifyBlock).
    pub fn modify_block(
        &mut self,
        base: i32,
        target: CId,
        props: Props,
        card: Option<KRef>,
    ) -> i32 {
        let (result, modified, modified_relics, fault) = self.query_block(base, target, props, card);
        self.apply_query_fault(fault);
        if modified {
            for c in self.all_creatures() {
                for p in self.listeners_of(c, Hook::AfterModifyingBlockAmount) {
                    if self.powers[p as usize].id != PowerId::Fasten { self.unported("after_modifying_block_amount"); }
                }
            }
            for r in modified_relics { self.relic_after_modifying_block(r, self.run.relics[r].owner, result, card); }
        }
        result
    }

    /// BlockVar.UpdatePreview does not execute AfterModifyingBlock callbacks.
    pub fn preview_block(&self, base: i32, target: CId, props: Props, card: Option<KRef>) -> Result<i32, &'static str> {
        let (amount, _, _, fault) = self.query_block(base, target, props, card);
        match fault { Some(reason) => Err(reason), None => Ok(amount) }
    }

    pub(crate) fn enchanted_damage(&self, base: i32, props: Props, k: KRef) -> f64 {
        let card = &self.cards[k as usize];
        (base as f64 + crate::card::enchant::damage_additive(card, props) as f64)
            * crate::card::enchant::damage_multiplicative(card, props)
    }

    pub(crate) fn enchanted_block(&self, base: i32, props: Props, k: KRef) -> f64 {
        base as f64 + crate::card::enchant::block_additive(&self.cards[k as usize], props) as f64
    }

    fn query_block(&self, base: i32, target: CId, props: Props, card: Option<KRef>)
        -> (i32, bool, ArrayVec<usize, 32>, Option<&'static str>) {
        let mut fault = None;
        let mut block = base as f64;
        if let Some(k) = card {
            // fire: core/hooks.py:405
            block = self.enchanted_block(base, props, k);
        }
        let mut modified = false;
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyBlockAdditive) {
                let d = self.power_block_additive(p, c, target, props, card, &mut fault);
                block += d as f64;
                modified |= d != 0;
            }
        }
        // relic fire: core/hooks.py:386
        for r in 0..self.run.relics.len() {
            let d = self.relic_block_additive(r, self.run.relics[r].owner, target, props, card);
            block += d as f64;
            modified |= d != 0;
        }
        let mut modified_relics = ArrayVec::<usize, 32>::new();
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyBlockMultiplicative) {
                let m = self.power_block_multiplicative(p, c, target, props, card, &mut fault);
                block *= m;
                modified |= m != 1.0;
            }
        }
        // relic fire: core/hooks.py:386
        for r in 0..self.run.relics.len() {
            let m = self.query_relic_block_multiplicative(r, self.run.relics[r].owner, target, props, card, &mut fault);
            block *= m;
            if m != 1.0 {
                modified = true;
                if modified_relics.len() < modified_relics.capacity() {
                    modified_relics.push(r);
                } else {
                    fault.get_or_insert("too many relic block modifiers");
                }
            }
        }
        // MultiplayerScalingModel.ModifyBlockMultiplicative (identity with one player).
        let (scaled, scaling_fault) = self.scale_monster_block_readonly(block, target, props);
        block = scaled; if let Some(reason) = scaling_fault { fault.get_or_insert(reason); }
        let result = (block.floor() as i32).max(0);
        (result, modified, modified_relics, fault)
    }

    fn power_block_additive(
        &self,
        p: PRef,
        owner: CId,
        target: CId,
        props: Props,
        card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> i32 {
        let pw = &self.powers[p as usize];
        match pw.id {
            // C# FastenPower.ModifyBlockAdditive also permits powered monster-move block.
            PowerId::Fasten => {
                if target == owner && props.is_powered_block()
                    && card.is_none_or(|k| self.cards[k as usize].has_tag(CardTag::Defend))
                {
                    pw.amount
                } else {
                    0
                }
            }
            PowerId::Dexterity => {
                if let Some(k) = card {
                    if self.cards[k as usize].owner != owner {
                        return 0;
                    }
                } else if target != owner {
                    return 0;
                }
                if !props.is_powered_block() {
                    0
                } else {
                    pw.amount
                }
            }
            _ => {
                fault.get_or_insert("block additive");
                0
            }
        }
    }

    fn power_block_multiplicative(
        &self,
        p: PRef,
        owner: CId,
        target: CId,
        props: Props,
        card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> f64 {
        match self.powers[p as usize].id {
            PowerId::Frail => {
                if target == owner && props.is_powered_block() {
                    FRAIL_MULTIPLIER
                } else {
                    1.0
                }
            }
            PowerId::NoBlock => {
                if target == owner && props.is_powered_block() && card.is_some() {
                    0.0
                } else {
                    1.0
                }
            }
            PowerId::Unmovable => {
                if self.cr(target).side == Side::Enemy || !props.is_card_or_monster_move() {
                    return 1.0;
                }
                if card.is_some_and(|k| self.cards[k as usize].owner != owner) {
                    return 1.0;
                }
                let exclude = self.card_play_token(card);
                let count = self
                    .hist
                    .block_this_turn
                    .iter()
                    .filter(|ev| {
                        ev.target == target
                            && ev.props.is_card_or_monster_move()
                            && ev.play_token != exclude
                    })
                    .count() as i32;
                if count < self.powers[p as usize].amount {
                    2.0
                } else {
                    1.0
                }
            }
            _ => {
                if let Some(v) = self.chars_block_multiplicative(p, owner, target) {
                    return v;
                }
                fault.get_or_insert("block multiplicative");
                1.0
            }
        }
    }

    fn galvanic_afflict_card(&mut self, k: KRef) {
        if self.cards[k as usize].card_type != CardType::Power {
            return;
        }
        match self.cards[k as usize].cv.affliction {
            None => {
                self.cards[k as usize].cv.affliction = Some(CardAffliction::Galvanized);
                self.cards[k as usize].cv.affliction_amount = 0;
            }
            Some(CardAffliction::Galvanized) => {}
            Some(_) => {}
        }
    }

    fn power_after_combat_end(&mut self, p: PRef, owner: CId) {
        match self.powers[p as usize].id {
            PowerId::ForbiddenGrimoire => {
                // ForbiddenGrimoirePower.AfterCombatEnd: one owned CardRemovalReward
                // per Amount, at this listener's position, not an immediate deck mutation.
                for _ in 0..self.powers[p as usize].amount {
                    self.end_rewards.push(EndReward::CardRemoval { owner });
                }
            }
            PowerId::Improvement => {
                if owner != PLAYER {
                    return;
                }
                let amount = self.powers[p as usize].amount.max(0);
                let mut candidates = Vec::new();
                for i in 0..self.run.deck.len() {
                    if self.run.is_deck_card_upgradable(i) {
                        candidates.push(i);
                    }
                }
                for _ in 0..amount {
                    if candidates.is_empty() {
                        break;
                    }
                    let pick = self
                        .run
                        .rngs
                        .combat_card_selection
                        .choice_index(candidates.len());
                    let deck_index = candidates.remove(pick);
                    self.run.upgrade_deck_card(deck_index);
                }
            }
            // C# RoyaltiesPower.AfterCombatEnd: room.AddExtraReward(GoldReward(Amount)).
            PowerId::Royalties => {
                let amount = self.powers[p as usize].amount.max(0);
                if owner == PLAYER {
                    self.run.extra_gold_reward += amount;
                } else {
                    self.players[owner].extra_gold_reward += amount;
                }
                self.end_rewards.push(EndReward::Gold { owner, amount });
            }
            _ => self.unported("after_combat_end"),
        }
    }

    /// C# Hook.ModifyHpLost: BeforeOsty runs on the original target; AfterOsty
    /// runs on the redirected receiver and again on the original target's overkill.
    pub(crate) fn modify_hp_lost_phase(
        &mut self, amount: i32, target: CId, dealer: Option<CId>, _props: Props, before: bool,
    ) -> i32 {
        let mut result = amount.max(0);
        for hook in [
            Hook::ModifyHpLostBeforeOstyLate,
            Hook::ModifyHpLost,
            Hook::ModifyHpLostLate,
        ] {
            if (hook == Hook::ModifyHpLostBeforeOstyLate) != before {
                continue;
            }
            for c in self.all_creatures() {
                for p in self.listeners_of(c, hook) {
                    match (hook, self.powers[p as usize].id) {
                        (Hook::ModifyHpLostBeforeOstyLate, PowerId::HardenedShell) => {
                            if target == c && result != 0 {
                                let remaining = (self.powers[p as usize].amount
                                    - self.powers[p as usize].vars[0])
                                    .max(0);
                                result = result.min(remaining);
                            }
                        }
                        (Hook::ModifyHpLost, PowerId::Intangible) => {
                            if target == c && result != 0 {
                                // IntangiblePower.cs:19: always one; Boot is
                                // a distinct, subsequent AfterOstyLate modifier.
                                result = result.min(1);
                            }
                        }
                        // SlipperyPower.ModifyHpLostAfterOsty: hp loss of 1 or more becomes 1,
                        // after block and for every damage source (orbs included).
                        (Hook::ModifyHpLost, PowerId::Slippery) => {
                            if target == c && result >= 1 {
                                result = 1;
                            }
                        }
                        (Hook::ModifyHpLostLate, PowerId::Buffer) => {
                            if target == c
                                && result > 0
                                && self.powers[p as usize].amount > 0
                            {
                                self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                                result = 0;
                                if self.powers[p as usize].amount <= 0
                                    && self.cr(c).power_list().contains(&p)
                                {
                                    self.remove_power(c, p);
                                }
                            }
                        }
                        _ => self.unported("modify_hp_lost"),
                    }
                }
            }
            if hook == Hook::ModifyHpLost {
                // relic fire: core/hooks.py modify_hp_lost_after_osty (between the power stages)
                for r in 0..self.run.relics.len() {
                    result = self.relic_modify_hp_lost(r, self.run.relics[r].owner, result, target, dealer, _props);
                }
            }
            if hook == Hook::ModifyHpLostLate {
                for r in 0..self.run.relics.len() {
                    result = self.relic_modify_hp_lost_after_osty_late(r, result, target, dealer, _props);
                }
            }
        }
        result.max(0)
    }

    pub fn modify_hand_draw(&mut self, base: i32, owner: CId) -> i32 {
        let mut draw = base;
        // powers (r7 hooks.modify_hand_draw), then relics; late stage the same.
        for p in self.listeners_of(owner, Hook::ModifyHandDraw) {
            draw = match self.powers[p as usize].id {
                PowerId::DrawCardsNextTurn => draw + self.powers[p as usize].amount,
                PowerId::MindRot => (draw - self.powers[p as usize].amount).max(0),
                PowerId::Clarity => draw + 1,
                _ => match self.chars_modify_hand_draw(p, draw).or_else(|| self.defect_modify_hand_draw(p, draw)) {
                    Some(v) => v,
                    None => {
                        self.unported("modify_hand_draw");
                        draw
                    }
                },
            };
        }
        let mut changed = ArrayVec::<usize, 32>::new();
        // relic fire: core/hooks.py:522
        for r in 0..self.run.relics.len() {
            let before = draw;
            draw = self.relic_modify_hand_draw(r, owner, draw, false);
            if draw != before && !changed.contains(&r) {
                if changed.len() < changed.capacity() { changed.push(r); } else { self.unported("too many draw modifiers"); }
            }
        }
        if !self.listeners_of(owner, Hook::ModifyHandDrawLate).is_empty() {
            self.unported("modify_hand_draw");
        }
        // relic fire: core/hooks.py:522
        for r in 0..self.run.relics.len() {
            let before = draw;
            draw = self.relic_modify_hand_draw(r, owner, draw, true);
            if draw != before && !changed.contains(&r) {
                if changed.len() < changed.capacity() { changed.push(r); } else { self.unported("too many draw modifiers"); }
            }
        }
        // relic fire: core/hooks.py:522
        for r in changed { self.relic_after_modifying_hand_draw(r, owner); }
        draw.max(0)
    }

    /// `Hook.ModifyMaxEnergy` over `owner`'s base max energy.
    pub fn max_energy(&mut self, owner: CId) -> i32 {
        let mut energy = self.players[owner].base_max_energy;
        for p in self.listeners_of(owner, Hook::ModifyMaxEnergy) {
            energy = match self.powers[p as usize].id {
                // Models/Powers/DemesnePower.ModifyMaxEnergy.
                PowerId::Pyre | PowerId::Demesne | PowerId::Friendship => energy + self.powers[p as usize].amount,
                PowerId::WasteAway => energy - self.powers[p as usize].amount,
                _ => {
                    self.unported("modify_max_energy");
                    energy
                }
            };
        }
        // relic fire: core/hooks.py:565 (after power modifiers)
        for r in 0..self.run.relics.len() {
            energy = self.relic_modify_max_energy(r, owner, energy);
        }
        energy.max(0)
    }

    /// One pass of C# `Hook.ModifyEnergyCostInCombat` over the owner's powers:
    /// `late == false` is `TryModifyEnergyCostInCombat` (Curious, Tangled), `late == true` is
    /// `TryModifyEnergyCostInCombatLate` (Corruption, FreeAttack/Skill/Power). Relics follow
    /// each pass (`relic_modify_card_cost`).
    pub fn modify_card_cost(&self, owner: CId, k: KRef, mut cost: i32, late: bool) -> i32 {
        let card_owner = self.cards[k as usize].owner;
        let card_type = self.cards[k as usize].card_type;
        for &p in &self.cr(owner).powers {
            let amount = self.powers[p as usize].amount;
            let modified = match (self.powers[p as usize].id, late) {
                // Models/Powers/BorrowedTimePower.TryModifyEnergyCostInCombat.
                (PowerId::BorrowedTime, false) if card_owner == owner => Some(cost + amount),
                // Models/Powers/VeilpiercerPower.TryModifyEnergyCostInCombatLate:
                // only Ethereal cards actually in the owner's hand or play pile.
                (PowerId::Veilpiercer, true) if card_owner == owner
                    && self.cards[k as usize].is_ethereal()
                    && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k)) =>
                {
                    Some(0)
                }
                (PowerId::Corruption, true) if card_owner == owner && card_type == CardType::Skill => {
                    Some(0)
                }
                (PowerId::Curious, false)
                    if card_owner == owner && card_type == CardType::Power && cost > 0 =>
                {
                    Some((cost - amount).max(0))
                }
                (PowerId::FreeAttack, true)
                    if amount > 0 && card_owner == owner && card_type == CardType::Attack =>
                {
                    Some(0)
                }
                // c1: powers/card_play_effects.py:684-690
                (PowerId::FreeSkill, true)
                    if amount > 0 && card_owner == owner && card_type == CardType::Skill =>
                {
                    Some(0)
                }
                // c2: powers/card_play_effects.py FreePowerPower
                (PowerId::FreePower, true)
                    if amount > 0 && card_owner == owner && card_type == CardType::Power
                        && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k)) =>
                {
                    Some(0)
                }
                // C# VoidFormPower.TryModifyEnergyCostInCombatLate
                (PowerId::VoidForm, true) if self.void_form_covers(p, owner, k) => Some(0),
                (PowerId::Tangled, false)
                    if card_owner == owner
                        && card_type == CardType::Attack
                        && self.cards[k as usize].cv.affliction == Some(CardAffliction::Entangled) =>
                {
                    Some(cost + amount)
                }
                _ => None,
            };
            if let Some(modified) = modified {
                // CardEnergyCost.GetWithModifiers clamps once after the whole global
                // pipeline. Curious's own clamp above is part of that power's C# method.
                cost = modified;
            }
        }
        cost
    }

    #[allow(clippy::too_many_arguments)]
    fn power_modify_card_play_result(
        &mut self,
        p: PRef,
        owner: CId,
        k: KRef,
        _is_auto_play: bool,
        energy_value: i32,
        pile: PileKind,
        pos: PilePos,
    ) -> (PileKind, PilePos) {
        let card_owner = self.cards[k as usize].owner;
        let card_type = self.cards[k as usize].card_type;
        match self.powers[p as usize].id {
            PowerId::Nostalgia => {
                // C# NostalgiaPower.ModifyCardPlayResultLocation; CardModel.OnPlayWrapper
                // computes the location before recording the current play's starts/repeats.
                let started = self.hist.play_start_cards.iter().filter(|&&q| {
                    let card = &self.cards[q as usize];
                    card.owner == owner && matches!(card.card_type, CardType::Attack | CardType::Skill)
                }).count();
                if card_owner == owner && matches!(card_type, CardType::Attack | CardType::Skill)
                    && pile == PileKind::Discard && (started as i32) < self.powers[p as usize].amount
                {
                    (PileKind::Draw, PilePos::Top)
                } else {
                    (pile, pos)
                }
            }
            // c2: remaining_a.py FeralPower: a 0-energy Attack returns to the top of the hand
            // until `amount` of them did this turn (vars[0] = zero-cost attacks returned).
            PowerId::Feral => {
                let pw = &self.powers[p as usize];
                if card_owner == owner && card_type == CardType::Attack && energy_value == 0 && pw.vars[0] < pw.amount {
                    (PileKind::Hand, PilePos::Top)
                } else {
                    (pile, pos)
                }
            }
            PowerId::Corruption => {
                if card_owner == owner && card_type == CardType::Skill {
                    (PileKind::Exhaust, pos)
                } else {
                    (pile, pos)
                }
            }
            PowerId::Rebound => {
                if card_owner == owner && pile == PileKind::Discard {
                    (PileKind::Draw, PilePos::Top)
                } else {
                    (pile, pos)
                }
            }
            _ => {
                self.unported("modify_card_play_result_pile_type_and_position");
                (pile, pos)
            }
        }
    }

    fn power_after_modifying_card_play_result(
        &mut self,
        p: PRef,
        owner: CId,
        _k: KRef,
        _pile: PileKind,
        _pos: PilePos,
    ) {
        match self.powers[p as usize].id {
            // C# NostalgiaPower.AfterModifyingCardPlayResultLocation only flashes.
            PowerId::Nostalgia => {}
            PowerId::Rebound => {
                self.powers[p as usize].amount -= 1;
                if self.powers[p as usize].amount <= 0 {
                    self.remove_power(owner, p);
                }
            }
            PowerId::Feral => self.powers[p as usize].vars[0] += 1,
            _ => self.unported("after_modifying_card_play_result_pile_or_position"),
        }
    }

    pub fn modify_card_play_result(
        &mut self,
        k: KRef,
        is_auto_play: bool,
        energy_value: i32,
        mut pile: PileKind,
        mut pos: PilePos,
    ) -> (PileKind, PilePos) {
        let mut modifiers: ArrayVec<(PRef, CId), 32> = ArrayVec::new();
        for owner in self.all_creatures() {
            for p in self.listeners_of(owner, Hook::ModifyCardPlayResultPileTypeAndPosition) {
                let previous = (pile, pos);
                (pile, pos) = self.power_modify_card_play_result(
                    p,
                    owner,
                    k,
                    is_auto_play,
                    energy_value,
                    pile,
                    pos,
                );
                if (pile, pos) != previous {
                    modifiers.push((p, owner));
                }
            }
        }
        for (p, owner) in modifiers {
            if self.powers[p as usize]
                .meta()
                .listens(Hook::AfterModifyingCardPlayResultPileOrPosition)
            {
                self.power_after_modifying_card_play_result(p, owner, k, pile, pos);
            }
        }
        (pile, pos)
    }

    fn power_modify_card_play_count(&mut self, p: PRef, owner: CId, k: KRef, count: i32) -> i32 {
        match self.powers[p as usize].id {
            // C# TagTeamPower.ModifyCardPlayCount: another player's enemy-targeted
            // attack consumes each applicable instance; all-enemy attacks need no target.
            PowerId::TagTeam => {
                if self.cards[k as usize].card_type == CardType::Attack
                    && Some(self.cards[k as usize].owner) != self.powers[p as usize].applier
                    && match self.card_target_type(k) {
                        TargetType::AnyEnemy => self.active_target == Some(owner),
                        TargetType::AllEnemies => true,
                        _ => false,
                    }
                {
                    count + self.powers[p as usize].amount
                } else {
                    count
                }
            }
            PowerId::OneTwoPunch => {
                if self.powers[p as usize].amount > 0
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    count + 1
                } else {
                    count
                }
            }
            PowerId::Duplication => {
                if self.powers[p as usize].amount > 0 && self.cards[k as usize].owner == owner {
                    count + 1
                } else {
                    count
                }
            }
            _ => {
                if let Some(v) = self.chars_modify_card_play_count(p, owner, k, count)
                    .or_else(|| self.defect_modify_card_play_count(p, owner, k, count))
                {
                    return v;
                }
                self.unported("modify_card_play_count");
                count
            }
        }
    }

    fn power_after_modifying_card_play_count(&mut self, p: PRef, owner: CId, k: KRef) {
        match self.powers[p as usize].id {
            // C# TagTeamPower.AfterModifyingCardPlayCount removes this instance.
            PowerId::TagTeam => self.remove_power(owner, p),
            PowerId::OneTwoPunch => {
                if self.powers[p as usize].amount > 0
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            PowerId::Duplication => {
                if self.powers[p as usize].amount > 0 && self.cards[k as usize].owner == owner {
                    self.creature_apply_power(
                        owner,
                        PowerId::Duplication,
                        -1,
                        Some(owner),
                        None,
                        false,
                        false,
                    );
                }
            }
            _ => {
                if !self.chars_after_modifying_card_play_count(p, owner, k)
                    && !self.defect_after_modifying_card_play_count(p, owner, k)
                {
                    self.unported("after_modifying_card_play_count");
                }
            }
        }
    }

    pub fn modify_card_play_count(
        &mut self,
        base: i32,
        k: KRef,
    ) -> (i32, SmallVec<[(PRef, CId); 32]>) {
        let mut count = base;
        let mut modifiers = SmallVec::new();
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ModifyCardPlayCount) {
                let before = count;
                count = self.power_modify_card_play_count(p, c, k, count);
                if count != before {
                    modifiers.push((p, c));
                }
            }
        }
        (count, modifiers)
    }

    /// Relic part of `hooks.modify_card_play_count` (runs after the power listeners); returns
    /// the relics that changed the count, for the after-modifying hook.
    pub fn relic_modify_card_play_count_all(&mut self, base: i32, k: KRef) -> (i32, ArrayVec<usize, 32>) {
        let mut count = base;
        let mut changed = ArrayVec::new();
        // relic fire: core/hooks.py:590
        for r in 0..self.run.relics.len() {
            let before = count;
            count = self.relic_modify_card_play_count(r, self.run.relics[r].owner, k, count);
            if count != before && !changed.is_full() {
                changed.push(r);
            }
        }
        (count, changed)
    }

    pub fn fire_after_modifying_card_play_count(
        &mut self,
        k: KRef,
        modifiers: SmallVec<[(PRef, CId); 32]>,
    ) {
        for (p, owner) in modifiers {
            if self.cr(owner).power_list().contains(&p)
                && self.powers[p as usize]
                    .meta()
                    .listens(Hook::AfterModifyingCardPlayCount)
            {
                self.power_after_modifying_card_play_count(p, owner, k);
            }
        }
    }

    pub fn should_draw(&mut self, owner: CId, from_hand_draw: bool) -> bool {
        for p in self.listeners_of(owner, Hook::ShouldDraw) {
            match self.powers[p as usize].id {
                PowerId::NoDraw => {
                    if !from_hand_draw {
                        return false;
                    }
                }
                _ => self.unported("should_draw"),
            }
        }
        // relic fire: core/hooks.py:762
        for r in 0..self.run.relics.len() {
            if self.relic_should_draw(r, owner, from_hand_draw) == Some(false) {
                return false;
            }
        }
        true
    }

    /// Body of the injected stun move (`MonsterAi::stun_move`).
    pub(crate) fn power_on_stunned_move(&mut self, owner: CId) {
        let Some(ai) = self.cr_mut(owner).ai.as_mut() else { return };
        match std::mem::take(&mut ai.stun_move) {
            crate::monster::StunMove::Nothing => {}
            crate::monster::StunMove::BeetleWakeUp => {
                if let Some(plating) = self.find_power(owner, PowerId::Plating) {
                    self.remove_power(owner, plating);
                }
            }
            crate::monster::StunMove::BowlbugRockDizzy => self.bowlbug_rock_regain_balance(owner),
        }
    }

    /// C# BurrowedPower.AfterBlockBroken only stuns a Tunneler, not another creature
    /// granted this power (e.g. via an enemy buff). StillDizzyMove is animation-only.
    pub(crate) fn power_after_block_broken(&mut self, owner: CId) {
        let Some(p) = self.find_power(owner, PowerId::Burrowed) else {
            return;
        };
        if !self.cr(owner).ai.as_ref().is_some_and(|ai| ai.kind == MonsterKind::Tunneler) {
            return;
        }
        // C# Creature.StunInternal is a no-op on a dead monster; power removal still runs.
        if self.cr(owner).is_alive() && !self.power_stun_enemy_with_follow_up(owner, "BITE_MOVE", crate::monster::StunMove::Nothing) {
            self.unported("burrowed block-broken stun");
        }
        self.remove_power(owner, p);
    }

    pub fn should_flush(&mut self, owner: CId) -> bool {
        for c in self.all_creatures() {
            for p in self.listeners_of(c, Hook::ShouldFlush) {
                match self.powers[p as usize].id {
                    // WellLaidPlansPower.cs:12-19: only Owner.Player skips
                    // the whole flush; no retain-count callback or selector.
                    PowerId::RetainHand | PowerId::WellLaidPlans if c == owner => return false,
                    PowerId::RetainHand | PowerId::WellLaidPlans => {}
                    _ => self.unported("should_flush"),
                }
            }
        }
        // relic fire: core/hooks.py should_flush (relics after powers)
        for r in 0..self.run.relics.len() {
            if self.relic_should_flush(r, owner) == Some(false) {
                return false;
            }
        }
        true
    }

    /// Optional power playability methods used by r7 `hooks.should_play`.
    pub fn powers_allow_card_play(&self, k: KRef) -> bool {
        let card_owner = self.cards[k as usize].owner;
        let card_type = self.cards[k as usize].card_type;
        if self.cards[k as usize].bound
            && self.find_power(card_owner, PowerId::ChainsOfBinding).is_some()
            && self.power_private_int(card_owner, PowerId::ChainsOfBinding, 1) != 0
        {
            return false;
        }
        if self.cards[k as usize].cv.affliction == Some(CardAffliction::Ringing)
            && self.find_power(card_owner, PowerId::Ringing).is_some()
            && self.hist.play_starts_this_turn > 0
        {
            return false;
        }
        if let Some(sloth) = self.find_power(card_owner, PowerId::Sloth) {
            // SlothPower counts the owner's plays in BeforeCardPlayed (each play start, so a
            // Havoc/Cascade card auto-played mid-effect already sees its parent counted).
            let played = self.powers[sloth as usize].vars[0];
            if played >= self.powers[sloth as usize].amount {
                return false;
            }
        }
        if card_type != CardType::Skill {
            return true;
        }
        if self.has_power(card_owner,PowerId::Smoggy)
            && self.cards[k as usize].cv.affliction == Some(CardAffliction::Smog)
        {
            return false;
        }
        // Relic vetoes are `relic_should_play` in `can_play_card`; ShouldFlush (Runic Pyramid,
        // Ringing Triangle) only decides the end-of-turn hand discard.
        true
    }

    /// `block_clear_preventer`.
    pub fn clear_block(&mut self, c: CId) {
        let mut should_clear = true;
        for o in self.all_creatures() {
            for p in self.listeners_of(o, Hook::ShouldClearBlock) {
                match self.powers[p as usize].id {
                    PowerId::Barricade | PowerId::Burrowed => {
                        if o == c {
                            should_clear = false;
                        }
                    }
                    _ => match self.chars_should_clear_block(p, o, c) {
                        Some(false) => should_clear = false,
                        Some(true) => {}
                        None => self.unported("should_clear_block"),
                    },
                }
            }
        }
        // Powers first (Barricade); a power preventer skips the relic pass.
        if !should_clear {
            return;
        }
        let mut preventer = None;
        // relic fire: core/hooks.py:650
        for r in 0..self.run.relics.len() {
            if self.relic_should_clear_block(r, self.run.relics[r].owner, c) == Some(false) {
                preventer = Some(r);
                break;
            }
        }
        if let Some(r) = preventer {
            // relic fire: core/hooks.py:665
            self.relic_after_preventing_block_clear(r, self.run.relics[r].owner, c);
        } else {
            let before=crate::between::PublicState::of(self.cr(c));
            self.cr_mut(c).block = 0;
            self.record_block_change(c,"clear",before);
        }
    }

    /// `Hook.ShouldPlayerResetEnergy(owner)` filters each relic's Owner.
    pub fn should_reset_energy(&mut self, owner: CId) -> bool {
        // relic fire: core/hooks.py:674
        for r in 0..self.run.relics.len() {
            if self.relic_should_reset_energy(r, owner) == Some(false) {
                return false;
            }
        }
        true
    }

    // ─────────────────── power-amount hooks around Creature.apply_power ───────────────────

    pub fn fire_before_power_amount_changed(
        &mut self,
        target: CId,
        id: PowerId,
        amount: i32,
        applier: Option<CId>,
        source: Option<KRef>,
        instance: PRef,
    ) {
        self.fire(
            Hook::BeforePowerAmountChanged,
            Ev::PowerAmount {
                target,
                id,
                amount,
                applier,
                source,
                instance: Some(instance),
            },
            None,
        );
    }

    pub fn modify_power_amount_given(
        &mut self,
        id: PowerId,
        amount: i32,
        giver: Option<CId>,
        target: CId,
        source: Option<KRef>,
    ) -> i32 {
        let mut result = amount;
        // C# applies additive given modifiers before any given multipliers.
        for r in 0..self.run.relics.len() {
            result += self.relic_power_given_additive(r, giver, id);
        }
        // relic fire: core/hooks.py:205
        for r in 0..self.run.relics.len() {
            result = self.relic_modify_power_amount_given(r, self.run.relics[r].owner, id, result, target, source);
        }
        result
    }

    pub fn modify_power_amount_received(&mut self, _id: PowerId, amount: i32, _target: CId, _applier: Option<CId>) -> i32 {
        let mut result = amount;
        // relic fire: core/hooks.py:227
        for r in 0..self.run.relics.len() {
            result = self.relic_modify_power_amount_received(r, self.run.relics[r].owner, _id, result, _target);
        }
        result
    }

    /// `CombatState.after_power_amount_changed`.
    pub fn after_power_amount_changed(
        &mut self,
        target: CId,
        id: PowerId,
        amount: i32,
        applier: Option<CId>,
        source: Option<KRef>,
    ) {
        // DebufferModel.AfterPowerAmountChanged counts changes, not stack
        // amounts. Negative buffs count as debuffs, temporary powers do not,
        // and a player applying a debuff to themselves is excluded.
        if let Some(owner) = applier
            && self.players.is_player(owner) && owner != target
            && Self::power_type_for_amount(id, amount) == PowerType::Debuff
            && !power_meta(id).temporary {
            self.player_debuffs_applied[owner as usize] += 1;
        }
        self.fire(
            Hook::AfterPowerAmountChanged,
            Ev::PowerAmount {
                target,
                id,
                amount,
                applier,
                source,
                instance: None,
            },
            None,
        );
    }

    pub fn power_on_removed(&mut self, p: PRef, owner: CId) {
        match self.powers[p as usize].id {
            PowerId::Burrowed => self.cr_mut(owner).block = 0,
            PowerId::Automation => self.automation_clear_instances(owner),
            PowerId::Dampen => {
                let dampened: Vec<KRef> = self
                    .cards
                    .iter()
                    .enumerate()
                    .filter_map(|(i, card)| card.cv.dampened.then_some(i as KRef))
                    .collect();
                let hexed = self.has_power(owner, PowerId::Hex);
                for k in dampened {
                    self.upgrade_card(k);
                    self.cards[k as usize].cv.dampened = false;
                    if hexed {
                        self.cards[k as usize].keywords |= KW_ETHEREAL;
                    }
                }
                self.powers[p as usize].payload = 0;
                self.dampen_clear_casters(owner);
            }
            PowerId::ChainsOfBinding => {
                self.set_power_private_int(owner, PowerId::ChainsOfBinding, 0, 0);
                self.set_power_private_int(owner, PowerId::ChainsOfBinding, 1, 0);
            }
            PowerId::Hex => {
                self.clear_power_affliction(owner, CardAffliction::Hexed, true);
                self.set_power_private_int(owner, PowerId::Hex, 0, 0);
            }
            PowerId::Tangled => {
                self.clear_power_affliction(owner, CardAffliction::Entangled, false);
                self.set_power_private_int(owner, PowerId::Tangled, 0, 0);
            }
            PowerId::Ringing => {
                self.clear_power_affliction(owner, CardAffliction::Ringing, false);
            }
            PowerId::DarkEmbrace => {
                self.set_power_private_int(owner, PowerId::DarkEmbrace, 0, 0)
            }
            PowerId::Tender => self.set_power_private_int(owner, PowerId::Tender, 0, 0),
            PowerId::Strangle => {
                self.strangle_cards.retain(|&(power, _, _)| power != p);
            }
            PowerId::VitalSpark => {
                // powers/remaining_c.py VitalSparkPower.on_removed.
                for k in self.all_players_pile_cards() {
                    if self.cards[k as usize].cv.affliction == Some(CardAffliction::Tainted) {
                        self.cards[k as usize].cv.affliction = None;
                        self.cards[k as usize].cv.affliction_amount = 0;
                    }
                }
            }
            PowerId::Sandpit => {
                // C# SandpitPower.AfterRemoved: if both owner and bound target are alive, the
                // target player and that player's Osty are force-killed.
                let Some(target) = self.powers[p as usize].target else {
                    self.unported("Sandpit missing target");
                    return;
                };
                if self.cr(owner).is_dead() || self.cr(target).is_dead() {
                    return;
                }
                let osties: Vec<CId> = self
                    .allies
                    .iter()
                    .copied()
                    .filter(|&c| {
                        self.cr(c).pet_owner == Some(target) && self.cr(c).monster_id == "OSTY"
                    })
                    .collect();
                self.force_kill_creature(target);
                for osty in osties {
                    self.force_kill_creature(osty);
                }
            }
            _ => {}
        }
    }

    // ───────────────────────────── turn ticks ─────────────────────────────

    /// `fire_after_turn_end`, including the legacy `on_turn_end_enemy_side` tick and removal.
    pub fn fire_after_turn_end(&mut self, side: Side) {
        for c in self.all_creatures() {
            let list = self.cr(c).power_list();
            for p in list {
                let meta = self.powers[p as usize].meta();
                let used_turn_hook = meta.listens(Hook::AfterTurnEnd)
                    || self.powers[p as usize].id == PowerId::Hellraiser;
                let mut used_legacy = false;
                if used_turn_hook {
                    self.power_event(p, c, Hook::AfterTurnEnd, Ev::Side(side));
                }
                if side == Side::Enemy && !used_turn_hook && meta.listens(Hook::OnTurnEndEnemySide)
                {
                    self.power_tick_enemy_side(p);
                    used_legacy = true;
                }
                if (used_turn_hook || used_legacy)
                    && self.powers[p as usize].should_remove()
                    && self.cr(c).powers.contains(&p)
                {
                    self.remove_power(c, p);
                }
            }
        }
        self.fire_relics(Hook::AfterTurnEnd, Ev::Side(side));
        self.fire(Hook::AfterTurnEndLate, Ev::Side(side), None);
    }

    /// `on_turn_end_enemy_side`.
    fn power_tick_enemy_side(&mut self, p: PRef) {
        let before=self.powers[p as usize].amount;
        let pw = &mut self.powers[p as usize];
        match pw.id {
            PowerId::Vulnerable
            | PowerId::Weak
            | PowerId::Frail
            | PowerId::Colossus
            | PowerId::Intangible => {
                if pw.skip_next_tick {
                    pw.skip_next_tick = false;
                } else {
                    pw.amount -= 1;
                }
            }
            _ => self.unported("on_turn_end_enemy_side"),
        }
        if before!=self.powers[p as usize].amount {
            self.record_public_power(p,"change",Some(before),true,false);
        }
    }

    /// `fire_after_side_turn_start`, including the legacy `on_turn_start_own_side` tick.
    pub fn fire_after_side_turn_start(&mut self, side: Side) {
        for c in self.all_creatures() {
            for p in self.cr(c).power_list() {
                let meta = self.powers[p as usize].meta();
                let used_turn_hook = meta.listens(Hook::AfterSideTurnStart);
                let mut used_legacy = false;
                if used_turn_hook {
                    self.power_event(p, c, Hook::AfterSideTurnStart, Ev::Side(side));
                } else if self.cr(c).side == side && meta.listens(Hook::OnTurnStartOwnSide) {
                    self.unported("on_turn_start_own_side");
                    used_legacy = true;
                }
                if (used_turn_hook || used_legacy)
                    && self.powers[p as usize].should_remove()
                    && self.cr(c).powers.contains(&p)
                {
                    self.remove_power(c, p);
                }
            }
        }
        self.fire_relics(Hook::AfterSideTurnStart, Ev::Side(side));
    }

    /// `fire_before_turn_end`: very-early, early and normal tiers.
    pub fn fire_before_turn_end(&mut self, side: Side) {
        if side == Side::Player {
            self.pending_turn_end_hooks = Some(TurnEndWalk {
                auto_post_owner: 0, tier: 0, loaded: false, listeners: Vec::new(), next: 0,
                stampede: None, detached: false, resume_stage: crate::combat::TurnStage::PlayerTurnEnd,
            });
            self.resume_turn_end_hooks();
            return;
        }
        for hook in [
            Hook::BeforeTurnEndVeryEarly,
            Hook::BeforeTurnEndEarly,
            Hook::BeforeTurnEnd,
        ] {
            self.fire(hook, Ev::Side(side), None);
            if self.fault.is_some() { return; }
        }
    }

    /// Models/Powers/StampedePower.cs:19-31 re-queries the current hand and samples
    /// Shuffle once per iteration, awaiting the complete child AutoPlay each time.
    fn stampede_autoplays(&mut self, p: PRef, owner: CId, mut next_iteration: i32) {
        // The C# loop condition reads this instance's current Amount on each iteration.
        while next_iteration < self.powers[p as usize].amount {
            // StampedePower.cs:24-31 has no ending guard: it still samples Shuffle.
            // CardCmd.AutoPlay (CardCmd.cs:36-39) then refuses the selected card,
            // leaving it in hand for subsequent iterations.
            let candidates: Vec<_> = self.players[owner].hand.iter().copied()
                .filter(|&k| self.cards[k as usize].card_type == CardType::Attack
                    && !self.cards[k as usize].is_unplayable()).collect();
            if candidates.is_empty() { return; }
            let i = self.run.rngs.shuffle.choice_index(candidates.len());
            next_iteration += 1;
            self.auto_play_card(candidates[i], None, false);
            if self.pending_choice.is_some() {
                if let Some(walk) = self.pending_turn_end_hooks.as_mut() {
                    walk.stampede = Some((p, owner, next_iteration));
                } else {
                    self.unported("Stampede selector outside a resumable turn-end listener walk");
                }
                return;
            }
        }
    }

    /// Hook.cs:699-711 snapshots IterateCombatHookListeners and awaits each callback.
    /// Preserve that cursor AND the current Stampede callback's loop across choices.
    /// PendingPlay finishes the child's hooks/result pile before this walk resumes.
    pub(crate) fn resume_turn_end_hooks(&mut self) {
        // CombatManager.cs:1201-1215 waits for AutoPostPlay completion before
        // BeforeSideTurnEnd, and :1411 runs AfterSideTurnEnd only after hand flush.
        // Tier zero adapts Stampede's C# AutoPostPlay hook from the legacy hook enum.
        const HOOKS: &[Hook] = &[Hook::BeforeTurnEnd, Hook::BeforeTurnEndVeryEarly, Hook::BeforeTurnEndEarly, Hook::BeforeTurnEnd];
        if self.pending_turn_end_hooks.is_none() {
            if self.has_parked_turn_end_tasks() {
                self.restore_first_live_choice();
                return;
            }
            self.pending_turn_end_hooks = self.turn_end_join.take();
        }
        while let Some(mut walk) = self.pending_turn_end_hooks.take() {
            if self.fault.is_some() { return; }
            if self.pending_choice.is_some() {
                if self.run.choice_scheduling.is_live() {
                    // CombatManager.cs:1191-1210 starts each seat up to its first
                    // pause. Hook.cs:955-976 does the same for EACH tier callback,
                    // and joins all three tiers only after assigning every task.
                    self.pending_turn_setup = Some(walk.resume_stage);
                    if walk.detached {
                        self.pending_turn_end_hooks = Some(walk);
                        return;
                    }
                    let mut task = walk.clone();
                    task.detached = true;
                    if walk.tier > 0 { task.next = task.listeners.len(); }
                    self.pending_turn_end_hooks = Some(task);
                    self.park_live_choice();
                    walk.stampede = None;
                    if walk.tier == 0 {
                        walk.next = walk.listeners.len();
                    }
                    self.pending_turn_end_hooks = Some(walk);
                    continue;
                }
                // The synchronous test-selector recordings finish this callback
                // before assigning the next one. Only Live parks independent
                // tasks (CombatManager.cs:1191-1210, Hook.cs:955-976).
                self.pending_turn_setup = Some(walk.resume_stage);
                self.pending_turn_end_hooks = Some(walk);
                return;
            }
            if let Some((p, owner, next_iteration)) = walk.stampede.take() {
                self.pending_turn_end_hooks = Some(walk);
                self.stampede_autoplays(p, owner, next_iteration);
                continue;
            }
            if walk.detached && (walk.tier > 0 || walk.next == walk.listeners.len()) {
                // Only the last completed task releases the phase barrier.
                if self.has_parked_turn_end_tasks() {
                    self.restore_first_live_choice();
                    return;
                }
                self.pending_turn_end_hooks = self.turn_end_join.take();
                continue;
            }
            if walk.tier == HOOKS.len() { return; }
            // Finish the in-flight callback's RNG iterations even after victory,
            // but Hook.cs:28-38 forbids starting listeners when over OR ending.
            if self.is_over_or_ending() {
                // Hook.cs:28-38 stops NEW listeners, not assigned tasks.
                // CombatManager.cs:1206-1216 joins every context before checking
                // victory. Keep an earlier seat's selector and in-flight RNG.
                if self.has_parked_turn_end_tasks() {
                    walk.tier = HOOKS.len();
                    walk.loaded = false;
                    walk.listeners.clear();
                    walk.next = 0;
                    self.turn_end_join = Some(walk);
                    self.restore_first_live_choice();
                } else {
                    // Last resumed task may still have unstarted card listeners.
                    // They are skipped, but its completed phase barrier must be
                    // released before CombatManager's CheckWinCondition.
                    self.turn_end_join = None;
                }
                return;
            }
            let hook = HOOKS[walk.tier];
            if !walk.loaded {
                for c in self.all_creatures() {
                    if walk.tier == 0 && c != walk.auto_post_owner { continue; }
                    if walk.tier == 0 && !self.is_player_taking_turn(c) { continue; }
                    walk.listeners.extend(self.listeners_of(c, hook).into_iter()
                        .filter(|&p| (self.powers[p as usize].id == PowerId::Stampede) == (walk.tier == 0))
                        .map(|p| TurnStartListener::Power(c, p)));
                    if walk.tier == 0 {
                        // CombatState.cs:272-327: powers/relics precede cards, and
                        // AllPiles keeps Hand, Draw, Discard, Exhaust, Play order.
                        if self.players.is_player(c) && self.players[c].hooks_active {
                            for k in self.pile_cards(c) {
                                if crate::card::auto_post_play_legacy_hooks(self.cards[k as usize].id) != 0 {
                                    walk.listeners.push(TurnStartListener::Card(k));
                                }
                            }
                        }
                        continue;
                    }
                    if self.players.is_player(c) && self.players[c].hooks_active {
                        for (r, relic) in self.run.relics.iter().enumerate() {
                            if relic.enabled && relic.owner == c
                                && relic_hook_mask(relic.id) & (1u128 << hook as u32) != 0 {
                                walk.listeners.push(TurnStartListener::Relic(r));
                            }
                        }
                    }
                }
                walk.loaded = true;
            }
            if walk.next == walk.listeners.len() {
                if walk.tier == 0 && (walk.auto_post_owner as usize) + 1 == self.players.len()
                    || walk.tier + 1 == HOOKS.len() {
                    if self.has_parked_turn_end_tasks() {
                        walk.tier += 1;
                        walk.loaded = false;
                        walk.listeners.clear();
                        walk.next = 0;
                        self.turn_end_join = Some(walk);
                        self.restore_first_live_choice();
                        return;
                    }
                    if walk.tier + 1 == HOOKS.len() { return; }
                }
                if walk.tier == 0 && (walk.auto_post_owner as usize) + 1 < self.players.len() {
                    walk.auto_post_owner += 1;
                } else {
                    walk.tier += 1;
                }
                walk.loaded = false;
                walk.listeners.clear();
                walk.next = 0;
                self.pending_turn_end_hooks = Some(walk);
                continue;
            }
            let listener = walk.listeners[walk.next];
            walk.next += 1;
            let owner = walk.auto_post_owner;
            self.pending_turn_end_hooks = Some(walk);
            match listener {
                TurnStartListener::Power(c, p) if self.cr(c).power_list().contains(&p) =>
                    self.power_event(p, c, hook, Ev::Side(Side::Player)),
                TurnStartListener::Relic(r) if self.run.relics[r].enabled
                    && self.players[self.run.relics[r].owner].hooks_active =>
                    self.relic_event(r, hook, Ev::Side(Side::Player)),
                TurnStartListener::Card(k) if self.card_listener_valid(k) =>
                    self.fire_card_after_auto_post_play(k, owner),
                _ => {}
            }
        }
    }

    /// `fire_after_energy_reset` (power removal after the first tier).
    pub fn fire_after_energy_reset(&mut self, owner: CId) {
        for p in self.listeners_of(owner, Hook::AfterEnergyReset) {
            self.power_event(p, owner, Hook::AfterEnergyReset, Ev::Creature(owner));
            if self.powers[p as usize].should_remove() && self.cr(owner).powers.contains(&p) {
                self.remove_power(owner, p);
            }
        }
        self.fire_relics_for_owner(Hook::AfterEnergyReset, Ev::Creature(owner), Some(owner));
        self.fire(Hook::AfterEnergyResetLate, Ev::Creature(owner), Some(owner));
    }

    pub fn fire_after_player_turn_start(&mut self, owner: CId) {
        self.fire_setup_hooks(owner, &[Hook::AfterPlayerTurnStartEarly,
            Hook::AfterPlayerTurnStart, Hook::AfterPlayerTurnStartLate],
            crate::combat::TurnStage::PostAfterPlayerTurnStart);
    }

    /// Stratagem.AfterShuffle awaits its own choice before the normal draw and
    /// AfterPlayerTurnStart callbacks. Other seated setup tasks remain runnable.
    pub(crate) fn defer_hand_draw_setup(&mut self, owner: CId, deferred: crate::choice::DeferredSetupChoice) {
        let walk = TurnStartWalk {
            owner, tier: 0, loaded: false, listeners: Vec::new(), next: 0,
            hooks: &[Hook::AfterPlayerTurnStartEarly, Hook::AfterPlayerTurnStart, Hook::AfterPlayerTurnStartLate],
            resume_stage: crate::combat::TurnStage::PostAfterPlayerTurnStart,
        };
        self.deferred_setup_choices.push((owner, Some(walk), deferred));
    }

    pub(crate) fn fire_setup_hooks(&mut self, owner: CId, hooks: &'static [Hook],
        resume_stage: crate::combat::TurnStage) {
        if self.pending_player_turn_hooks.is_some() {
            self.unported("concurrent player turn-start selectors require seat scheduler");
            return;
        }
        self.pending_player_turn_hooks = Some(TurnStartWalk {
            owner, tier: 0, loaded: false, listeners: Vec::new(), next: 0,
            hooks, resume_stage,
        });
        self.resume_player_turn_hooks();
    }

    /// C# Hook.AfterPlayerTurnStart awaits one listener before invoking the next.
    /// Store the current tier's snapshot/cursor across selectors; build later tiers
    /// only after previous callbacks finish, so newly added listeners are visible.
    pub(crate) fn resume_player_turn_hooks(&mut self) {
        while let Some(mut walk) = self.pending_player_turn_hooks.take() {
            if let Some(deferred) = self.signalled_setup_choice.take() {
                self.deferred_setup_choices.push((walk.owner, Some(walk), deferred));
                return;
            }
            if self.pending_choice.is_some() {
                self.pending_player_turn_hooks = Some(walk);
                return;
            }
            let hook = walk.hooks[walk.tier];
            if !walk.loaded {
                // Hook.cs:714-740 checks at each Early/normal/Late loop start,
                // not between callbacks or when a paused snapshot resumes.
                if self.is_over_or_ending() { return; }
                // C# CombatState.IterateHookListeners: snapshot the WHOLE combat, not
                // just the player whose turn starts. Contains is rechecked on resume.
                for c in self.all_creatures() {
                    walk.listeners.extend(self.listeners_of(c, hook).into_iter()
                        .map(|p| TurnStartListener::Power(c, p)));
                    if !self.players.is_player(c) {
                        walk.listeners.push(TurnStartListener::Monster(c));
                        continue;
                    }
                    if !self.players[c].hooks_active { continue; }
                    for (r, relic) in self.run.relics.iter().enumerate() {
                        if relic.enabled && relic.owner == c
                            && relic_hook_mask(relic.id) & (1u128 << hook as u32) != 0 {
                            walk.listeners.push(TurnStartListener::Relic(r));
                        }
                    }
                    // PotionModel and OrbModel (including all concrete subclasses)
                    // inherit no-op AfterPlayerTurnStart{Early,Late} callbacks.
                    // AllPiles order: Hand, Draw, Discard, Exhaust, Play.
                    for k in self.pile_cards(c) {
                        walk.listeners.push(TurnStartListener::Card(k));
                        if let Some(a) = self.cards[k as usize].cv.affliction {
                            walk.listeners.push(TurnStartListener::Affliction(k, a));
                        }
                        for &(name, _) in &self.cards[k as usize].enchantments {
                            walk.listeners.push(TurnStartListener::Enchantment(k, name));
                        }
                    }
                }
                walk.loaded = true;
            }
            if walk.next == walk.listeners.len() {
                if walk.tier + 1 == walk.hooks.len() { return; }
                walk.tier += 1;
                walk.loaded = false;
                walk.listeners.clear();
                walk.next = 0;
                self.pending_player_turn_hooks = Some(walk);
                continue;
            }
            let listener = walk.listeners[walk.next];
            walk.next += 1;
            let owner = walk.owner;
            self.pending_player_turn_hooks = Some(walk);
            match listener {
                TurnStartListener::Power(c, p) if self.cr(c).power_list().contains(&p) =>
                    self.power_event(p, c, hook, Ev::Creature(owner)),
                TurnStartListener::Relic(r) if self.run.relics[r].enabled
                    && self.players[self.run.relics[r].owner].hooks_active =>
                    self.relic_event(r, hook, Ev::Creature(owner)),
                // Audited C# Models/{Cards,Afflictions,Enchantments,Monsters}:
                // no concrete class overrides any of these three callbacks.
                // Keep their snapshot positions and Contains checks; Imbued is
                // AfterAutoPrePlayPhaseEntered, NOT AfterPlayerTurnStart.
                TurnStartListener::Card(k) if self.card_listener_valid(k) => {}
                TurnStartListener::Affliction(k, a) if self.card_listener_valid(k)
                    && self.cards[k as usize].cv.affliction == Some(a) => {}
                TurnStartListener::Enchantment(k, name) if self.card_listener_valid(k)
                    && self.cards[k as usize].enchantments.iter().any(|&(n, _)| n == name) => {}
                TurnStartListener::Monster(c) if self.all_creatures().contains(&c) => {}
                _ => {}
            }
        }
    }

    fn power_before_hand_draw(&mut self, p: PRef, owner: CId, turn_owner: CId) {
        // CreativeAi/SpectrumShift/ForegoneConclusion.BeforeHandDraw explicitly
        // return for another player. Do not mistake those no-ops for unported hooks.
        if owner != turn_owner && matches!(self.powers[p as usize].id,
            PowerId::CreativeAi | PowerId::SpectrumShift | PowerId::ForegoneConclusion) {
            return;
        }
        match self.powers[p as usize].id {
            // CallOfTheVoidPower.BeforeHandDraw: independent one-card samples,
            // keyword applied before entry, whole generated batch materialized first.
            PowerId::CallOfTheVoid => {
                if owner == turn_owner {
                    let mut generated = Vec::new();
                    for _ in 0..self.powers[p as usize].amount.max(0) {
                        generated.extend(self.generate_unlocked_owner_cards(owner, None, 1));
                    }
                    for &k in &generated { self.cards[k as usize].keywords |= KW_ETHEREAL; }
                    for k in generated { self.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(owner)); }
                }
            }
            PowerId::HelloWorld => {
                if owner == turn_owner && self.cr(owner).is_player {
                    let count = self.powers[p as usize].amount.max(0) as usize;
                    let generated =
                        self.generate_character_cards(owner, None, Some(CardRarity::Common), count, true);
                    for k in generated {
                        self.add_generated_card(k, PileKind::Hand, PilePos::None, false);
                    }
                }
            }
            _ => {
                if !self.chars_before_hand_draw(p, owner, turn_owner)
                    && !(owner == turn_owner
                        && (self.defect_before_hand_draw(p, owner) || self.regent_before_hand_draw(p, owner)))
                {
                    self.unported("before_hand_draw");
                }
            }
        }
    }

    pub fn fire_before_combat_start(&mut self) {
        self.fire(Hook::BeforeCombatStart, Ev::None, None);
        self.fire(Hook::BeforeCombatStartLate, Ev::None, None);
    }

    pub fn fire_after_combat_victory(&mut self) {
        self.fire(Hook::AfterCombatVictoryEarly, Ev::None, None);
        self.fire(Hook::AfterCombatVictory, Ev::None, None);
    }
}
