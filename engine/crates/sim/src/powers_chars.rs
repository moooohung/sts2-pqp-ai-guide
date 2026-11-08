//! Powers applied by other base characters' cards (Silent, Defect, Regent, Necrobinder) that
//! an Ironclad run meets through Prism, Kaleidoscope and similar relics. `hooks.rs` calls these
//! from each hook's fallback arm; a `false` / `None` return keeps the fail-closed `unported`.

use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, PilePos, Props};
use crate::creature::{PRef, Side};
use crate::tables::*;

impl Combat {
    /// Models/Powers/SoulboundPower.AfterCardGeneratedForCombat: per-instance recursion
    /// guard; match the actual creator to Applier, not the generated Soul's owner.
    pub(crate) fn necro_after_card_generated(&mut self, p: PRef, owner: CId, card: KRef, creator: Option<CId>) -> bool {
        if self.powers[p as usize].id != PowerId::Soulbound {
            return false;
        }
        if creator.is_some() && creator == self.powers[p as usize].applier
            && self.cards[card as usize].id == CardId::Soul && self.powers[p as usize].vars[0] == 0
        {
            self.powers[p as usize].vars[0] = 1;
            let amount = self.powers[p as usize].amount;
            let souls: Vec<_> = (0..amount).filter_map(|_| self.new_card(owner, CardId::Soul, false)).collect();
            for soul in souls {
                self.add_generated_card_by(soul, PileKind::Draw, PilePos::Random, Some(owner));
            }
            self.powers[p as usize].vars[0] = 0;
        }
        true
    }
    /// DoomPower.ShouldDoomTrigger / DoomKill. Resolve the same-side snapshot only once,
    /// from its first doomed owner, preserving CombatState creature order.
    pub(crate) fn doom_side_end(&mut self, owner: CId, side: Side) {
        if self.is_over_or_ending() || self.cr(owner).side != side || !self.cr(owner).is_alive() {
            return;
        }
        let doomed: Vec<_> = self.all_creatures().into_iter().filter(|&c|
            self.cr(c).side == side && self.cr(c).is_alive()
                && self.has_power(c, PowerId::Doom)
                && self.cr(c).hp <= self.power_amount(c, PowerId::Doom)).collect();
        if doomed.first().copied() != Some(owner) {
            return;
        }
        self.doom_kill(&doomed);
    }

    /// Models/Powers/DoomPower.DoomKill: consume the full captured list in order.
    /// No IsEnding check between creatures; CreatureCmd.Kill handles death rules.
    pub(crate) fn doom_kill(&mut self, doomed: &[CId]) {
        for &c in doomed { self.kill_creature(c); }
        // DoomPower.DoomKill awaits AfterDiedToDoom after the entire captured
        // list, including death-removal/fatal veto changes made by Kill.
        self.relics_after_died_to_doom(doomed);
    }

    #[inline]
    fn pw_amount(&self, p: PRef) -> i32 {
        self.powers[p as usize].amount
    }

    /// `dealer is owner or dealer.pet_owner is owner`.
    #[inline]
    fn dealt_by_owner_or_pet(&self, owner: CId, dealer: Option<CId>) -> bool {
        dealer.is_some_and(|d| d == owner || self.cr(d).pet_owner == Some(owner))
    }

    /// r7 `_amounts_for_played_cards[id(card)] = amount` (dict keyed by card identity).
    fn power_card_amount_set(&mut self, owner: CId, id: PowerId, k: KRef, amount: i32) {
        if let Some(e) = self.power_card_amounts.iter_mut().find(|e| e.0 == owner && e.1 == id && e.2 == k) {
            e.3 = amount;
        } else {
            self.power_card_amounts.push((owner, id, k, amount));
        }
    }

    /// r7 `_amounts_for_played_cards.pop(id(card), 0)`.
    fn power_card_amount_take(&mut self, owner: CId, id: PowerId, k: KRef) -> i32 {
        match self.power_card_amounts.iter().position(|e| e.0 == owner && e.1 == id && e.2 == k) {
            Some(i) => self.power_card_amounts.remove(i).3,
            None => 0,
        }
    }

    /// `add_generated_card_to_creature_hand(owner, make_shiv(upgraded))` (cards/status.py:1206).
    pub(crate) fn add_shiv_to_hand(&mut self, owner: CId, upgraded: bool) {
        if let Some(k) = self.new_card(owner, CardId::Shiv, upgraded) {
            self.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
        }
    }

    /// `clone_card_to_hand(owner, card)` (core/combat.py:3068-3072).
    pub(crate) fn clone_card_to_hand(&mut self, k: KRef) {
        let iid = self.run.new_card_id();
        let clone = self.cards[k as usize].clone_with_id(iid);
        let g = self.cards.len() as KRef;
        self.card_hook_mask |= CARD_HOOKS[clone.id.index()];
        self.cards.push(clone);
        self.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
    }

    /// `request_discard(owner, count)` (core/combat.py:2479-2504): a plain hand -> discard
    /// move with the discard hook, no Sly auto-play.
    pub(crate) fn request_discard(&mut self, owner: CId, count: i32) {
        if count <= 0 {
            return;
        }
        let n = if self.collect_setup_choices { count as u16 }
            else { (count as usize).min(self.players[owner].hand.len()) as u16 };
        if n == 0 && !self.collect_setup_choices {
            return;
        }
        let hand: Vec<KRef> = self.players[owner].hand.to_vec();
        let r = Resolver { src: ChoiceSrc::Card(CardId::ToolsOfTheTrade), k: 0, target: None, step: 1, arg: 0 };
        self.request_multi_card_choice_for_player(owner, &hand, PileKind::Hand, r, n, Some(n), false);
    }

    /// The `request_discard` resolver.
    pub(crate) fn resolve_request_discard(&mut self, selected: &[KRef]) {
        for &k in selected {
            let p = &mut self.players[self.cards[k as usize].owner];
            if let Some(pos) = p.hand.iter().position(|&x| x == k) {
                p.hand.remove(pos);
                p.discard.push(k);
                self.fire_after_card_changed_piles(k, PileKind::Hand, None);
                self.fire_after_card_discarded(k);
                // Commands/CardCmd.cs:153-154, including ToolsOfTheTrade discards.
                self.fire_relic_after_card_discarded(k);
            }
        }
    }

    /// PoisonPower.trigger (powers/turn_effects.py:577-602); OUTBREAK calls it directly.
    pub(crate) fn poison_trigger(&mut self, owner: CId) {
        let Some(p) = self.find_power(owner, PowerId::Poison) else { return };
        if !self.cr(owner).is_alive() || self.pw_amount(p) <= 0 {
            return;
        }
        let side = self.cr(owner).side;
        let accelerant: i32 = self
            .all_creatures()
            .into_iter()
            .filter(|&c| self.cr(c).side != side && self.cr(c).is_alive())
            .map(|c| self.power_amount(c, PowerId::Accelerant))
            .sum();
        let count = self.pw_amount(p).min(1 + accelerant);
        for _ in 0..count.max(0) {
            if !self.cr(owner).is_alive() || self.pw_amount(p) <= 0 {
                break;
            }
            let current = self.pw_amount(p);
            self.record_public_power(p,"trigger",Some(current),true,false);
            self.deal_damage(None, owner, current, Props::UNBLOCKABLE.or(Props::UNPOWERED));
            if self.cr(owner).is_alive() {
                self.powers[p as usize].amount -= 1;
                self.record_public_power(p,"change",Some(current),true,false);
                self.after_power_amount_changed(owner, PowerId::Poison, -1, None, None);
                if self.pw_amount(p) <= 0 && self.find_power(owner, PowerId::Poison) == Some(p) {
                    self.remove_power(owner, p);
                }
            }
        }
    }

    pub(crate) fn chars_before_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        match self.powers[p as usize].id {
            // powers/card_play_effects.py:494-498
            PowerId::PhantomBlades => {
                if side == self.cr(owner).side {
                    self.set_power_private_int(owner, PowerId::PhantomBlades, 0, 0);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_after_side_turn_start(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        let amount = self.pw_amount(p);
        let own_side = side == self.cr(owner).side;
        match self.powers[p as usize].id {
            // NeurosurgePower.AfterSideTurnStart: participating owner receives Doom
            // attributed to that owner (including Shroud/DeathsDoor interactions).
            PowerId::Neurosurge => {
                if own_side && self.can_receive_power(owner) {
                    self.creature_apply_power(owner, PowerId::Doom, amount, Some(owner), None, false, false);
                }
            }
            // Models/Powers/CountdownPower.cs AfterSideTurnStart: owner participant,
            // random hittable enemy on CombatTargets, no draw when the list is empty.
            PowerId::Countdown => {
                if own_side {
                    let enemies = self.hittable_enemies();
                    if !enemies.is_empty() {
                        let target = enemies[self.run.rngs.combat_targets.choice_index(enemies.len())];
                        self.apply_power_from(target, PowerId::Doom, amount, Some(owner), None);
                    }
                }
            }
            // C# PrepTimePower.AfterSideTurnStart.
            PowerId::PrepTime => {
                if own_side {
                    self.apply_power_from(owner, PowerId::Vigor, amount, Some(owner), None);
                }
            }
            // C# ReflectPower.AfterSideTurnStart.
            PowerId::Reflect => {
                if own_side {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.powers[p as usize].amount <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            // powers/block_modifiers.py:55-58
            PowerId::Blur => {
                if side == Side::Player {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            // powers/turn_effects.py:138-142
            PowerId::NoxiousFumes => {
                if own_side {
                    for e in self.hittable_enemies() {
                        self.apply_power_from(e, PowerId::Poison, amount, Some(owner), None);
                    }
                }
            }
            // powers/turn_effects.py:572-575
            PowerId::Poison => {
                if own_side {
                    self.poison_trigger(owner);
                }
            }
            // powers/remaining_c.py:551-554
            PowerId::ShadowStep => {
                if side == Side::Player {
                    self.creature_apply_power(owner, PowerId::DoubleDamage, amount, None, None, false, false);
                    self.powers[p as usize].amount = 0;
                }
            }
            // powers/turn_effects.py:908-910
            PowerId::WraithForm => {
                if own_side {
                    self.apply_power_from(owner, PowerId::Dexterity, -amount, Some(owner), None);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_after_turn_end(&mut self, p: PRef, owner: CId, side: Side) -> bool {
        let amount = self.pw_amount(p);
        let own_side = side == self.cr(owner).side;
        match self.powers[p as usize].id {
            // OblivionPower.AfterSideTurnEnd expires every applier's mark on PLAYER end.
            PowerId::Oblivion => {
                if side == Side::Player {
                    self.oblivion_cards.retain(|&(power, _, _)| power != p);
                    self.remove_power(owner, p);
                }
            }
            // UnderworldPower.AfterSideTurnEnd removes on ENEMY side, irrespective
            // of which player applied the power.
            PowerId::Underworld => {
                if side == Side::Enemy { self.remove_power(owner, p); }
            }
            // C# TemporaryDexterityPower.AfterSideTurnEnd (Fade/HelicalDart).
            PowerId::Fade | PowerId::HelicalDart => {
                if own_side {
                    self.remove_power(owner, p);
                    self.creature_apply_power(owner, PowerId::Dexterity, -amount, Some(owner), None, false, false);
                }
            }
            // C# CoveredPower/InterceptPower.AfterSideTurnEnd: enemy side only.
            PowerId::Covered | PowerId::Intercept => {
                if side == Side::Enemy {
                    self.remove_power(owner, p);
                }
            }
            // C# PanachePower.AfterSideTurnEnd resets CardsLeft, not alreadyApplied.
            PowerId::Panache => {
                if own_side {
                    self.powers[p as usize].vars[0] = 0;
                }
            }
            // C# TemporaryStrengthPower.AfterSideTurnEnd (also inherited by CoordinatePower).
            id if Self::is_temporary_strength_loss(id) || matches!(id, PowerId::ReptileTrinket | PowerId::Coordinate) => {
                if own_side {
                    let sign = if Self::is_temporary_strength_loss(id) { 1 } else { -1 };
                    self.remove_power(owner, p);
                    self.creature_apply_power(owner, PowerId::Strength, sign * amount, Some(owner), None, false, false);
                }
            }
            // DoomPower.AfterSideTurnEnd: player Doom triggers AFTER side-end effects.
            PowerId::Doom => {
                if side == Side::Player {
                    self.doom_side_end(owner, side);
                }
            }
            // Models/Powers/DebilitatePower.cs AfterSideTurnEnd.
            PowerId::Debilitate => {
                if own_side {
                    self.creature_apply_power(owner, PowerId::Debilitate, -1, None, None, false, false);
                }
            }
            // SicEmPower / BorrowedTimePower.AfterSideTurnEnd remove on owner participation.
            PowerId::SicEm | PowerId::BorrowedTime => {
                if own_side {
                    self.remove_power(owner, p);
                }
            }
            // powers/remaining_a.py:161-164, turn_effects.py:778-781
            PowerId::Anticipate | PowerId::TemporaryDexterity => {
                if own_side {
                    self.creature_apply_power(owner, PowerId::Dexterity, -amount, Some(owner), None, false, false);
                    self.powers[p as usize].amount = 0;
                }
            }
            // turn_effects.py:337-339, remaining_a.py:760-762, remaining_c.py:585-587
            PowerId::Burst => {
                // BurstPower.AfterSideTurnEnd removes the instance, including
                // unspent charges, rather than leaving a zero-amount listener.
                if own_side { self.remove_power(owner,p); }
            }
            PowerId::CorrosiveWave | PowerId::Shadowmeld => {
                if own_side {
                    self.powers[p as usize].amount = 0;
                }
            }
            // powers/card_play_effects.py:972-974
            PowerId::Concoct => {
                if !own_side {
                    self.powers[p as usize].amount = 0;
                }
            }
            // C# FlankingPower/KnockdownPower.AfterSideTurnEnd: remove this instance
            // when the owner's side finishes, not the applier's side.
            PowerId::Flanking | PowerId::Knockdown => {
                if own_side {
                    self.remove_power(owner, p);
                }
            }
            // powers/damage_modifiers.py:117-120
            PowerId::DoubleDamage => {
                if side == Side::Player {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_on_card_drawn(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let amount = self.pw_amount(p);
        match self.powers[p as usize].id {
            // CacophonyPower.AfterCardDrawn counts ALL draws, including teammates.
            // vars[0] is elapsed draws (33 - C# Cards.BaseValue), per instance.
            PowerId::Cacophony => {
                self.powers[p as usize].vars[0] += 1;
                if self.powers[p as usize].vars[0] >= 33 {
                    self.powers[p as usize].vars[0] = 0;
                    let enemies = self.hittable_enemies();
                    // Rng.NextItem returns null without drawing on an empty list.
                    if !enemies.is_empty() {
                        let e = enemies[self.run.rngs.combat_targets.choice_index(enemies.len())];
                        let damage = self.modify_damage(amount, Some(owner), e, Props::UNPOWERED, None);
                        self.apply_damage(e, damage, Props::UNPOWERED, Some(owner));
                    }
                }
            }
            // Models/Powers/PagestormPower.cs AfterCardDrawn includes extra draws.
            PowerId::Pagestorm => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].is_ethereal() {
                    self.draw_cards(owner, amount, false);
                }
            }
            // powers/remaining_a.py:746-758
            PowerId::CorrosiveWave => {
                if self.cards[k as usize].owner != owner {
                    return true;
                }
                for e in self.hittable_enemies() {
                    self.apply_power_from(e, PowerId::Poison, amount, Some(owner), None);
                }
            }
            // powers/remaining_c.py:950-966
            PowerId::Speedster => {
                if self.drawing_from_hand
                    || self.cards[k as usize].owner != owner
                    || self.side != self.cr(owner).side
                {
                    return true;
                }
                for e in self.hittable_enemies() {
                    self.deal_damage(Some(owner), e, amount, Props::UNPOWERED);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_before_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let id = self.powers[p as usize].id;
        match id {
            // OblivionPower.BeforeCardPlayed: snapshot amount only for Applier.Player.
            PowerId::Oblivion => {
                if self.powers[p as usize].applier.is_some_and(|a|
                    self.players.is_player(a) && self.cards[k as usize].owner == a)
                {
                    if self.oblivion_cards.iter().any(|&(power, card, _)| power == p && card == k) {
                        self.unported("Oblivion duplicate in-flight card");
                    } else {
                        self.oblivion_cards.push((p, k, self.pw_amount(p)));
                    }
                }
            }
            // SpiritOfAshPower.BeforeCardPlayed: Ethereal PLAY, not an exhaust hook.
            PowerId::SpiritOfAsh => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].is_ethereal() {
                    let gained = self.power_gain_unpowered_block(owner, self.pw_amount(p));
                    if gained > 0 { self.record_block_gained_event(owner, Props::UNPOWERED, None); }
                }
            }
            // DanseMacabrePower.BeforeCardPlayed / CardEnergyCost.GetResolved:
            // X uses CapturedXValue; ordinary cards use all cost modifiers.
            PowerId::DanseMacabre => {
                if self.cards[k as usize].owner == owner {
                    let cost = if self.cards[k as usize].x_cost { self.cards[k as usize].captured_x }
                        else { self.modified_card_cost(owner, k) };
                    if cost >= 2 {
                        let gained = self.power_gain_unpowered_block(owner, self.pw_amount(p));
                        if gained > 0 { self.record_block_gained_event(owner, Props::UNPOWERED, None); }
                    }
                }
            }
            // Models/Powers/VeilpiercerPower.BeforeCardPlayed consumes a stack only
            // for an owned Ethereal card in Hand/Play (not autoplay from Exhaust).
            PowerId::Veilpiercer => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].is_ethereal()
                    && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k))
                {
                    self.creature_apply_power(owner, id, -1, None, None, false, false);
                }
            }
            // C# ImitationLearningPower.BeforeCardPlayed: clone before OnPlay,
            // only for the first play in a series, and for this watched player.
            PowerId::ImitationLearning => {
                let Some(player) = self.powers[p as usize].target else {
                    self.unported("ImitationLearning power without PlayerTarget");
                    return true;
                };
                if player == self.cards[k as usize].owner
                    && self.cards[k as usize].card_type == CardType::Power
                    && self.pending_play.last().is_some_and(|f|
                        f.card == k && f.remaining == f.play_count)
                {
                    let iid = self.run.new_card_id();
                    let mut clone = self.cards[k as usize].clone_with_id(iid);
                    clone.owner = owner; // C# CardModel.CreateCloneForPlayer
                    let q = self.cards.len() as KRef;
                    self.card_hook_mask |= CARD_HOOKS[clone.id.index()];
                    self.cards.push(clone);
                    self.imitation_clones.push((p, k, q));
                }
            }
            // C# CalamityPower.BeforeCardPlayed records this owner's attacks.
            PowerId::Calamity => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].card_type == CardType::Attack {
                    self.power_card_amount_set(owner, id, k, self.pw_amount(p));
                }
            }
            // powers/card_play_effects.py:231-235, 516-520
            PowerId::Afterimage | PowerId::SerpentForm => {
                if self.cards[k as usize].owner == owner {
                    let amount = self.pw_amount(p);
                    self.power_card_amount_set(owner, id, k, amount);
                }
            }
            // powers/card_play_effects.py:692-702
            PowerId::FreeSkill => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                    && self.pw_amount(p) > 0
                {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.pw_amount(p) <= 0 {
                        self.remove_power(owner, p);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_after_card_played(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        let id = self.powers[p as usize].id;
        match id {
            // OblivionPower.AfterCardPlayed removes the stored snapshot before Apply<Doom>.
            // A fresh mark applied during this card's OnPlay has no snapshot for that play.
            PowerId::Oblivion => {
                if let Some(i) = self.oblivion_cards.iter().position(|&(power, card, _)| power == p && card == k) {
                    let (_, _, amount) = self.oblivion_cards.remove(i);
                    self.apply_power_from(owner, PowerId::Doom, amount, self.powers[p as usize].applier, None);
                }
            }
            // Models/Powers/DevourLifePower.AfterCardPlayed: Soul only, owned by this player.
            // HauntPower.AfterCardPlayed: one CombatTargets draw per owned Soul,
            // anonymous unpowered/unblockable HP loss (not an attack).
            PowerId::Haunt => {
                if self.cards[k as usize].id == CardId::Soul && self.cards[k as usize].owner == owner {
                    let enemies = self.hittable_enemies();
                    if !enemies.is_empty() {
                        let e = enemies[self.run.rngs.combat_targets.choice_index(enemies.len())];
                        let props = Props::UNBLOCKABLE.or(Props::UNPOWERED);
                        let damage = self.modify_damage(self.pw_amount(p), None, e, props, None);
                        self.apply_damage(e, damage, props, None);
                    }
                }
            }
            PowerId::DevourLife => {
                if self.cards[k as usize].id == CardId::Soul && self.cards[k as usize].owner == owner {
                    self.summon_osty(owner, self.pw_amount(p));
                }
            }
            // C# ImitationLearningPower.AfterCardPlayed: remove the pending pair
            // and decrement this instance before recursively autoplaying its clone.
            PowerId::ImitationLearning => {
                if let Some(i) = self.imitation_clones.iter().position(|&(power, card, _)| power == p && card == k) {
                    let (_, _, clone) = self.imitation_clones.remove(i);
                    self.modify_power_instance(p, -1, None, None);
                    self.auto_play_card(clone, None, false);
                }
            }
            // C# CalamityPower.AfterCardPlayed / CardFactory.GetForCombat uses current
            // Amount, replacement sampling and the owner's character/player-count pool.
            PowerId::Calamity => {
                if self.power_card_amount_take(owner, id, k) > 0 {
                    let Some(character) = self.players[owner].character else {
                        self.unported("Calamity owner character not supplied");
                        return true;
                    };
                    // The MP lists are read in GenerateAllCards order from C#
                    // Models/CardPools/{Character}CardPool.cs, then filtered exactly like
                    // CardFactory.FilterForCombat / FilterForPlayerCount. r7 pools are solo-only.
                    let Some(pool) = crate::cardgen::character_pool(character.into(), self.players.len() > 1, Some(CardType::Attack), None) else {
                        self.unported("Calamity character pool");
                        return true;
                    };
                    if pool.is_empty() {
                        self.unported("Calamity empty attack pool");
                        return true;
                    }
                    let mut remaining = self.pw_amount(p).max(0) as usize;
                    let mut generated = Vec::new();
                    while remaining > 0 {
                        let count = remaining.min(10);
                        generated.extend(self.generate_pool_cards(owner, pool, count, false));
                        remaining -= count;
                    }
                    // C# materializes the whole generated list before entering any card.
                    for card in generated {
                        self.add_generated_card(card, PileKind::Hand, PilePos::Bottom, true);
                    }
                }
            }
            // C# PanachePower.AfterCardPlayed: each instance ignores its first owner play.
            PowerId::Panache => {
                if self.cards[k as usize].owner == owner {
                    if self.powers[p as usize].vars[1] != 0 {
                        self.powers[p as usize].vars[0] += 1;
                        if self.powers[p as usize].vars[0] >= 5 {
                            let amount = self.pw_amount(p);
                            self.power_damage_all_enemies(owner, amount);
                            self.powers[p as usize].vars[0] = 0;
                        }
                    }
                    self.powers[p as usize].vars[1] = 1;
                }
            }
            // powers/card_play_effects.py:237-242
            PowerId::Afterimage => {
                let amount = self.power_card_amount_take(owner, id, k);
                if amount > 0 {
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            // powers/card_play_effects.py:522-536
            PowerId::SerpentForm => {
                let amount = self.power_card_amount_take(owner, id, k);
                if amount > 0 {
                    let enemies = self.hittable_enemies();
                    if enemies.is_empty() {
                        return true;
                    }
                    let t = enemies[self.run.rngs.combat_targets.choice_index(enemies.len())];
                    self.deal_damage(Some(owner), t, amount, Props::UNPOWERED);
                }
            }
            // powers/remaining_b.py:985-993
            PowerId::MasterPlanner => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].card_type == CardType::Skill {
                    self.cards[k as usize].keywords |= KW_SLY;
                }
            }
            // powers/card_play_effects.py:765-771
            PowerId::Sneaky => {
                if self.cards[k as usize].owner != owner && self.cards[k as usize].card_type == CardType::Attack {
                    let amount = self.pw_amount(p);
                    self.power_gain_unpowered_block(owner, amount);
                }
            }
            // powers/card_play_effects.py:488-492
            PowerId::PhantomBlades => {
                if self.cards[k as usize].owner == owner && self.cards[k as usize].has_tag(CardTag::Shiv) {
                    self.set_power_private_int(owner, PowerId::PhantomBlades, 0, 1);
                }
            }
            _ => return false,
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn chars_after_power_amount_changed(
        &mut self,
        p: PRef,
        owner: CId,
        target: CId,
        changed: PowerId,
        delta: i32,
        applier: Option<CId>,
        source: Option<KRef>,
    ) -> bool {
        let id = self.powers[p as usize].id;
        match id {
            // Models/Powers/ShroudPower.AfterPowerAmountChanged has no delta/sign
            // or target restriction: every Doom change attributed to this owner.
            PowerId::Shroud => {
                if applier == Some(owner) && changed == PowerId::Doom {
                    let gained = self.power_gain_unpowered_block(owner, self.pw_amount(p));
                    if gained > 0 {
                        self.record_block_gained_event(owner, Props::UNPOWERED, None);
                    }
                }
            }
            // C# TemporaryStrengthPower.AfterPowerAmountChanged skips the fresh application.
            PowerId::Fade | PowerId::HelicalDart => {
                if owner == target && changed == id && delta != self.pw_amount(p) {
                    self.creature_apply_power(owner, PowerId::Dexterity, delta, applier, source, false, false);
                }
            }
            id if Self::is_temporary_strength_loss(id) || matches!(id, PowerId::ReptileTrinket | PowerId::Coordinate) => {
                if owner == target && changed == id && delta != self.pw_amount(p) {
                    let sign = if Self::is_temporary_strength_loss(id) { -1 } else { 1 };
                    self.creature_apply_power(owner, PowerId::Strength, sign * delta, applier, source, false, false);
                }
            }
            // SleightOfFleshPower.AfterPowerAmountChanged uses the type of this
            // OFFSET, not the resulting power amount; ITemporaryPower is excluded.
            PowerId::SleightOfFlesh => {
                if delta != 0 && applier == Some(owner) && self.cr(target).side == Side::Enemy
                    && Self::power_type_for_amount(changed, delta) == PowerType::Debuff
                    && !power_meta(changed).temporary
                {
                    let damage = self.modify_damage(self.pw_amount(p), Some(owner), target, Props::UNPOWERED, None);
                    self.apply_damage(target, damage, Props::UNPOWERED, Some(owner));
                }
            }
            // powers/remaining_a.py:146-158
            PowerId::Anticipate => {
                if owner == target
                    && changed == PowerId::Anticipate
                    && self.find_power(owner, PowerId::Anticipate) == Some(p)
                    // TemporaryDexterityPower.BeforeApplied handled the fresh
                    // application before attachment; only later stacks reach here.
                    && delta != self.pw_amount(p)
                    && !self.consume_ignore_next_instance(p)
                {
                    self.creature_apply_power(owner, PowerId::Dexterity, delta, applier, source, false, false);
                }
            }
            // powers/turn_effects.py:766-776
            PowerId::TemporaryDexterity => {
                if owner == target && changed == id && delta != 0 && !self.consume_ignore_next_instance(p) {
                    self.creature_apply_power(owner, PowerId::Dexterity, delta, applier, source, false, false);
                }
            }
            // C# OrbitPower / MonologuePower.InstanceType: each application has its own
            // counter and internal data; creature_apply_power uses the instanced metadata.
            // CoveredPower.AfterApplied's localized name has no further combat hook.
            PowerId::Orbit | PowerId::Monologue | PowerId::Covered => {}
            // powers/card_play_effects.py:446-465
            PowerId::PhantomBlades => {
                if owner == target && changed == id && delta > 0 {
                    for k in self.pile_cards(owner) {
                        if self.cards[k as usize].has_tag(CardTag::Shiv) {
                            self.cards[k as usize].keywords |= KW_RETAIN;
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_after_damage_given(
        &mut self,
        p: PRef,
        owner: CId,
        target: CId,
        dealer: Option<CId>,
        damage: i32,
        total_damage: i32,
        props: Props,
    ) -> bool {
        match self.powers[p as usize].id {
            // UnderworldPower.AfterDamageGiven excludes the owner AND its pets,
            // but includes other same-side creatures; blocked damage counts.
            PowerId::Underworld => {
                if let Some(d) = dealer
                    && self.cr(d).side == self.cr(owner).side && d != owner
                    && self.cr(d).pet_owner != Some(owner)
                    && props.is_powered_attack() && total_damage > 0 && self.can_receive_power(target)
                {
                    self.creature_apply_power(target, PowerId::Doom, total_damage * self.pw_amount(p),
                        Some(owner), None, false, false);
                }
            }
            // ReaperFormPower.AfterDamageGiven: owner's or pet's powered damage, including
            // block, but excluding overkill (Entities/Creatures/DamageResult.TotalDamage).
            PowerId::ReaperForm => {
                if props.is_powered_attack() && total_damage > 0
                    && self.dealt_by_owner_or_pet(owner, dealer)
                    && self.can_receive_power(target)
                {
                    self.creature_apply_power(target, PowerId::Doom, total_damage * self.pw_amount(p),
                        Some(owner), None, false, false);
                }
            }
            // SicEmPower.AfterDamageGiven: even fully blocked hits count, but only this
            // mark's applier's Osty summons, never a teammate's Osty or the player.
            PowerId::SicEm => {
                if target == owner && let Some(d) = dealer
                    && self.cr(d).monster_id == "OSTY"
                    && let Some(summoner) = self.cr(d).pet_owner
                    && self.powers[p as usize].applier == Some(summoner)
                {
                    self.summon_osty(summoner, self.pw_amount(p));
                }
            }
            // powers/card_play_effects.py:347-357, 961-970
            PowerId::Envenom | PowerId::Concoct => {
                if dealer == Some(owner) && props.is_powered_attack() && damage > 0 {
                    let amount = self.pw_amount(p);
                    // EnvenomPower.cs:24 / ConcoctPower.cs:25: null cardSource.
                    self.apply_power_from(target, PowerId::Poison, amount, Some(owner), None);
                }
            }
            // C# MonarchsGazePower.AfterDamageGiven: every powered attack hit (no damage check).
            PowerId::MonarchsGaze => {
                if dealer == Some(owner) && props.is_powered_attack() {
                    let amount = self.pw_amount(p);
                    self.apply_power_from(target, PowerId::MonarchsGazeStrengthDown, amount, Some(owner), None);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_after_player_turn_start(&mut self, p: PRef, owner: CId) -> bool {
        match self.powers[p as usize].id {
            // SummonNextTurnPower.AfterPlayerTurnStart tests the snapshot but summons
            // the CURRENT amount, then removes the whole instance.
            PowerId::SummonNextTurn => {
                if self.powers[p as usize].vars[0] != 0 {
                    self.summon_osty(owner, self.pw_amount(p));
                    self.remove_power(owner, p);
                }
            }
            // powers/turn_effects.py:302-304
            PowerId::ToolsOfTheTrade => {
                if self.cr(owner).is_player {
                    let amount = self.pw_amount(p);
                    self.request_discard(owner, amount);
                }
            }
            // C# TyrannyPower.AfterPlayerTurnStart / r7 request_exhaust: exhaust exactly
            // min(Amount, hand) chosen hand cards.
            PowerId::Tyranny => {
                if self.cr(owner).is_player
                    && let n = (self.pw_amount(p).max(0) as usize).min(self.players[owner].hand.len()) as u16
                    && (n > 0 || self.collect_setup_choices)
                {
                    let cards: arrayvec::ArrayVec<KRef, 10> = self.players[owner].hand.iter().copied().collect();
                    let r = crate::choice::Resolver {
                        src: crate::choice::ChoiceSrc::Card(CardId::TyrannyCard),
                        k: 0, target: Some(owner), step: 1, arg: 0,
                    };
                    let n = if self.collect_setup_choices { self.pw_amount(p).max(0) as u16 } else { n };
                    self.request_multi_card_choice_for_player(owner, &cards, PileKind::Hand, r, n, Some(n), false);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_before_hand_draw(&mut self, p: PRef, owner: CId, turn_owner: CId) -> bool {
        match self.powers[p as usize].id {
            // SentryModePower.BeforeHandDraw creates each BASE SweepingGaze just
            // before adding it, including when a full hand sends it to discard.
            PowerId::SentryMode => {
                if owner == turn_owner {
                    for _ in 0..self.pw_amount(p) {
                        if let Some(k) = self.new_card(owner, CardId::SweepingGaze, false) {
                            self.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
                        }
                    }
                }
            }
            // powers/turn_effects.py:537-539 -> core/combat.py:4286-4292
            PowerId::InfiniteBlades => {
                if owner == turn_owner && self.cr(owner).is_player {
                    for _ in 0..self.pw_amount(p).max(0) {
                        self.add_shiv_to_hand(owner, false);
                    }
                }
            }
            // powers/remaining_b.py:1170-1179
            PowerId::Nightmare => {
                if owner == turn_owner && self.cr(owner).is_player
                    && self.nightmare_cards.iter().any(|e| e.0 == owner)
                {
                    let selected: Vec<(KRef, i32)> =
                        self.nightmare_cards.iter().filter(|e| e.0 == owner).map(|e| (e.1, e.2)).collect();
                    for (k, amount) in selected {
                        for _ in 0..amount {
                            self.clone_card_to_hand(k);
                        }
                    }
                    self.nightmare_cards.retain(|e| e.0 != owner);
                    self.remove_power(owner, p);
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn chars_damage_additive(
        &self,
        p: PRef,
        owner: CId,
        dealer: Option<CId>,
        _target: CId,
        props: Props,
        source_card: Option<KRef>,
    ) -> Option<i32> {
        let amount = self.pw_amount(p);
        let active_shiv = source_card.is_some_and(|k| self.cards[k as usize].has_tag(CardTag::Shiv));
        Some(match self.powers[p as usize].id {
            // powers/damage_modifiers.py:285-298
            // Models/Powers/CalcifyPower.ModifyDamageAdditive: Osty only, owned by this player.
            PowerId::Calcify => {
                if props.is_powered_attack() && dealer.is_some_and(|d|
                    self.cr(d).monster_id == "OSTY" && self.cr(d).pet_owner == Some(owner))
                { amount } else { 0 }
            }
            PowerId::Accuracy => {
                if dealer == Some(owner) && props.is_powered_attack() && active_shiv { amount } else { 0 }
            }
            // powers/card_play_effects.py:467-486
            PowerId::PhantomBlades => {
                let shiv_done = self.hist.played_this_turn.iter().any(|&(actor, k)| {
                    actor == owner && self.cards[k as usize].has_tag(CardTag::Shiv)
                });
                if dealer == Some(owner)
                    && props.is_powered_attack()
                    && self.power_private_int(owner, PowerId::PhantomBlades, 0) == 0
                    && !shiv_done
                    && active_shiv
                {
                    amount
                } else {
                    0
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn chars_damage_multiplicative(
        &self,
        p: PRef,
        owner: CId,
        dealer: Option<CId>,
        target: Option<CId>,
        props: Props,
        source_card: Option<KRef>,
    ) -> Option<f64> {
        let amount = self.pw_amount(p);
        Some(match self.powers[p as usize].id {
            // GigantificationPower.ModifyDamageMultiplicative ignores target/dealer:
            // the source card's OWNER determines qualification, including Osty attacks.
            PowerId::Gigantification => {
                if props.is_powered_attack() && source_card.is_some_and(|k|
                    self.cards[k as usize].owner == owner
                        && (self.powers[p as usize].vars[0] == 0
                            || self.powers[p as usize].vars[1] == k as i32 + 1))
                { 3.0 } else { 1.0 }
            }
            // LethalityPower.ModifyDamageMultiplicative: first Attack start only, excluding
            // replay iterations; the source card's owner, not the dealer, owns this bonus.
            PowerId::Lethality => {
                let boosted = props.is_powered_attack() && source_card.is_some_and(|k| {
                    if self.cards[k as usize].owner != owner { return false; }
                    let in_play = self.players[owner].play.contains(&k);
                    if in_play && self.pending_play.iter().any(|f|
                        f.card == k && f.play_count - f.remaining > 1)
                    { return false; }
                    self.hist.attack_starts_this_turn[owner as usize] <= i32::from(in_play)
                });
                if boosted { 1.0 + amount as f64 / 100.0 } else { 1.0 }
            }
            // HangPower.ModifyDamageMultiplicative: only Hang card damage to this owner.
            PowerId::Hang => {
                if target == Some(owner) && source_card.is_some_and(|k| self.cards[k as usize].id == CardId::Hang) {
                    amount as f64
                } else {
                    1.0
                }
            }
            // C# FlankingPower/KnockdownPower: exempt this instance's applier.
            PowerId::Flanking | PowerId::Knockdown => {
                if target != Some(owner) || !props.is_powered_attack()
                    || dealer == self.powers[p as usize].applier
                {
                    1.0
                } else {
                    amount as f64
                }
            }
            // powers/remaining_c.py:1662-1671
            PowerId::Tracking => {
                if props.is_powered_attack()
                    && self.dealt_by_owner_or_pet(owner, dealer)
                    && target.is_some_and(|t| self.has_power(t, PowerId::Weak))
                {
                    1.0 + amount as f64 / 100.0
                } else {
                    1.0
                }
            }
            // powers/damage_modifiers.py:105-114
            PowerId::DoubleDamage => {
                if self.dealt_by_owner_or_pet(owner, dealer) && props.is_powered_attack() && source_card.is_some() {
                    2.0
                } else {
                    1.0
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn chars_block_multiplicative(&self, p: PRef, owner: CId, target: CId) -> Option<f64> {
        Some(match self.powers[p as usize].id {
            // powers/remaining_c.py:576-583
            PowerId::Shadowmeld => {
                if target == owner { 2f64.powi(self.pw_amount(p)) } else { 1.0 }
            }
            _ => return None,
        })
    }

    pub(crate) fn chars_modify_hand_draw(&mut self, p: PRef, draw: i32) -> Option<i32> {
        Some(match self.powers[p as usize].id {
            // powers/turn_effects.py:299-300
            PowerId::ToolsOfTheTrade => draw + self.pw_amount(p),
            // C# TyrannyPower.ModifyHandDraw: +Amount.
            // Models/Powers/DemesnePower.ModifyHandDraw shares the owner's additive draw.
            PowerId::Tyranny | PowerId::Demesne => draw + self.pw_amount(p),
            _ => return None,
        })
    }

    pub(crate) fn chars_modify_card_play_count(&mut self, p: PRef, owner: CId, k: KRef, count: i32) -> Option<i32> {
        Some(match self.powers[p as usize].id {
            // powers/turn_effects.py:321-330
            PowerId::Burst => {
                if self.pw_amount(p) > 0
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                {
                    count + 1
                } else {
                    count
                }
            }
            _ => return None,
        })
    }

    pub(crate) fn chars_after_modifying_card_play_count(&mut self, p: PRef, owner: CId, k: KRef) -> bool {
        match self.powers[p as usize].id {
            // BurstPower.AfterModifyingCardPlayCount -> PowerCmd.Decrement.
            PowerId::Burst => {
                if self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                    && self.pw_amount(p) > 0
                {
                    self.set_recorded_power_amount(p,self.powers[p as usize].amount-1);
                    if self.pw_amount(p)<=0 {self.remove_power(owner,p);}
                }
            }
            _ => return false,
        }
        true
    }

    /// `should_clear_block`: `Some(false)` prevents clearing `c`'s block.
    pub(crate) fn chars_should_clear_block(&mut self, p: PRef, owner: CId, c: CId) -> Option<bool> {
        match self.powers[p as usize].id {
            // powers/block_modifiers.py:50-53
            PowerId::Blur => Some(owner != c),
            _ => None,
        }
    }
}
