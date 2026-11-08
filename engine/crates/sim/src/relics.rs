//! r7 combat relic bodies. Per-relic mutable state stays in `Relic`.
use crate::tables::PowerId;
use crate::character::Character;

use arrayvec::ArrayVec;
use smallvec::SmallVec;

use crate::card::{Card, CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, PilePos, Props, MAX_HAND_SIZE, PLAYER};
use crate::creature::Side;
use crate::hooks::Ev;
use crate::tables::*;
mod owner;
pub use owner::OWNER_AWARE_RELICS;

impl Combat {
    pub(crate) fn relics_after_died_to_doom(&mut self, doomed: &[CId]) {
        // BookRepairKnife.AfterDiedToDoom counts all other captured creatures,
        // not just enemies and not only those that actually remained dead.
        // Confirmed C#: BookRepairKnife.cs:24 has no IsDead test; DoomPower.cs:40
        // passes the original captured list after Kill. In contrast, frozen r7
        // relics/uncommon.py:79-97 excludes captured creatures that survived.
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id != RelicId::BookRepairKnife || !self.relic_enabled(r) { continue; }
            let owner = self.run.relics[r].owner;
            let count = doomed.iter().filter(|&&c|
                c != owner && self.should_owner_death_trigger_fatal(c)).count() as i32;
            if count > 0 { self.heal(owner, 3 * count); }
        }
    }

    /// Room-entry adapter for pre-room snapshots (the DLL recorder). Solo r7
    /// snapshots and RunManager already ran their run-layer room callbacks;
    /// they must not call this a second time.
    pub fn relic_base_room_entered(&mut self) {
        // MawBank.AfterRoomEntered: BaseRoom == room and !HasItemBeenBought.
        // This entry point is only for the base room, never a nested room.
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::MawBank
                && !self.run.relics[r].flag1 && self.relic_enabled(r)
            {
                let owner = self.run.relics[r].owner;
                // Pre-room oracle snapshots use the existing gold-gain path,
                // including BowlerHat and DragonFruit. Normal run room entry is
                // owned separately by r1_v4; do not duplicate its payout here.
                self.change_player_gold(owner, 12);
            }
        }
    }

    /// Metronome.AfterOrbChanneled: count only the owner's channels, activate
    /// exactly once at seven. No RNG is consumed by its all-enemy damage.
    pub(crate) fn relics_after_orb_channeled(&mut self, player: CId) {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::Metronome
                && self.run.relics[r].owner == player && self.relic_enabled(r)
            {
                self.run.relics[r].counter += 1;
                if self.run.relics[r].counter == 7 {
                    self.relic_damage_all_enemies_for_player(player, 30);
                }
            }
        }
    }

    // PowerCmd.Apply in Models/Relics passes null cardSource, independently of
    // the active play stack (Shuriken.AfterCardPlayed is one example).
    fn apply_relic_power(&mut self, target: CId, id: PowerId, amount: i32, applier: Option<CId>) -> Option<crate::creature::PRef> {
        self.apply_power_from(target, id, amount, applier, None)
    }
    #[inline]
    fn relic_enabled(&mut self, r: usize) -> bool {
        if !self.players[self.run.relics[r].owner].hooks_active { return false; }
        if self.run.relics[r].owner != PLAYER && !OWNER_AWARE_RELICS.contains(&self.run.relics[r].id) {
            if self.fault.is_none() { self.fault_relic = Some(self.run.relics[r].id); }
            self.unported("relic owner hooks not ported");
            return false;
        }
        self.run.relics[r].enabled
    }

    pub(crate) fn apply_query_fault(&mut self, fault: Option<&'static str>) {
        if let Some(reason) = fault {
            if self.fault.is_none() && reason == "relic owner hooks not ported" {
                self.fault_relic = self.run.relics.iter().find(|relic| {
                    self.players[relic.owner].hooks_active && relic.owner != PLAYER
                        && !OWNER_AWARE_RELICS.contains(&relic.id)
                }).map(|relic| relic.id);
            }
            self.unported(reason);
        }
    }

    pub(crate) fn query_relic_enabled(&self, r: usize, fault: &mut Option<&'static str>) -> bool {
        // CombatState.IsModelValidForHooks: modifiers/lifecycle entry points
        // obey Owner.IsActiveForHooks just like normal fire_relics dispatch.
        if !self.players[self.run.relics[r].owner].hooks_active { return false; }
        if self.run.relics[r].owner != PLAYER && !OWNER_AWARE_RELICS.contains(&self.run.relics[r].id) {
            fault.get_or_insert("relic owner hooks not ported");
            return false;
        }
        self.run.relics[r].enabled
    }

    fn relic_gain_energy_for_owner(&mut self, owner: CId, amount: i32) {
        // combat.gain_energy: refused once the combat is over OR ending (r7 core/combat.py).
        if amount > 0 && self.cr(owner).is_alive() && !self.is_over_or_ending() {
            self.players[owner].energy+= amount;
        }
    }

    fn relic_gain_block_for_owner(&mut self, owner: CId, amount: i32) {
        let _block = self.orb_trace("gain_block", Some(owner));
        // r7 relic helpers call Creature.gain_block directly.  In particular,
        // Daughter of the Wind's AfterCardPlayed can run after a lethal
        // auto-play has already ended/cleared combat state, and that late
        // block remains visible in the terminal observation.
        if amount <= 0 || self.cr(owner).is_dead() {
            return;
        }
        // relics/common.py `_gain_unpowered_block`: Creature.gain_block, no block modifiers
        // (Shadowmeld must not double relic block).
        let before = self.cr(owner).block;
        self.gain_recorded_block(owner,amount);
        if self.cr(owner).block > before {
            // fire: core/hooks.py after_block_gained requires the actual gained amount.
            self.fire(
                Hook::AfterBlockGained,
                Ev::BlockGained {
                    creature: owner,
                    amount: self.cr(owner).block - before,
                },
                None,
            );
        }
    }

    fn relic_damage_all_enemies_for_player(&mut self, owner: CId, amount: i32) {
        let targets = self.hittable_enemies();
        for target in targets {
            if self.cr(owner).is_dead() {
                break;
            }
            let damage = self.calculate_damage(amount, Some(owner), target, Props::UNPOWERED);
            self.apply_damage(target, damage, Props::UNPOWERED, Some(owner));
        }
        self.flush_pending_attack();
    }

    /// `combat.upgrade_card(card)`: the engine version (consumes the reference/template ids like r7).
    fn relic_upgrade_card(&mut self, k: KRef) {
        self.upgrade_card(k);
    }

    fn relic_upgrade_random_deck_cards(&mut self, owner: CId, count: usize) {
        let mut candidates: Vec<KRef> = Vec::new();
        for i in 0..self.relic_owner_deck(owner).len() {
            if self.relic_owner_deck_card_upgradable(owner, i) {
                candidates.push(i as KRef);
            }
        }
        crate::flow::dotnet_sort_cards(&mut candidates, self.relic_owner_deck(owner));
        self.run.rngs.niche.shuffle(&mut candidates);
        for i in candidates.into_iter().take(count) {
            self.relic_owner_upgrade_deck_card(owner, i as usize);
        }
    }

    fn relic_random_enemy(&mut self) -> Option<CId> {
        let enemies = self.hittable_enemies();
        if enemies.is_empty() {
            None
        } else {
            let i = self.run.rngs.combat_targets.choice_index(enemies.len());
            Some(enemies[i])
        }
    }

    fn relic_whispering_earring_resolve_choices(&mut self, owner: CId) {
        // relic fire: relics/shop_event.py:3710-3727
        while !self.is_over {
            let Some(choice) = self.pending_choice.clone() else { break };
            if choice.player != owner {
                self.unported("Whispering Earring choice belongs to another owner");
                return;
            }
            if choice.is_multi() {
                let take = usize::from(choice.max).min(self.choice_options.len());
                for i in 0..take {
                    if self.pending_choice.is_none() || !self.resolve_pending_choice(Some(i)) {
                        return;
                    }
                }
                if self.pending_choice.is_some() && !self.resolve_pending_choice(None) {
                    return;
                }
            } else {
                let selection = (!self.choice_options.is_empty()).then_some(0);
                if !self.resolve_pending_choice(selection) {
                    return;
                }
            }
        }
    }

    fn relic_whispering_earring_autoplay(&mut self, owner: CId) {
        let previous_play_phase = self.in_play_phase;
        self.in_play_phase = true;
        self.relic_whispering_earring_autoplay_inner(owner);
        self.in_play_phase = previous_play_phase;
    }

    fn relic_whispering_earring_autoplay_inner(&mut self, owner: CId) {
        // Models/Relics/WhisperingEarring.cs:38: first owner turn, cap 13,
        // first playable hand card. VakuuCardSelector.cs:12 takes first options.
        let turn = self.players[owner].turn_number;
        for _ in 0..13 {
            if self.is_over_or_ending() || self.pending_choice.is_some()
                || self.players[owner].turn_number != turn {
                return;
            }
            let hand: ArrayVec<KRef, MAX_HAND_SIZE> = self.players[owner].hand.iter().copied().collect();
            let Some(k) = hand.into_iter().find(|&k| self.can_play_card(k)) else {
                return;
            };
            // relic fire: relics/shop_event.py:3696 target_type_for
            let target_type = self.card_target_type(k);
            let target = match target_type {
                TargetType::AnyEnemy => self.hittable_enemies().first().copied(),
                TargetType::AnyAlly => self.random_ally_target(self.cards[k as usize].owner),
                TargetType::Self_ => Some(owner),
                _ => None,
            };
            if matches!(target_type, TargetType::AnyEnemy | TargetType::AnyAlly) && target.is_none() {
                return;
            }
            self.remove_card_from_piles(k);
            self.execute_card_play_mode(k, target, true, false, true);
            self.relic_whispering_earring_resolve_choices(owner);
            self.check_combat_end();
            if self.is_over {
                return;
            }
        }
    }

    #[inline]
    fn relic_choice_resolver(id: RelicId, owner: CId) -> Resolver {
        Resolver { src: ChoiceSrc::Relic(id), k: owner as KRef, target: None, step: 0, arg: 0 }
    }

    fn relic_generate_colorless_cards(&mut self, owner: CId, count: usize) -> SmallVec<[KRef; 10]> {
        let Some(pool) = crate::cardgen::character_pool(crate::character::CardPool::Colorless, false, None, None) else {
            self.unported("Toolbox colorless card pool");
            return SmallVec::new();
        };
        self.generate_pool_cards(owner, pool, count, true)
    }

    /// Fire-and-forget relic hooks moved out of `hooks.rs`.
    pub(crate) fn relic_event(&mut self, r: usize, hook: Hook, ev: Ev) {
        let id = self.run.relics[r].id;
        let owner = self.run.relics[r].owner;
        if id != RelicId::PaelsEye && ev.is_player_side()
            && !self.is_player_taking_turn(owner)
            && matches!(hook, Hook::BeforeSideTurnStart | Hook::AfterSideTurnStart |
                Hook::BeforeTurnEndVeryEarly | Hook::BeforeTurnEndEarly | Hook::BeforeTurnEnd |
                Hook::AfterTurnEnd | Hook::AfterTurnEndLate)
        { return; }
        let turn_start = matches!(hook, Hook::AfterPlayerTurnStartEarly |
            Hook::AfterPlayerTurnStart | Hook::AfterPlayerTurnStartLate);
        // C# callbacks receive the starting player separately from the listener's
        // owner. The concrete relics below all guard player == Owner.
        if (turn_start || matches!(hook, Hook::BeforeHandDraw | Hook::AfterEnergyReset | Hook::AfterEnergyResetLate))
            && matches!(ev, Ev::Creature(player) if player != owner) { return; }
        if owner != PLAYER && !OWNER_AWARE_RELICS.contains(&id) {
            if self.fault.is_none() {
                self.fault_relic = Some(id);
            }
            self.unported("relic owner hooks not ported");
            return;
        }
        match (id, hook) {
            (RelicId::BlessedAntler, Hook::BeforeHandDraw)
                if self.players[owner].turn_number == 1 => {
                // BlessedAntler.cs:45-55: create the whole batch before adding
                // each generated card to Owner's Draw at a random position.
                // Frozen r7 relics/shop_event.py:1106 BlessedAntler instead uses
                // round_number == 1; C# does not repeat on a round-one extra turn.
                let mut cards = Vec::new();
                for _ in 0..3 {
                    if let Some(k) = self.new_card(owner, CardId::Dazed, false) { cards.push(k); }
                }
                for k in cards {
                    self.add_generated_card_by(k, PileKind::Draw, PilePos::Random, Some(owner));
                }
            }
            (RelicId::EmotionChip, Hook::AfterPlayerTurnStart) => {
                // EmotionChip.cs LostHpInPreviousTurn / AfterPlayerTurnStart;
                // CombatHistoryEntry.HappenedLastPlayerTurn ignores side/round.
                let previous = self.players[owner].turn_number - 1;
                if self.hist.damage_received_turns.iter().any(|&(receiver, turn, blocked)|
                    receiver == owner && turn == previous && !blocked) {
                    // EmotionChip.cs:54 calls OrbCmd.Passive(countAffectedByHooks:
                    // true), unlike r7 orb.on_passive. GoldPlatedCables therefore
                    // repeats the front orb. Seven-corpus gate checks this path;
                    // no new first differences were observed in ebf5494.
                    let count = self.players[owner].orbs.orbs.len();
                    for i in 0..count { self.orb_passive_at(owner, i); }
                }
            }
            // EmotionChip's other hooks update visual status only.
            (RelicId::EmotionChip, Hook::AfterDamageReceived | Hook::AfterCombatEnd) => {}
            (RelicId::BoundPhylactery, Hook::BeforeCombatStart) => {
                let _source = crate::between::relic_source(owner, "BOUND_PHYLACTERY");
                // Models/Relics/BoundPhylactery.cs BeforeCombatStart -> SummonPet.
                self.summon_osty(owner, 1);
            }
            (RelicId::BoundPhylactery, Hook::AfterEnergyResetLate)
                if self.players[owner].turn_number != 1 => {
                let _source = crate::between::relic_source(owner, "BOUND_PHYLACTERY");
                // AfterEnergyResetLate guards player == Owner (dispatch above).
                self.summon_osty(owner, 1);
            }
            (RelicId::CrackedCore, Hook::BeforeSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/CrackedCore.cs BeforeSideTurnStart.
                self.channel_orb(owner, crate::orbs::OrbKind::Lightning);
            }
            (RelicId::FuneraryMask, Hook::BeforeHandDraw)
                if self.players[owner].turn_number == 1 => {
                // Models/Relics/FuneraryMask.cs BeforeHandDraw: three Soul cards
                // at random Draw positions BEFORE the first owner's hand draw.
                for _ in 0..3 {
                    self.create_card_in_pile_by_player(owner, CardId::Soul, false, PileKind::Draw, PilePos::Random);
                }
            }
            (RelicId::GalacticDust, Hook::OnStarsSpent) => {
                // Models/Relics/GalacticDust.cs AfterStarsSpent: cumulative
                // remainder survives turns/combats; one spend can cross many tens.
                if let Ev::Stars { owner: spender, amount } = ev && spender == owner {
                    self.run.relics[r].counter += amount;
                    let blocks = self.run.relics[r].counter / 10;
                    if blocks > 0 {
                        self.relic_gain_block_for_owner(owner, blocks * 10);
                        self.run.relics[r].counter %= 10;
                    }
                }
            }
            (RelicId::MiniRegent, Hook::OnStarsSpent) => {
                // Models/Relics/MiniRegent.cs AfterStarsSpent: first Owner spend.
                if let Ev::Stars { owner: spender, .. } = ev
                    && spender == owner && !self.run.relics[r].flag1 {
                    self.run.relics[r].flag1 = true;
                    self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
                }
            }
            (RelicId::MiniRegent, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].flag1 = false;
            }
            (RelicId::MiniRegent, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::IvoryTile, Hook::AfterCardPlayed) => {
                // Models/Relics/IvoryTile.cs AfterCardPlayed reads CardPlay's
                // immutable Resources.EnergyValue, not modified X effect value.
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner
                    && self.pending_play.iter().rev().find(|p| p.card == k)
                        .is_some_and(|p| p.energy_value >= 3) {
                    self.relic_gain_energy_for_owner(owner, 1);
                }
            }
            (RelicId::BoneFlute, Hook::AfterAttack) => {
                // Models/Relics/BoneFlute.cs AfterAttack: Owner's Osty only.
                if let Ev::Attack { attacker: Some(attacker), .. } = ev
                    && self.cr(attacker).monster_id == "OSTY"
                    && self.cr(attacker).pet_owner == Some(owner) {
                    self.relic_gain_block_for_owner(owner, 2);
                }
            }
            (RelicId::BurningBlood, Hook::AfterCombatVictory) => {
                // BurningBlood.AfterCombatVictory heals Owner.Creature, never
                // LocalContext's player; CreatureCmd.Heal applies the HP cap.
                if self.cr(owner).is_alive() {
                    self.heal(owner, 6);
                }
            }
            (RelicId::BlackBlood, Hook::AfterCombatVictory) => {
                // Models/Relics/BlackBlood.cs AfterCombatVictory: living Owner.
                if self.cr(owner).is_alive() {
                    self.heal(owner, 12);
                }
            }
            (RelicId::FishingRod, Hook::AfterCombatEnd) => {
                if self.run.room_type != RoomType::Monster {
                    return;
                }
                self.run.relics[r].counter += 1;
                if self.run.relics[r].counter % 3 == 0 {
                    // FishingRod.cs AfterCombatEnd upgrades one Owner Deck card.
                    let mut options = Vec::new();
                    for i in 0..self.relic_owner_deck(owner).len() {
                        if self.relic_owner_deck_card_upgradable(owner, i) { options.push(i); }
                    }
                    if !options.is_empty() {
                        let i = options[self.run.rngs.niche.choice_index(options.len())];
                        self.relic_owner_upgrade_deck_card(owner, i);
                    }
                }
            }
            // Anchor.BeforeCombatStart: 10 Unpowered block to Owner.Creature.
            (RelicId::Anchor, Hook::BeforeCombatStart) => self.relic_gain_block_for_owner(owner, 10),
            (RelicId::FakeAnchor, Hook::BeforeCombatStart) => self.relic_gain_block_for_owner(owner, 4),
            (RelicId::BloodVial, Hook::AfterPlayerTurnStartLate) if self.round == 1 => {
                self.heal(owner, 2);
            }
            // FakeBloodVial.cs:19-23 guards player == Owner (dispatch) and
            // Owner.PlayerCombatState.TurnNumber <= 1, not shared round.
            // Frozen r7 relics/shop_event.py:1646 FakeBloodVial instead checks
            // round_number == 1, so same-round extra turns deliberately differ.
            (RelicId::FakeBloodVial, Hook::AfterPlayerTurnStartLate) if self.players[owner].turn_number <= 1 => {
                self.heal(owner, 1);
            }
            (RelicId::BronzeScales, Hook::BeforeCombatStart) => {
                // BronzeScales.AfterRoomEntered(CombatRoom): Owner.Creature Thorns 3.
                self.apply_relic_power(owner, PowerId::Thorns, 3, Some(owner));
            }
            (RelicId::CentennialPuzzle, Hook::AfterDamageReceived) => {
                // CentennialPuzzle.cs AfterDamageReceived: in-progress Owner,
                // positive UnblockedDamage, three separate Draw commands.
                if let Ev::Damage { target, amount, .. } = ev
                    && !self.is_over && target == owner
                    && amount > 0
                    && !self.run.relics[r].flag1
                {
                    self.run.relics[r].flag1 = true;
                    // CentennialPuzzle.cs:40-48 awaits three distinct Draws,
                    // not a single authorized batch of three cards.
                    self.draw_centennial_batches(owner, 3);
                }
            }
            (RelicId::CentennialPuzzle, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::Permafrost, Hook::BeforeCombatStart) => self.run.relics[r].flag1 = false,
            (RelicId::Permafrost, Hook::AfterCardPlayed) => {
                // Permafrost.AfterCardPlayed: Owner's first Power, in progress;
                // AfterRoomEntered resets activation (our combat-entry reset above).
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && !self.run.relics[r].flag1
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Power
                {
                    self.run.relics[r].flag1 = true;
                    self.relic_gain_block_for_owner(owner, 7);
                }
            }
            // FestivePopper.cs AfterPlayerTurnStart: starting player == Owner
            // (dispatch guard), Owner.PlayerCombatState.TurnNumber == 1.
            (RelicId::FestivePopper, Hook::AfterPlayerTurnStart) if self.players[owner].turn_number == 1 => {
                self.relic_damage_all_enemies_for_player(owner, 9);
            }
            (RelicId::MercuryHourglass, Hook::AfterPlayerTurnStart) => {
                // MercuryHourglass.AfterPlayerTurnStart: player == Owner;
                // relic_event's turn-start guard supplies that exact filter.
                self.relic_damage_all_enemies_for_player(owner, 3);
            }
            (RelicId::MrStruggles, Hook::AfterPlayerTurnStart) => {
                // MrStruggles.AfterPlayerTurnStart: player == Owner (dispatch
                // guard), Owner.PlayerCombatState.TurnNumber, not global round.
                // r7 sts2_env/relics/shop_event.py:2361-2374 instead uses
                // combat.round_number; extra turns deliberately follow C#.
                self.relic_damage_all_enemies_for_player(owner, self.players[owner].turn_number);
            }
            // Models/Relics/RoyalPoison.cs:18: only Owner's first turn,
            // not the shared round (extra turns must not repeat the damage).
            (RelicId::RoyalPoison, Hook::AfterPlayerTurnStart) if self.players[owner].turn_number <= 1 => {
                let damage = self.calculate_damage(4, None, owner, Props::UNBLOCKABLE.or(Props::UNPOWERED));
                self.apply_damage(owner, damage, Props::UNBLOCKABLE.or(Props::UNPOWERED), None);
            }
            (RelicId::Lantern, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                self.relic_gain_energy_for_owner(owner, 1);
            }
            // Models/Relics/VeryHotCocoa.cs:21: Owner's first participating turn.
            (RelicId::VeryHotCocoa, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                self.relic_gain_energy_for_owner(owner, 4);
            }
            (RelicId::Pantograph, Hook::BeforeCombatStart)
                if self.run.room_type == RoomType::Boss && self.cr(owner).is_alive() => {
                // Models/Relics/Pantograph.cs:27: living Owner only in boss room.
                self.heal(owner, 25);
            }
            (RelicId::RedSkull, Hook::BeforeCombatStart) => {
                self.run.relics[r].flag1 = false;
                self.relic_red_skull_check(r, owner);
            }
            (RelicId::RedSkull, Hook::AfterCurrentHpChanged) => {
                // RedSkull.cs checks its Owner after ANY HP change in progress;
                // the event's creature is intentionally not used as a filter.
                if !self.is_over {
                    self.relic_red_skull_check(r, owner);
                }
            }
            (RelicId::RedSkull, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::Vajra, Hook::BeforeCombatStart) => {
                // Models/Relics/Vajra.cs AfterRoomEntered: Owner.Creature.
                self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
            }
            (RelicId::FakeSneckoEye, Hook::BeforeCombatStart) => {
                // Models/Relics/FakeSneckoEye.cs ApplyPower: Owner is target/applier.
                self.apply_relic_power(owner, PowerId::Confused, 1, Some(owner));
            }
            (RelicId::LavaLamp, Hook::BeforeCombatStart) => self.run.relics[r].flag1 = false,
            (RelicId::SlingOfCourage, Hook::BeforeCombatStart) if self.run.room_type == RoomType::Elite => {
                // Models/Relics/SlingOfCourage.cs:21 AfterRoomEntered: Owner.Creature.
                self.apply_relic_power(owner, PowerId::Strength, 2, Some(owner));
            }
            (RelicId::VenerableTeaSet, Hook::AfterEnergyReset) if self.run.relics[r].flag1 => {
                self.relic_gain_energy_for_owner(owner, 2);
                self.run.relics[r].flag1 = false;
            }
            (RelicId::FakeVenerableTeaSet, Hook::AfterEnergyReset) if self.run.relics[r].flag1 => {
                // FakeVenerableTeaSet.cs AfterEnergyReset: Owner only (dispatch
                // guard), consume the saved RestSite flag after gaining one.
                self.relic_gain_energy_for_owner(owner, 1);
                self.run.relics[r].flag1 = false;
            }
            (RelicId::HappyFlower, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // HappyFlower.AfterSideTurnStart advances saved TurnsSeen modulo 3.
                self.run.relics[r].counter = (self.run.relics[r].counter + 1) % 3;
                if self.run.relics[r].counter == 0 {
                    self.relic_gain_energy_for_owner(owner, 1);
                }
            }
            (RelicId::FakeHappyFlower, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = (self.run.relics[r].counter + 1) % 5;
                if self.run.relics[r].counter == 0 {
                    self.relic_gain_energy_for_owner(owner, 1);
                }
            }
            (RelicId::BoneTea, Hook::AfterPlayerTurnStart)
                if self.players[owner].turn_number <= 1 && self.run.relics[r].counter < 1 =>
            {
                // BoneTea.AfterPlayerTurnStart. Counter stores 1 - CombatsLeft
                // for compatibility with existing saved r7 used-count snapshots.
                self.run.relics[r].counter += 1;
                let hand = self.players[owner].hand.clone();
                for k in hand {
                    self.relic_upgrade_card(k);
                }
            }
            (RelicId::EmberTea, Hook::BeforeCombatStart) if self.run.relics[r].counter < 5 => {
                // Models/Relics/EmberTea.cs:57-64: remaining combats decrease
                // once per combat; counter stores the complementary used count.
                self.run.relics[r].counter += 1;
                self.apply_relic_power(owner, PowerId::Strength, 2, None);
            }
            (RelicId::DivineRight, Hook::BeforeCombatStart) => {
                // Models/Relics/DivineRight.cs:16-20, AfterRoomEntered(CombatRoom).
                self.gain_stars(owner, 3);
            }
            (RelicId::Girya, Hook::BeforeCombatStart) if self.run.relics[r].counter > 0 => {
                // Girya.AfterRoomEntered(CombatRoom): Owner gains saved TimesLifted.
                self.apply_relic_power(owner, PowerId::Strength, self.run.relics[r].counter, Some(owner));
            }
            (RelicId::BeltBuckle, Hook::BeforeCombatStart) => {
                // BeltBuckle.cs BeforeCombatStart: Owner.Potions, not seat zero.
                self.run.relics[r].flag1 = false;
                self.relic_belt_buckle_inventory(r, owner, false);
            }
            (RelicId::BeltBuckle, Hook::AfterCombatVictory) => {
                self.run.relics[r].flag1 = false;
            }
            (RelicId::GhostSeed, Hook::BeforeCombatStart) => {
                // Models/Relics/GhostSeed.cs AfterRoomEntered: Owner's AllCards.
                let cards = self.pile_cards(owner);
                for k in cards {
                    self.relic_ghost_seed(owner, k);
                }
            }
            (RelicId::GhostSeed, Hook::AfterCardEnteredCombat) => {
                if let Ev::Card(k) = ev {
                    self.relic_ghost_seed(owner, k);
                }
            }
            (RelicId::IronClub, Hook::AfterCardPlayed) => {
                // IronClub.CardsPlayed is saved across combats, including a
                // terminal card; the draw alone requires IsInProgress.
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner {
                    self.run.relics[r].counter += 1;
                    if !self.is_over && self.run.relics[r].counter % 4 == 0 {
                        // IronClub.cs:82-94 ends with awaited Draw; the hook
                        // cursor and PendingPlay keep later listeners/result parked.
                        self.draw_cards_for_branch(owner, 1);
                    }
                }
            }
            (RelicId::HelicalDart, Hook::AfterCardPlayed) => {
                // HelicalDart.AfterCardPlayed checks the Shiv TAG, not the ID.
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].has_tag(CardTag::Shiv)
                {
                    self.apply_relic_power(owner, PowerId::HelicalDart, 1, Some(owner));
                }
            }
            (RelicId::LunarPastry, Hook::AfterTurnEnd) if ev.is_player_side() => {
                // LunarPastry.AfterSideTurnEnd: participants contains Owner;
                // relic_event's participation guard also excludes extra turns.
                self.gain_stars(owner, 1);
            }
            (RelicId::Metronome, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::Nunchaku, Hook::AfterCardPlayed) => {
                // Nunchaku.AfterCardPlayed counts even the terminal attack,
                // but grants energy only while CombatManager.IsInProgress.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter += 1;
                    if !self.is_over && self.run.relics[r].counter % 10 == 0 {
                        self.relic_gain_energy_for_owner(owner, 1);
                    }
                }
            }
            (RelicId::TuningFork, Hook::AfterCardPlayed) => {
                // TuningFork.cs AfterCardPlayed: persistent SkillsPlayed, including
                // terminal plays; subtract the threshold only after gaining block.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter >= 10 {
                        self.relic_gain_block_for_owner(owner, 7);
                        self.run.relics[r].counter -= 10;
                    }
                }
            }
            (RelicId::Pendulum, Hook::BeforeHandDraw) => {
                self.run.relics[r].counter = (self.run.relics[r].counter + 1) % 3;
            }
            (RelicId::NinjaScroll, Hook::BeforeHandDraw)
                if self.players[owner].turn_number <= 1 => {
                // Models/Relics/NinjaScroll.cs:23 -> Cards/Shiv.cs:88:
                // create the whole batch first; creator defaults to Owner.
                let cards: Vec<_> = (0..3).filter_map(|_| self.new_card(owner, CardId::Shiv, false)).collect();
                for k in cards { self.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true); }
            }
            (RelicId::PowerCell, Hook::BeforeSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/PowerCell.cs:21: local (not global) cost, exclude X.
                // Extensions/ListExtensions.cs:10 sorts before Fisher-Yates.
                let mut cards: Vec<_> = self.players[owner].draw.iter().copied().filter(|&k| {
                    let card = &self.cards[k as usize];
                    !card.x_cost && card.local_energy_cost() == 0
                }).collect();
                crate::flow::dotnet_sort_cards(&mut cards, &self.cards);
                self.run.rngs.combat_card_selection.shuffle(&mut cards);
                for k in cards.into_iter().take(2) { crate::content::move_to_hand(self, k); }
            }
            (RelicId::Regalite, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                // Models/Relics/Regalite.cs:47: participating Owner's turn.
                self.run.relics[r].flag1 = false;
            }
            (RelicId::Regalite, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::Regalite, Hook::AfterCardGeneratedForCombat) => {
                // Models/Relics/Regalite.cs:37: creator == Owner, NOT card.Owner.
                if let Ev::Generated { creator: Some(creator), .. } = ev
                    && creator == owner && !self.run.relics[r].flag1 {
                    self.run.relics[r].flag1 = true;
                    self.relic_gain_block_for_owner(owner, 4);
                }
            }
            (RelicId::PollinousCore, Hook::BeforeHandDraw) => {
                // Models/Relics/PollinousCore.cs:88: player == Owner (dispatch guard).
                self.run.relics[r].counter += 1;
            }
            (RelicId::BrilliantScarf, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::BrilliantScarf, Hook::AfterCardPlayed) => {
                // Models/Relics/BrilliantScarf.cs AfterCardPlayed: manual Owner plays.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && !self.pending_play.last().is_some_and(|p| p.card == k && p.is_auto)
                {
                    self.run.relics[r].counter += 1;
                }
            }
            (RelicId::BrilliantScarf, Hook::AfterCombatEnd) => self.run.relics[r].counter = 0,
            (RelicId::MusicBox, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].flag1 = false;
                self.run.relics[r].card = None;
            }
            (RelicId::MusicBox, Hook::BeforeCardPlayed) => {
                // Models/Relics/MusicBox.cs BeforeCardPlayed: first Owner attack.
                if let Ev::Card(k) = ev
                    && !self.run.relics[r].flag1
                    && self.run.relics[r].card.is_none()
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].card = Some(k);
                }
            }
            (RelicId::MusicBox, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev && self.run.relics[r].card == Some(k) {
                    self.run.relics[r].flag1 = true;
                    self.run.relics[r].card = None;
                    let iid = self.run.new_card_id();
                    let mut clone = self.cards[k as usize].clone_with_id(iid);
                    clone.owner = owner;
                    // C# MusicBox: CardCmd.ApplyKeyword(Ethereal) is an intrinsic keyword; it
                    // survives Hex removal even when the source's Ethereal came from Hexed.
                    clone.keywords |= KW_ETHEREAL;
                    clone.cv.affliction_added_ethereal = false;
                    let q = self.cards.len() as KRef;
                    self.cards.push(clone);
                    if self.players[owner].hand.len() < MAX_HAND_SIZE {
                        self.players[owner].hand.push(q);
                    } else {
                        self.players[owner].discard.push(q);
                    }
                    self.fire_after_card_changed_piles(q, PileKind::None, None);
                }
            }
            (RelicId::MusicBox, Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
                self.run.relics[r].card = None;
            }
            (RelicId::PenNib, Hook::BeforeCardPlayed) => {
                // PenNib.cs BeforeCardPlayed / NotifyAttackPlayed: saved modulo
                // counter and the exact AttackToDouble instance.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter = (self.run.relics[r].counter + 1) % 10;
                    if self.run.relics[r].counter == 0 { self.run.relics[r].card = Some(k); }
                }
            }
            (RelicId::PenNib, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev && self.run.relics[r].card == Some(k) {
                    self.run.relics[r].card = None;
                }
            }
            (RelicId::PaelsLegion, Hook::BeforeCombatStart) => {
                // Models/Relics/PaelsLegion.cs:129/221: add Owner's pet.
                self.summon_event_pet(owner, RelicId::PaelsLegion);
            }
            (RelicId::PaelsLegion, Hook::AfterCardPlayed) => {
                // Models/Relics/PaelsLegion.cs:173: only the affected CardPlay.
                if let Ev::Card(k) = ev && self.run.relics[r].card == Some(k) {
                    self.run.relics[r].card = None;
                    self.run.relics[r].counter = 2;
                }
            }
            (RelicId::PaelsLegion, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // Models/Relics/PaelsLegion.cs:187: participating Owner's cooldown.
                self.run.relics[r].counter -= 1;
            }
            (RelicId::PaelsLegion, Hook::AfterCombatEnd) => {
                self.run.relics[r].counter = 0;
                self.run.relics[r].card = None;
            }
            (RelicId::JossPaper, Hook::AfterCardExhausted) => {
                // The attribution-sensitive body runs in fire_relic_after_card_exhausted.
            }
            (RelicId::JossPaper, Hook::AfterTurnEnd) if ev.is_player_side() => {
                // JossPaper.AfterSideTurnEnd buffers ethereal exhausts until
                // Owner participates, then DrawIfThresholdMet uses that Owner.
                let buffered = self.run.relics[r].counter2;
                if buffered > 0 {
                    self.run.relics[r].counter += buffered;
                    self.run.relics[r].counter2 = 0;
                    let draws = self.run.relics[r].counter / 5;
                    if draws > 0 {
                        self.draw_cards(owner, draws, false);
                    }
                    self.run.relics[r].counter %= 5;
                }
            }
            (RelicId::JossPaper, Hook::AfterCombatEnd) => self.run.relics[r].counter2 = 0,
            (RelicId::PetrifiedToad, Hook::BeforeCombatStartLate) => {
                // PetrifiedToad.BeforeCombatStartLate -> PotionCmd.TryToProcure:
                // owner inventory, deny Sozu, notify only after successful insertion.
                if self.relic_owner_has_enabled(owner, RelicId::Sozu) { return; }
                let success = if owner == PLAYER {
                    self.add_potion(PotionId::Potionshapedrock)
                } else if let Some(slots) = self.relic_owner_potions(owner) {
                    if let Some(slot) = slots.iter_mut().find(|p| p.is_none()) {
                        *slot = Some(PotionId::Potionshapedrock);
                        true
                    } else { false }
                } else { false };
                if success {
                    self.fire_relic_after_potion_procured(owner);
                }
            }
            (RelicId::PhilosophersStone, Hook::AfterCreatureAddedToCombat) => {
                // Models/Relics/PhilosophersStone.cs:41 AfterCreatureAddedToCombat: opposing side only.
                if let Ev::Creature(c) = ev && self.cr(c).side != self.cr(owner).side {
                    self.apply_relic_power(c, PowerId::Strength, 1, None);
                }
            }
            (RelicId::PumpkinCandle, Hook::AfterCombatEnd) => {
                // Models/Relics/PumpkinCandle.cs:71: saved owner KindleCount,
                // clamped at zero; the setter disables the exhausted candle.
                self.run.relics[r].counter = (self.run.relics[r].counter - 1).max(0);
                if self.run.relics[r].counter <= 0 {
                    self.run.relics[r].enabled = false;
                }
            }
            (RelicId::TeaOfDiscourtesy, Hook::BeforeCombatStart) if self.run.relics[r].counter < 1 => {
                self.run.relics[r].counter += 1;
                // Models/Relics/TeaOfDiscourtesy.cs:56 BeforeCombatStart: Owner's Draw pile.
                for _ in 0..2 {
                    let _ = self.create_card_in_pile_by_player(owner, CardId::Dazed, false, PileKind::Draw, PilePos::Random);
                }
                self.run.relics[r].enabled = false;
            }
            (RelicId::WongosMysteryTicket, Hook::AfterCombatEnd) => {
                // Models/Relics/WongosMysteryTicket.cs:72: counter increments;
                // GaveRelic disables the model in the separate reward layer.
                self.run.relics[r].counter += 1;
            }
            (RelicId::ToyBox, Hook::AfterCombatEnd) => {
                // Models/Relics/ToyBox.cs:99: Owner's first unmelted wax relic,
                // every three combats until 15; never melt a teammate's relic.
                if self.run.relics[r].counter >= 15 { return; }
                self.run.relics[r].counter += 1;
                if self.run.relics[r].counter % 3 == 0 && self.run.relics[r].counter <= 15 {
                    // relic fire: relics/shop_event.py:3588-3593
                    if let Some(wax) = (0..self.run.relics.len()).find(|&i| {
                        self.run.relics[i].owner == owner
                            && self.run.relics[i].is_wax && !self.run.relics[i].is_melted
                    }) {
                        self.run.relics[wax].is_melted = true;
                        self.run.relics[wax].enabled = false;
                    }
                }
                if self.run.relics[r].counter >= 15 {
                    self.run.relics[r].enabled = false;
                }
            }
            (RelicId::PureWater, Hook::BeforeCombatStart) => {
                let Some(id) = CardId::from_name("Miracle") else {
                    self.unported("PureWater Miracle");
                    return;
                };
                let Some(mut card) = Card::from_factory(id, false, self.run.new_card_id()) else {
                    self.unported("PureWater Miracle");
                    return;
                };
                card.owner = PLAYER;
                let k = self.cards.len() as KRef;
                self.cards.push(card);
                if self.players[PLAYER].hand.len() < MAX_HAND_SIZE {
                    self.players[PLAYER].hand.push(k);
                } else {
                    self.players[PLAYER].discard.push(k);
                }
                self.fire_after_card_changed_piles(k, PileKind::None, None);
            }
            (RelicId::HandDrill, Hook::AfterDamageGiven) => {
                // HandDrill.AfterBlockBroken: Owner or Owner's pet broke
                // any non-player's block. There is no attack/card requirement.
                if let Ev::Damage { target, dealer, was_block_broken: true, .. } = ev
                    && dealer.is_some_and(|d| d == owner || self.cr(d).pet_owner == Some(owner))
                    && !self.cr(target).is_player
                {
                    self.apply_relic_power(target, PowerId::Vulnerable, 2, Some(owner));
                }
            }
            (RelicId::PaelsEye, Hook::BeforeCardPlayed) => {
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner
                    && !self.pending_play.last().is_some_and(|p| p.is_auto) {
                    self.run.relics[r].flag2 = true;
                }
            }
            (RelicId::PaelsEye, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].flag2 = false;
                // WasOwnerPartOfLastPlayerTurn; counter3 stores its negation.
                self.run.relics[r].counter3 = i32::from(!self.is_player_taking_turn(owner));
            }
            (RelicId::PaelsEye, Hook::BeforeTurnEndEarly)
                if ev.is_player_side() && self.is_player_taking_turn(owner)
                    && !self.run.relics[r].flag1 && !self.run.relics[r].flag2 =>
            {
                // relic fire: relics/shop_event.py:2508-2518
                let cards: ArrayVec<KRef, MAX_HAND_SIZE> = self.players[owner].hand.iter().copied().collect();
                for k in cards {
                    if let Some(i) = self.players[owner].hand.iter().position(|&q| q == k) {
                        self.players[owner].hand.remove(i);
                        self.players[owner].exhaust.push(k);
                        self.fire_after_card_changed_piles(k, PileKind::Hand, None);
                        self.fire_after_card_exhausted(k);
                    }
                }
            }
            (RelicId::PaelsEye, Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
                self.run.relics[r].flag2 = false;
                self.run.relics[r].counter3 = 0;
            }
            (RelicId::ChemicalX, Hook::BeforeCardPlayed) => {}
            (RelicId::ThrowingAxe, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
            }
            (RelicId::ChoicesParadox, Hook::BeforeCombatStart | Hook::AfterCombatEnd)
            | (RelicId::Toolbox, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
            }
            (RelicId::ChoicesParadox, Hook::AfterPlayerTurnStart) if self.round == 1 && !self.run.relics[r].flag1 => {
                let generated = self.generate_character_cards(owner,None, None, 5, true);
                if !generated.is_empty() {
                    for &g in &generated {
                        self.cards[g as usize].keywords |= KW_RETAIN;
                    }
                    self.run.relics[r].flag1 = true;
                    let resolver = Self::relic_choice_resolver(RelicId::ChoicesParadox, owner);
                    self.request_card_choice_for_player(owner, &generated, PileKind::None, resolver, false);
                }
            }
            (RelicId::Toolbox, Hook::BeforeHandDraw) if self.round == 1 && !self.run.relics[r].flag1 => {
                let generated = self.relic_generate_colorless_cards(owner, 3);
                if !generated.is_empty() {
                    self.run.relics[r].flag1 = true;
                    let resolver = Self::relic_choice_resolver(RelicId::Toolbox, owner);
                    self.request_card_choice_for_player(owner, &generated, PileKind::None, resolver, false);
                }
            }
            (RelicId::GamblingChip, Hook::AfterPlayerTurnStart)
                if self.round == 1 && self.pending_choice.is_none() && !self.players[owner].hand.is_empty() =>
            {
                let cards: ArrayVec<KRef, MAX_HAND_SIZE> = self.players[owner].hand.iter().copied().collect();
                let resolver = Self::relic_choice_resolver(RelicId::GamblingChip, owner);
                self.request_multi_card_choice_for_player(owner,
                    &cards,
                    PileKind::Hand,
                    resolver,
                    0,
                    Some(cards.len() as u16),
                    true,
                );
            }
            // VexingPuzzlebox.cs AfterPlayerTurnStart: player == Owner (dispatch
            // guard above), Owner TurnNumber == 1, Owner's unlocked character pool.
            (RelicId::VexingPuzzlebox, Hook::AfterPlayerTurnStart) if self.players[owner].turn_number == 1 => {
                if let Some(&g) = self.generate_unlocked_owner_cards(owner, None, 1).first() {
                    self.set_temporary_free_this_turn(g);
                    self.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
                }
            }
            (RelicId::Crossbow, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // Crossbow.cs AfterSideTurnStart: unlocked Owner attack pool.
                // Partial epochs are not represented; preserve r7's solo pool
                // and documented Ironclad default, while MP remains fail closed.
                let generated = if self.players.len() == 1
                    && (self.run.all_cards_unlocked != Some(true) || self.players[owner].character.is_none()) {
                    let character = self.players[owner].character.unwrap_or(Character::Ironclad);
                    if let Some(pool) = crate::cardgen::character_pool(character.into(), false, Some(CardType::Attack), None) {
                        self.generate_pool_cards(owner, pool, 1, true).into_iter().collect::<Vec<_>>()
                    } else {
                        self.unported("Crossbow unsupported solo character pool"); Vec::new()
                    }
                } else { self.generate_unlocked_owner_cards(owner, Some(CardType::Attack), 1) };
                if let Some(&g) = generated.first() {
                    self.set_temporary_free_this_turn(g);
                    self.add_generated_card_by(g, PileKind::Hand, PilePos::Bottom, Some(owner));
                }
            }
            (RelicId::DelicateFrond, Hook::BeforeCombatStart) => {
                // Models/Relics/DelicateFrond.cs:12-21: Owner's out-of-combat
                // potion pool, stopping on failed procurement (including Sozu).
                self.fill_empty_potion_slots_for_relic(owner);
            }
            (RelicId::Byrdpip, Hook::BeforeCombatStart) => {
                // Models/Relics/Byrdpip.cs:64-71: PlayerCmd.AddPet(Owner).
                self.summon_event_pet(owner, RelicId::Byrdpip);
            }
            (RelicId::PhylacteryUnbound, Hook::BeforeCombatStart) => {
                // Models/Relics/PhylacteryUnbound.cs:32: Owner, relic summon source.
                self.summon_osty(owner, 5);
            }
            (RelicId::PhylacteryUnbound, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // Models/Relics/PhylacteryUnbound.cs:37: participating Owner.
                self.summon_osty(owner, 2);
            }
            (RelicId::OrangeDough, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/OrangeDough.cs:21: distinct unlocked colorless
                // combat cards, multiplayer filtering, CombatCardGeneration RNG.
                let cards = self.generate_unlocked_colorless_cards(owner, 2);
                for k in cards {
                    self.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
                }
            }
            (RelicId::RunicCapacitor, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/RunicCapacitor.cs:18: Owner's slots, not global round.
                self.add_orb_slots(owner, 3);
            }
            (RelicId::InfusedCore, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/InfusedCore.cs AfterSideTurnStart: three Owner orbs.
                for _ in 0..3 { self.channel_orb(owner, crate::orbs::OrbKind::Lightning); }
            }
            (RelicId::FencingManual, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/FencingManual.cs AfterSideTurnStart: Forge(10, Owner).
                self.forge(owner, 10, None);
            }
            (RelicId::BigHat, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/BigHat.cs AfterSideTurnStart: distinct canonical
                // Ethereal cards from the unlocked Owner pool, in factory order.
                let cards = self.generate_unlocked_ethereal_cards(owner, 2);
                for k in cards { self.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(owner)); }
            }
            (RelicId::DataDisk, Hook::BeforeCombatStart) => {
                // Models/Relics/DataDisk.cs AfterRoomEntered(CombatRoom).
                self.apply_relic_power(owner, PowerId::Focus, 1, Some(owner));
            }
            (RelicId::HistoryCourse, Hook::AfterAutoPrePlayPhaseEntered) => {
                // C# HistoryCourse.AfterAutoPrePlayPhaseEntered / HappenedLastPlayerTurn:
                // finished attacks only, original cards only, and this owner's previous turn.
                if let Ev::Creature(player) = ev && player == owner && self.players[owner].turn_number > 1 {
                    let previous = self.players[owner].turn_number - 1;
                    let card = self.hist.finished_plays.iter().rev().find_map(|&(actor, turn, k)| {
                        let card = &self.cards[k as usize];
                        (actor == owner && turn == previous && card.card_type == CardType::Attack && !card.cv.is_dupe).then_some(k)
                    });
                    if let Some(k) = card {
                        // C# CardModel.CreateDupe clones the original and removes Exhaust.
                        let iid = self.run.new_card_id();
                        let mut dupe = self.cards[k as usize].clone_with_id(iid);
                        dupe.owner = owner;
                        dupe.cv.is_dupe = true;
                        dupe.keywords &= !KW_EXHAUST;
                        let copy = self.cards.len() as KRef;
                        self.cards.push(dupe);
                        self.auto_play_card(copy, None, false);
                    }
                }
            }
            (RelicId::FurCoat, Hook::BeforeCombatStart) => {
                // Models/Relics/FurCoat.cs BeforeCombatStart: marked coordinates,
                // not the owner seat or act index; SetCurrentHp is not damage.
                if self.run.coord.is_some_and(|coord| self.run.relics[r].fur_coat_has_coord(coord)) {
                    for e in self.hittable_enemies() {
                        self.relic_fur_coat_set_hp(e);
                    }
                }
            }
            (RelicId::FurCoat, Hook::AfterCreatureAddedToCombat) => {
                // FurCoat.cs AfterCreatureAddedToCombat also affects later enemies.
                if let Ev::Creature(c) = ev
                    && self.cr(c).side == Side::Enemy
                    && self.run.coord.is_some_and(|coord| self.run.relics[r].fur_coat_has_coord(coord)) {
                    self.relic_fur_coat_set_hp(c);
                }
            }
            (RelicId::PaelsTooth, Hook::AfterCombatEnd) if self.cr(owner).is_alive() => {
                self.relic_paels_tooth_return_one(r);
            }
            (RelicId::ToastyMittens, Hook::AfterPlayerTurnStart) => {
                let cards: ArrayVec<KRef, MAX_HAND_SIZE> = self.players[owner].hand.iter().copied().collect();
                let resolver = Self::relic_choice_resolver(RelicId::ToastyMittens, owner);
                self.request_card_choice_for_player(owner, &cards, PileKind::Hand, resolver, false);
            }
            (RelicId::WhisperingEarring, Hook::BeforePlayPhaseStart)
                if matches!(ev, Ev::Creature(player) if player == owner)
                    && self.is_player_taking_turn(owner)
                    && self.players[owner].turn_number <= 1 => {
                // Models/Relics/WhisperingEarring.cs:38 late auto-pre-play callback:
                // existing per-seat BeforePlayPhaseStart follows that seat's setup.
                // Frozen r7 relics/shop_event.py:3725 checks round_number > 1;
                // C# Owner.TurnNumber excludes a round-one extra turn.
                self.relic_whispering_earring_autoplay(owner);
            }
            (RelicId::WarHammer, Hook::AfterCombatVictory) if self.run.room_type == RoomType::Elite => {
                // Models/Relics/WarHammer.cs:19: Owner's upgradable deck,
                // stable-sort then shared Niche RNG shuffle, upgrade up to four.
                self.relic_upgrade_random_deck_cards(owner, 4);
            }
            (RelicId::UnsettlingLamp, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                // UnsettlingLamp.BeforeCombatStart/AfterCombatEnd reset its source
                // and first triggering power's ITemporaryPower internal exemption.
                self.run.relics[r].flag1 = false;
                self.run.relics[r].doubled_powers.get_or_insert_with(Vec::new).clear();
                self.run.relics[r].card = None;
            }
            (RelicId::UnsettlingLamp, Hook::BeforePowerAmountChanged) => {
                // UnsettlingLamp.BeforePowerAmountChanged: exact applier, opposite
                // side, explicit card source, visible Debuff, and no Artifact.
                if let Ev::PowerAmount { target, id, amount, applier, source: Some(k), instance: Some(p) } = ev
                    && self.run.relics[r].card.is_none()
                    && !self.run.relics[r].flag1
                    && applier == Some(owner)
                    && self.cr(target).side != self.cr(owner).side
                    && power_meta(id).visible
                    && Self::power_type_for_amount(id, amount) == PowerType::Debuff
                    && !self.has_power(target, PowerId::Artifact)
                {
                    self.run.relics[r].card = Some(k);
                    self.run.relics[r].doubled_powers.get_or_insert_with(Vec::new).push(p);
                }
            }
            (RelicId::UnsettlingLamp, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev && self.run.relics[r].card == Some(k) {
                    self.run.relics[r].flag1 = true;
                }
            }
            // Akabeko.AfterSideTurnStart uses Owner's turn, not the party round.
            (RelicId::Akabeko, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                self.apply_relic_power(owner, PowerId::Vigor, 8, Some(owner));
            }
            (RelicId::Bread, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number == 1 => {
                // Bread.AfterSideTurnStart: participating Owner's turn one.
                self.players[owner].energy = (self.players[owner].energy - 2).max(0);
            }
            (RelicId::Brimstone, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // Brimstone.AfterSideTurnStart: Owner applies self Strength;
                // opposing living creatures receive Strength with null applier.
                self.apply_relic_power(owner, PowerId::Strength, 2, Some(owner));
                for e in self.alive_enemies() {
                    // Bypass apply_power_from's legacy local-player fallback:
                    // C# explicitly passes null for BOTH applier and source.
                    self.creature_apply_power(e, PowerId::Strength, 1, None, None, false, false);
                }
            }
            (RelicId::SealOfGold, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // SealOfGold.cs:25-32 checks and spends Owner.Gold, never another
                // player's inventory. Missing teammate entry gold fails closed.
                if let Some(gold) = self.relic_owner_gold(owner) {
                    if gold >= 3 {
                        self.relic_gain_energy_for_owner(owner, 1);
                        if owner == PLAYER { self.run.gold -= 3; }
                        else { self.players[owner].gold = Some(gold - 3); }
                    }
                }
            }
            // BagOfMarbles.cs BeforeSideTurnStart: participating Owner, TurnNumber <= 1.
            (RelicId::BagOfMarbles, Hook::BeforeSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                let enemies = self.hittable_enemies();
                for enemy in enemies {
                    self.apply_relic_power(enemy, PowerId::Vulnerable, 1, Some(owner));
                }
            }
            // Bellows.cs AfterPlayerTurnStart: player == Owner, TurnNumber <= 1.
            (RelicId::Bellows, Hook::AfterPlayerTurnStart) if self.players[owner].turn_number <= 1 => {
                let hand = self.players[owner].hand.clone();
                for k in hand {
                    self.relic_upgrade_card(k);
                }
            }
            (RelicId::HornCleat, Hook::AfterBlockCleared) => {
                // HornCleat.AfterBlockCleared: creature == Owner.Creature.
                if matches!(ev, Ev::Creature(c) if c == owner) && self.players[owner].turn_number == 2 {
                    self.relic_gain_block_for_owner(owner, 14);
                }
            }
            (RelicId::SparklingRouge, Hook::AfterBlockCleared) => {
                if matches!(ev, Ev::Creature(c) if c == owner) && self.players[owner].turn_number == 3 {
                    self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
                    self.apply_relic_power(owner, PowerId::Dexterity, 1, Some(owner));
                }
            }
            (RelicId::CaptainsWheel, Hook::AfterBlockCleared) => {
                // CaptainsWheel.cs AfterBlockCleared: owner TurnNumber == 3.
                if matches!(ev, Ev::Creature(c) if c == owner) && self.players[owner].turn_number == 3 {
                    self.relic_gain_block_for_owner(owner, 18);
                }
            }
            (RelicId::Kusarigama, Hook::BeforeCombatStart) => self.run.relics[r].counter = 0,
            (RelicId::Kusarigama, Hook::AfterCardPlayed) => {
                // Kusarigama.AfterCardPlayed: Owner's third Attack in progress;
                // CombatTargets selects the victim, damage is Unpowered.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter % 3 == 0
                        && let Some(target) = self.relic_random_enemy()
                    {
                        let damage = self.calculate_damage(6, Some(owner), target, Props::UNPOWERED);
                        self.apply_damage(target, damage, Props::UNPOWERED, Some(owner));
                        self.flush_pending_attack();
                    }
                }
            }
            // Kusarigama.AfterSideTurnEnd resets only participating Owner.
            (RelicId::Kusarigama, Hook::AfterTurnEnd) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
            }
            // Vambrace.cs BeforeCombatStart/AfterCombatEnd clear both fields.
            (RelicId::Vambrace, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
                self.run.relics[r].card = None;
            }
            (RelicId::Vambrace, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.run.relics[r].card == Some(k)
                {
                    self.run.relics[r].flag1 = true;
                }
            }
            (RelicId::ArtOfWar, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].flag2 = true;
                }
            }
            (RelicId::ArtOfWar, Hook::AfterTurnEnd) if ev.is_player_side() => {
                self.run.relics[r].flag1 = self.run.relics[r].flag2;
                self.run.relics[r].flag2 = false;
            }
            (RelicId::ArtOfWar, Hook::AfterEnergyReset) if self.players[owner].turn_number > 1 => {
                if !self.run.relics[r].flag1 { self.relic_gain_energy_for_owner(owner, 1); }
                self.run.relics[r].flag1 = false;
                self.run.relics[r].flag2 = false;
            }
            (RelicId::ArtOfWar, Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
                self.run.relics[r].flag2 = false;
            }
            (RelicId::CharonsAshes, Hook::AfterCardExhausted) => {
                // CharonsAshes.cs AfterCardExhausted: card.Owner == Owner,
                // including ethereal exhausts, not another player's card.
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner {
                    self.relic_damage_all_enemies_for_player(owner, 3);
                }
            }
            (RelicId::BurningSticks, Hook::BeforeCombatStart | Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
            }
            (RelicId::BurningSticks, Hook::AfterCardExhausted) => {
                // Models/Relics/BurningSticks.cs:44-53: first Owner Skill only.
                if let Ev::Card(k) = ev
                    && !self.run.relics[r].flag1
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                {
                    self.run.relics[r].flag1 = true;
                    let iid = self.run.new_card_id();
                    let mut clone = self.cards[k as usize].clone_with_id(iid);
                    clone.owner = owner;
                    let clone_ref = self.cards.len() as KRef;
                    self.cards.push(clone);
                    if self.players[owner].hand.len() < MAX_HAND_SIZE {
                        self.players[owner].hand.push(clone_ref);
                    } else {
                        self.players[owner].discard.push(clone_ref);
                    }
                    self.fire_after_card_changed_piles(clone_ref, PileKind::None, None);
                }
            }
            (RelicId::ForgottenSoul, Hook::AfterCardExhausted) => {
                // ForgottenSoul.cs AfterCardExhausted: Owner's exhausted card,
                // one CombatTargets roll, Owner dealer and null card source.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && let Some(target) = self.relic_random_enemy()
                {
                    let active = self.active_card.take();
                    let damage = self.calculate_damage(1, Some(owner), target, Props::UNPOWERED);
                    self.apply_damage_with_card_source(target, damage, Props::UNPOWERED, Some(owner), None);
                    self.flush_pending_attack();
                    self.active_card = active;
                }
            }
            (RelicId::IntimidatingHelmet, Hook::BeforeCardPlayed) => {
                // IntimidatingHelmet.cs BeforeCardPlayed: actual EnergyValue paid
                // by Owner, not printed cost or another player's payment.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].energy_spent >= 2
                {
                    self.relic_gain_block_for_owner(owner, 4);
                }
            }
            (RelicId::MummifiedHand, Hook::AfterCardPlayed) => {
                // MummifiedHand.cs AfterCardPlayed: only Owner's powers in progress.
                if let Ev::Card(k) = ev
                    && !self.is_over && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Power
                {
                    self.relic_mummified_hand(owner);
                }
            }
            (RelicId::GamePiece, Hook::AfterCardPlayed) => {
                // GamePiece.AfterCardPlayed: only Owner's powers, in progress.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Power
                {
                    self.draw_cards(owner, 1, false);
                }
            }
            (RelicId::LostWisp, Hook::AfterCardPlayed) => {
                // LostWisp.AfterCardPlayed requires IsInProgress and Owner's Power.
                // r7 relics/shop_event.py:2281-2296 omits the in-progress check.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Power
                {
                    self.relic_damage_all_enemies_for_player(owner, 8);
                }
            }
            (RelicId::RadiantPearl, Hook::BeforeHandDraw) if self.players[owner].turn_number == 1 => {
                // Models/Relics/RadiantPearl.cs:32: player == Owner (dispatch),
                // first Owner turn, including a late join, never a repeated extra turn.
                // Frozen r7 relics/shop_event.py:3079 instead checks
                // combat.round_number == 1, repeating on same-round extra turns.
                let _ = self.create_card_in_pile_by_player(owner, CardId::Luminesce, false, PileKind::Hand, PilePos::Bottom);
            }
            (RelicId::JeweledMask, Hook::BeforeHandDraw) if self.players[owner].turn_number <= 1 => {
                // JeweledMask.BeforeHandDraw: Owner's first turn, prefer non-Innate
                // Powers, then CombatCardSelection.NextItem in draw-pile order.
                // r7 relics/shop_event.py:2127-2144 uses global round and no preference.
                let mut candidates: Vec<KRef> = self.players[owner].draw.iter().copied()
                    .filter(|&k| self.cards[k as usize].card_type == CardType::Power)
                    .collect();
                if candidates.iter().any(|&k| !self.cards[k as usize].is_innate()) {
                    candidates.retain(|&k| !self.cards[k as usize].is_innate());
                }
                if !candidates.is_empty() {
                    let i = self.run.rngs.combat_card_selection.choice_index(candidates.len());
                    let k = candidates[i];
                    self.set_temporary_free_this_turn(k);
                    self.move_cards_to_pile(&[k], PileKind::Hand, PilePos::Bottom);
                }
            }
            // RainbowRing.BeforeSideTurnStart: participating Owner only.
            (RelicId::RainbowRing, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
                self.run.relics[r].counter2 = 0;
                self.run.relics[r].counter3 = 0;
                self.run.relics[r].flag1 = false;
            }
            (RelicId::RainbowRing, Hook::AfterCardPlayed) => self.relic_rainbow_ring(r, owner, ev),
            (RelicId::RainbowRing, Hook::AfterCombatEnd) => {
                self.run.relics[r].counter = 0;
                self.run.relics[r].counter2 = 0;
                self.run.relics[r].counter3 = 0;
                self.run.relics[r].flag1 = false;
            }
            (RelicId::RazorTooth, Hook::AfterCardPlayed) => {
                // RazorTooth.cs AfterCardPlayed: Owner's upgradable Attack/Skill,
                // including a terminal play (no IsInProgress guard in C#).
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && matches!(self.cards[k as usize].card_type, CardType::Attack | CardType::Skill)
                    && !self.cards[k as usize].upgraded
                    && card_info(self.cards[k as usize].id, true).is_some_and(|info| info.upgraded)
                {
                    self.relic_upgrade_card(k);
                }
            }
            (RelicId::Shuriken, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::Shuriken, Hook::AfterCardPlayed) => {
                // Shuriken.AfterCardPlayed counts only Owner's Attacks in progress.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter % 3 == 0 {
                        self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
                    }
                }
            }
            (RelicId::Shuriken, Hook::AfterCombatEnd) => self.run.relics[r].counter = 0,
            (RelicId::CloakClasp, Hook::BeforeTurnEnd) if ev.is_player_side() => {
                // CloakClasp.BeforeSideTurnEnd reads Owner's Hand pile.
                let n = self.players[owner].hand.len() as i32;
                if n > 0 {
                    self.relic_gain_block_for_owner(owner, n);
                }
            }
            (RelicId::SwordOfStone, Hook::AfterCombatVictory) => {
                // SwordOfStone.cs:40-50 uses room.RoomType. Frozen r7's
                // getattr(combat, "is_elite", False) never triggered. C# applies
                // to solo too: the fifth solo elite win now replaces the relic
                // with SwordOfJade, unlike frozen r7's inert counter.
                if self.run.room_type == RoomType::Elite {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter >= 5 {
                        self.run.relics[r].id = RelicId::SwordOfJade;
                        self.run.relics[r].counter = 0;
                        self.run.relics[r].counter2 = 0;
                        self.run.relics[r].counter3 = 0;
                        self.run.relics[r].flag1 = false;
                        self.run.relics[r].flag2 = false;
                        self.run.relics[r].card = None;
                    }
                }
            }
            (RelicId::SwordOfJade, Hook::BeforeCombatStart) => {
                // Models/Relics/SwordOfJade.cs:21 AfterRoomEntered: Owner, null applier.
                self.apply_relic_power(owner, PowerId::Strength, 3, None);
            }
            (RelicId::Gorget, Hook::BeforeCombatStart) => {
                // Gorget.AfterRoomEntered(CombatRoom): Plating 4 on Owner.
                self.apply_relic_power(owner, PowerId::Plating, 4, Some(owner));
            }
            (RelicId::OddlySmoothStone, Hook::BeforeCombatStart) => {
                // OddlySmoothStone.AfterRoomEntered(CombatRoom): Owner Dexterity 1.
                self.apply_relic_power(owner, PowerId::Dexterity, 1, Some(owner));
            }
            (RelicId::Candelabra, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number == 2 => {
                self.relic_gain_energy_for_owner(owner, 2);
            }
            (RelicId::Chandelier, Hook::AfterSideTurnStart) if ev.is_player_side() && self.players[owner].turn_number == 3 => {
                self.relic_gain_energy_for_owner(owner, 3);
            }
            (RelicId::Sai, Hook::AfterSideTurnStart) if ev.is_player_side() => {
                // Models/Relics/Sai.cs:19: participating Owner's unpowered block.
                self.relic_gain_block_for_owner(owner, 7);
            }
            (RelicId::DiamondDiadem, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // DiamondDiadem.cs:24-30 AfterSideTurnStart explicitly tests
                // Owner.PlayerCombatState.TurnNumber <= 1. Unlike frozen r7's
                // round-one guard, a later solo extra turn in round one must NOT
                // regrant block/Blur. Preserve the C# owner-turn test for solo too.
                self.relic_gain_block_for_owner(owner, 20);
                self.apply_relic_power(owner, PowerId::Blur, 1, Some(owner));
            }
            (RelicId::DivineDestiny, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // DivineDestiny.cs AfterSideTurnStart: participating Owner only.
                self.gain_stars(owner, 7);
            }
            (RelicId::BoomingConch, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1
                    && self.run.room_type == RoomType::Elite => {
                // BoomingConch.cs AfterSideTurnStart: first Owner turn in Elite.
                self.relic_gain_energy_for_owner(owner, 1);
            }
            (RelicId::MeatOnTheBone, Hook::AfterCombatVictoryEarly) => {
                // MeatOnTheBone.cs AfterCombatVictoryEarly/WillHealOnCombatFinished:
                // living Owner at <= floor(MaxHp * .5), capped Heal(12).
                if self.cr(owner).is_alive() && self.cr(owner).hp <= self.cr(owner).max_hp / 2 {
                    self.heal(owner, 12);
                }
            }
            (RelicId::RippleBasin, Hook::BeforeTurnEnd) if ev.is_player_side() => {
                // RippleBasin.cs BeforeSideTurnEnd: CardPlaysFinished this turn
                // must contain no Attack played by Owner (teammates don't count).
                let attacks = self.hist.played_this_turn.iter().any(|&(actor, k)| {
                    actor == owner && self.cards[k as usize].card_type == CardType::Attack
                });
                if !attacks {
                    self.relic_gain_block_for_owner(owner, 4);
                }
            }
            (RelicId::OrnamentalFan, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::OrnamentalFan, Hook::AfterCardPlayed) => {
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter % 3 == 0 {
                        self.relic_gain_block_for_owner(owner, 4);
                    }
                }
            }
            // OrnamentalFan.AfterCombatEnd clears only IsActivating; the
            // attack count survives until Owner's next BeforeSideTurnStart.
            (RelicId::OrnamentalFan, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            // LetterOpener.BeforeCombatStart initializes SkillsPlayedThisTurn;
            // AfterSideTurnStart leaves turn 1 intact (pre-turn autoplay may count).
            (RelicId::LetterOpener, Hook::BeforeCombatStart) => self.run.relics[r].counter = 0,
            (RelicId::LetterOpener, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number != 1 => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::LetterOpener, Hook::AfterCardPlayed) => {
                // LetterOpener.AfterCardPlayed: Owner's third Skill, in progress,
                // deals 5 Unpowered damage with Owner.Creature as dealer.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Skill
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter % 3 == 0 {
                        self.relic_damage_all_enemies_for_player(owner, 5);
                    }
                }
            }
            // LetterOpener.cs AfterCombatEnd resets only IsActivating/status.
            // SkillsPlayedThisTurn survives until the next BeforeCombatStart.
            (RelicId::LetterOpener, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::Kunai, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter = 0;
            }
            (RelicId::Kunai, Hook::AfterCardPlayed) => {
                // Kunai.cs AfterCardPlayed: only Owner's attacks while in progress.
                if let Ev::Card(k) = ev
                    && !self.is_over
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.run.relics[r].counter += 1;
                    if self.run.relics[r].counter % 3 == 0 {
                        self.apply_relic_power(owner, PowerId::Dexterity, 1, Some(owner));
                    }
                }
            }
            (RelicId::Kunai, Hook::AfterCombatEnd) => self.run.relics[r].counter = 0,
            (RelicId::Pocketwatch, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                self.run.relics[r].counter2 = self.run.relics[r].counter;
                self.run.relics[r].counter = 0;
            }
            (RelicId::Pocketwatch, Hook::AfterCardPlayed) => {
                // Pocketwatch.AfterCardPlayed ignores terminal and other-owner plays.
                if let Ev::Card(k) = ev && !self.is_over && self.cards[k as usize].owner == owner {
                    self.run.relics[r].counter += 1;
                }
            }
            (RelicId::Pocketwatch, Hook::AfterCombatEnd) => {
                self.run.relics[r].counter = 0;
                self.run.relics[r].counter2 = 0;
            }
            // LastingCandy.BeforeCombatRewardOffered increments CombatRewardsSeen
            // only for Owner's CardReward. It has no combat-end callback.
            (RelicId::LastingCandy, Hook::AfterCombatEnd) => {}
            (RelicId::DemonTongue, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                // DemonTongue.cs BeforeSideTurnStart resets only participating Owner.
                self.run.relics[r].flag1 = false;
            }
            (RelicId::DemonTongue, Hook::AfterDamageReceived) => {
                // DemonTongue.cs AfterDamageReceived: first positive UnblockedDamage
                // to Owner during Owner's side; set latch before Heal's HP callbacks.
                if let Ev::Damage { target, amount, .. } = ev
                    && target == owner
                    && amount > 0
                    && self.side == self.cr(owner).side
                    && !self.run.relics[r].flag1
                {
                    self.run.relics[r].flag1 = true;
                    self.heal(owner, amount);
                }
            }
            (RelicId::BeatingRemnant, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                // BeatingRemnant.cs BeforeSideTurnStart: reset participating Owner.
                self.run.relics[r].counter = 0;
            }
            (RelicId::BeatingRemnant, Hook::AfterDamageReceived) => {
                // BeatingRemnant.cs AfterDamageReceived: UnblockedDamage, Owner only.
                if let Ev::Damage { target, amount, .. } = ev && !self.is_over && target == owner {
                    self.run.relics[r].counter += amount;
                }
            }
            (RelicId::LavaLamp, Hook::AfterDamageReceived) => {
                // Models/Relics/LavaLamp.cs AfterDamageReceived: Owner only,
                // positive unblocked damage, excluding Unblockable HP loss.
                if let Ev::Damage { target, amount, props, .. } = ev
                    && target == owner && amount > 0 && !props.has(Props::UNBLOCKABLE)
                {
                    self.run.relics[r].flag1 = true;
                }
            }
            (RelicId::SelfFormingClay, Hook::AfterDamageReceived) => {
                // SelfFormingClay.cs: in-progress positive UnblockedDamage to Owner.
                if let Ev::Damage { target, amount, .. } = ev && !self.is_over && target == owner && amount > 0 {
                    self.apply_relic_power(owner, PowerId::SelfFormingClay, 3, Some(owner));
                }
            }
            // RedMask.BeforeSideTurnStart uses Owner's TurnNumber, not round.
            (RelicId::RedMask, Hook::BeforeSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                let enemies = self.hittable_enemies();
                for enemy in enemies {
                    self.apply_relic_power(enemy, PowerId::Weak, 1, Some(owner));
                }
            }
            (RelicId::SneckoEye, Hook::BeforeCombatStart) => {
                // Models/Relics/SneckoEye.cs:33: Owner.Creature (ApplyPower).
                self.apply_relic_power(owner, PowerId::Confused, 1, Some(owner));
            }
            (RelicId::TwistedFunnel, Hook::BeforeSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/TwistedFunnel.cs:26: Owner's first participating
                // turn, HittableEnemies (not merely alive), Owner applier.
                for enemy in self.hittable_enemies() {
                    self.apply_relic_power(enemy, PowerId::Poison, 4, Some(owner));
                }
            }
            (RelicId::SymbioticVirus, Hook::AfterSideTurnStart)
                if ev.is_player_side() && self.players[owner].turn_number <= 1 => {
                // Models/Relics/SymbioticVirus.cs:29: Owner's first turn.
                self.channel_orb(owner, crate::orbs::OrbKind::Dark);
            }
            (RelicId::Orichalcum, Hook::BeforeTurnEndVeryEarly) if ev.is_player_side() => {
                // Orichalcum.cs:38-50 latches at the very-early tier. Do not
                // recheck block after intervening end-turn listeners.
                if self.cr(owner).block == 0 { self.run.relics[r].flag1 = true; }
            }
            (RelicId::Orichalcum, Hook::BeforeTurnEnd) if self.run.relics[r].flag1 => {
                self.run.relics[r].flag1 = false;
                self.relic_gain_block_for_owner(owner, 6);
            }
            // Orichalcum.cs BeforeSideTurnStart: only participating Owner resets.
            (RelicId::Orichalcum, Hook::BeforeSideTurnStart) if ev.is_player_side() => self.run.relics[r].flag1 = false,
            // ParryingShield.cs AfterSideTurnEnd: participating owner's block.
            (RelicId::ParryingShield, Hook::AfterTurnEnd) if ev.is_player_side() && self.cr(owner).block >= 10 => {
                if let Some(target) = self.relic_random_enemy() {
                    let damage = self.calculate_damage(6, Some(owner), target, Props::UNPOWERED);
                    self.apply_damage(target, damage, Props::UNPOWERED, Some(owner));
                    self.flush_pending_attack();
                }
            }
            // StoneCalendar.cs BeforeSideTurnEnd: participating Owner's seventh
            // turn, NOT the shared round number (extra turns advance only turns).
            (RelicId::StoneCalendar, Hook::BeforeTurnEnd)
                if ev.is_player_side() && self.players[owner].turn_number == 7 => {
                self.relic_damage_all_enemies_for_player(owner, 52);
            }
            (RelicId::ScreamingFlagon, Hook::BeforeTurnEnd) if ev.is_player_side() && self.players[owner].hand.is_empty() => {
                // ScreamingFlagon.cs:21-27: Owner's hand, unpowered AOE with
                // Owner as dealer and no cardSource (even in nested dispatch).
                let active = self.active_card.take();
                self.relic_damage_all_enemies_for_player(owner, 20);
                self.active_card = active;
            }
            (RelicId::FakeOrichalcum, Hook::BeforeTurnEndVeryEarly) if ev.is_player_side() => {
                // Models/Relics/FakeOrichalcum.cs BeforeSideTurnEndVeryEarly:
                // latch Owner's zero block before other end-turn block gains.
                if self.cr(owner).block == 0 { self.run.relics[r].flag1 = true; }
            }
            (RelicId::FakeOrichalcum, Hook::BeforeTurnEnd) if self.run.relics[r].flag1 => {
                self.run.relics[r].flag1 = false;
                self.relic_gain_block_for_owner(owner, 3);
            }
            (RelicId::FakeOrichalcum, Hook::BeforeSideTurnStart) if ev.is_player_side() => self.run.relics[r].flag1 = false,
            (RelicId::PaelsTears, Hook::BeforeTurnEnd) if ev.is_player_side() => {
                // Models/Relics/PaelsTears.cs:38: participating Owner's energy,
                // not the local player's. relic_event's participant guard applies.
                self.run.relics[r].flag1 = self.players[owner].energy > 0;
            }
            (RelicId::PaelsTears, Hook::AfterSideTurnStart) if ev.is_player_side() && self.run.relics[r].flag1 => {
                // Models/Relics/PaelsTears.cs:48: grant only to participating Owner.
                self.relic_gain_energy_for_owner(owner, 2);
            }
            // Models/Relics/PaelsTears.cs:57: combat-local latch never persists.
            (RelicId::PaelsTears, Hook::AfterCombatEnd) => self.run.relics[r].flag1 = false,
            (RelicId::VelvetChoker, Hook::BeforeSideTurnStart) if ev.is_player_side() => {
                // Models/Relics/VelvetChoker.cs:92: participating Owner.
                self.run.relics[r].counter = 0;
            }
            (RelicId::VelvetChoker, Hook::BeforeCombatStart) => {
                // Models/Relics/VelvetChoker.cs:74: reset on entering combat.
                self.run.relics[r].counter = 0;
            }
            (RelicId::VelvetChoker, Hook::AfterCardPlayed) => {
                // Models/Relics/VelvetChoker.cs:64: count only Owner's cards.
                if let Ev::Card(k) = ev && self.cards[k as usize].owner == owner {
                    self.run.relics[r].counter += 1;
                }
            }
            (RelicId::VelvetChoker, Hook::AfterCombatEnd) => self.run.relics[r].counter = 0,
            (RelicId::DaughterOfTheWind, Hook::AfterCardPlayed) => {
                // DaughterOfTheWind.cs:18-24: only Owner's attacks, manual or auto.
                if let Ev::Card(k) = ev
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Attack
                {
                    self.relic_gain_block_for_owner(owner, 1);
                }
            }
            (RelicId::StoneCracker, Hook::BeforeCombatStart) => self.relic_stone_cracker(owner),
            (RelicId::ChosenCheese, Hook::AfterCombatEnd) => {
                // ChosenCheese.AfterCombatEnd -> CreatureCmd.GainMaxHp(Owner).
                self.gain_max_hp_for_relic(owner, 1);
            }
            // RuinedHelmet.cs resets only AfterCombatEnd. Resetting here at
            // BeforeCombatStart could erase an earlier Vajra/Girya activation.
            (RelicId::RuinedHelmet, Hook::AfterCombatEnd) => {
                self.run.relics[r].flag1 = false;
            }
            _ => {}
        }
    }

    pub(crate) fn relic_after_gold_gained(&mut self, owner: CId) {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::DragonFruit
                && self.run.relics[r].owner == owner && self.relic_enabled(r) {
                // DragonFruit.cs AfterGoldGained -> CreatureCmd.GainMaxHp(1).
                self.gain_max_hp_for_relic(owner, 1);
            }
        }
    }

    pub(crate) fn relic_modify_gold_gained(&mut self, owner: CId, amount: i32) -> i32 {
        let mut bowler = false;
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].owner != owner
                || !matches!(self.run.relics[r].id, RelicId::BowlerHat | RelicId::Ectoplasm)
                || !self.relic_enabled(r) { continue; }
            match self.run.relics[r].id {
                // Ectoplasm.cs ModifyGoldGained: zero for Owner, regardless of
                // BowlerHat's ordering. No gain event follows a zero amount.
                RelicId::Ectoplasm => return 0,
                RelicId::BowlerHat => bowler = true,
                _ => unreachable!(),
            }
        }
        // BowlerHat.cs: non-stackable 1.25; PlayerCmd casts only after modifiers.
        if bowler { (i64::from(amount) * 5 / 4) as i32 } else { amount }
    }

    fn relic_fur_coat_set_hp(&mut self, creature: CId) {
        // CreatureCmd.SetCurrentHp: notify HP listeners, not damage listeners.
        let before = self.cr(creature).hp;
        self.cr_mut(creature).hp = 1;
        if before != 1 { self.after_hp_changed(creature, 1 - before); }
    }

    pub(crate) fn relic_after_flush(&mut self, owner: CId, retained: &[KRef]) {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id != RelicId::Bookmark
                || self.run.relics[r].owner != owner || !self.relic_enabled(r) { continue; }
            // Models/Relics/Bookmark.cs AfterFlush: retained snapshot, local cost,
            // including whole-hand retention. X costs never enter the draw.
            let options: Vec<_> = retained.iter().copied().filter(|&k| {
                let card = &self.cards[k as usize];
                !card.x_cost && card.local_energy_cost() > 0
            }).collect();
            if !options.is_empty() {
                let k = options[self.run.rngs.combat_card_selection.choice_index(options.len())];
                self.cards[k as usize].add_energy_cost(-1, crate::card::CostExpiry::Played);
            }
        }
    }

    pub(crate) fn relic_initial_enemy_room_entered(&mut self, enemy: CId) {
        // CombatRoom.cs:203 calls Hook.AfterRoomEntered before monster
        // AfterAddedToRoom. PhilosophersStone.cs:51 therefore applies Strength
        // before starting powers (e.g. BygoneEffigy.AfterAddedToRoom's Slow).
        // Summons instead use the ordinary AfterCreatureAddedToCombat dispatch.
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::PhilosophersStone && self.relic_enabled(r) {
                self.relic_event(r, Hook::AfterCreatureAddedToCombat, Ev::Creature(enemy));
            }
        }
    }

    fn relic_ghost_seed(&mut self, owner: CId, k: KRef) {
        let card = &mut self.cards[k as usize];
        if card.owner == owner
            && card.rarity == CardRarity::Basic
            && (card.has_tag(CardTag::Strike) || card.has_tag(CardTag::Defend))
            && !card.is_ethereal()
        {
            card.keywords |= KW_ETHEREAL;
        }
    }

    fn relic_red_skull_check(&mut self, r: usize, owner: CId) {
        let low = self.cr(owner).hp <= self.cr(owner).max_hp * 50 / 100;
        if low && !self.run.relics[r].flag1 {
            self.apply_relic_power(owner, PowerId::Strength, 3, Some(owner));
            self.run.relics[r].flag1 = true;
        } else if !low && self.run.relics[r].flag1 {
            self.apply_relic_power(owner, PowerId::Strength, -3, Some(owner));
            self.run.relics[r].flag1 = false;
        }
    }

    fn relic_mummified_hand(&mut self, owner: CId) {
        // MummifiedHand.cs:31-46 has four ordered fallback pools. None ignores
        // local/global cost modifiers; CostsEnergyOrStars(true) includes both.
        let hand = self.players[owner].hand.clone();
        let preferred: ArrayVec<KRef, MAX_HAND_SIZE> = hand.iter().copied().filter(|&k| {
            let card = &self.cards[k as usize];
            (!card.x_cost && card.original_cost > 0) || card.star_cost > 0
        }).collect();
        let mut paying = ArrayVec::<KRef, MAX_HAND_SIZE>::new();
        for &k in &hand {
            if (!self.cards[k as usize].x_cost && self.modified_card_cost(owner, k) > 0)
                || (!self.cards[k as usize].star_x && self.modified_star_cost(k) > 0) {
                paying.push(k);
            }
        }
        let first: ArrayVec<KRef, MAX_HAND_SIZE> = preferred.iter().copied()
            .filter(|k| paying.contains(k)).collect();
        let candidates = if !first.is_empty() { &first }
            else if !paying.is_empty() { &paying }
            else if !preferred.is_empty() { &preferred }
            else { &hand };
        if candidates.is_empty() { return; }
        let i = self.run.rngs.combat_card_selection.choice_index(candidates.len());
        let card = &mut self.cards[candidates[i] as usize];
        card.set_energy_cost(0, crate::card::CostExpiry::TurnOrPlayed, false);
        card.cv.turn_star_cost_override = Some(0);
    }

    fn relic_rainbow_ring(&mut self, r: usize, owner: CId, ev: Ev) {
        // RainbowRing.cs AfterCardPlayed: owner, in progress, once per turn.
        let Ev::Card(k) = ev else { return; };
        if self.cards[k as usize].owner != owner || self.is_over || self.run.relics[r].flag1 {
            return;
        }
        match self.cards[k as usize].card_type {
            CardType::Attack => self.run.relics[r].counter += 1,
            CardType::Skill => self.run.relics[r].counter2 += 1,
            CardType::Power => self.run.relics[r].counter3 += 1,
            _ => {}
        }
        if !self.run.relics[r].flag1
            && self.run.relics[r].counter > 0
            && self.run.relics[r].counter2 > 0
            && self.run.relics[r].counter3 > 0
        {
            self.run.relics[r].flag1 = true;
            self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
            self.apply_relic_power(owner, PowerId::Dexterity, 1, Some(owner));
        }
    }

    fn relic_stone_cracker(&mut self, owner: CId) {
        // StoneCracker.AfterRoomEntered(CombatRoom): upgradable cards in Owner's
        // Draw, StableShuffle(CombatCardSelection), upgrade the first two.
        // is_card_upgradable builds the upgraded template: one instance id per non-upgraded card.
        let mut candidates: Vec<KRef> = Vec::new();
        for i in 0..self.players[owner].draw.len() {
            let k = self.players[owner].draw[i];
            if self.cards[k as usize].upgraded {
                continue;
            }
            self.run.new_card_id();
            if Card::from_factory(self.cards[k as usize].id, true, 0).is_some_and(|c| c.upgraded) {
                candidates.push(k);
            }
        }
        crate::flow::dotnet_sort_cards(&mut candidates, &self.cards);
        self.run.rngs.combat_card_selection.shuffle(&mut candidates);
        for k in candidates.into_iter().take(2) {
            self.relic_upgrade_card(k);
        }
    }

    pub(crate) fn relic_damage_additive(
        &mut self, r: usize, owner: CId, dealer: Option<CId>, target: CId,
        props: Props, card: Option<KRef>,
    ) -> i32 {
        let mut fault = None;
        let value = self.query_relic_damage_additive(r, owner, dealer, target, props, card, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_damage_additive(
        &self,
        r: usize,
        owner: CId,
        dealer: Option<CId>,
        _target: CId,
        props: Props,
        card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> i32 {
        if !self.query_relic_enabled(r, fault) {
            return 0;
        }
        match self.run.relics[r].id {
            RelicId::StrikeDummy => {
                // StrikeDummy.cs ModifyDamageAdditive: Strike tag + powered attack;
                // the source's card type is deliberately not constrained.
                let Some(k) = card else { return 0; };
                let c = &self.cards[k as usize];
                if (dealer == Some(owner) || c.owner == owner)
                    && c.has_tag(CardTag::Strike)
                    && props.is_powered_attack()
                {
                    3
                } else {
                    0
                }
            }
            RelicId::MiniatureCannon => {
                // MiniatureCannon.ModifyDamageAdditive requires an upgraded source
                // and (dealer == Owner.Creature OR cardSource.Owner == Owner).
                // IsPoweredAttack is the sole type constraint; card type is not.
                let Some(k) = card else { return 0; };
                let c = &self.cards[k as usize];
                if (dealer == Some(owner) || c.owner == owner)
                    && c.upgraded
                    && props.is_powered_attack()
                { 3 } else { 0 }
            }
            RelicId::MysticLighter => {
                let Some(k) = card else { return 0; };
                let c = &self.cards[k as usize];
                // MysticLighter.ModifyDamageAdditive: only source ownership,
                // enchantment and IsPoweredAttack, regardless of card Type/dealer.
                // Deliberate r7 divergence: sts2_env/relics/shop_event.py:523-540
                // additionally requires CardType.ATTACK and accepts dealer ownership.
                if c.owner == owner
                    && !c.enchantments.is_empty()
                    && props.is_powered_attack()
                { 9 } else { 0 }
            }
            RelicId::FakeStrikeDummy => {
                // FakeStrikeDummy.cs ModifyDamageAdditive: dealer OR card Owner
                // belongs to this relic, Strike tag, powered attacks only.
                let Some(k) = card else { return 0; };
                let c = &self.cards[k as usize];
                if (dealer == Some(owner) || c.owner == owner)
                    && c.has_tag(CardTag::Strike)
                    && props.is_powered_attack()
                { 1 } else { 0 }
            }
            _ => 0,
        }
    }

    pub(crate) fn relic_damage_multiplicative(
        &mut self, r: usize, owner: CId, dealer: Option<CId>, target: Option<CId>,
        props: Props, card: Option<KRef>,
    ) -> f64 {
        let mut fault = None;
        let value = self.query_relic_damage_multiplicative(r, owner, dealer, target, props, card, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_damage_multiplicative(
        &self,
        r: usize,
        owner: CId,
        dealer: Option<CId>,
        target: Option<CId>,
        props: Props,
        card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> f64 {
        if !self.query_relic_enabled(r, fault) { return 1.0; }
        match self.run.relics[r].id {
            // Models/Relics/VitruvianMinion.cs:17: source tag and owner only.
            RelicId::VitruvianMinion if self.relic_owned_minion_card(owner, card) => 2.0,
            // Models/Relics/UndyingSigil.cs:22: incoming powered attack from
            // another creature whose current HP is at most its Doom amount.
            RelicId::UndyingSigil if target == Some(owner) && props.is_powered_attack()
                && dealer.is_some_and(|d| d != owner && self.cr(d).hp <= self.power_amount(d, PowerId::Doom)) => 0.5,
            RelicId::PenNib => {
                // PenNib.cs ModifyDamageMultiplicative: Owner.Creature or
                // Owner.Osty; preview the tenth attack only outside the Play pile.
                let Some(k) = card else { return 1.0; };
                let owned_dealer = dealer.is_some_and(|d| d == owner ||
                    (self.cr(d).monster_id == "OSTY" && self.cr(d).pet_owner == Some(owner)));
                if !owned_dealer || !props.is_powered_attack() { return 1.0; }
                let doubled = match self.run.relics[r].card {
                    Some(trigger) => trigger == k,
                    None => self.run.relics[r].counter == 9 && self.card_pile(k) != PileKind::Play,
                };
                if doubled { 2.0 } else { 1.0 }
            }
            _ => 1.0,
        }
    }

    pub(crate) fn relic_block_additive(
        &self,
        _r: usize,
        _owner: CId,
        _target: CId,
        _props: Props,
        _card: Option<KRef>,
    ) -> i32 {
        0
    }

    fn relic_owned_minion_card(&self, owner: CId, card: Option<KRef>) -> bool {
        card.is_some_and(|k| self.cards[k as usize].owner == owner
            && self.cards[k as usize].has_tag(CardTag::Minion))
    }

    pub(crate) fn relic_block_multiplicative(
        &mut self, r: usize, owner: CId, target: CId, props: Props, card: Option<KRef>,
    ) -> f64 {
        let mut fault = None;
        let value = self.query_relic_block_multiplicative(r, owner, target, props, card, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_block_multiplicative(
        &self,
        r: usize,
        owner: CId,
        _target: CId,
        props: Props,
        card: Option<KRef>,
        fault: &mut Option<&'static str>,
    ) -> f64 {
        if !self.query_relic_enabled(r, fault) {
            return 1.0;
        }
        match self.run.relics[r].id {
            // Models/Relics/VitruvianMinion.cs:34: source owner, not recipient.
            RelicId::VitruvianMinion if self.relic_owned_minion_card(owner, card) => 2.0,
            RelicId::Vambrace => {
                // Vambrace.cs ModifyBlockMultiplicative checks cardSource.Owner,
                // not the block recipient (ally-targeted cards qualify too).
                if self.run.relics[r].card.is_some() && self.run.relics[r].card != card {
                    return 1.0;
                }
                if card.is_some_and(|k| self.cards[k as usize].owner == owner)
                    && !self.run.relics[r].flag1
                    && card.is_some()
                    && props.is_card_or_monster_move()
                {
                    2.0
                } else {
                    1.0
                }
            }
            RelicId::PaelsLegion => {
                // Models/Relics/PaelsLegion.cs:134: cardSource.Owner, NOT target.
                if card.is_some_and(|k| self.cards[k as usize].owner == owner)
                    && self.run.relics[r].counter <= 0
                    && props.is_card_or_monster_move()
                {
                    2.0
                } else {
                    1.0
                }
            }
            _ => 1.0,
        }
    }

    pub(crate) fn relic_after_modifying_block(&mut self, r: usize, owner: CId, amount: i32, card: Option<KRef>) {
        debug_assert_eq!(self.run.relics[r].owner, owner);
        if !self.relic_enabled(r) || amount <= 0 || card.is_none() {
            return;
        }
        match self.run.relics[r].id {
            RelicId::Vambrace => self.run.relics[r].card = card,
            RelicId::PaelsLegion
                // Models/Relics/PaelsLegion.cs:155: positive affected CardPlay.
                if self.run.relics[r].counter <= 0
                    && self.active_card.is_some()
                    && self.run.relics[r].card.is_none_or(|k| Some(k) == card) =>
            {
                self.run.relics[r].card = card;
            }
            _ => {}
        }
    }

    pub(crate) fn relic_modify_hand_draw(&mut self, r: usize, owner: CId, draw: i32, late: bool) -> i32 {
        if self.run.relics[r].owner != owner { return draw; }
        if !self.relic_enabled(r) {
            return draw;
        }
        match (self.run.relics[r].id, late) {
            // BagOfPreparation.cs ModifyHandDraw: player == Owner, TurnNumber <= 1.
            (RelicId::BagOfPreparation, false) if self.players[owner].turn_number <= 1 => draw + 2,
            // Models/Relics/RingOfTheSnake.cs:14: Owner's first hand draw.
            (RelicId::RingOfTheSnake, false) if self.players[owner].turn_number <= 1 => draw + 2,
            // BoomingConch.cs ModifyHandDraw uses real room type and Owner turn.
            (RelicId::BoomingConch, false) if self.players[owner].turn_number <= 1
                && self.run.room_type == RoomType::Elite => draw + 2,
            // Models/Relics/RingOfTheDrake.cs:20: Owner.TurnNumber, not round.
            // Frozen r7 relics/starter.py:75 checks round_number <= 3; extra
            // turns consume the C# owner counter even within the same round.
            (RelicId::RingOfTheDrake, false) if self.players[owner].turn_number <= 3 => draw + 2,
            // BigMushroom.cs ModifyHandDraw: only Owner's first turn.
            // Frozen r7 relics/shop_event.py:1023 BigMushroom uses round_number
            // == 1 instead, applying the penalty again on round-one extra turns.
            (RelicId::BigMushroom, false) if self.players[owner].turn_number == 1 => (draw - 2).max(0),
            // Models/Relics/PaelsBlood.cs:14: player == Owner (guard above).
            (RelicId::PaelsBlood, false) => draw + 1,
            // Pocketwatch.ModifyHandDraw excludes only Owner's first turn.
            (RelicId::Pocketwatch, false) if self.players[owner].turn_number != 1 && self.run.relics[r].counter2 <= 3 => draw + 3,
            // Models/Relics/SneckoEye.cs:38: owner guard above.
            (RelicId::SneckoEye, false) => draw + 2,
            // Fiddle.cs ModifyHandDraw: two extra cards for Owner.
            (RelicId::Fiddle, true) => draw + 2,
            (RelicId::Pendulum, false) if self.run.relics[r].counter == 0 => draw + 1,
            // Models/Relics/PollinousCore.cs:104: activates at >= Turns.
            (RelicId::PollinousCore, false) if self.run.relics[r].counter >= 4 => draw + 2,
            _ => draw,
        }
    }

    pub(crate) fn relic_after_modifying_hand_draw(&mut self, r: usize, owner: CId) {
        if self.run.relics[r].owner != owner { return; }
        if self.relic_enabled(r) && self.run.relics[r].id == RelicId::PollinousCore {
            self.run.relics[r].counter = 0;
        }
    }

    pub(crate) fn relic_modify_max_energy(&mut self, r: usize, owner: CId, energy: i32) -> i32 {
        if self.run.relics[r].owner != owner { return energy; }
        if !self.relic_enabled(r) { return energy; }
        match self.run.relics[r].id {
            // Sozu.cs:22-29: Owner-only energy (guard above).
            RelicId::Sozu => energy + 1,
            // Models/Relics/PrismaticGem.cs:19: owner-only energy (guard above).
            RelicId::BloodSoakedRose | RelicId::Ectoplasm | RelicId::PrismaticGem => energy + 1,
            // Models/Relics/SpikedGauntlets.cs:18: owner-only energy.
            RelicId::BlessedAntler | RelicId::SpikedGauntlets | RelicId::VelvetChoker => energy + 1,
            // Bread.ModifyMaxEnergy: only Owner turn one lacks the bonus.
            RelicId::Bread if self.players[owner].turn_number != 1 => energy + 1,
            // Models/Relics/PhilosophersStone.cs:32 ModifyMaxEnergy: owner guard above.
            RelicId::PhilosophersStone | RelicId::WhisperingEarring => energy + 1,
            // Models/Relics/PumpkinCandle.cs:58: owner guard above; positive KindleCount.
            RelicId::PumpkinCandle if self.run.relics[r].counter > 0 => energy + 1,
            // Models/Relics/PaelsFlesh.cs:37: Owner's third turn onward.
            // Its side-start callbacks only update UI; the energy reset uses
            // this modified maximum, including same-round extra turns.
            RelicId::PaelsFlesh if self.players[owner].turn_number >= 3 => energy + 1,
            _ => energy,
        }
    }

    /// `late`: C# TryModifyEnergyCostInCombat (Spiked Gauntlets) vs ...Late (Brilliant Scarf).
    pub(crate) fn relic_modify_card_cost(&mut self, r: usize, owner: CId, k: KRef, cost: i32, late: bool) -> i32 {
        let mut fault = None;
        let value = self.query_relic_modify_card_cost(r, owner, k, cost, late, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_modify_card_cost(&self, r: usize, owner: CId, k: KRef, cost: i32, late: bool,
        fault: &mut Option<&'static str>) -> i32 {
        if !self.query_relic_enabled(r, fault) { return cost; }
        match self.run.relics[r].id {
            RelicId::SpikedGauntlets
                // Models/Relics/SpikedGauntlets.cs:27: cost modifier only;
                // no independent ShouldPlay restriction after later overrides.
                if !late
                    && self.cards[k as usize].owner == owner
                    && self.cards[k as usize].card_type == CardType::Power
                    && !self.cards[k as usize].x_cost => cost + 1,
            RelicId::BrilliantScarf
                if late
                    && self.run.relics[r].counter == 4
                    && self.cards[k as usize].owner == owner
                    && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k) || self.active_card == Some(k)) => 0,
            _ => cost,
        }
    }

    pub(crate) fn relic_modify_star_cost(&mut self, r: usize, owner: CId, k: KRef, cost: i32) -> i32 {
        let mut fault = None;
        let value = self.query_relic_modify_star_cost(r, owner, k, cost, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_modify_star_cost(&self, r: usize, owner: CId, k: KRef, cost: i32,
        fault: &mut Option<&'static str>) -> i32 {
        if !self.query_relic_enabled(r, fault) {
            return cost;
        }
        match self.run.relics[r].id {
            RelicId::BrilliantScarf
                if self.run.relics[r].counter == 4
                    && self.cards[k as usize].owner == owner
                    && (self.players[owner].hand.contains(&k) || self.players[owner].play.contains(&k) || self.active_card == Some(k)) => 0,
            _ => cost,
        }
    }

    pub(crate) fn relic_modify_x_value(&mut self, r: usize, owner: CId, k: KRef, value: i32) -> i32 {
        // ChemicalX.cs ModifyXValue: only the card Owner, never another seat.
        if self.relic_enabled(r)
            && self.run.relics[r].id == RelicId::ChemicalX
            && self.cards[k as usize].owner == owner
        {
            value + 2
        } else {
            value
        }
    }

    pub(crate) fn relic_modify_card_play_count(&mut self, r: usize, owner: CId, k: KRef, count: i32) -> i32 {
        // Models/Relics/ThrowingAxe.cs:38 ModifyCardPlayCount: card.Owner == Owner.
        if self.relic_enabled(r)
            && self.run.relics[r].id == RelicId::ThrowingAxe
            && self.cards[k as usize].owner == owner
            && !self.run.relics[r].flag1
        {
            count + 1
        } else {
            count
        }
    }

    pub(crate) fn relic_after_modifying_card_play_count(&mut self, r: usize, owner: CId, k: KRef) {
        if self.relic_enabled(r)
            && self.run.relics[r].id == RelicId::ThrowingAxe
            && self.cards[k as usize].owner == owner
            && !self.run.relics[r].flag1
        {
            self.run.relics[r].flag1 = true;
        }
    }

    pub(crate) fn resolve_relic_choice(&mut self, resolver: Resolver, selected: &[KRef]) -> bool {
        let ChoiceSrc::Relic(id) = resolver.src else {
            return false;
        };
        let owner = resolver.k as CId;
        match id {
            RelicId::GamblingChip => {
                // Models/Relics/GamblingChip.cs:23 -> CardCmd.cs:132:
                // normal discard history, relic hooks, replacement draw, Sly.
                self.discard_cards(owner, selected, selected.len() as i32);
            }
            RelicId::Toolbox | RelicId::ChoicesParadox => {
                if let Some(&k) = selected.first() {
                    self.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
                }
            }
            RelicId::ToastyMittens => {
                if let Some(&k) = selected.first()
                    && let Some(i) = self.players[owner].hand.iter().position(|&q| q == k)
                {
                    self.players[owner].hand.remove(i);
                    self.cards[k as usize].owner = owner;
                    self.players[owner].exhaust.push(k);
                    self.fire_after_card_changed_piles(k, PileKind::Hand, None);
                    self.fire_after_card_exhausted(k);
                }
                self.apply_relic_power(owner, PowerId::Strength, 1, Some(owner));
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn relic_joss_after_exhaust(&mut self, r: usize, k: KRef, exhausted_by_ethereal: bool) -> bool {
        // JossPaper.cs AfterCardExhausted / DrawIfThresholdMet.
        let owner = self.run.relics[r].owner;
        if self.cards[k as usize].owner != owner {
            return false;
        }
        if !self.relic_enabled(r) { return false; }
        if exhausted_by_ethereal {
            self.run.relics[r].counter2 += 1;
            return false;
        }
        self.run.relics[r].counter += 1;
        let draws = self.run.relics[r].counter / 5;
        if draws <= 0 { return false; }
        self.draw_cards_for_branch(owner, draws);
        true
    }

    pub(crate) fn relic_power_given_additive(&mut self, r: usize, giver: Option<CId>, id: PowerId) -> i32 {
        let mut fault = None;
        let amount = self.query_relic_power_given_additive(r, giver, id, &mut fault);
        self.apply_query_fault(fault);
        amount
    }

    pub(crate) fn query_relic_power_given_additive(&self, r: usize, giver: Option<CId>, id: PowerId,
        fault: &mut Option<&'static str>) -> i32 {
        // Models/Relics/SneckoSkull.cs:19: giver == Owner.Creature; no source-card
        // requirement (Poison potions and relics qualify too).
        if self.run.relics[r].id == RelicId::SneckoSkull
            && giver == Some(self.run.relics[r].owner) && id == PowerId::Poison
            && self.query_relic_enabled(r, fault) { 1 } else { 0 }
    }

    pub(crate) fn relic_modify_power_amount_given(
        &mut self,
        r: usize,
        owner: CId,
        id: PowerId,
        amount: i32,
        _target: CId,
        source: Option<KRef>,
    ) -> i32 {
        let mut fault = None;
        let amount = self.query_relic_modify_power_amount_given(r, owner, id, amount, source, &mut fault);
        self.apply_query_fault(fault);
        amount
    }

    pub(crate) fn query_relic_modify_power_amount_given(&self, r: usize, owner: CId, id: PowerId,
        amount: i32, source: Option<KRef>, fault: &mut Option<&'static str>) -> i32 {
        if !self.query_relic_enabled(r, fault)
            || self.run.relics[r].id != RelicId::UnsettlingLamp
            || self.run.relics[r].flag1
            || Self::power_type_for_amount(id, amount) != PowerType::Debuff
        {
            return amount;
        }
        debug_assert_eq!(self.run.relics[r].owner, owner);
        // UnsettlingLamp.ModifyPowerAmountGivenMultiplicative: only the source
        // selected by BeforePowerAmountChanged; no active-card/owner heuristic.
        if source.is_none() || self.run.relics[r].card != source { return amount; }
        if self.run.relics[r].doubled_powers.as_ref().is_some_and(|powers|
            powers.iter().any(|&p| Self::relic_temporary_internal_power(self.powers[p as usize].id) == Some(id))) {
            return amount;
        }
        amount * 2
    }

    pub fn relic_temporary_internal_power(id: PowerId) -> Option<PowerId> {
        // Audited TemporaryStrength/Dexterity/FocusPower.InternallyAppliedPower
        // and their concrete subclasses. Legacy generic names are the simulator's
        // existing identities for those same temporary-power families.
        match id {
            PowerId::TemporaryStrength | PowerId::FlexPotion | PowerId::ShacklingPotion | PowerId::DarkShackles
            | PowerId::EnfeeblingTouch | PowerId::Coordinate | PowerId::CrushUnder | PowerId::DyingStar
            | PowerId::FeedingFrenzy | PowerId::Mangle | PowerId::MonarchsGazeStrengthDown
            | PowerId::PiercingWail | PowerId::ReptileTrinket | PowerId::SetupStrike => Some(PowerId::Strength),
            PowerId::TemporaryDexterity | PowerId::SpeedPotion | PowerId::Anticipate | PowerId::Fade | PowerId::HelicalDart => Some(PowerId::Dexterity),
            PowerId::TemporaryFocus | PowerId::FocusedStrike | PowerId::Hotfix | PowerId::HyperbeamFocusDown | PowerId::Synchronize => Some(PowerId::Focus),
            _ => None,
        }
    }

    pub(crate) fn relic_should_prevent_player_death(&mut self, r: usize, owner: CId) -> bool {
        if self.run.relics[r].owner != owner { return false; }
        if !self.relic_enabled(r)
            || self.run.relics[r].id != RelicId::LizardTail
            || self.run.relics[r].flag1
        {
            return false;
        }
        // LizardTail.cs AfterPreventingDeath: once per run, heal the dying
        // Owner by max(1, MaxHp * .5); Creature.SetCurrentHpInternal truncates.
        self.run.relics[r].flag1 = true;
        let before = self.cr(owner).hp;
        let hp = (self.cr(owner).max_hp / 2).max(1);
        self.cr_mut(owner).hp = hp.min(self.cr(owner).max_hp);
        let healed = self.cr(owner).hp - before;
        if healed > 0 && !self.is_over { self.after_hp_changed(owner, healed); }
        true
    }

    pub(crate) fn relic_should_play(&mut self, r: usize, owner: CId, k: KRef) -> Option<bool> {
        let mut fault = None;
        let value = self.query_relic_should_play(r, owner, k, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub(crate) fn query_relic_should_play(&self, r: usize, owner: CId, k: KRef,
        fault: &mut Option<&'static str>) -> Option<bool> {
        if !self.query_relic_enabled(r, fault) { return None; }
        match self.run.relics[r].id {
            RelicId::VelvetChoker
                if self.cards[k as usize].owner == owner && self.run.relics[r].counter >= 6 => Some(false),
            _ => None,
        }
    }

    pub(crate) fn relic_should_draw(&mut self, r: usize, owner: CId, from_hand_draw: bool) -> Option<bool> {
        if self.run.relics[r].owner != owner { return None; }
        if !self.relic_enabled(r) { return None; }
        match self.run.relics[r].id {
            // Fiddle.cs ShouldDraw permits hand draws and enemy-side draws.
            RelicId::Fiddle if !from_hand_draw && self.side == Side::Player => Some(false),
            _ => None,
        }
    }

    pub(crate) fn relic_should_reset_energy(&mut self, r: usize, owner: CId) -> Option<bool> {
        if self.run.relics[r].owner != owner { return None; }
        if !self.relic_enabled(r) { return None; }
        match self.run.relics[r].id {
            // IceCream.ShouldPlayerResetEnergy excludes only Owner's first turn.
            RelicId::IceCream if self.players[owner].turn_number != 1 => Some(false),
            _ => None,
        }
    }

    pub(crate) fn relic_modify_hp_lost(
        &mut self, r: usize, owner: CId, amount: i32, target: CId, dealer: Option<CId>, props: Props,
    ) -> i32 {
        if !self.relic_enabled(r) { return amount; }
        match self.run.relics[r].id {
            // BeatingRemnant.cs ModifyHpLostAfterOsty: combat-in-progress owner cap.
            RelicId::BeatingRemnant if target == owner && !self.is_over => {
                amount.min((20 - self.run.relics[r].counter).max(0))
            }
            // TungstenRod.cs ModifyHpLostAfterOsty. hooks.rs invokes this after
            // Osty's interception and before the later HP-loss power modifiers.
            RelicId::TungstenRod if target == owner => (amount - 1).max(0),
            _ => amount,
        }
    }

    pub(crate) fn relic_modify_hp_lost_after_osty_late(
        &mut self, r: usize, amount: i32, target: CId, dealer: Option<CId>, props: Props,
    ) -> i32 {
        // Models/Relics/TheBoot.cs:26: AFTER block, Osty redirection and normal
        // HP-loss modifiers. Dealer is Owner.Creature or Owner.Osty, never an
        // unrelated party member; no card-source requirement.
        if self.run.relics[r].id != RelicId::TheBoot || !self.relic_enabled(r) {
            return amount;
        }
        let owner = self.run.relics[r].owner;
        if target != owner && props.is_powered_attack() && (1..5).contains(&amount)
            && dealer.is_some_and(|d| d == owner || self.osty(owner) == Some(d)) {
            5
        } else { amount }
    }

    pub(crate) fn relic_should_clear_block(&mut self, r: usize, owner: CId, creature: CId) -> Option<bool> {
        // SturdyClamp.cs ShouldClearBlock guards creature == Owner.Creature.
        if !self.relic_enabled(r) { return None; }
        match self.run.relics[r].id {
            RelicId::SturdyClamp if creature == owner => Some(false),
            _ => None,
        }
    }

    pub(crate) fn relic_after_preventing_block_clear(&mut self, r: usize, owner: CId, creature: CId) {
        // SturdyClamp.cs AfterPreventingBlockClear: only this preventer's Owner,
        // retain at most 10, never grant block to an owner with less.
        if self.relic_enabled(r) && self.run.relics[r].id == RelicId::SturdyClamp && creature == owner {
            self.cr_mut(owner).block = self.cr(owner).block.min(10);
        }
    }

    pub(crate) fn relic_should_flush(&mut self, r: usize, owner: CId) -> Option<bool> {
        if self.run.relics[r].owner != owner { return None; }
        if !self.relic_enabled(r) { return None; }
        match self.run.relics[r].id {
            // Models/Relics/RunicPyramid.cs:10: player == Owner (guard above).
            RelicId::RunicPyramid => Some(false),
            // Models/Relics/RingingTriangle.cs:15: owner's first turn only.
            RelicId::RingingTriangle if self.players[owner].turn_number <= 1 => Some(false),
            _ => None,
        }
    }

    pub(crate) fn relic_modify_power_amount_received(
        &mut self, r: usize, owner: CId, id: PowerId, amount: i32, target: CId,
    ) -> i32 {
        if !self.relic_enabled(r) { return amount; }
        if self.run.relics[r].id == RelicId::RuinedHelmet
            && target == owner && id == PowerId::Strength && amount > 0 && !self.run.relics[r].flag1
        {
            // RuinedHelmet.cs TryModifyPowerAmountReceived / AfterModifying...:
            // first positive Strength received by Owner only; reset AfterCombatEnd.
            self.run.relics[r].flag1 = true;
            amount * 2
        } else {
            amount
        }
    }

    pub(crate) fn fire_relic_after_death(&mut self, dead: CId) {
        for r in 0..self.run.relics.len() {
            // GremlinHorn.cs AfterDeath ignores wasRemovalPrevented and checks
            // target.Side != Owner.Creature.Side, not kill credit/card ownership.
            if self.cr(dead).side != self.cr(self.run.relics[r].owner).side {
                self.relic_after_death(r, self.run.relics[r].owner);
            }
        }
    }

    fn relic_after_death(&mut self, r: usize, owner: CId) {
        if self.run.relics[r].id == RelicId::GremlinHorn && self.relic_enabled(r)
            && self.players[owner].hooks_active {
            self.relic_gain_energy_for_owner(owner, 1);
            self.draw_cards(owner, 1, false);
        }
    }

    pub(crate) fn fire_relic_after_card_discarded(&mut self, k: KRef) {
        // Models/Relics/Tingsha.cs:18 and ToughBandages.cs:20:
        // the discarded card belongs to Owner, whose side must be current.
        let owner = self.cards[k as usize].owner;
        if self.cr(owner).side != self.side { return; }
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].owner != owner || !self.relic_enabled(r) { continue; }
            match self.run.relics[r].id {
                RelicId::Tingsha => {
                    // Tingsha uses RunState.Rng.CombatTargets over HittableEnemies.
                    if let Some(target) = self.relic_random_enemy() {
                        self.deal_damage(Some(owner), target, 3, Props::UNPOWERED);
                    }
                }
                RelicId::ToughBandages => self.relic_gain_block_for_owner(owner, 3),
                _ => {}
            }
        }
    }

    pub(crate) fn fire_relic_after_shuffle(&mut self, owner: CId) {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].owner != owner
                || !matches!(self.run.relics[r].id, RelicId::BiiigHug | RelicId::TheAbacus)
                || !self.relic_enabled(r) || !self.players[owner].hooks_active { continue; }
            match self.run.relics[r].id {
                RelicId::BiiigHug => {
                    // BiiigHug.AfterShuffle: AddGeneratedCardToCombat(soot, Draw, creator: Owner, Random).
                    let _ = self.create_card_in_pile_by_player(owner, CardId::Soot, false, PileKind::Draw, PilePos::Random);
                }
                // Models/Relics/TheAbacus.cs:21: only Owner's shuffle.
                RelicId::TheAbacus => self.relic_gain_block_for_owner(owner, 6),
                _ => {}
            }
        }
    }

    /// C# UnceasingTop.AfterHandEmptied: the caller has entered AutoPrePlay/Play.
    pub(crate) fn fire_relic_after_hand_emptied_for_player(&mut self, owner: CId) {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].enabled && self.run.relics[r].id == RelicId::UnceasingTop
                && self.run.relics[r].owner == owner && self.players[owner].hooks_active {
                // UnceasingTop.AfterHandEmptied awaits Draw and has no work
                // afterward. Preserve its batch across a shuffle selector.
                self.draw_cards_for_branch(owner, 1);
            }
        }
    }

    pub(crate) fn fire_relic_after_potion_used(&mut self, owner: CId, _potion: PotionId, _target: Option<CId>) {
        if self.is_over { return; }
        for r in 0..self.run.relics.len() {
            if !matches!(self.run.relics[r].id, RelicId::ReptileTrinket | RelicId::BeltBuckle)
                || !self.relic_enabled(r) {
                continue;
            }
            let relic_owner = self.run.relics[r].owner;
            match self.run.relics[r].id {
                // ReptileTrinket.cs AfterPotionUsed checks potion.Owner == Owner.
                RelicId::ReptileTrinket if relic_owner == owner => {
                    self.apply_relic_power(relic_owner, PowerId::ReptileTrinket, 3, Some(relic_owner));
                }
                // BeltBuckle.cs intentionally has NO potion-owner check. Every
                // listener inspects its OWN inventory after any potion event.
                RelicId::BeltBuckle
                    if !self.run.relics[r].flag1 =>
                {
                    self.relic_belt_buckle_inventory(r, relic_owner, false);
                }
                _ => {}
            }
        }
    }

    /// r7 relic-only potion procurement callback. The current combat engine has no generic
    /// hook enum entry for this lifecycle, so potion-producing paths call this helper directly.
    pub(crate) fn fire_relic_after_potion_procured(&mut self, _owner: CId) {
        if self.is_over { return; }
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::BeltBuckle
                && self.relic_enabled(r)
                && self.run.relics[r].flag1
            {
                self.relic_belt_buckle_inventory(r, self.run.relics[r].owner, true);
            }
        }
    }

    /// r7 relic-only potion discard callback; see the procurement helper above.
    pub(crate) fn fire_relic_after_potion_discarded(&mut self, _owner: CId) {
        if self.is_over { return; }
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].id == RelicId::BeltBuckle
                && self.relic_enabled(r)
                && !self.run.relics[r].flag1
            {
                self.relic_belt_buckle_inventory(r, self.run.relics[r].owner, false);
            }
        }
    }

    fn relic_belt_buckle_inventory(&mut self, r: usize, owner: CId, procured: bool) {
        let Some(potions) = self.relic_owner_potions(owner) else { return; };
        if potions.iter().any(Option::is_some) == procured && self.run.relics[r].flag1 == procured {
            // BeltBuckle.cs ApplyDexterity/RemoveDexterity: flag first; both
            // applier and cardSource are null, including the -2 removal.
            self.run.relics[r].flag1 = !procured;
            self.creature_apply_power(owner, PowerId::Dexterity, if procured { -2 } else { 2 }, None, None, false, false);
        }
    }

    /// r7 extra-turn value hook. The engine does not yet have an extra-turn scheduler.
    #[allow(dead_code)]
    pub(crate) fn relic_should_take_extra_turn(&mut self, r: usize, owner: CId) -> Option<bool> {
        // Hook.ShouldTakeExtraTurn uses CombatState.IsModelValidForHooks:
        // an inactive relic owner is not a listener, even in solo.
        if self.run.relics[r].owner != owner || !self.players[owner].hooks_active { return None; }
        if self.relic_enabled(r)
            && self.run.relics[r].id == RelicId::PaelsEye
            && !self.run.relics[r].flag1
            && !self.run.relics[r].flag2
            && self.run.relics[r].counter3 == 0
        {
            Some(true)
        } else {
            None
        }
    }

    /// r7 extra-turn completion hook paired with relic_should_take_extra_turn.
    #[allow(dead_code)]
    pub(crate) fn relic_after_taking_extra_turn(&mut self, r: usize, owner: CId) {
        if self.run.relics[r].owner != owner { return; }
        if self.relic_enabled(r) && self.run.relics[r].id == RelicId::PaelsEye {
            self.run.relics[r].flag1 = true;
        }
    }
}

impl Ev {
    #[inline]
    pub(crate) fn is_player_side(self) -> bool {
        matches!(self, Ev::Side(Side::Player))
    }
}
