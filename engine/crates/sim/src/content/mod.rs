//! Game content ported from r7: card effects (`cards/*.py`), monster moves (`monsters/*.py`)
//! and encounter setups (`encounters/*.py`). Anything not ported marks the combat unported.
//!
//! One file per content group, each with the same small interface, so groups are ported
//! independently. Dispatch tries each group in turn; a group returns `false` / `None` for ids
//! it does not own.

use smallvec::SmallVec;

mod cards_basic;
mod cards_colorless;
pub(crate) use cards_colorless::CARD_NAME_RANK;
mod cards_defect;
mod cards_regent;
mod cards_necro;
mod kit;
pub(crate) use kit::move_to_hand;
mod cards_ironclad_attacks;
mod cards_ironclad_skills;
pub(crate) mod exhaust_calls;
mod cards_silent;
mod monsters_act1;
mod battleworn_dummy;
mod monsters_act2;
pub(crate) mod monsters_act3;
mod monsters_underdocks;
mod potions;

use crate::card::{CId, KRef};
use crate::combat::*;
use crate::creature::{Creature, Side};
use crate::monster::{BranchCtx, MonsterAi};
use crate::rng::{deterministic_hash64, Rng};
use crate::tables::*;
use arrayvec::ArrayVec;

/// `order_combat_pile_selector_cards(draw, "draw")` for engine code outside `content`.
pub(crate) fn draw_selector_order_pub(c: &Combat, who: CId) -> Vec<KRef> {
    kit::draw_selector_order(c, who)
}

/// `count_card_plays_finished_this_turn(owner)` for engine code outside `content`.
pub(crate) fn plays_finished_pub(c: &Combat, who: crate::card::CId) -> i32 {
    kit::plays_finished(c, who)
}

pub const TOUGH_ASC: i32 = 8;
pub const DEADLY_ASC: i32 = 9;

/// r7 `ascension_value(asc, level, high, base)` style helper.
#[inline]
pub fn asc_value(asc: i32, threshold: i32, high: i32, base: i32) -> i32 {
    if asc >= threshold { high } else { base }
}

/// `encounter_generation_seed(run_seed, total_floor, model_entry)`.
pub fn encounter_seed(run_seed: u64, total_floor: i32, encounter_id: crate::identity::EncounterId) -> u64 {
    run_seed.wrapping_add(total_floor as u64).wrapping_add(deterministic_hash64(encounter_id.name()))
}

/// Index of a monster state by r7 state id.
pub fn state_index(def: &MonsterDef, id: &str) -> u8 {
    def.states.iter().position(|d| d.id == id).unwrap_or_else(|| panic!("no state {id}")) as u8
}

#[inline]
fn ai_kind_band(c: &Creature) -> (MonsterKind, usize) {
    let ai = c.ai.as_ref().expect("monster without AI");
    (ai.kind, ai.band as usize)
}

/// Callable branch weight (`RandomBranchState` weight callables): `None` = not this group's.
fn branch_weight(c: &Combat, e: CId, ai: &MonsterAi, target: u8) -> f64 {
    monsters_act1::weight(c, e, ai, target)
        .or_else(|| monsters_underdocks::weight(c, e, ai, target))
        .or_else(|| monsters_act2::weight(c, e, ai, target))
        .or_else(|| monsters_act3::weight(c, e, ai, target))
        .unwrap_or_else(|| panic!("callable weight not ported for {:?}", ai.kind))
}

/// `ConditionalBranchState`: index into `targets` of the first branch whose predicate holds.
fn branch_condition(c: &Combat, e: CId, ai: &MonsterAi, targets: &'static [u8]) -> u8 {
    monsters_act1::condition(c, e, ai, targets)
        .or_else(|| monsters_underdocks::condition(c, e, ai, targets))
        .or_else(|| monsters_act2::condition(c, e, ai, targets))
        .or_else(|| monsters_act3::condition(c, e, ai, targets))
        .unwrap_or_else(|| panic!("conditional branch not ported for {:?}", ai.kind))
}

impl Combat {
    /// r7 Bowlbug Rock's ImbalancedPower stores a private off-balance bit that its
    /// post-headbutt conditional branch consumes on the next move.
    pub(crate) fn note_bowlbug_rock_fully_blocked(&mut self, e: CId) {
        monsters_act2::act2_note_bowlbug_rock_fully_blocked(self, e);
    }

    /// `BowlbugRock.DizzyMove`: `IsOffBalance = false`.
    pub(crate) fn bowlbug_rock_regain_balance(&mut self, e: CId) {
        monsters_act2::bowlbug_rock_regain_balance(self, e);
    }

    pub(crate) fn spawn_infested_wrigglers(&mut self) {
        monsters_act2::spawn_infested_wrigglers(self);
    }

    pub(crate) fn escape_monster(&mut self, e: CId) {
        monsters_act3::escape_creature(self, e);
    }

    pub(crate) fn spawn_stock_axebot(&mut self, stock: i32) {
        monsters_act3::spawn_stock_axebot(self, stock);
    }

    pub(crate) fn spawn_doormaker(&mut self) -> CId {
        monsters_act3::spawn_doormaker(self)
    }

    // ───────────────────────────── cards ─────────────────────────────

    /// `registry.play_card_effect`.
    pub fn play_card_effect(&mut self, k: KRef, target: Option<CId>) {
        // AnyAlly dispatch must not unlock the old solo approximations. Only effects
        // checked against the C# and the original-DLL v4 fixtures may run here.
        if self.card_target_type(k) == TargetType::AnyAlly
            && !matches!(self.cards[k as usize].id,
                CardId::BelieveInYou | CardId::Blaze | CardId::Concoct
                | CardId::Constellation | CardId::DemonicShield | CardId::Ignition
                | CardId::Lift | CardId::Mimic | CardId::CoordinateCard | CardId::ImitationLearning
                | CardId::InterceptCard | CardId::Tutor | CardId::Fade | CardId::Largesse | CardId::Soulbound)
        {
            self.fault_card = Some(self.cards[k as usize].id);
            self.unported("unverified ally card effect");
            return;
        }
        if cards_basic::play(self, k, target)
            || cards_colorless::play(self, k, target)
            || cards_ironclad_attacks::play(self, k, target)
            || cards_ironclad_skills::play(self, k, target)
            || cards_silent::play(self, k, target)
            || cards_defect::play(self, k, target)
            || cards_regent::play(self, k, target)
            || cards_necro::play(self, k, target)
        {
            return;
        }
        let c = &self.cards[k as usize];
        if !(c.is_unplayable() || c.is_status()) {
            if self.fault.is_none() {
                self.fault_card = Some(c.id);
            }
            self.unported("card effect");
        }
    }

    /// `unique_cards_in_piles(all_piles)` (hand, draw, discard, exhaust, play) that register
    /// `hook`. The arena-wide mask skips the scan when no combat card has the hook at all.
    fn card_listeners(&self, hook: CardHook) -> SmallVec<[KRef; 16]> {
        let bit = 1u32 << hook as u32;
        let mut out = SmallVec::new();
        if self.card_hook_mask & bit == 0 {
            return out;
        }
        for owner in 0..self.players.len() as CId {
        if !self.players[owner].hooks_active { continue; }
        for pile in [&self.players[owner].hand[..], &self.players[owner].draw[..], &self.players[owner].discard[..], &self.players[owner].exhaust[..], &self.players[owner].play[..]] {
            for &k in pile {
                let id = self.cards[k as usize].id;
                if (CARD_HOOKS[id.index()] & !crate::card::auto_post_play_legacy_hooks(id)) & bit != 0 && !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        }
        out
    }

    /// `fire_card_after_card_drawn` for every listener card (after power/relic listeners).
    pub(crate) fn card_listener_valid(&self, k: KRef) -> bool {
        let owner = self.cards[k as usize].owner;
        // C# CombatState.Contains/IsModelValidForHooks, checked at callback time.
        self.players[owner].hooks_active && self.pile_cards(owner).contains(&k)
    }

    pub fn fire_card_after_card_drawn(&mut self, drawn: KRef) {
        for l in self.card_listeners(CardHook::AfterCardDrawnHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_ironclad_skills::on_after_card_drawn(self, l, drawn);
            cards_regent::on_after_card_drawn(self, l, drawn);
        }
    }

    /// C# Hook.AfterAutoPostPlayPhaseEntered card callback, awaited by TurnEndWalk.
    pub(crate) fn fire_card_after_auto_post_play(&mut self, listener: KRef, owner: CId) {
        cards_regent::on_after_auto_post_play(self, listener, owner);
        cards_ironclad_attacks::on_after_auto_post_play(self, listener, owner);
    }

    /// `_fire_after_card_played_late(played)` (core/combat.py:4258-4267): late effects of every
    /// card in the piles, after the enchantment hooks.
    pub fn fire_card_late_effects(&mut self, played: KRef) {
        for l in self.card_listeners(CardHook::LateEffects) {
            if !self.card_listener_valid(l) { continue; }
            cards_regent::on_late_effect(self, l, played);
            cards_necro::late_effect(self, l, played);
        }
    }

    pub(crate) fn fire_card_after_death(&mut self, prevented: bool) {
        for l in self.card_listeners(CardHook::AfterDeathHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_necro::after_death(self, l, prevented);
        }
    }

    pub(crate) fn fire_card_after_attack(&mut self, attacker: Option<CId>) {
        for l in self.card_listeners(CardHook::AfterAttackHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_necro::after_attack(self, l, attacker);
        }
    }

    /// `fire_card_after_card_exhausted` for every listener card (before power/relic listeners).
    pub fn fire_card_after_card_exhausted(&mut self, exhausted: KRef) {
        for l in self.card_listeners(CardHook::AfterCardExhaustedHooks) {
            if !self.card_listener_valid(l) { continue; }
            // C# Midnight.AfterCardExhausted: every player's exhaust reduces this combat's cost.
            if self.cards[l as usize].id == CardId::Midnight {
                self.cards[l as usize].add_energy_cost(-1, crate::card::CostExpiry::Combat);
            }
            cards_ironclad_skills::on_after_card_exhausted(self, l, exhausted);
        }
    }

    /// Card-level `_apply_card_after_card_entered_combat` listeners, after power/relic hooks.
    pub fn fire_card_after_card_entered_combat(&mut self, entered: KRef) {
        for l in self.card_listeners(CardHook::AfterCardEnteredCombatHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_necro::after_entered(self, l, entered);
            if self.cards[entered as usize].cv.is_clone {
                continue;
            }
            // C# Midnight.AfterCardEnteredCombat: initialize from global exhaust history.
            if l == entered && self.cards[l as usize].id == CardId::Midnight {
                self.cards[l as usize].add_energy_cost(-self.hist.exhausted_combat, crate::card::CostExpiry::Combat);
            }
            cards_ironclad_attacks::on_after_card_entered_combat(self, l, entered);
            cards_silent::on_after_card_entered_combat(self, l, entered);
        }
    }

    /// `_apply_card_after_card_played` (core/combat.py:1453-1458): every listener card in the
    /// owner's piles, after the power/relic `after_card_played` listeners.
    pub fn fire_card_after_card_played(&mut self, played: KRef) {
        for l in self.card_listeners(CardHook::AfterCardPlayedHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_silent::on_after_card_played(self, l, played);
            cards_necro::after_played(self, l, played);
        }
    }

    /// Card-level `fire_card_after_card_generated_for_combat` listeners (after powers/relics).
    pub fn fire_card_after_card_generated(&mut self, generated: KRef, _by_player: bool) {
        for l in self.card_listeners(CardHook::AfterCardGeneratedForCombatHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_defect::on_after_card_generated(self, l, generated);
        }
    }

    /// `CardInstance.target_type_for(owner)` (cards/registry.py card_target_type_for).
    pub fn card_target_type(&self, k: KRef) -> TargetType {
        let card = &self.cards[k as usize];
        // C# MadScience.TargetType: AnyEnemy only as an attack.
        if card.id == CardId::MadScience && card.card_type != CardType::Attack {
            return TargetType::Self_;
        }
        cards_silent::target_type(self, k)
            .or_else(|| cards_regent::target_type(self, k))
            .unwrap_or(card.target)
    }

    /// `CardInstance.is_playable_by_card_logic` (cards/registry.py is_card_playable).
    pub fn card_playable_by_logic(&mut self, k: KRef) -> bool {
        match self.card_playability_readonly(k) {
            Ok(value) => value,
            Err(reason) => { self.unported(reason); true }
        }
    }

    pub fn card_playability_readonly(&self, k: KRef) -> Result<bool, &'static str> {
        let card = &self.cards[k as usize];
        if !card_info(card.id, card.upgraded).is_some_and(|i| i.custom_playability) {
            return Ok(true);
        }
        if let Some(playable) = cards_silent::is_playable(self, k) {
            return Ok(playable);
        }
        if self.cards[k as usize].id == CardId::HighFive {
            let who = self.cards[k as usize].owner;
            return Ok(self.osty(who).is_some_and(|o| self.cr(o).is_alive()));
        }
        // C# Clash.IsPlayable: every card in hand is an attack.
        if self.cards[k as usize].id == CardId::Clash {
            return Ok(self.players[PLAYER].hand.iter().all(|&q| self.cards[q as usize].card_type == CardType::Attack));
        }
        Err("card playability hook")
    }

    /// Card-level `_apply_card_before_card_played` listeners, before power/relic hooks.
    pub fn fire_card_before_card_played(&mut self, played: KRef) {
        for l in self.card_listeners(CardHook::BeforeCardPlayedHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_ironclad_attacks::on_before_card_played(self, l, played);
        }
    }

    /// Card-level `before_hand_draw` listeners over all piles.
    pub fn fire_card_before_hand_draw(&mut self, owner: CId) {
        // fire: core/combat.py:1291-1301
        for l in self.card_listeners(CardHook::BeforeHandDrawHooks) {
            if !self.card_listener_valid(l) { continue; }
            cards_regent::on_before_hand_draw(self, l, owner);
            if self.is_over {
                return;
            }
        }
    }

    /// `CardInstance.allows_hand_card_play`: only listener cards currently in hand participate.
    pub fn cards_allow_hand_card_play(&self, played: KRef, is_auto_play: bool) -> bool {
        // fire: core/combat.py:1477-1482
        let bit = 1u32 << CardHook::ShouldPlayHooks as u32;
        if self.card_hook_mask & bit == 0 {
            return true;
        }
        // Enthralled.ShouldPlay only restricts the card's own player, while the
        // listener remains in that owner's hand (autoplays are always allowed).
        let owner = self.cards[played as usize].owner;
        let hand: ArrayVec<KRef, MAX_HAND_SIZE> = self.players[owner].hand.iter().copied().collect();
        for l in hand {
            if CARD_HOOKS[self.cards[l as usize].id.index()] & bit != 0
                && cards_ironclad_skills::on_should_play(self, l, played, is_auto_play) == Some(false)
            {
                return false;
            }
        }
        true
    }

    /// Run-deck card `after_combat_end` hooks. The deck is mutated in original order.
    pub fn fire_card_after_combat_end(&mut self) {
        // C# Guilty.AfterCombatEnd: every owner's persistent deck participates.
        let bit = 1u32 << CardHook::AfterCombatEndHooks as u32;
        for owner in 0..self.players.len() as CId {
            let mut i = 0;
            while i < self.player_deck(owner).len() {
                let id = self.player_deck(owner)[i].id;
                if CARD_HOOKS[id.index()] & bit != 0
                    && !cards_ironclad_skills::on_after_combat_end_deck(&mut self.player_deck_mut(owner)[i])
                {
                    self.remove_player_deck_card(owner,i);
                } else {
                    i += 1;
                }
            }
        }
    }

    /// `fire_card_turn_end_in_hand(card, combat, cards_in_hand_at_turn_end)`.
    pub fn card_turn_end_in_hand(&mut self, k: KRef, cards_in_hand: i32) {
        cards_ironclad_skills::on_turn_end_in_hand(self, k, cards_in_hand);
    }

    /// Continue the effect that asked for a card choice (r7 resolver closure).
    pub fn resolve_choice_effect(&mut self, r: crate::choice::Resolver, selected: &[KRef]) {
        if cards_basic::resolve(self, r, selected)
            || cards_colorless::resolve(self, r, selected)
            // fire: cards/ironclad.py:426-430
            || cards_ironclad_attacks::resolve(self, r, selected)
            || cards_ironclad_skills::resolve(self, r, selected)
            || cards_silent::resolve(self, r, selected)
            || cards_defect::resolve(self, r, selected)
            || cards_regent::resolve(self, r, selected)
            || cards_necro::resolve(self, r, selected)
            || potions::resolve(self, r, selected)
            // relic fire: relics/rare.py:327
            || self.resolve_relic_choice(r, selected)
            || monsters_act2::resolve(self, r, selected)
        {
            return;
        }
        self.unported("choice resolver");
    }

    // relic fire: relics/shop_event.py:1438
    pub(crate) fn fill_empty_potion_slots_for_relic(&mut self, owner: CId) {
        potions::fill_empty_potion_slots_out_of_combat_pool(self, owner);
    }

    pub(crate) fn gain_max_hp_for_relic(&mut self, owner: CId, amount: i32) {
        potions::gain_max_hp(self, owner, amount);
    }

    /// `potion.use(combat, user, target)` -> `get_potion_effect(id)`.
    pub fn potion_effect(&mut self, id: PotionId, target: Option<CId>) {
        self.potion_effect_for_player(crate::combat::PLAYER, id, target);
    }

    /// Explicit potion owner, independent of the targeted/choosing player.
    pub fn potion_effect_for_player(&mut self, owner: CId, id: PotionId, target: Option<CId>) {
        if !potions::use_effect(self, owner, id, target) {
            self.unported("potion effect");
        }
    }

    /// A monster-added card (statuses) for player `owner`:
    /// `add_generated_card_to_creature_<pile>(owner, ..., added_by_player=False)`.
    pub fn create_card_in_pile(&mut self, owner: CId, id: CardId, upgraded: bool, pile: PileKind, pos: PilePos) -> Option<KRef> {
        let k = self.new_card(owner, id, upgraded)?;
        self.add_generated_card(k, pile, pos, false);
        Some(k)
    }

    /// A player-added card: `add_generated_card_to_creature_<pile>(owner, ..., added_by_player=True)`.
    pub fn create_card_in_pile_by_player(&mut self, owner: CId, id: CardId, upgraded: bool, pile: PileKind, pos: PilePos) -> Option<KRef> {
        let k = self.new_card(owner, id, upgraded)?;
        self.add_generated_card(k, pile, pos, true);
        Some(k)
    }

    // ───────────────────────────── monsters ─────────────────────────────

    /// Roll the next move with combat-aware branch callbacks.
    pub fn roll_enemy_move(&mut self, e: CId) {
        let mut ai = self.cr_mut(e).ai.take().expect("enemy without AI");
        let mut rng = self.run.rngs.monster_ai.clone();
        {
            let this = &*self;
            let weight = |ai: &MonsterAi, t: u8| branch_weight(this, e, ai, t);
            let condition = |ai: &MonsterAi, targets: &'static [u8]| branch_condition(this, e, ai, targets);
            ai.roll_move(&mut rng, &BranchCtx { weight: &weight, condition: &condition });
        }
        self.run.rngs.monster_ai = rng;
        self.cr_mut(e).ai = Some(ai);
    }

    /// Move effects (`MoveState.effect_fn`).
    pub fn monster_move_effect(&mut self, e: CId, mv: u8) {
        if mv == crate::monster::ILLUSION_REVIVE {
            self.power_illusion_revive(e);
            return;
        }
        let ai = self.cr(e).ai.as_ref().unwrap();
        let (kind, state) = (ai.kind, ai.state_id(mv));
        if battleworn_dummy::move_effect(self, e, kind, state)
            || monsters_act1::move_effect(self, e, kind, state)
            || monsters_underdocks::move_effect(self, e, kind, state)
            || monsters_act2::move_effect(self, e, kind, state)
            || monsters_act3::move_effect(self, e, kind, state)
        {
            return;
        }
        self.unported("monster move");
    }

    /// Create a monster from its factory (`create_*`): ranged HP model + state machine.
    /// `flags`: factory arguments read by the monster's own code (meaning is per monster).
    pub fn create_monster(&mut self, kind: MonsterKind, flags: u32) -> (Creature, MonsterAi) {
        let band: u8 = if self.run.ascension >= DEADLY_ASC {1}
            else if self.run.ascension >= TOUGH_ASC {2}else{0};
        let def = monster_def(kind, band as usize);
        let initial = monsters_act1::initial_state(kind, flags, def)
            .or_else(|| monsters_underdocks::initial_state(kind, flags, def))
            .or_else(|| monsters_act2::initial_state(kind, flags, def))
            .or_else(|| monsters_act3::initial_state(kind, flags, def))
            .unwrap_or(def.initial);
        let mut creature = Creature::player(def.hp.0, def.hp.0);
        creature.is_player = false;
        creature.side = Side::Enemy;
        creature.monster_id = def.monster_id;
        creature.hp_range = Some(def.hp);
        let mut ai = MonsterAi {
            kind, band, current: initial, log: ArrayVec::new(), used: 0, performed_first: false, next_unset: false,
            performed_once: 0, stun_follow_up: 0, stun_move: Default::default(), revive_follow_up: 0, revive_performed_once: false,
            flags, rng: Rng::new(0),
            rolled_stun_follow_up: false,
        };
        // C# MonsterMoveStateMachine constructor logs an initial visible move.
        ai.log_selected_move(initial);
        (creature, ai)
    }

    /// `configure_encounter_slots`: slot names in screen order for slotted encounters.
    pub fn configure_encounter_slots(&mut self, slots: &'static [&'static str]) {
        self.encounter_slots = slots;
    }

    /// `_next_encounter_slot_name`: first configured slot no current enemy occupies.
    fn next_encounter_slot(&self) -> Option<&'static str> {
        self.encounter_slots.iter().copied().find(|s| !self.enemies.iter().any(|&e| self.cr(e).slot == Some(*s)))
    }

    /// `_prepare_enemy_for_combat` + `add_enemy` (no explicit slot).
    pub fn add_enemy(&mut self, creature: Creature, ai: MonsterAi) -> CId {
        self.add_enemy_in_slot(creature, ai, None)
    }

    /// `add_enemy(creature, ai, slot_name=...)`.
    pub fn add_enemy_in_slot(&mut self, creature: Creature, ai: MonsterAi, slot: Option<&'static str>) -> CId {
        let c = self.prepare_enemy_in_slot(creature, ai, slot);
        self.insert_prepared_enemy(c);
        c
    }

    /// `_prepare_enemy_for_combat` without `add_enemy`; Surprise needs this split because r7
    /// prepares Fat Gremlin before it creates/adds Sneaky Gremlin.
    pub(crate) fn prepare_enemy_in_slot(&mut self, mut creature: Creature, mut ai: MonsterAi, slot: Option<&'static str>) -> CId {
        // Original Combat/CombatState.cs:125-152: AddPlayer and
        // CreateCreature both AttachCreature, consuming the same global id.
        // Native creature storage is append-only, including pets and escaped
        // creatures, so its next index is that attachment id, not enemy ordinal.
        creature.combat_id = self.creatures.len() as CId;
        self.next_enemy_combat_id += 1;
        let (min_hp, max_hp) = creature.hp_range.unwrap_or((creature.max_hp, creature.max_hp));
        let used: ArrayVec<i32, 8> = self.enemies.iter().map(|&e| self.cr(e).max_hp).collect();
        // Setup-time only (once per enemy), so a plain Vec is fine here.
        let available: Vec<i32> = (min_hp..=max_hp).filter(|hp| !used.contains(hp)).collect();
        let base_hp = if !available.is_empty() {
            available[self.run.rngs.niche.next_int(0, available.len() as i64 - 1) as usize]
        } else {
            self.run.rngs.niche.next_int(min_hp as i64, max_hp as i64) as i32
        };
        // CombatState.AddCreature: SetUniqueMonsterHpValue, then ScaleMonsterHpForMultiplayer.
        let hp = self.scale_monster_hp(base_hp);
        creature.max_hp = hp;
        creature.hp = hp;
        let (col, row) = self.run.coord.unwrap_or((0, 0));
        let local_seed = self.run.rngs.seed
            .wrapping_add(col as u64).wrapping_add(row as u64)
            .wrapping_add(self.run.act_index as u64).wrapping_add(creature.combat_id as u64);
        ai.rng = Rng::new(local_seed);
        creature.slot = slot.or_else(|| self.next_encounter_slot());
        creature.ai = Some(ai);
        let starting = monster_def(ai_kind_band(&creature).0, ai_kind_band(&creature).1).powers;
        let c = self.creatures.len() as CId;
        self.creatures.push(creature);
        if !self.combat_started {
            self.relic_initial_enemy_room_entered(c);
        }
        // Original Inklet.cs:59-63 (and other AfterAddedToRoom overrides)
        // applies these after the room-entry checkpoint. Normal native play
        // still applies them eagerly; the read-only pre-hook view does not.
        if self.replay_room_entry && !self.combat_started { return c; }
        // Factory `creature.apply_power(...)` runs before the creature joins a combat, so r7
        // stacks these without any hook (core/creature.py apply_power with combat_state None).
        for &(id, amount) in starting {
            if !crate::powers::is_ported(id) {
                if self.fault.is_none() {
                    self.fault_power = Some(id);
                }
                self.unported("monster starting power");
            }
            if amount == 0 {
                continue;
            }
            let primary = self.find_power(c, id);
            if let Some(p) = primary.filter(|_| !power_meta(id).instanced) {
                self.powers[p as usize].amount += amount;
            } else {
                // C# monsters apply these in AfterAddedToRoom through PowerCmd.Apply (scaled).
                // GremlinMerc.AfterAddedToRoom applies one Thievery per player, Target = that player.
                let targets: ArrayVec<Option<CId>, { crate::player::MAX_PLAYERS }> = if id == PowerId::Thievery {
                    (0..self.players.len() as CId).map(Some).collect()
                } else {
                    [None].into_iter().collect()
                };
                let amount = self.scale_power_for_multiplayer(id, amount, c);
                for (i, target) in targets.into_iter().enumerate() {
                    let p = self.powers.len() as crate::creature::PRef;
                    self.powers.push(crate::creature::Power {
                        id, amount, payload: 0, skip_next_tick: false, applier: None, ignore_next_instance: false, owner: c, vars: [0; 4], target,
                    });
                    self.creatures[c as usize].attach_power(p, id, primary.is_some() || i > 0);
                }
            }
        }
        c
    }

    pub(crate) fn insert_prepared_enemy(&mut self, c: CId) {
        self.enemies.push(c);
        self.sort_enemies_by_slot();
        // r7 core/combat.py:1006-1023 (DLL CombatManager.AfterCreatureAdded).
        if self.combat_started && self.side == Side::Enemy {
            // Enemy-side summons keep NextMove unset until end-of-enemy-turn preparation.
            let Some(ai) = self.cr_mut(c).ai.as_mut() else {
                self.unported("insert prepared enemy missing AI");
                return;
            };
            ai.next_unset = true;
        }
        if self.combat_started {
            self.record_creature_lifecycle(c,"spawn",crate::between::PublicState::of(self.cr(c)));
        }
        match self.cr(c).monster_id {
            "GAS_BOMB" => {
                self.apply_power_to(c, PowerId::Minion, 1, Some(c));
            }
            "ZAPBOT" => {
                self.apply_power_to(c, PowerId::HighVoltage, 2, Some(c));
            }
            _ => {}
        }
        if self.combat_started {
            self.fire(Hook::AfterCreatureAddedToCombat, crate::hooks::Ev::Creature(c), None);
            if self.side == Side::Player {
                self.roll_enemy_move(c);
            }
        }
    }

    /// Encounter setups by r7 encounter id (`setup_<id lowercase>`).
    pub fn setup_encounter_id(&mut self, encounter_id: crate::identity::EncounterId, rng: &mut Rng) {
        if battleworn_dummy::setup(self, encounter_id, rng)
            || monsters_act1::setup_encounter(self, encounter_id, rng)
            || monsters_underdocks::setup_encounter(self, encounter_id, rng)
            || monsters_act2::setup_encounter(self, encounter_id, rng)
            || monsters_act3::setup_encounter(self, encounter_id, rng)
        {
            return;
        }
        self.unported("encounter");
    }

    /// Compatibility/test ingestion boundary. Gameplay state carries EncounterId.
    pub fn setup_encounter(&mut self, encounter_id: &str, rng: &mut Rng) {
        let Some(id) = crate::identity::EncounterId::from_name(encounter_id) else {
            self.unported("encounter");
            return;
        };
        self.setup_encounter_id(id, rng);
    }
}
