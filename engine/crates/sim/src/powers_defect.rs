//! Defect powers (r7 `powers/*.py` classes named per arm). Each `defect_*` returns `false` /
//! `None` for powers it does not own, so `hooks.rs` can fall through to `unported`.

use crate::card::{CId, KRef};
use crate::combat::{Combat, Props};
use crate::creature::{PRef, Side};
use crate::orbs::OrbKind;
use crate::tables::*;

impl Combat {
    #[inline]
    fn dpw(&self, p: PRef) -> i32 {
        self.powers[p as usize].amount
    }

    /// TemporaryFocusPower subclasses (FocusedStrike, Hotfix, Synchronize).
    #[inline]
    pub(crate) fn is_temporary_focus(id: PowerId) -> bool {
        matches!(id, PowerId::FocusedStrike | PowerId::Hotfix | PowerId::Synchronize | PowerId::HyperbeamFocusDown)
    }

    pub(crate) fn temporary_focus_sign(id: PowerId) -> i32 {
        if id == PowerId::HyperbeamFocusDown { -1 } else { 1 }
    }

    pub(crate) fn defect_before_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        match self.powers[p as usize].id {
            // FeralPower: reset the returned-attack count on the owner's turn.
            PowerId::Feral => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].vars[0] = 0;
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_after_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        let amount = self.dpw(p);
        let own = side == self.cr(owner).side;
        match self.powers[p as usize].id {
            // remaining_a.py CoolantPower
            PowerId::Coolant => {
                if own {
                    let n = self.count_distinct_orb_kinds(owner);
                    if n > 0 {
                        self.power_gain_unpowered_block(owner, n * amount);
                    }
                }
            }
            // turn_effects.py BiasedCognitionPower
            PowerId::BiasedCognition => {
                if own {
                    self.apply_power_to(owner, PowerId::Focus, -amount, Some(owner));
                }
            }
            // LightningRod / Spinner fall back to after_energy_reset only when no energy reset ran
            // this turn; the player's turn setup always resets energy first.
            PowerId::LightningRod | PowerId::Spinner => {}
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_after_energy_reset(&mut self, p: PRef, owner: CId) -> bool {
        match self.powers[p as usize].id {
            // remaining_b.py LightningRodPower
            PowerId::LightningRod => {
                if self.cr(owner).is_player {
                    self.channel_orb(owner, OrbKind::Lightning);
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            // monster.py SpinnerPower
            PowerId::Spinner => {
                for _ in 0..self.dpw(p).max(0) {
                    self.channel_orb(owner, OrbKind::Glass);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_before_turn_end(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        match self.powers[p as usize].id {
            // remaining_b.py HailstormPower: at least one Frost orb.
            PowerId::Hailstorm => {
                if side == self.cr(owner).side && self.count_orbs(owner, OrbKind::Frost) >= 1 {
                    let amount = self.dpw(p);
                    for e in self.hittable_enemies() {
                        self.deal_damage(Some(owner), e, amount, Props::UNPOWERED);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_after_turn_end(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        let amount = self.dpw(p);
        let own = side == self.cr(owner).side;
        match self.powers[p as usize].id {
            id if Self::is_temporary_focus(id) => {
                if own {
                    self.remove_power(owner, p);
                    self.creature_apply_power(owner, PowerId::Focus, -Self::temporary_focus_sign(id)*amount, None, None, false, false);
                }
            }
            // remaining_a.py ConsumingShadowPower
            PowerId::ConsumingShadow => {
                if own {
                    for _ in 0..amount.max(0) {
                        self.evoke_last_orb(owner);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_on_card_drawn(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        match self.powers[p as usize].id {
            // C# IterationPower.AfterCardDrawn filters history by the power owner.
            PowerId::Iteration => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].is_status()
                    && self.hist.status_drawn_this_turn[owner as usize] <= 1
                {
                    let amount = self.dpw(p);
                    self.draw_cards(owner, amount, false);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_before_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let id = self.powers[p as usize].id;
        let card = &self.cards[k as usize];
        let own_power = card.owner == owner && card.card_type == CardType::Power;
        match id {
            // turn_effects.py StormPower, remaining_c.py SubroutinePower
            PowerId::Storm | PowerId::Subroutine => {
                if own_power {
                    let amount = self.dpw(p);
                    self.power_card_amounts.push((owner, id, k, amount));
                }
            }
            // card_play_effects.py FreePowerPower
            PowerId::FreePower => {
                if own_power && self.dpw(p) > 0 {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.dpw(p) <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_after_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let id = self.powers[p as usize].id;
        match id {
            PowerId::Storm | PowerId::Subroutine => {
                let amount = match self.power_card_amounts.iter().position(|e| e.0 == owner && e.1 == id && e.2 == k) {
                    Some(i) => self.power_card_amounts.remove(i).3,
                    None => 0,
                };
                if id == PowerId::Storm {
                    for _ in 0..amount.max(0) {
                        self.channel_orb(owner, OrbKind::Lightning);
                    }
                } else if amount > 0 && !self.is_over_or_ending() {
                    self.players[owner].energy += amount;
                }
            }
            _ => return false,
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn defect_after_power_amount_changed(&mut self, p: PRef, owner: CId, target: CId, changed: PowerId,
                                                    delta: i32, applier: Option<CId>, source: Option<KRef>) -> bool {
        let id = self.powers[p as usize].id;
        if id == PowerId::Feral {
            // FeralPower.after_power_amount_changed: count this turn's 0-energy Attack starts.
            if owner == target && changed == PowerId::Feral && delta > 0 {
                let n = self.hist.play_start_cards.iter().filter(|&&q| {
                    let c = &self.cards[q as usize];
                    c.owner == owner && c.card_type == CardType::Attack && c.energy_spent == 0
                }).count() as i32;
                self.powers[p as usize].vars[0] = n;
            }
            return true;
        }
        if !Self::is_temporary_focus(id) {
            return false;
        }
        // TemporaryFocusPower.AfterPowerAmountChanged skips initial application,
        // whose delta equals the attached instance's amount (BeforeApplied did it).
        if owner == target && changed == id && delta != 0 && delta != self.dpw(p) && !self.consume_ignore_next_instance(p) {
            self.creature_apply_power(owner, PowerId::Focus, Self::temporary_focus_sign(id)*delta, applier, source, false, false);
        }
        true
    }

    pub(crate) fn defect_after_player_turn_start(&mut self, p: PRef, owner: CId) -> bool {
        match self.powers[p as usize].id {
            // turn_effects.py LoopPower
            PowerId::Loop => {
                if self.cr(owner).is_player {
                    for _ in 0..self.dpw(p).max(0) {
                        self.trigger_first_orb_passive(owner);
                    }
                }
            }
            // C# HibernatePower.AfterPlayerTurnStart: PowerCmd.Decrement removes it at 0.
            PowerId::Hibernate => {
                self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                if self.powers[p as usize].amount <= 0 {
                    self.remove_power(owner, p);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_before_hand_draw(&mut self, p: PRef, owner: CId) -> bool {
        match self.powers[p as usize].id {
            // turn_effects.py CreativeAiPower: a random Power card to hand per stack.
            PowerId::CreativeAi => {
                if self.cr(owner).is_player {
                    for _ in 0..self.dpw(p).max(0) {
                        // C# CreativeAiPower.BeforeHandDraw uses
                        // player.Character.CardPool.GetUnlockedCards(...,
                        // CardMultiplayerConstraint), then GetDistinctForCombat.
                        for g in self.generate_unlocked_owner_cards(owner, Some(CardType::Power), 1) {
                            self.add_generated_card(g, crate::combat::PileKind::Hand, crate::combat::PilePos::Bottom, true);
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_after_card_generated(&mut self, p: PRef, owner: CId, k: KRef, by_player: bool) -> bool {
        let status = self.cards[k as usize].is_status();
        match self.powers[p as usize].id {
            // remaining_c.py SmokestackPower
            PowerId::Smokestack => {
                if by_player && status {
                    let amount = self.dpw(p);
                    for e in self.hittable_enemies() {
                        self.deal_damage(Some(owner), e, amount, Props::UNPOWERED);
                    }
                }
            }
            // remaining_c.py TrashToTreasurePower
            PowerId::TrashToTreasure => {
                if by_player && status {
                    for _ in 0..self.dpw(p).max(0) {
                        self.channel_random_orb(owner);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn defect_damage_additive(&self, p: PRef, owner: CId, dealer: Option<CId>, props: Props,
        source_card: Option<crate::card::KRef>, preview: bool, fault: &mut Option<&'static str>) -> Option<i32> {
        match self.powers[p as usize].id {
            // C# OneForAllPower.ModifyDamageAdditive: qualify by the source card's
            // owner, not the damage dealer. Osty attacks carry the player's card as
            // cardSource while the pet itself is the dealer.
            PowerId::OneForAll => {
                let hit = props.is_powered_attack() && source_card.is_some_and(|k| {
                    let c = &self.cards[k as usize];
                    c.owner == owner && !c.x_cost && if preview { self.query_modified_card_cost(owner, k, fault) == 0 } else { c.energy_spent == 0 }
                });
                Some(if hit { self.dpw(p) } else { 0 })
            }
            _ => None,
        }
    }

    pub(crate) fn defect_modify_hand_draw(&mut self, p: PRef, draw: i32) -> Option<i32> {
        match self.powers[p as usize].id {
            PowerId::MachineLearning => Some(draw + self.dpw(p)),
            _ => None,
        }
    }

    pub(crate) fn defect_modify_card_play_count(&mut self, p: PRef, owner: CId, k: KRef, count: i32) -> Option<i32> {
        let card = &self.cards[k as usize];
        match self.powers[p as usize].id {
            // remaining_c.py SignalBoostPower
            PowerId::SignalBoost => Some(
                if card.owner == owner && card.card_type == CardType::Power && self.dpw(p) > 0 { count + 1 } else { count },
            ),
            // C# EchoFormPower.ModifyCardPlayCount: count only this owner's first-in-series starts.
            PowerId::EchoForm => {
                if card.owner != owner {
                    return Some(count);
                }
                let starts = self.hist.first_series_actors.iter().filter(|&&actor| actor == owner).count() as i32;
                Some(if starts < self.dpw(p) { count + 1 } else { count })
            }
            _ => None,
        }
    }

    pub(crate) fn defect_after_modifying_card_play_count(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        match self.powers[p as usize].id {
            PowerId::SignalBoost => {
                let card = &self.cards[k as usize];
                if card.owner == owner && card.card_type == CardType::Power && self.dpw(p) > 0 {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.dpw(p) <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            _ => return false,
        }
        true
    }
}
