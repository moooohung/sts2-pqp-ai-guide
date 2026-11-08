//! Regent mechanics: stars (C# `PlayerCmd.GainStars` / star costs), forge (`ForgeCmd.Forge`,
//! Sovereign Blade) and the powers that listen to them.

use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind, PilePos, Props};
use crate::creature::PRef;
use crate::hooks::Ev;
use crate::tables::*;
use smallvec::SmallVec;

impl Combat {
    /// `combat.gain_stars(owner, amount)`: skipped while the combat is over or ending.
    pub fn gain_stars(&mut self, owner: CId, amount: i32) {
        if amount <= 0 || self.is_over_or_ending() || !self.players.is_player(owner) {
            return;
        }
        self.players[owner].stars += amount;
        self.cr_mut(owner).stars += amount;
        self.hist.stars_gained_this_turn[owner as usize] += amount;
        self.fire(Hook::OnStarsGained, Ev::Stars { owner, amount }, Some(owner));
    }

    /// `combat.spend_stars(owner, amount)`: returns the stars actually spent.
    pub fn spend_stars(&mut self, owner: CId, amount: i32) -> i32 {
        if amount <= 0 || !self.players.is_player(owner) {
            return 0;
        }
        let spent = amount.min(self.players[owner].stars);
        if spent <= 0 {
            return 0;
        }
        self.players[owner].stars -= spent;
        self.cr_mut(owner).stars = self.players[owner].stars;
        self.fire(Hook::OnStarsSpent, Ev::Stars { owner, amount: spent }, Some(owner));
        spent
    }

    /// `ForgeCmd.GetSovereignBlades(player, includeExhausted)`: non-dupe blades in the piles.
    pub(crate) fn sovereign_blades(&self, owner: CId, include_exhausted: bool) -> SmallVec<[KRef; 16]> {
        let mut out = SmallVec::new();
        let p = &self.players[owner];
        let piles: [&[KRef]; 5] = [&p.hand, &p.draw, &p.discard, &p.play, &p.exhaust];
        for (i, pile) in piles.into_iter().enumerate() {
            if i == 4 && !include_exhausted {
                continue;
            }
            for &k in pile {
                let c = &self.cards[k as usize];
                if c.id == CardId::SovereignBlade && !c.cv.is_dupe {
                    out.push(k);
                }
            }
        }
        out
    }

    /// `combat.forge(owner, amount, source)` (C# ForgeCmd.Forge): a new blade in hand when no
    /// live blade exists, +amount damage on every blade (exhausted ones too), then AfterForge.
    pub fn forge(&mut self, owner: CId, amount: i32, source: Option<KRef>) {
        if self.is_over_or_ending() || amount <= 0 || !self.players.is_player(owner) || !self.cr(owner).is_alive() {
            return;
        }
        if self.sovereign_blades(owner, false).is_empty()
            && let Some(blade) = self.new_card(owner, CardId::SovereignBlade, false)
        {
            self.add_generated_card(blade, PileKind::Hand, PilePos::Bottom, true);
        }
        for k in self.sovereign_blades(owner, true) {
            let card = &mut self.cards[k as usize];
            card.damage = Some(card.damage.unwrap_or(10) + amount);
        }
        self.fire(Hook::AfterForge, Ev::Forge { forger: owner, amount, source }, None);
    }

    /// r7 `active_card_play_is_last_in_series`.
    pub(crate) fn play_is_last_in_series(&self) -> bool {
        self.pending_play.last().is_none_or(|p| p.remaining <= 0)
    }

    /// Bodies of the star/forge hooks; `false` for powers this file does not own.
    pub(crate) fn regent_power_event(&mut self, p: PRef, owner: CId, hook: Hook, ev: Ev) -> bool {
        let amount = self.powers[p as usize].amount;
        match (self.powers[p as usize].id, hook, ev) {
            // BlackHolePower.AfterStarsGained: `amount` unpowered damage to every enemy.
            (PowerId::BlackHole, Hook::OnStarsGained, Ev::Stars { amount: stars, .. }) => {
                if stars > 0 {
                    self.black_hole_damage(owner, amount);
                }
            }
            // ChildOfTheStarsPower.AfterStarsSpent: amount * spent unpowered block.
            (PowerId::ChildOfTheStars, Hook::OnStarsSpent, Ev::Stars { amount: stars, .. }) => {
                if stars > 0 {
                    self.power_gain_unpowered_block(owner, amount * stars);
                }
            }
            // HammerTimePower.AfterForge forges for the other living players: none in solo play.
            (PowerId::HammerTime, Hook::AfterForge, _) => {}
            // OrbitPower.AfterEnergySpent: Amount energy per 4 energy the owner spent
            // (vars[0] = energy spent, vars[1] = triggers so far).
            (PowerId::Orbit, Hook::AfterEnergySpent, Ev::EnergySpent { card, amount: spent }) => {
                if spent > 0 && self.cards[card as usize].owner == owner {
                    let pw = &mut self.powers[p as usize];
                    pw.vars[0] += spent;
                    let triggers = pw.vars[0] / 4 - pw.vars[1];
                    if triggers > 0 {
                        pw.vars[1] += triggers;
                        if !self.is_over_or_ending() {
                            self.players[owner].energy += amount * triggers;
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `after_card_played`; `false` for powers this file does not own.
    pub(crate) fn regent_after_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // BlackHolePower.AfterCardPlayed: the card spent stars (last play of a series).
            PowerId::BlackHole => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].cv.stars_spent_for_play.unwrap_or(0) > 0
                    && self.play_is_last_in_series()
                {
                    self.black_hole_damage(owner, amount);
                }
            }
            // ParryPower: Amount unpowered block per Sovereign Blade the owner plays.
            PowerId::Parry => {
                if self.cards[k as usize].id == CardId::SovereignBlade && self.cards[k as usize].owner == owner {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            // SeekingEdgePower has no hooks (Sovereign Blade reads it).
            PowerId::SeekingEdge => {}
            // C# PaleBlueDotPower.AfterCardPlayed: once per turn, when the owner has finished
            // 5 card plays this turn, DrawCardsNextTurn(Amount) (vars[0] = activated). The data
            // table lists the C# hooks (r7's modify_hand_draw version is patched in the recorder).
            PowerId::PaleBlueDot => {
                if self.cards[k as usize].owner == owner
                    && self.powers[p as usize].vars[0] == 0
                    && crate::content::plays_finished_pub(self, owner) >= 5
                {
                    self.powers[p as usize].vars[0] = 1;
                    self.apply_power_to(owner, PowerId::DrawCardsNextTurn, amount, Some(owner));
                }
            }
            // MonologuePower.AfterCardPlayed: +1 Strength for a remembered card; amount shows
            // the Strength applied so far (vars[0]).
            PowerId::Monologue => {
                let slot = self.monologue_plays.iter().position(|&entry| entry == (p, k));
                if let Some(i) = slot {
                    self.monologue_plays.swap_remove(i);
                    self.creature_apply_power(owner, PowerId::Strength, 1, None, None, false, false);
                    let pw = &mut self.powers[p as usize];
                    pw.vars[0] += 1;
                    pw.amount = pw.vars[0];
                }
            }
            // C# VoidFormPower.AfterCardPlayed: own, non-auto, last play of a series.
            PowerId::VoidForm => {
                if self.cards[k as usize].owner == owner
                    && !self.pending_play.last().is_some_and(|p| p.card == k && p.is_auto)
                    && self.play_is_last_in_series()
                {
                    self.powers[p as usize].vars[0] += 1;
                }
            }
            _ => return false,
        }
        true
    }

    /// `eligible_registered_cards(COLORLESS, "combat")` + `create_cards_from_ids(distinct=True)`.
    pub(crate) fn generate_colorless(&mut self, owner: CId, count: usize) -> SmallVec<[KRef; 10]> {
        let Some(pool) = crate::cardgen::character_pool(crate::character::CardPool::Colorless, false, None, None) else {
            self.unported("colorless card pool");
            return SmallVec::new();
        };
        self.generate_pool_cards(owner, pool, count, true)
    }

    /// Regent powers' `after_turn_end`.
    pub(crate) fn regent_after_turn_end(&mut self, p: PRef, owner: CId, side: crate::creature::Side) -> bool {
        match self.powers[p as usize].id {
            // ConquerorPower.AfterTurnEnd: ticks down on the owner's side.
            PowerId::Conqueror => {
                if side == self.cr(owner).side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            // C# PaleBlueDotPower.AfterSideTurnEnd: re-arm for the next turn.
            PowerId::PaleBlueDot => {
                if side == self.cr(owner).side {
                    self.powers[p as usize].vars[0] = 0;
                }
            }
            // MonologuePower: undo the Strength it gave, then the power goes.
            PowerId::Monologue => {
                if side == self.cr(owner).side {
                    let applied = self.powers[p as usize].vars[0];
                    self.creature_apply_power(owner, PowerId::Strength, -applied, None, None, false, false);
                    self.remove_power(owner, p);
                    self.monologue_plays.retain(|&(power, _)| power != p);
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `after_energy_reset`.
    pub(crate) fn regent_after_energy_reset(&mut self, p: PRef, owner: CId) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // StarNextTurnPower: gain the stars, then removed.
            PowerId::StarNextTurn => {
                if self.cr(owner).is_player {
                    self.gain_stars(owner, amount);
                    self.powers[p as usize].amount = 0;
                }
            }
            // GenesisPower.AfterEnergyReset: Amount stars every turn.
            PowerId::Genesis => {
                if self.cr(owner).is_player {
                    self.gain_stars(owner, amount);
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `after_side_turn_start`.
    pub(crate) fn regent_after_side_turn_start(&mut self, p: PRef, owner: CId, side: crate::creature::Side) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // FurnacePower: forge Amount on the owner's side.
            PowerId::Furnace => {
                if side == self.cr(owner).side {
                    self.forge(owner, amount, None);
                }
            }
            // The energy reset already ran this turn (solo play always resets energy).
            PowerId::StarNextTurn | PowerId::Genesis => {}
            _ => return false,
        }
        true
    }

    /// Regent powers' `before_card_played`.
    pub(crate) fn regent_before_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // TheSealedThronePower.BeforeCardPlayed: Amount stars per own card.
            PowerId::TheSealedThrone => {
                if self.cards[k as usize].owner == owner {
                    self.gain_stars(owner, amount);
                }
            }
            // MonologuePower.BeforeCardPlayed: each instance remembers every in-flight card.
            PowerId::Monologue => {
                if self.cards[k as usize].owner == owner {
                    self.monologue_plays.push((p, k));
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `after_card_generated_for_combat`.
    pub(crate) fn regent_after_card_generated(&mut self, p: PRef, owner: CId, card: KRef, by_player: bool) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // PillarOfCreationPower: Amount unpowered block per card the owner generated.
            PowerId::PillarOfCreation => {
                if by_player && self.cards[card as usize].owner == owner {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            // C# ArsenalPower.AfterCardGeneratedForCombat: Amount Strength per card the owner
            // generated (the data table lists this C# hook; r7's colorless-play version is
            // patched in the recorder).
            PowerId::Arsenal => {
                if by_player && self.cards[card as usize].owner == owner {
                    self.creature_apply_power(owner, PowerId::Strength, amount, Some(owner), None, false, false);
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `before_hand_draw` (owner is the drawing player).
    pub(crate) fn regent_before_hand_draw(&mut self, p: PRef, owner: CId) -> bool {
        let amount = self.powers[p as usize].amount;
        match self.powers[p as usize].id {
            // SpectrumShiftPower: Amount distinct colorless cards into hand.
            PowerId::SpectrumShift => {
                if self.cr(owner).is_player && amount > 0 {
                    for g in self.generate_colorless(owner, amount as usize) {
                        self.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
                    }
                }
            }
            // ForegoneConclusionPower: shuffle if needed, pick Amount draw-pile cards to hand.
            PowerId::ForegoneConclusion => {
                if self.cr(owner).is_player {
                    self.shuffle_if_needed(owner);
                    if amount > 0 && !self.players[owner].draw.is_empty() {
                        let cards = crate::content::draw_selector_order_pub(self, owner);
                        let n = (amount as usize).min(cards.len()) as u16;
                        let r = crate::choice::Resolver {
                            src: crate::choice::ChoiceSrc::Card(CardId::ForegoneConclusion),
                            k: 0, target: Some(owner), step: 1, arg: 0,
                        };
                        self.request_multi_card_choice(&cards, PileKind::Draw, r, n, Some(n), false);
                    }
                    // C# ForegoneConclusionPower.BeforeHandDraw ends with PowerCmd.Remove(this).
                    self.remove_power(owner, p);
                }
            }
            _ => return false,
        }
        true
    }

    /// Regent powers' `modify_damage_multiplicative`.
    pub(crate) fn regent_damage_multiplicative(&self, p: PRef, owner: CId, target: CId, props: Props,
        source_card: Option<crate::card::KRef>) -> Option<f64> {
        match self.powers[p as usize].id {
            // ConquerorPower: Sovereign Blade hits on the owner deal double.
            PowerId::Conqueror => Some(
                if target == owner
                    && props.is_powered_attack()
                    && source_card.is_some_and(|k| self.cards[k as usize].id == CardId::SovereignBlade)
                {
                    2.0
                } else {
                    1.0
                },
            ),
            _ => None,
        }
    }

    /// C# SwordSagePower: every Sovereign Blade of the owner carries BaseReplayCount +Amount
    /// (AfterPowerAmountChanged on existing cards, AfterCardEnteredCombat on new ones, clones
    /// copy it), so the bonus is the owner's current Sword Sage amount.
    pub(crate) fn sword_sage_replays(&self, k: KRef) -> i32 {
        let card = &self.cards[k as usize];
        if card.id != CardId::SovereignBlade {
            return 0;
        }
        self.find_power(card.owner, PowerId::SwordSage).map_or(0, |p| self.powers[p as usize].amount.max(0))
    }

    /// C# `VoidFormPower.ShouldSkip` negated: the owner's card in hand or play while fewer
    /// than Amount cards were played this turn (vars[0]).
    pub(crate) fn void_form_covers(&self, p: PRef, owner: CId, k: KRef) -> bool {
        let pw = &self.powers[p as usize];
        self.cards[k as usize].owner == owner
            && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k))
            && pw.vars[0] < pw.amount
    }

    fn black_hole_damage(&mut self, owner: CId, amount: i32) {
        for e in self.hittable_enemies() {
            self.deal_damage(Some(owner), e, amount, Props::UNPOWERED);
        }
    }
}
