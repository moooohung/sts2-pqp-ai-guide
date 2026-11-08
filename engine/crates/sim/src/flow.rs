//! r7 combat.py turn flow, draw/shuffle and the card-play pipeline.

use crate::card::{CId, KRef};
use crate::combat::*;
use crate::creature::Side;
use crate::hooks::Ev;
use crate::tables::*;

/// A parent can suspend after its child has already queued work. Physical
/// insertion order therefore is not the awaited call-stack order.
pub(crate) fn take_suspended_at_depth<T>(stack:&mut Vec<T>,depth:usize,depth_of:impl Fn(&T)->usize)->Option<T> {
    let index=stack.iter().rposition(|work|depth_of(work)==depth)?;
    Some(stack.remove(index))
}

#[cfg(test)]
mod well_laid_plans_tests;
#[cfg(test)]
mod relic_flush_review_a_tests;

/// `_sort_cards_like_dotnet_list_sort`: .NET IntrospectiveSort by
/// `(reference_card_entry, upgraded)` (unstable; the exact algorithm decides tie order).
pub fn dotnet_sort_cards(items: &mut [KRef], cards: &[crate::card::Card]) {
    let key = |k: KRef| {
        let c = &cards[k as usize];
        (card_info(c.id, false).or(card_info(c.id, true)).map_or("", |i| i.entry), c.upgraded as u8)
    };
    let less = |a: KRef, b: KRef| key(a) < key(b);
    dotnet_sort_indices(items, less);
}

/// Shared .NET List.Sort implementation for index-based source comparers.
pub(crate) fn dotnet_sort_indices(items: &mut [KRef], less: impl Fn(KRef, KRef) -> bool) {
    let n = items.len();
    if n > 1 {
        let depth = 2 * (usize::BITS - n.leading_zeros()) as i32;
        intro_sort(items, 0, n, depth, &less);
    }
}

fn swap_if_greater(v: &mut [KRef], i: usize, j: usize, less: &impl Fn(KRef, KRef) -> bool) {
    if less(v[j], v[i]) {
        v.swap(i, j);
    }
}

fn insertion_sort(v: &mut [KRef], start: usize, count: usize, less: &impl Fn(KRef, KRef) -> bool) {
    for offset in 0..count.saturating_sub(1) {
        let value = v[start + offset + 1];
        let mut index = (start + offset) as isize;
        while index >= start as isize && less(value, v[index as usize]) {
            v[index as usize + 1] = v[index as usize];
            index -= 1;
        }
        v[(index + 1) as usize] = value;
    }
}

fn down_heap(v: &mut [KRef], start: usize, mut index: usize, count: usize, less: &impl Fn(KRef, KRef) -> bool) {
    let value = v[start + index - 1];
    while index <= count >> 1 {
        let mut child = 2 * index;
        if child < count && less(v[start + child - 1], v[start + child]) {
            child += 1;
        }
        if !less(value, v[start + child - 1]) {
            break;
        }
        v[start + index - 1] = v[start + child - 1];
        index = child;
    }
    v[start + index - 1] = value;
}

fn heap_sort(v: &mut [KRef], start: usize, count: usize, less: &impl Fn(KRef, KRef) -> bool) {
    for index in (1..=count >> 1).rev() {
        down_heap(v, start, index, count, less);
    }
    for remaining in (2..=count).rev() {
        v.swap(start, start + remaining - 1);
        down_heap(v, start, 1, remaining - 1, less);
    }
}

fn partition(v: &mut [KRef], start: usize, count: usize, less: &impl Fn(KRef, KRef) -> bool) -> usize {
    let last = start + count - 1;
    let middle = start + ((count - 1) >> 1);
    swap_if_greater(v, start, middle, less);
    swap_if_greater(v, start, last, less);
    swap_if_greater(v, middle, last, less);
    let pivot = v[middle];
    let pivot_slot = last - 1;
    v.swap(middle, pivot_slot);
    let (mut left, mut right) = (start, pivot_slot);
    while left < right {
        left += 1;
        while less(v[left], pivot) {
            left += 1;
        }
        right -= 1;
        while less(pivot, v[right]) {
            right -= 1;
        }
        if left >= right {
            break;
        }
        v.swap(left, right);
    }
    if left != pivot_slot {
        v.swap(left, pivot_slot);
    }
    left
}

fn intro_sort(v: &mut [KRef], start: usize, mut count: usize, mut depth: i32, less: &impl Fn(KRef, KRef) -> bool) {
    while count > 1 {
        if count <= 16 {
            match count {
                2 => swap_if_greater(v, start, start + 1, less),
                3 => {
                    swap_if_greater(v, start, start + 1, less);
                    swap_if_greater(v, start, start + 2, less);
                    swap_if_greater(v, start + 1, start + 2, less);
                }
                _ => insertion_sort(v, start, count, less),
            }
            return;
        }
        if depth == 0 {
            heap_sort(v, start, count, less);
            return;
        }
        depth -= 1;
        let pivot = partition(v, start, count, less);
        let right_start = pivot + 1;
        intro_sort(v, right_start, start + count - right_start, depth, less);
        count = pivot - start;
    }
}

impl Combat {
    // ───────────────────────────── combat start ─────────────────────────────

    /// `CombatState.start_combat`.
    pub fn start_combat(&mut self) {
        self.prepare_combat_start();
        // fire: core/combat.py:1092
        crate::card::enchant::move_initial_imbued_to_back(self);
        self.fire_before_combat_start();
        self.start_player_turn();
    }

    /// Common initialization prefix, also used on an offline copy by the
    /// immutable room-entry replay view. No original opening-turn effects run.
    pub(crate) fn prepare_combat_start(&mut self) {
        self.combat_started = true;
        self.side = Side::Player;
        self.round = 1;
        self.hist.damage.clear();
        self.hist.reset_side_turn();
        for owner in 0..self.players.len() as CId {
            self.reset_player_combat_state(owner);
        }
    }

    /// `_reset_player_combat_state(owner)`: empty piles, the deck shuffled into the draw pile.
    fn reset_player_combat_state(&mut self, owner: CId) {
        let p = &mut self.players[owner];
        p.energy = 0;
        p.stars = 0;
        p.hand.clear();
        self.creatures[owner as usize].stars = 0;
        for i in 0..self.players[owner].starting_deck.len() {
            let k = self.players[owner].starting_deck[i];
            self.cards[k as usize].owner = owner;
            // fire: core/combat.py:1086
            crate::card::enchant::reset_combat(&mut self.cards[k as usize]);
            if crate::card::has_unwired_card_hook(self.cards[k as usize].id) {
                if self.fault.is_none() {
                    self.fault_card = Some(self.cards[k as usize].id);
                }
                self.unported("card hook");
            }
        }
        let p = &mut self.players[owner];
        p.draw.clear();
        p.draw.extend_from_slice(&p.starting_deck);
        p.discard.clear();
        p.exhaust.clear();
        p.play.clear();
        p.draw_known = Default::default();
        self.run.rngs.shuffle.shuffle(&mut self.players[owner].draw);
    }

    // ───────────────────────────── player turn ─────────────────────────────

    /// `_start_player_turn` + `_continue_player_turn_setup` (no pending choices are ported yet).
    pub fn start_player_turn(&mut self) {
        self.between_stage("player_start", None);
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.action_queue.phase("player_turn",self.round);}
        self.side = Side::Player;
        self.player_turn_phase_one_done = false;
        self.auto_begin_enemy_turn = false;
        self.players_ready_to_begin_enemy_turn = 0;
        // CombatManager.StartTurn clears both readiness sets, then immediately
        // SetReadyToEndTurn's dead/non-participating seats.  Treat a player who
        // died during the previous side the same way here; extra turns only
        // leave their selected living seats as participants.
        let all_players = self.all_player_mask();
        self.players_ready_to_end_turn = all_players & !self.player_turn_participants();
        self.setup_auto_pre_play_done.fill(false);
        for owner in 0..self.players.len() as CId {
            if !self.is_player_taking_turn(owner) { self.setup_auto_pre_play_done[owner as usize] = true; }
        }
        self.turn_count += 1;
        // Creature.BeforeTurnStart snapshots AmountOnTurnStart BEFORE turn hooks
        // and hand draw. Invoke autoplays during that draw must wait until next turn.
        for power in &mut self.powers {
            if power.id == PowerId::SummonNextTurn
                && (self.extra_turn_players == 0 || self.extra_turn_players & (1 << power.owner) != 0)
            { power.vars[0] = power.amount; }
        }
        // fire: core/combat.py:1168
        for (index, p) in self.players.0.iter_mut().enumerate() {
            // CombatManager.SwitchSides increments every listed player's turn,
            // including dead allies (CreatureCmd.Kill keeps them in Players).
            if self.extra_turn_players == 0 || self.extra_turn_players & (1 << index) != 0 {
                p.turn_number += 1;
            }
        }
        // CombatManager.SwitchSides increments the selected TurnNumbers before
        // SwitchFromPlayerToEnemySide dispatches AfterTakingExtraTurn.
        if self.extra_turn_players != 0 { self.fire_after_taking_extra_turn_now(); }
        // StarsModifiedEntry uses round/side history: gains during combat
        // entry belong to round 1 Player just like its first StartTurn.
        let entry_stars=if self.turn_count==1 {self.hist.stars_gained_this_turn}
            else{[0;crate::player::MAX_PLAYERS]};
        self.hist.reset_side_turn();
        self.hist.stars_gained_this_turn=entry_stars;
        self.hist.played_this_turn.clear();
        self.hist.energy_spent_this_turn.fill(0);
        self.fire(Hook::BeforeSideTurnStart, Ev::Side(Side::Player), None);
        // CombatManager.StartTurn -> PrepareForNextTurn happens here, AFTER
        // the previous side's Doom/death callbacks, and not on extra turns.
        if self.extra_turn_players==0 {
            let enemies:Vec<_>=self.enemies.iter().copied().filter(|&e|!self.cr(e).escaped).collect();
            for e in enemies {self.roll_enemy_move(e);}
        }
        self.continue_player_turn_setup(TurnStage::Block);
    }

    /// r7 `_continue_player_turn_setup(stage)` / C# `StartTurn`: block clears, then every
    /// player's `SetupPlayerTurn` in `Players` order, then the side-level steps. A choice opened
    /// by a stage pauses the setup; `resolve_pending_choice` resumes it at the next stage
    /// (`pending_turn_setup`) for the same `setup_player`.
    pub(crate) fn continue_player_turn_setup(&mut self, mut stage: TurnStage) {
        if stage == TurnStage::Block {
            for owner in 0..self.players.len() as CId {
                if !self.is_player_taking_turn(owner) { continue; }
                // Creature.AfterTurnStart skips clear only on this player's first
                // turn; an extra turn can advance TurnNumber while round stays 1.
                if self.players[owner].turn_number != 1 {
                    self.clear_block(owner);
                }
                self.fire(Hook::AfterBlockCleared, Ev::Creature(owner), None);
            }
            self.setup_player = 0;
            stage = TurnStage::Energy;
        }
        while (self.setup_player as usize) < self.players.len() {
            if !self.is_player_taking_turn(self.setup_player) {
                self.setup_player += 1;
                stage = TurnStage::Energy;
                continue;
            }
            self.collect_setup_choices = self.run.choice_scheduling.is_live();
            let finished = self.setup_player_turn(stage);
            self.collect_setup_choices = false;
            if !finished {
                return;
            }
            self.setup_player += 1;
            stage = TurnStage::Energy;
        }
        // After the per-player stages r7 only returns on a choice (no resume point).
        self.fire_after_side_turn_start(Side::Player);
        if self.pending_choice.is_some() || self.is_over {
            return;
        }
        for owner in 0..self.players.len() as CId {
            if !self.is_player_taking_turn(owner) { continue; }
            if self.players[owner].orbs.capacity > 0 {
                // fire: core/combat.py:1263-1269 (AfterTurnStartOrbTrigger: Plasma)
                self.trigger_orb_passives(owner, true);
                self.check_combat_end();
                if self.pending_choice.is_some() || self.is_over {
                    return;
                }
            }
        }
        self.continue_auto_pre_play(0, false);
    }

    /// Assign paused setup tasks after all seats' initial setup, side-start hooks
    /// and orbs. Only now does FromHand enumerate its candidate list.
    fn continue_auto_pre_play(&mut self, next_player: CId, mut before_play: bool) {
        if self.run.choice_scheduling.is_live() {
            if before_play && (next_player as usize) < self.players.len() {
                // Resume AFTER the awaited auto-pre-play callback, not by
                // invoking HistoryCourse/Imbued a second time.
                self.setup_auto_pre_play_done[next_player as usize] = true;
                self.fire(Hook::BeforePlayPhaseStart, Ev::Creature(next_player), None);
                if self.pending_choice.is_some() {
                    self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: next_player + 1, before_play: false });
                    return;
                }
            }
            self.continue_live_auto_pre_play();
            return;
        }
        for owner in next_player..self.players.len() as CId {
            if self.setup_auto_pre_play_done[owner as usize] { continue; }
            if self.cr(owner).is_dead() {
                self.deferred_setup_choices.retain(|(p, _, _)| *p != owner);
                self.setup_enchant_pending[owner as usize] = false;
                before_play = false;
                continue;
            }
            if !before_play {
                self.enter_auto_pre_play(owner);
            }
            before_play = false;
            if self.pending_choice.is_some() {
                self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: owner, before_play: true });
                return;
            }
            if self.pending_choice.is_some() || self.is_over {
                return;
            }
            self.fire(Hook::BeforePlayPhaseStart, Ev::Creature(owner), None);
            if self.pending_choice.is_some() {
                self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: owner + 1, before_play: false });
                return;
            }
            if self.pending_choice.is_some() || self.is_over {
                return;
            }
            self.setup_auto_pre_play_done[owner as usize] = true;
        }
        self.check_combat_end();
    }

    fn enter_auto_pre_play(&mut self, owner: CId) {
        // CombatManager.RunAutoPrePlayPhase: CheckForEmptyHand precedes the hook.
        if self.players[owner].hand.is_empty() {
            self.fire_relic_after_hand_emptied_for_player(owner);
        }
        if self.setup_enchant_pending[owner as usize] {
            self.setup_enchant_pending[owner as usize] = false;
            crate::card::enchant::after_player_turn_start(self, owner);
        }
        if self.run.choice_scheduling.is_live() {
            // Hook.AfterAutoPrePlayPhaseEntered awaits its snapshot; a child
            // autoplay's entire PendingPlay stack belongs to this seat's action.
            self.fire_setup_hooks(owner, &[Hook::AfterAutoPrePlayPhaseEntered],
                TurnStage::AutoPrePlay { next_player: owner, before_play: true });
        } else {
            self.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(owner), None);
        }
    }

    /// CombatManager.StartTurn assigns all setup tasks and checks victory before
    /// queued hook actions become externally visible. ActionQueueSet skips a
    /// GatheringPlayerChoice action, so each other ready seat can open its UI.
    fn continue_live_auto_pre_play(&mut self) {
        if self.is_over { return; }
        if let Some((owner, stage)) = self.pending_live_setup.take() {
            let previous = self.setup_player;
            self.setup_player = owner;
            let finished = self.setup_player_turn(stage);
            self.setup_player = previous;
            if self.is_over { return; }
            if !finished {
                if let Some(stage) = self.pending_turn_setup.take() {
                    self.pending_live_setup = Some((owner, stage));
                }
                self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: owner, before_play: false });
                // An automatic first selector can resume setup into a second
                // real selector. C# parks that hook action and dispatches the
                // other ready queues, just as for the original setup selector.
                if self.pending_choice.is_some() { self.park_live_choice(); }
                else { return; }
            }
        }
        for owner in 0..self.players.len() as CId {
            if self.cr(owner).is_dead() {
                self.deferred_setup_choices.retain(|(p, _, _)| *p != owner);
                continue;
            }
            if self.setup_auto_pre_play_done[owner as usize]
                || self.pending_choice_for_player(owner).is_some()
                || self.deferred_setup_choices.iter().any(|(p, _, _)| *p == owner) { continue; }
            self.enter_auto_pre_play(owner);
            if self.is_over { return; }
            if self.pending_choice.is_some() {
                // ActionExecutor skips GatheringPlayerChoice actions. Preserve
                // the nested play/draw/autoplay stacks and dispatch the next seat.
                self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: owner, before_play: true });
                self.park_live_choice();
                continue;
            }
            if self.is_over { return; }
            self.fire(Hook::BeforePlayPhaseStart, Ev::Creature(owner), None);
            self.setup_auto_pre_play_done[owner as usize] = true;
        }
        self.check_combat_end();
        if self.is_over { self.live_choices.clear(); return; }
        if !self.deferred_setup_choices.is_empty() {
            let (owner, walk, deferred) = self.deferred_setup_choices.remove(0);
            let setup = walk.is_some();
            if let Some(walk) = walk {
                self.pending_live_setup = Some((owner, walk.resume_stage));
                self.pending_player_turn_hooks = Some(walk);
            }
            // CardSelectCmd.FromHand enumerates AFTER SignalPlayerChoiceBegun's
            // ExecutionStartedTask, not at assignment, on every turn.
            self.present_setup_choice(deferred);
            if self.pending_choice.is_none() { self.resume_player_turn_hooks(); }
            if self.pending_choice.is_some() {
                if setup {
                    self.pending_turn_setup = Some(TurnStage::AutoPrePlay { next_player: owner, before_play: false });
                }
                self.park_live_choice();
            }
            self.continue_live_auto_pre_play();
            return;
        }
        self.restore_first_live_choice();
    }

    /// C# `SetupPlayerTurn(setup_player)` from `stage` on; `false` when a choice paused it or
    /// the combat ended.
    fn setup_player_turn(&mut self, mut stage: TurnStage) -> bool {
        let owner = self.setup_player;
        if self.cr(owner).is_dead() {
            return true;
        }
        macro_rules! pause_if_choice {
            ($next:expr) => {
                if self.pending_choice.is_some() {
                    if self.collect_setup_choices {
                        self.unported("live setup selector bypassed deferred choice scheduling");
                    }
                    self.pending_turn_setup = Some($next);
                    return false;
                }
                if self.signalled_setup_choice.is_some() {
                    self.unported("live setup selector requires a resumable callback");
                    return false;
                }
                if self.deferred_setup_choices.iter().any(|(p, _, _)| *p == owner) {
                    return true;
                }
            };
        }
        if stage == TurnStage::Energy {
            let max_energy = self.max_energy(owner);
            // relic fire: core/combat.py:1200
            if self.should_reset_energy(owner) {
                self.players[owner].energy = max_energy;
            } else {
                self.players[owner].energy += max_energy;
            }
            self.fire_after_energy_reset(owner);
            pause_if_choice!(TurnStage::BeforeHandDraw);
            stage = TurnStage::BeforeHandDraw;
        }
        if stage == TurnStage::BeforeHandDraw {
            if self.run.choice_scheduling.is_live() {
                self.fire_setup_hooks(owner, &[Hook::BeforeHandDraw], TurnStage::CardBeforeHandDraw);
            } else {
                self.fire(Hook::BeforeHandDraw, Ev::Creature(owner), Some(owner));
            }
            pause_if_choice!(TurnStage::CardBeforeHandDraw);
            stage = TurnStage::CardBeforeHandDraw;
        }
        if stage == TurnStage::CardBeforeHandDraw {
            self.apply_card_before_hand_draw(owner);
            if self.is_over {
                return false;
            }
            pause_if_choice!(TurnStage::BeforeHandDrawLate);
            stage = TurnStage::BeforeHandDrawLate;
        }
        if stage == TurnStage::BeforeHandDrawLate {
            self.fire(Hook::BeforeHandDrawLate, Ev::Creature(owner), Some(owner));
            if self.is_over {
                return false;
            }
            pause_if_choice!(TurnStage::Draw);
            stage = TurnStage::Draw;
        }
        if stage == TurnStage::Draw {
            let mut draw_count = self.modify_hand_draw(BASE_DRAW, owner);
            if self.players[owner].turn_number == 1 {
                // _prepare_opening_draw_for_owner: innate cards first.
                let p = &mut self.players[owner];
                let cards = &self.cards;
                let innate: Vec<KRef> = p.draw.iter().copied().filter(|&k| cards[k as usize].is_innate()).collect();
                if !innate.is_empty() {
                    p.draw.retain(|&k| !cards[k as usize].is_innate());
                    p.draw.splice(0..0, innate.iter().copied());
                }
                draw_count = draw_count.max(innate.len() as i32).max(0).min(MAX_HAND_SIZE as i32);
            }
            self.draw_cards(owner, draw_count, true);
            if self.signalled_setup_choice.as_ref().is_some_and(|d| d.draw.is_some()) {
                let deferred = self.signalled_setup_choice.take().unwrap();
                self.defer_hand_draw_setup(owner, deferred);
                return true;
            }
            pause_if_choice!(TurnStage::AfterPlayerTurnStart);
            stage = TurnStage::AfterPlayerTurnStart;
        }
        if stage == TurnStage::AfterPlayerTurnStart {
            // CardSelectCmd.FromHand only signals begun without a test Selector.
            // Both modes use the same resumable listener walk.
            self.fire_after_player_turn_start(owner);
            if self.is_over {
                return false;
            }
            if self.deferred_setup_choices.iter().any(|(p, _, _)| *p == owner) {
                self.setup_enchant_pending[owner as usize] = true;
                return true;
            }
            pause_if_choice!(TurnStage::PostAfterPlayerTurnStart);
            stage = TurnStage::PostAfterPlayerTurnStart;
        }
        if stage == TurnStage::PostAfterPlayerTurnStart {
            self.setup_enchant_pending[owner as usize] = true;
        }
        true
    }

    /// r7 `_pending_turn_setup`: dispatch the continuation captured when a card choice paused
    /// either player-turn setup or an enemy move.
    pub(crate) fn resume_turn_after_choice(&mut self, stage: TurnStage) {
        match stage {
            TurnStage::PlayerTurnEnd => {
                self.resume_turn_end_hooks();
                if self.pending_turn_end_hooks.is_none() && self.pending_choice.is_none() {
                    self.finish_player_turn_after_hooks();
                }
            }
            TurnStage::AutoPrePlay { next_player, before_play } => {
                self.continue_auto_pre_play(next_player, before_play);
            }
            TurnStage::EnemyAfterMove { enemy, mv, next_index } => {
                self.finish_enemy_move_after_choice(enemy, mv, next_index);
            }
            _ => self.continue_player_turn_setup(stage),
        }
    }

    fn apply_card_before_hand_draw(&mut self, owner: CId) {
        let round = self.round;
        // C# ThrummingHatchet.BeforeHandDraw: return the owner's card unless already in hand.
        let returning: Vec<KRef> = self.pile_cards(owner).into_iter()
            .filter(|&k| self.cards[k as usize].cv.return_before_hand_draw_round == Some(round))
            .collect();
        for k in returning {
            self.cards[k as usize].cv.return_before_hand_draw_round = None;
            if !self.players[owner].hand.contains(&k) {
                crate::content::move_to_hand(self, k);
            }
        }
        // fire: core/combat.py:1298-1301
        self.fire_card_before_hand_draw(owner);
    }

    /// `owner`'s hand, draw, discard, exhaust, play (r7 `all_piles` order).
    pub fn pile_cards(&self, owner: CId) -> Vec<KRef> {
        let p = &self.players[owner];
        let mut out = Vec::with_capacity(p.hand.len() + p.draw.len() + p.discard.len() + p.exhaust.len() + p.play.len());
        out.extend_from_slice(&p.hand);
        out.extend_from_slice(&p.draw);
        out.extend_from_slice(&p.discard);
        out.extend_from_slice(&p.exhaust);
        out.extend_from_slice(&p.play);
        out
    }

    /// Every player's `pile_cards`, in `Players` order (combat-wide effects such as enemy powers).
    pub fn all_players_pile_cards(&self) -> Vec<KRef> {
        let mut out = Vec::new();
        for owner in 0..self.players.len() as CId {
            out.extend(self.pile_cards(owner));
        }
        out
    }

    // ───────────────────────────── draw / shuffle ─────────────────────────────

    /// `_draw_cards_for_creature`; returns the last card drawn (r7 returns the drawn list), even
    /// when a draw hook (Hellraiser) already moved it out of the hand.
    pub fn draw_cards(&mut self, owner: CId, count: i32, from_hand_draw: bool) -> Option<KRef> {
        self.draw_cards_with_caller(owner, count, from_hand_draw, false)
    }

    /// An awaited card draw whose caller is represented by the active PendingPlay.
    /// Most callers end after Draw; callers with work after Draw store that work on
    /// the frame so it runs only after a shuffle selector and the remaining batch.
    pub(crate) fn draw_cards_at_end_of_play(&mut self, owner: CId, count: i32) -> Option<KRef> {
        let supported = self.active_card.is_some_and(|k|
            self.pending_play.last().is_some_and(|f| f.card == k && f.owner == owner));
        self.draw_cards_with_caller(owner, count, false, supported)
    }

    /// Expertise awaits Draw's exact result, not a hand snapshot or nested draws.
    pub(crate) fn draw_cards_then_retain(&mut self,owner:CId,count:i32) {
        self.draw_cards_collect_result(owner,count);
    }

    pub(crate) fn draw_cards_then_discard_nonzero(&mut self,owner:CId,count:i32) {
        self.draw_cards_collect_result(owner,count);
    }

    fn draw_cards_collect_result(&mut self,owner:CId,count:i32) {
        let frame=self.pending_play.len().checked_sub(1).expect("draw result without play frame");
        self.pending_play[frame].post_draw_cards=Some(Vec::new());
        self.draw_cards_with_result(owner,count,false,true,Some(frame));
        if self.pending_choice.is_none() && self.signalled_setup_choice.is_none() {
            self.finish_draw_result(frame);
        }
    }

    fn finish_draw_result(&mut self,frame:usize) {
        if let Some(cards)=self.pending_play[frame].post_draw_cards.take() {
            if self.cards[self.pending_play[frame].card as usize].id == CardId::Scrape {
                let owner=self.pending_play[frame].owner;
                let mut discarded=Vec::new();
                for card in cards {
                    // EnergyCost.GetWithModifiers preserves an unplayable
                    // negative cost; modified_card_cost clamps payable energy.
                    if self.cards[card as usize].cost<0 || self.modified_card_cost(owner,card)!=0 || self.cards[card as usize].x_cost {
                        discarded.push(card);
                    }
                }
                self.discard_cards(owner,&discarded,0);
            } else {
                for card in cards {self.cards[card as usize].single_turn_retain=true;}
            }
        }
    }

    /// Reboot.cs:29 awaits Shuffle (including Stratagem), then starts Draw.
    pub(crate) fn shuffle_then_draw(&mut self,owner:CId,count:i32) {
        self.shuffle_piles(owner);
        if self.pending_choice.is_some() {
            self.pending_play.last_mut().expect("awaited shuffle without card frame")
                .post_shuffle_draw=Some(count);
        } else {
            self.draw_cards_at_end_of_play(owner,count);
        }
    }

    /// C# Modded.OnPlay: AddSlots, await Draw, then increase this card's combat cost.
    pub(crate) fn draw_cards_then_increase_combat_cost(&mut self, owner: CId, count: i32, card: KRef) {
        self.draw_cards_at_end_of_play(owner, count);
        if self.pending_choice.is_some() {
            let frame = self.pending_play.last_mut().expect("active card draw without PendingPlay");
            frame.post_draw_cost_card = Some(card);
        } else {
            self.cards[card as usize].add_combat_cost(1);
        }
    }

    pub(crate) fn draw_cards_for_branch(&mut self, owner: CId, count: i32) {
        self.draw_cards_with_caller(owner, count, false, true);
    }

    pub(crate) fn draw_centennial_batches(&mut self, owner: CId, count: i32) {
        for remaining in (0..count).rev() {
            self.draw_cards_for_branch(owner, 1);
            if self.pending_choice.is_some() {
                if remaining > 0 {
                    self.pending_puzzle_draws.push((self.pending_play.len(), owner, remaining));
                }
                return;
            }
            if self.fault.is_some() { return; }
        }
    }

    fn draw_cards_with_caller(&mut self, owner: CId, count: i32, from_hand_draw: bool, caller_supported: bool) -> Option<KRef> {
        self.draw_cards_with_result(owner,count,from_hand_draw,caller_supported,None)
    }

    fn draw_cards_with_result(&mut self,owner:CId,count:i32,from_hand_draw:bool,caller_supported:bool,collector:Option<usize>)->Option<KRef> {
        // fire: core/combat.py:2127
        if self.is_over_or_ending() || !self.should_draw(owner, from_hand_draw) {
            return None;
        }
        self.draw_cards_authorized(owner, count, from_hand_draw, caller_supported,collector)
    }

    /// CardPileCmd.DrawInternal checks ShouldDraw once, before the awaited draw batch.
    fn draw_cards_authorized(&mut self, owner: CId, count: i32, from_hand_draw: bool, caller_supported: bool,collector:Option<usize>) -> Option<KRef> {
        if self.is_over_or_ending() { return None; }
        let mut last = None;
        let mut remaining = count.max(0);
        while remaining > 0 {
            if self.players[owner].hand.len() >= MAX_HAND_SIZE {
                break;
            }
            self.shuffle_if_needed(owner);
            if let Some(deferred) = self.signalled_setup_choice.as_mut() {
                if from_hand_draw || caller_supported {
                    deferred.draw = Some((remaining, from_hand_draw,collector));
                } else {
                    self.unported("caller awaiting suspended draw continuation");
                }
                return last;
            }
            if self.pending_choice.is_some() {
                self.pending_draws.push((self.pending_play.len(), owner, remaining, from_hand_draw,collector));
                // CardPileCmd.DrawInternal is awaited (e.g. SneckoOil.OnUse).
                // Tail-draw card callers resume through PendingPlay after the batch.
                if !from_hand_draw && !caller_supported {
                    self.unported("caller awaiting suspended draw continuation");
                }
                return last;
            }
            if self.players[owner].draw.is_empty() {
                break;
            }
            remaining -= 1;
            let k = self.players[owner].draw.remove(0);
            last = Some(k);
            self.cards[k as usize].owner = owner;
            self.players[owner].hand.push(k);
            if let Some(frame)=collector {
                self.pending_play[frame].post_draw_cards.as_mut().unwrap().push(k);
            }
            self.fire_after_card_changed_piles(k, PileKind::Draw, None);
            self.hist.cards_drawn_combat += 1;
            self.hist.drawn_by_owner_combat[owner as usize] += 1;
            // CardPileCmd.Draw records CardDrawn before AfterCardDrawn callbacks.
            if !from_hand_draw {
                self.hist.extra_draws_this_turn[owner as usize] += 1;
            }
            if self.cards[k as usize].is_status() {
                self.hist.status_drawn_this_turn[owner as usize] += 1;
            }
            let depth=self.pending_play.len();
            if self.power_amount(owner, PowerId::Hellraiser) > 0 && self.cards[k as usize].has_tag(CardTag::Strike) {
                let infinite=self.hittable_enemies().iter().all(|&enemy|self.cr(enemy).hp_display_infinite);
                let p=self.find_power(owner,PowerId::Hellraiser).unwrap();
                let count=&mut self.powers[p as usize].vars[0];
                let auto_play=if infinite {
                    // Original HellraiserPower permits nine auto-plays against
                    // infinite displays; later Strikes stay in hand for Pillage.
                    let allowed=*count<9;
                    *count+=1;
                    allowed
                }else{
                    *count=0;
                    true
                };
                if auto_play {self.auto_play_card(k, None, false);}
            }
            if self.pending_choice.is_some() {
                // AfterCardDrawnEarly awaits the child play. Later draw hooks
                // and the remaining batch belong to the caller's stack depth.
                self.pending_drawn_hooks.push((depth,k,from_hand_draw));
                self.pending_draws.push((depth,owner,remaining,from_hand_draw,collector));
                if !from_hand_draw && !caller_supported {
                    self.unported("caller awaiting suspended draw continuation");
                }
                return last;
            }
            self.finish_drawn_card(k,from_hand_draw);
            if self.pending_choice.is_some() {
                self.pending_draws.push((depth, owner, remaining, from_hand_draw,collector));
                if !from_hand_draw && !caller_supported {
                    self.unported("caller awaiting suspended draw continuation");
                }
                return last;
            }
        }
        last
    }

    fn finish_drawn_card(&mut self,k:KRef,from_hand_draw:bool) {
        let prev_from_hand = std::mem::replace(&mut self.drawing_from_hand, from_hand_draw);
        self.fire(Hook::OnCardDrawn, Ev::Card(k), None);
        self.drawing_from_hand = prev_from_hand;
        self.fire_card_after_card_drawn(k);
        crate::card::enchant::on_card_drawn(self, k);
    }

    /// `_shuffle_if_needed` for `owner`'s piles.
    pub fn shuffle_if_needed(&mut self, owner: CId) {
        if self.players[owner].draw.is_empty() && !self.players[owner].discard.is_empty() {
            self.shuffle_piles(owner);
        }
    }

    /// CardPileCmd.Shuffle: shuffle discard AND draw, even when draw is nonempty.
    pub(crate) fn shuffle_piles(&mut self, owner:CId) {
        if self.is_over_or_ending() { return; }
            let moved=std::mem::take(&mut self.players[owner].discard);
            self.players[owner].draw.extend(moved.iter().copied());
            let shuffle_capture=self.record_shuffle(owner,&moved);
            for k in moved {
                self.fire_after_card_changed_piles(k, PileKind::Discard, None);
            }
            drop(shuffle_capture);
            let p = &mut self.players[owner];
            dotnet_sort_cards(&mut p.draw, &self.cards);
            p.draw_known = Default::default();
            self.run.rngs.shuffle.shuffle(&mut self.players[owner].draw);
            // fire: core/combat.py:2178
            crate::card::enchant::modify_shuffle_order(self, owner);
            // relic fire: core/hooks.py:1084
            // StratagemPower.AfterShuffle is the sole power override; the exported
            // r7 hook enum lacks AfterShuffle (it calls this power's on_shuffle).
            if let Some(p) = self.find_power(owner, PowerId::Stratagem) {
                let cards = crate::content::draw_selector_order_pub(self, owner);
                let count = self.powers[p as usize].amount.max(0) as u16;
                self.request_multi_card_choice_for_player(owner, &cards, PileKind::Draw,
                    crate::choice::Resolver { src: crate::choice::ChoiceSrc::Card(CardId::Stratagem),
                        k: owner as KRef, target: Some(owner), step: 1, arg: 0 },
                    count, Some(count), false);
                if self.collect_setup_choices && self.signalled_setup_choice.is_some() {
                    return;
                }
            }
            if self.pending_choice.is_some() {
                self.pending_shuffle_relics = Some(owner);
            } else {
                self.fire_relic_after_shuffle(owner);
            }
    }

    // ───────────────────────────── card play ─────────────────────────────

    pub fn modified_card_cost(&mut self, owner: CId, k: KRef) -> i32 {
        let mut fault = None;
        let value = self.query_modified_card_cost(owner, k, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub fn modified_card_cost_readonly(&self, owner: CId, k: KRef) -> Result<i32, &'static str> {
        let mut fault = None;
        let value = self.query_modified_card_cost(owner, k, &mut fault);
        fault.map_or(Ok(value), Err)
    }

    pub(crate) fn query_modified_card_cost(&self, owner: CId, k: KRef, fault: &mut Option<&'static str>) -> i32 {
        if self.cards[k as usize].original_cost < 0 {
            return 0;
        }
        let cost = self.cards[k as usize].local_energy_cost();
        // C# Hook.ModifyEnergyCostInCombat returns a negative local result unchanged,
        // before invoking any global listener; GetAmountToSpend then clamps it to zero.
        if cost < 0 {
            return 0;
        }
        if self.cards[k as usize].x_cost {
            return cost;
        }
        // C# Hook.ModifyEnergyCostInCombat: every listener's TryModifyEnergyCostInCombat
        // (powers, then relics), then every listener's ...Late (so Free Power's 0 wins over
        // Spiked Gauntlets' +1).
        let mut cost = cost;
        for late in [false, true] {
            cost = self.modify_card_cost(owner, k, cost, late);
            for r in 0..self.run.relics.len() {
                cost = self.query_relic_modify_card_cost(r, self.run.relics[r].owner, k, cost, late, fault);
            }
        }
        cost.max(0)
    }

    pub fn modified_star_cost(&mut self, k: KRef) -> i32 {
        let mut fault = None;
        let value = self.query_modified_star_cost(k, &mut fault);
        self.apply_query_fault(fault);
        value
    }

    pub fn modified_star_cost_readonly(&self, k: KRef) -> Result<i32, &'static str> {
        let mut fault = None;
        let value = self.query_modified_star_cost(k, &mut fault);
        fault.map_or(Ok(value), Err)
    }

    fn query_modified_star_cost(&self, k: KRef, fault: &mut Option<&'static str>) -> i32 {
        let c = &self.cards[k as usize];
        let mut cost = c.cv.turn_star_cost_override.or(c.cv.combat_star_cost_override).unwrap_or(c.star_cost).max(0);
        // C# Hook.ModifyStarCost: powers (VoidFormPower.TryModifyStarCost), then relics.
        let owner = c.owner;
        if cost > 0 && let Some(p) = self.find_power(owner, PowerId::VoidForm)
            && self.void_form_covers(p, owner, k)
        {
            cost = 0;
        }
        // relic fire: core/combat.py card star-cost pipeline
        for r in 0..self.run.relics.len() {
            cost = self.query_relic_modify_star_cost(r, self.run.relics[r].owner, k, cost, fault).max(0);
        }
        cost
    }

    /// `CombatState.can_play_card`.
    pub fn can_play_card(&mut self, k: KRef) -> bool {
        let mut fault = None;
        let playable = self.card_legality(k, &mut fault);
        self.apply_query_fault(fault);
        playable
    }

    /// The same legality query without changing fault markers or advancing work.
    /// Unsupported gameplay is an error, never a policy-created illegal mask.
    pub fn can_play_card_readonly(&self, k: KRef) -> Result<bool, &'static str> {
        let mut fault = None;
        let playable = self.card_legality(k, &mut fault);
        match fault { Some(reason) => Err(reason), None => Ok(playable) }
    }

    fn card_legality(&self, k: KRef, fault: &mut Option<&'static str>) -> bool {
        if k as usize >= self.cards.len() { *fault = Some("invalid card arena handle"); return false; }
        if self.is_over_or_ending() {
            return false;
        }
        if self.cards[k as usize].is_unplayable() {
            return false;
        }
        // relic fire: core/hooks.py:714
        for r in 0..self.run.relics.len() {
            if self.query_relic_should_play(r, self.run.relics[r].owner, k, fault) == Some(false) {
                return false;
            }
        }
        let owner = self.cards[k as usize].owner;
        if !self.is_player_taking_turn(owner) { return false; }
        // C# CardModel.CanPlay: AnyAlly requires at least two living player creatures,
        // even though IsValidTarget permits selecting self once that condition holds.
        if self.card_target_type(k) == TargetType::AnyAlly
            && (0..self.players.len() as CId).filter(|&p| self.cr(p).is_alive()).count() <= 1
        {
            return false;
        }
        // fire: core/combat.py:1478
        if !self.card_playability_readonly(k).unwrap_or_else(|reason| { fault.get_or_insert(reason); true }) {
            return false;
        }
        // fire: core/hooks.py:715
        if !self.powers_allow_card_play(k) {
            return false;
        }
        // fire: core/combat.py:1477-1482
        if !self.cards_allow_hand_card_play(k, false) {
            return false;
        }
        if self.query_modified_star_cost(k, fault) > self.players[owner].stars {
            return false;
        }
        let x = self.cards[k as usize].x_cost;
        if !x && self.query_modified_card_cost(owner, k, fault) > self.players[owner].energy {
            return false;
        }
        true
    }

    /// `_resolve_target` for a hand play.
    fn resolve_target(&self, k: KRef, target_index: Option<usize>) -> Option<CId> {
        match self.card_target_type(k) {
            TargetType::Self_ | TargetType::None => Some(self.cards[k as usize].owner),
            TargetType::AnyEnemy => {
                if let Some(i) = target_index && i < self.enemies.len() && self.can_hit(self.enemies[i]) {
                    return Some(self.enemies[i]);
                }
                self.hittable_enemies().first().copied()
            }
            // C# CardModel.IsValidTarget accepts living allied pets as well.
            // Target indices follow CombatState.Allies: players, then pets.
            TargetType::AnyAlly => target_index
                .and_then(|i| self.player_side().get(i).copied())
                .filter(|&p| self.cr(p).is_alive()),
            _ => None,
        }
    }

    /// `play_card_from_creature(player, hand_index, target_index)`: `actor` is the player
    /// (creature id) whose hand card is played.
    pub fn play_card(&mut self, actor: CId, hand_index: usize, target_index: Option<usize>) -> bool {
        self.with_live_queue_action(|c| c.play_card_action(actor, hand_index, target_index))
    }

    fn play_card_action(&mut self, actor: CId, hand_index: usize, target_index: Option<usize>) -> bool {
        if !self.players.is_player(actor) || hand_index >= self.players[actor].hand.len() || self.pending_choice.is_some() {
            return false;
        }
        let k = self.players[actor].hand[hand_index];
        if !self.can_play_card(k) {
            return false;
        }
        let target = self.resolve_target(k, target_index);
        if matches!(self.card_target_type(k), TargetType::AnyEnemy | TargetType::AnyAlly) && target.is_none() {
            return false;
        }
        if target.is_some_and(|t| self.cr(t).is_pet)
            && crate::tables::ORIGINAL_PET_TARGET_FAULTS.contains(&self.cards[k as usize].id) {
            self.unported("original game exception: player-only ally card targeted a pet");
            return false;
        }
        self.players[actor].hand.remove(hand_index);
        let prev = self.in_play_phase;
        self.in_play_phase = true;
        self.execute_card_play(k, target, true, false);
        // relic fire: core/combat.py:1683
        if self.players[actor].hand.is_empty() {
            self.fire_relic_after_hand_emptied_for_player(actor);
        }
        self.in_play_phase = prev;
        self.check_combat_end();
        true
    }

    /// `_default_card_play_result` + `modify_card_play_result_pile_type_and_position`.
    fn card_play_result(&mut self, mut owner: CId, k: KRef, force_exhaust: bool, is_auto_play: bool, energy_value: i32) -> PlayResult {
        let c = &self.cards[k as usize];
        let (mut pile, mut pos) = if c.cv.is_dupe || c.card_type == CardType::Power {
            (PileKind::None, PilePos::None)
        } else if force_exhaust || c.exhausts() {
            (PileKind::Exhaust, PilePos::Bottom)
        } else {
            (PileKind::Discard, PilePos::Bottom)
        };
        if c.id == CardId::TheBall {
            // C# TheBall.GetResultLocationForCardPlay: pass to a random living teammate,
            // preserving exhaust/none destinations, but shuffle a discard into their draw.
            let allies: Vec<_> = self.living_players().into_iter().filter(|&p| p != owner).collect();
            if !allies.is_empty() {
                owner = allies[self.run.rngs.combat_targets.choice_index(allies.len())];
                if pile == PileKind::Discard {
                    pile = PileKind::Draw;
                    pos = PilePos::Random;
                }
            }
        }
        // fire: core/combat.py:1603
        (pile, pos) = self.modify_card_play_result(k, is_auto_play, energy_value, pile, pos);
        PlayResult { pile, pos, owner }
    }

    /// `_execute_card_play`.
    pub fn execute_card_play(&mut self, k: KRef, target: Option<CId>, spend_energy: bool, force_exhaust: bool) {
        self.execute_card_play_mode(k, target, spend_energy, force_exhaust, !spend_energy);
    }

    /// Same pipeline as `execute_card_play`, with r7's independent `is_auto_play` bit.
    /// Whispering Earring is the combat path that both spends energy and sets this bit.
    // relic fire: relics/shop_event.py:3743-3744
    pub(crate) fn execute_card_play_mode(
        &mut self,
        k: KRef,
        target: Option<CId>,
        spend_energy: bool,
        force_exhaust: bool,
        is_auto_play: bool,
    ) {
        self.flush_pending_attack();
        let owner = self.cards[k as usize].owner;
        let old_pile = if spend_energy { PileKind::Hand } else { self.card_pile(k) };
        self.remove_card_from_piles(k);
        // fire: core/combat.py:1714-1732 `_card_being_played_for_cost`.
        // The Rust relic helpers already treat a card in `play` as eligible for the same
        // cost modifiers.  Keep it there only for the exact r7 cost/spend window, then restore
        // the pre-play zone until the normal play-pile insertion below.
        let cost_marker_inserted = spend_energy && !self.players[owner].play.contains(&k);
        if cost_marker_inserted {
            self.players[owner].play.push(k);
        }
        let mut energy_spent = 0;
        if spend_energy {
            energy_spent = if self.cards[k as usize].x_cost { self.players[owner].energy } else { self.modified_card_cost(owner, k) };
        }
        if self.cards[k as usize].x_cost {
            // CardModel.SpendEnergy / CardCmd.AutoPlay capture raw available energy;
            // ResolveEnergyXValue applies Chemical X separately for the card effect.
            self.cards[k as usize].captured_x = if spend_energy { energy_spent } else { self.players[owner].energy };
        }
        let mut x_value = energy_spent;
        if self.cards[k as usize].x_cost {
            // relic fire: core/hooks.py modify_x_value
            for r in 0..self.run.relics.len() {
                x_value = self.relic_modify_x_value(r, self.run.relics[r].owner, k, x_value);
            }
        }
        self.cards[k as usize].energy_spent = x_value;
        self.cards[k as usize].cv.stars_spent_for_play = Some(0);
        if spend_energy {
            let p = &mut self.players[owner];
            p.energy = (p.energy - energy_spent).max(0);
            if energy_spent > 0 {
                self.hist.energy_spent_this_turn[owner as usize] += energy_spent;
            }
            self.fire(Hook::AfterEnergySpent, Ev::EnergySpent { card: k, amount: energy_spent }, None);
            // fire: core/combat.py:1728-1730
            let star_cost = self.modified_star_cost(k);
            if star_cost > 0 {
                let spent = self.spend_stars(owner, star_cost);
                self.cards[k as usize].cv.stars_spent_for_play = Some(spent);
            }
        }
        if cost_marker_inserted
            && let Some(pos) = self.players[owner].play.iter().position(|&q| q == k)
        {
            self.players[owner].play.remove(pos);
        }
        if !self.players[owner].play.contains(&k) {
            self.players[owner].play.push(k);
        }
        // Manual AddDuringManualCardPlay always notifies; autoplay uses Add,
        // which suppresses same-pile moves.
        if !is_auto_play || old_pile != PileKind::Play {
            self.fire_after_card_changed_piles(k, old_pile, None);
        }
        let energy_value = if spend_energy { energy_spent } else if self.cards[k as usize].x_cost { self.players[owner].energy } else { self.modified_card_cost(owner, k) };
        let result = self.card_play_result(owner, k, force_exhaust, is_auto_play, energy_value);
        let prev_target = self.active_target;
        self.active_target = target;
        // fire: core/combat.py:1754
        let base = 1
            + self.cards[k as usize].replay
            + self.sword_sage_replays(k)
            + crate::card::enchant::play_count_bonus(&self.cards[k as usize]);
        // fire: core/combat.py:1755 (modify_card_play_count: power listeners, then relics)
        let (play_count, play_count_modifiers) = self.modify_card_play_count(base, k);
        let (play_count, relic_modifiers) = self.relic_modify_card_play_count_all(play_count, k);
        // fire: core/combat.py:1756
        self.fire_after_modifying_card_play_count(k, play_count_modifiers);
        for r in relic_modifiers {
            self.relic_after_modifying_card_play_count(r, self.run.relics[r].owner, k);
        }
        self.active_target = prev_target;
        let frame = PendingPlay {
            card: k, target, owner, remaining: play_count, play_count, energy_spent, energy_value,
            force_exhaust, is_auto: is_auto_play, result, awaiting_after_hook: false,
            after_play_walk: None,
            exhaust_caller: None,
            result_started: false, result_added: false,
            team_draw_after_owner: None,
            post_draw_cost_card: None,
            post_shuffle_draw: None,
            post_draw_cards: None,
            post_draw_discard_card: None,
            post_draw_constellation: None,
            post_draw_power: None,
        };
        let stop = self.pending_play.len();
        self.pending_play.push(frame);
        self.resume_pending_play(stop);
    }

    /// `_resume_pending_play(stop_before=parent)`: frames below `stop` belong to the parent.
    pub(crate) fn resume_pending_play(&mut self, stop: usize) {
        loop {
            if self.pending_choice.is_some() { return; }
            if let Some(owner) = self.pending_shuffle_relics.take() {
                self.fire_relic_after_shuffle(owner);
                continue;
            }
            if let Some(frame)=self.pending_play.last_mut()
                && let Some(count)=frame.post_shuffle_draw.take() {
                let owner=frame.owner;
                self.draw_cards_with_caller(owner,count,false,true);
                continue;
            }
            if let Some((_,k,from_hand_draw))=take_suspended_at_depth(
                &mut self.pending_drawn_hooks,self.pending_play.len(),|d|d.0) {
                self.finish_drawn_card(k,from_hand_draw);
                continue;
            }
            if let Some((_,owner,count,from_hand_draw,collector))=take_suspended_at_depth(
                &mut self.pending_draws,self.pending_play.len(),|d|d.0) {
                self.draw_cards_authorized(owner, count, from_hand_draw, true,collector);
                continue;
            }
            if let Some((depth,mut walk))=take_suspended_at_depth(
                &mut self.pending_exhaust_walks,self.pending_play.len(),|w|w.0) {
                if !self.resume_exhaust_walk(&mut walk) {
                    self.pending_exhaust_walks.push((depth, walk));
                    return;
                }
                continue;
            }
            if let Some((_,owner,count))=take_suspended_at_depth(
                &mut self.pending_puzzle_draws,self.pending_play.len(),|w|w.0) {
                self.draw_centennial_batches(owner, count);
                continue;
            }
            if let Some((depth,mut walk))=take_suspended_at_depth(
                &mut self.pending_damage_walks,self.pending_play.len(),|w|w.0) {
                if !self.resume_damage_received_walk(&mut walk) {
                    self.pending_damage_walks.push((depth, walk));
                    return;
                }
                continue;
            }
            if let Some((depth,mut walk))=take_suspended_at_depth(
                &mut self.pending_monster_attacks,self.pending_play.len(),|w|w.0) {
                if !self.resume_monster_attack(&mut walk) {
                    self.pending_monster_attacks.push((depth, walk));
                    return;
                }
                self.flush_pending_attack();
                continue;
            }
            if self.resume_autoplay_snapshot() { continue; }
            if self.pending_potion_after.is_some_and(|p| p.0 == self.pending_play.len()) {
                let (_, owner, potion, target) = self.pending_potion_after.take().unwrap();
                self.finish_potion_after_use(owner, potion, target);
                continue;
            }
            if self.pending_play.len() <= stop { break; }
            let top = self.pending_play.len() - 1;
            let (k, target, owner) = {
                let f = &self.pending_play[top];
                (f.card, f.target, f.owner)
            };
            if self.cr(owner).is_dead() {
                // CardModel.OnPlayWrapper returns once its owner died: no further plays,
                // after-play hooks or result pile; then EndCardOrPotionEffect removes the dead
                // player's cards (play pile included) when no play of that player is left.
                self.pending_play.pop();
                if !self.pending_play.iter().any(|f| f.owner == owner) && !self.living_players().is_empty() {
                    self.remove_dead_player_cards(owner);
                }
                continue;
            }
            if self.pending_play[top].awaiting_after_hook {
                if let Some(mut caller) = self.pending_play[top].exhaust_caller.take() {
                    if !self.resume_exhaust_caller(&mut caller) {
                        self.pending_play[top].exhaust_caller = Some(caller);
                        return;
                    }
                }
                if let Some((next, count)) = self.pending_play[top].team_draw_after_owner.take() {
                    self.continue_team_draw(next, count);
                    if self.pending_choice.is_some() { return; }
                }
                if let Some(card) = self.pending_play[top].post_draw_cost_card.take() {
                    self.cards[card as usize].add_combat_cost(1);
                }
                self.finish_draw_result(top);
                if let Some((card,target))=self.pending_play[top].post_draw_constellation.take() {
                    self.finish_constellation_draw(card,target);
                }
                if let Some((owner,power,amount))=self.pending_play[top].post_draw_power.take() {
                    self.apply_power_to(owner,power,amount,Some(owner));
                }
                if let Some(card)=self.pending_play[top].post_draw_discard_card.take() {
                    self.request_draw_followup_discard(card);
                    if self.pending_choice.is_some() {return;}
                }
                if self.fault.is_some() { return; }
                if self.pending_choice.is_some() {
                    return;
                }
                if !self.after_card_play_effect(k) { return; }
                self.pending_play[top].awaiting_after_hook = false;
                if self.is_over {
                    self.pending_play.truncate(stop);
                    return;
                }
            }
            if self.pending_play[top].remaining <= 0 {
                if !self.pending_play[top].result_started {
                    let p = &mut self.players[owner];
                    if let Some(pos) = p.play.iter().position(|&x| x == k) { p.play.remove(pos); }
                    let in_pile = p.hand.contains(&k) || p.draw.contains(&k) || p.discard.contains(&k)
                        || p.exhaust.contains(&k) || p.play.contains(&k);
                    self.pending_play[top].result_started = true;
                    self.pending_play[top].result_added = !in_pile;
                    if !in_pile { self.move_to_play_result(k, self.pending_play[top].result); }
                    if self.pending_choice.is_some() || self.fault.is_some() { return; }
                }
                let frame = self.pending_play.pop().unwrap();
                // C# CardModel.OnPlayWrapper: cleanup after all repeats and result-pile move.
                self.cards[k as usize].after_card_played_cost_cleanup();
                if frame.result_added && self.pending_play.is_empty() && self.end_turn_after_play && self.side == Side::Player && !self.is_over {
                    self.end_turn_after_play = false;
                    self.end_player_turn();
                }
                continue;
            }
            // _apply_card_before_card_played + fire_before_card_played
            // fire: core/combat.py:1446-1451
            // CardModel.OnPlayWrapper checks IsOverOrEnding before each play.
            if self.pending_play[top].remaining < self.pending_play[top].play_count && self.is_over_or_ending() {
                self.pending_play[top].remaining=0;
                continue;
            }
            self.fire_card_before_card_played(k);
            self.fire(Hook::BeforeCardPlayed, Ev::Card(k), None);
            // fire: core/combat.py:1839-1841
            self.hist.play_starts_this_turn += 1;
            self.hist.play_start_cards.push(k);
            self.record_card_play(k);
            if self.cards[k as usize].card_type == CardType::Attack {
                self.hist.attack_starts_this_turn[owner as usize] += 1;
            }
            if self.pending_play[top].remaining == self.pending_play[top].play_count {
                self.hist.first_series_actors.push(owner);
            }
            self.pending_play[top].remaining -= 1;
            let prev = self.active_card;
            self.active_card = Some(k);
            self.play_card_effect(k, target);
            self.flush_pending_attack();
            self.active_card = prev;
            self.flush_pending_attack();
            if self.fault.is_some() { return; }
            if self.pending_choice.is_some() {
                self.pending_play[top].awaiting_after_hook = true;
                return;
            }
            if self.cr(owner).is_dead() {
                continue;
            }
            self.pending_play[top].awaiting_after_hook = true;
            if !self.after_card_play_effect(k) { return; }
            self.pending_play[top].awaiting_after_hook = false;
            let _ = owner;
            if self.is_over {
                self.pending_play.truncate(stop);
                return;
            }
        }
    }

    /// `_finish_card_play` + `fire_after_card_played` + card/enchantment after-play steps.
    fn after_card_play_effect(&mut self, k: KRef) -> bool {
        let top = self.pending_play.len() - 1;
        let mut walk = match self.pending_play[top].after_play_walk.take() {
            Some(walk) => walk,
            None => {
                self.finish_card_play(k);
                self.start_after_play_walk()
            }
        };
        if !self.resume_after_play_walk(&mut walk, k) {
            self.pending_play[top].after_play_walk = Some(walk);
            return false;
        }
        // fire: core/combat.py:1793
        self.fire_card_after_card_played(k);
        // fire: core/combat.py:1794
        // fire: core/combat.py:1858
        crate::card::enchant::on_card_played(self, k);
        // fire: core/combat.py:1795
        // fire: core/combat.py:1859
        crate::card::enchant::after_card_played(self, k);
        // fire: core/combat.py:1796 / 1860
        self.fire_card_late_effects(k);
        true
    }

    fn finish_card_play(&mut self, k: KRef) {
        let owner = self.cards[k as usize].owner;
        self.hist.finished_plays.push((owner, self.players[owner].turn_number, k));
        if self.cards[k as usize].is_ethereal() {
            self.hist.ethereal_plays_finished[self.cards[k as usize].owner as usize] += 1;
        }
        self.hist.played_this_turn.push((owner, k));
        self.hist.played_combat.push((owner, k));
    }

    /// `_move_card_to_play_result`.
    fn move_to_play_result(&mut self, k: KRef, mut r: PlayResult) {
        let original_owner = self.cards[k as usize].owner;
        // C# CardPileCmd.Add rejects combat-pile additions while IsEnding,
        // before resolving Random (and thus before consuming Shuffle).
        // resume_pending_play already took the card out of Play; retain it there
        // until combat cleanup, as OnPlayWrapper does after the rejected Add.
        if matches!(r.pile, PileKind::Draw | PileKind::Discard | PileKind::Hand)
            && self.is_over_or_ending()
        {
            self.players[original_owner].play.push(k);
            return;
        }
        // C# GiveToAnotherPlayer refuses dead recipients and combat-ending transfers;
        // OnPlayWrapper then uses the same destination on the original owner's card.
        let transfer = r.pile != PileKind::None && r.owner != original_owner
            && self.cr(r.owner).is_alive() && !self.is_over_or_ending();
        if transfer {
            self.cards[k as usize].owner = r.owner;
        } else {
            r.owner = original_owner;
        }
        match r.pile {
            PileKind::None => self.fire_after_card_changed_piles(k, PileKind::Play, None),
            PileKind::Exhaust => {
                self.players[r.owner].exhaust.push(k);
                self.fire_after_card_changed_piles(k, PileKind::Play, None);
                // GiveToAnotherPlayer only Add's to Exhaust; it does not CardCmd.Exhaust.
                if !transfer { self.fire_after_card_exhausted(k); }
            }
            PileKind::Discard | PileKind::Draw | PileKind::Hand => {
                // C# CardPileCmd.Add: an add to a full hand goes to Discard instead
                // (e.g. Feral returning a 0-cost attack after its play filled the hand).
                let pile = if r.pile == PileKind::Hand
                    && self.players[r.owner].hand.len() >= crate::combat::MAX_HAND_SIZE
                { PileKind::Discard } else { r.pile };
                self.insert_pile_from(k, PileKind::Play, pile, r.pos);
                self.fire_after_card_changed_piles(k, PileKind::Play, None);
            }
            PileKind::Play => {}
        }
    }

    /// Insert `k` into its owner's `pile` at `pos`. Draw-pile positions it sets count as
    /// watched by the owner only (as for a card from the owner's hand).
    pub(crate) fn insert_pile(&mut self, k: KRef, pile: PileKind, pos: PilePos) {
        self.insert_pile_from(k, PileKind::Hand, pile, pos);
    }

    /// Put `k` on its owner's draw pile top or bottom, recording who watched (`public`: every seat).
    pub(crate) fn put_on_draw(&mut self, k: KRef, at_top: bool, public: bool) {
        let p = &mut self.players[self.cards[k as usize].owner];
        if at_top { p.draw.insert(0, k) } else { p.draw.push(k) }
        p.draw_known.record(&p.draw, at_top, public);
    }

    /// `insert_pile` for a card that left `from` (PileKind::None: generated); a draw-pile
    /// position from a face-up pile or a generated card is watched by every seat.
    pub(crate) fn insert_pile_from(&mut self, k: KRef, from: PileKind, pile: PileKind, pos: PilePos) {
        let public = matches!(from, PileKind::None | PileKind::Discard | PileKind::Play | PileKind::Exhaust);
        let owner = self.cards[k as usize].owner;
        // CardPilePosition.RANDOM: shuffle_rng.next_int(0, len) (r7 core/combat.py).
        let random_slot = |c: &mut Combat, len: usize| c.run.rngs.shuffle.next_int(0, len as i64) as usize;
        match (pile, pos) {
            (PileKind::Discard, PilePos::Random) => {
                let i = random_slot(self, self.players[owner].discard.len());
                self.players[owner].discard.insert(i, k);
            }
            (PileKind::Draw, PilePos::Random) => {
                let i = random_slot(self, self.players[owner].draw.len());
                self.players[owner].draw.insert(i, k);
                self.players[owner].draw_known = Default::default();
            }
            (PileKind::Hand, PilePos::Random) => {
                let i = random_slot(self, self.players[owner].hand.len());
                self.players[owner].hand.insert(i, k);
            }
            (PileKind::Discard, PilePos::Top) => self.players[owner].discard.insert(0, k),
            (PileKind::Discard, _) => self.players[owner].discard.push(k),
            (PileKind::Draw, PilePos::Top) => self.put_on_draw(k, true, public),
            (PileKind::Draw, _) => self.put_on_draw(k, false, public),
            (PileKind::Hand, PilePos::Top) => self.players[owner].hand.insert(0, k),
            (PileKind::Hand, _) => self.players[owner].hand.push(k),
            (PileKind::Exhaust, _) => self.players[owner].exhaust.push(k),
            (PileKind::Play, _) => self.players[owner].play.push(k),
            _ => {}
        }
    }

    pub fn fire_after_card_exhausted(&mut self, k: KRef) {
        self.fire_after_card_exhausted_kind(k, false);
    }

    fn fire_after_card_exhausted_kind(&mut self, k: KRef, exhausted_by_ethereal: bool) {
        // fire: core/combat.py was_card_exhausted_this_turn history
        self.hist.exhausted_this_turn += 1;
        self.hist.exhausted_combat += 1;
        // card hooks (fire_card_after_card_exhausted) first, then powers, then relics.
        self.fire_card_after_card_exhausted(k);
        self.fire_power_relic_after_card_exhausted(k, exhausted_by_ethereal);
    }

    // ───────────────────────────── end of turn ─────────────────────────────

    #[inline]
    fn all_player_mask(&self) -> u8 {
        ((1u16 << self.players.len()) - 1) as u8
    }

    /// C# `CombatManager.IsPartOfPlayerTurn`, restricted to living players for
    /// simulator scheduling.  During an extra turn only the selected seats take
    /// part; ordinary player turns contain every living seated player.
    pub fn player_turn_participants(&self) -> u8 {
        if self.side != Side::Player || self.is_over { return 0; }
        let mut mask = 0;
        for owner in 0..self.players.len() as CId {
            if self.is_player_taking_turn(owner) && self.cr(owner).is_alive() {
                mask |= 1 << owner;
            }
        }
        mask
    }

    #[inline]
    pub fn is_player_ready_to_end_turn(&self, owner: CId) -> bool {
        (owner as usize) < self.players.len()
            && self.players_ready_to_end_turn & (1 << owner) != 0
    }

    #[inline]
    pub fn is_player_ready_to_begin_enemy_turn(&self, owner: CId) -> bool {
        (owner as usize) < self.players.len()
            && self.players_ready_to_begin_enemy_turn & (1 << owner) != 0
    }

    /// Read-only phase-two availability, including clients whose creatures died.
    pub fn needs_enemy_turn_acknowledgement(&self, owner: CId) -> bool {
        self.players.is_player(owner) && !self.is_over && self.fault.is_none()
            && self.side == Side::Player && self.player_turn_phase_one_done
            && !self.is_player_ready_to_begin_enemy_turn(owner)
    }

    /// Existing oracle/replay path: all seats end together and automatically
    /// cross C#'s second readiness barrier once phase one completes.
    pub fn end_player_turn(&mut self) {
        self.with_live_queue_action(|c| {
            if c.fault.is_some() || c.is_over || c.pending_choice.is_some() || c.side != Side::Player {
                return;
            }
            c.auto_begin_enemy_turn = true;
            c.players_ready_to_end_turn = c.all_player_mask();
            c.try_finish_player_turn_phase_one();
        });
    }

    /// C# `PlayerCmd.EndTurn(player, ...)`: mark only this seat ready.  Phase one
    /// begins once every living participant has ended; dead/non-participating
    /// seats are equivalent to StartTurn's automatic ready actions.
    pub fn end_player_turn_for(&mut self, owner: CId) {
        self.with_live_queue_action(|c| c.end_player_turn_for_action(owner));
    }

    fn end_player_turn_for_action(&mut self, owner: CId) {
        if self.fault.is_some() || self.is_over || self.pending_choice.is_some() || self.side != Side::Player {
            return;
        }
        if (owner as usize) >= self.players.len() { return; }
        self.players_ready_to_end_turn |= 1 << owner;
        self.try_finish_player_turn_phase_one();
    }

    fn try_finish_player_turn_phase_one(&mut self) {
        if self.player_turn_phase_one_done || self.pending_turn_end_hooks.is_some() { return; }
        // A participant may die after StartTurn.  The trainer-facing scheduler
        // must not wait forever on that seat.
        let all_players = self.all_player_mask();
        self.players_ready_to_end_turn |= all_players & !self.player_turn_participants();
        if self.players_ready_to_end_turn & all_players != all_players { return; }
        self.between_stage("turn_end", None);
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.action_queue.phase("end_turn_phase_one",self.round);}
        self.fire_before_turn_end(Side::Player);
        if self.pending_turn_end_hooks.is_some() || self.pending_choice.is_some() { return; }
        self.finish_player_turn_after_hooks();
    }

    fn finish_player_turn_after_hooks(&mut self) {
        if self.fault.is_some() { return; }
        self.check_combat_end();
        if self.is_over {
            return;
        }
        // C# EndPlayerTurnPhaseOneInternal: DoTurnEnd per player, then
        // BeforeFlush.  The actual hand flush belongs to phase two, after every
        // client has sent ReadyToBeginEnemyTurnAction.
        let mut before_flush = true;
        for owner in 0..self.players.len() as CId {
            if !self.is_player_taking_turn(owner) { continue; }
            if self.players[owner].orbs.capacity > 0 {
                // fire: core/combat.py:1893-1899 (BeforeTurnEndOrbTrigger)
                self.trigger_orb_passives(owner, false);
                self.check_combat_end();
                if self.is_over {
                    return;
                }
            }
            if !self.resolve_end_of_turn_hand(owner) {
                before_flush = false;
                break;
            }
        }
        if before_flush {
            for owner in 0..self.players.len() as CId {
                if !self.is_player_taking_turn(owner) { continue; }
                self.before_flush_hand(owner);
            }
        }
        self.check_combat_end();
        if self.is_over {
            return;
        }
        self.player_turn_phase_one_done = true;
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.action_queue.phase("await_enemy_ack",self.round);}
        if self.auto_begin_enemy_turn {
            self.players_ready_to_begin_enemy_turn = self.all_player_mask();
            self.try_finish_player_turn_phase_two();
        }
    }

    /// C# `CombatManager.SetReadyToBeginEnemyTurn`.  Unlike end-turn
    /// participation, this barrier contains every seated player: each client
    /// acknowledges phase one before the shared side switch can happen.
    pub fn set_ready_to_begin_enemy_turn(&mut self, owner: CId) {
        self.with_live_queue_action(|c| {
            if c.fault.is_some() || c.is_over || c.side != Side::Player
                || !c.player_turn_phase_one_done || (owner as usize) >= c.players.len() {
                return;
            }
            c.players_ready_to_begin_enemy_turn |= 1 << owner;
            c.try_finish_player_turn_phase_two();
        });
    }

    fn try_finish_player_turn_phase_two(&mut self) {
        if !self.player_turn_phase_one_done
            || self.players_ready_to_begin_enemy_turn & self.all_player_mask() != self.all_player_mask() {
            return;
        }
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.action_queue.phase("end_turn_phase_two",self.round);}
        for owner in 0..self.players.len() as CId {
            if !self.is_player_taking_turn(owner) { continue; }
            self.flush_hand(owner);
        }
        self.cleanup_cards_end_of_turn();
        self.end_defer_depth += 1;
        self.fire_after_turn_end(Side::Player);
        self.end_defer_depth -= 1;
        // relic fire: core/combat.py:1916-1920
        if self.should_take_extra_turn_now() {
            self.start_player_turn();
            return;
        }
        if self.fault.is_some() { return; }
        self.execute_enemy_turn();
        if self.is_over || self.pending_choice.is_some() {
            return;
        }
        self.round += 1;
        self.start_player_turn();
    }

    /// Phase-one half of C# `BeforeFlush`, run before the
    /// ReadyToBeginEnemyTurn barrier.
    fn before_flush_hand(&mut self, owner: CId) {
        self.fire(Hook::BeforeFlush, Ev::Creature(owner), None);
        crate::card::enchant::before_flush(self, owner);
    }

    /// `_resolve_end_of_turn_hand` up to the flush (C# `DoTurnEnd`'s hand part) for `owner`;
    /// `false` when the combat started ending (r7 then skips the flush).
    fn resolve_end_of_turn_hand(&mut self, owner: CId) -> bool {
        let cards_in_hand = self.players[owner].hand.len() as i32;
        let turn_end: arrayvec::ArrayVec<KRef, MAX_HAND_SIZE> =
            self.players[owner].hand.iter().copied().filter(|&k| self.cards[k as usize].turn_end_in_hand).collect();
        let ethereal: Vec<KRef> = self.players[owner].hand.iter().copied()
            .filter(|&k| !turn_end.contains(&k) && self.cards[k as usize].is_ethereal()).collect();
        for k in ethereal {
            let p = &mut self.players[owner];
            if let Some(pos) = p.hand.iter().position(|&x| x == k) {
                p.hand.remove(pos);
            }
            p.exhaust.push(k);
            self.fire_after_card_changed_piles(k, PileKind::Hand, None);
            self.end_defer_depth += 1;
            // fire: core/combat.py:1955
            self.fire_after_card_exhausted_kind(k, true);
            self.end_defer_depth -= 1;
            if self.is_over_or_ending() {
                return false;
            }
        }
        // Turn-end-in-hand cards resolve from Play (r7 core/combat.py _resolve_end_of_turn_hand).
        for k in turn_end {
            let p = &mut self.players[owner];
            let Some(pos) = p.hand.iter().position(|&x| x == k) else { continue };
            p.hand.remove(pos);
            p.play.push(k);
            self.fire_after_card_changed_piles(k, PileKind::Hand, None);
            self.end_defer_depth += 1;
            if self.cards[k as usize].turn_end_in_hand {
                self.card_turn_end_in_hand(k, cards_in_hand);
            }
            self.end_defer_depth -= 1;
            if self.is_over_or_ending() {
                return false;
            }
            if self.cr(owner).is_dead() {
                // HandlePlayerDeath already removed every card of the dead player.
                return true;
            }
            let p = &mut self.players[owner];
            if let Some(i) = p.play.iter().position(|&x| x == k) {
                p.play.remove(i);
            }
            if self.cards[k as usize].is_ethereal() {
                self.players[owner].exhaust.push(k);
                self.fire_after_card_changed_piles(k, PileKind::Play, None);
                // fire: core/combat.py:1982
                self.fire_after_card_exhausted_kind(k, true);
            } else {
                self.players[owner].discard.push(k);
                self.fire_after_card_changed_piles(k, PileKind::Play, None);
                self.fire_after_card_discarded(k);
            }
        }
        true
    }

    /// C# phase-two hand flush for `owner` (retained cards stay, in hand order).
    fn flush_hand(&mut self, owner: CId) {
        let retained: Vec<KRef> = self.players[owner].hand.iter().copied().filter(|&k| self.cards[k as usize].should_retain_this_turn()).collect();
        let remaining: Vec<KRef> = self.players[owner].hand.iter().copied().filter(|k| !retained.contains(k)).collect();
        let flush = self.should_flush(owner);
        if flush {
            for k in remaining {
                self.move_card_to_pile(k, PileKind::Discard, PilePos::Bottom);
                self.fire_after_card_discarded(k);
            }
            let p = &mut self.players[owner];
            p.hand.clear();
            p.hand.extend(retained);
        }
        // C# WellLaidPlansPower.ShouldFlush / RunicPyramid.ShouldFlush:
        // skipping the flush leaves the actual hand unchanged, including order.
        // Regrouping retained/non-retained cards would silently reorder it.
        // Hook.IterateCombatHookListeners (Hook.cs:28-33) yields no listeners
        // while IsOverOrEnding && !IsStarting; this is not token cancellation.
        if !self.is_over_or_ending() {
            let retained: Vec<_> = self.players[owner].hand.iter().copied().collect();
            self.relic_after_flush(owner, &retained);
        }
    }

    /// r7 discard history, also used by hand flush. C# only dispatches
    /// AfterCardDiscarded for CardCmd.DiscardAndDraw, not hand flushing.
    pub(crate) fn fire_after_card_discarded(&mut self, _k: KRef) {
        self.hist.discarded_this_turn += 1;
        self.hist.discarded_by_owner_this_turn[self.cards[_k as usize].owner as usize] += 1;
    }

    /// `CombatState.discard_cards(cards, draw_count)` (core/combat.py:3038): hand cards go to
    /// `owner`'s discard pile, then `owner` draws, then every discarded Sly card auto-plays.
    pub fn discard_cards(&mut self, owner: CId, cards: &[KRef], draw_count: i32) {
        let mut sly: arrayvec::ArrayVec<KRef, MAX_HAND_SIZE> = arrayvec::ArrayVec::new();
        for &k in cards {
            let p = &mut self.players[owner];
            let Some(pos) = p.hand.iter().position(|&x| x == k) else { continue };
            p.hand.remove(pos);
            p.discard.push(k);
            self.fire_after_card_changed_piles(k, PileKind::Hand, None);
            if self.cards[k as usize].is_sly() {
                sly.push(k);
            }
            self.fire_after_card_discarded(k);
            // Commands/CardCmd.cs:153-154: explicit discard history then hook.
            self.fire_relic_after_card_discarded(k);
        }
        if draw_count > 0 {
            self.draw_cards(owner, draw_count, false);
        }
        for k in sly {
            self.auto_play_card(k, None, false);
        }
    }

    fn cleanup_cards_end_of_turn(&mut self) {
        for owner in 0..self.players.len() as CId {
            if self.side == Side::Player && !self.is_player_taking_turn(owner) { continue; }
            for k in self.pile_cards(owner) {
                self.cards[k as usize].end_of_turn_cleanup();
            }
        }
    }

    // ───────────────────────────── enemy turn ─────────────────────────────

    fn execute_enemy_turn(&mut self) {
        self.between_stage("enemy_start", None);
        self.extra_turn_players = 0;
        self.side = Side::Enemy;
        if self.players.len()>1 && self.run.choice_scheduling.is_live() {self.action_queue.phase("enemy_turn",self.round);}
        self.hist.reset_side_turn();
        self.hist.played_this_turn.clear();
        self.hist.energy_spent_this_turn.fill(0);
        self.fire(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy), None);
        for e in self.alive_enemies() {
            self.clear_block(e);
            self.fire(Hook::AfterBlockCleared, Ev::Creature(e), None);
        }
        self.fire_after_side_turn_start(Side::Enemy);
        self.check_combat_end();
        if self.is_over {
            return;
        }
        self.continue_enemy_moves(0, false);
    }

    /// r7 `_continue_enemy_moves`: a pending choice is observed after the current monster move,
    /// before `on_move_performed`, and resumes from the following enemy once resolved.
    fn continue_enemy_moves(&mut self, start_index: usize, resume_player_turn: bool) {
        let with_turns: Vec<CId> = self.enemies.iter().copied().filter(|&e| !self.cr(e).escaped).collect();
        for (index, e) in with_turns.into_iter().enumerate().skip(start_index) {
            let Some(ai) = self.cr(e).ai.as_ref() else { continue };
            let mv = if ai.next_unset { u8::MAX - 1 } else { ai.current };
            if mv == u8::MAX - 1 {
                // UNSET_MOVE performs nothing.
                if let Some(ai) = self.cr_mut(e).ai.as_mut() { ai.performed_first = true; }
            } else {
                self.between_stage("enemy_action", Some(e));
                self.perform_enemy_move(e, mv);
                if self.pending_choice.is_some() {
                    self.pending_turn_setup = Some(TurnStage::EnemyAfterMove {
                        enemy: e,
                        mv,
                        next_index: index + 1,
                    });
                    return;
                }
                if let Some(ai) = self.cr_mut(e).ai.as_mut() {
                    ai.on_move_performed(mv);
                }
            }
            self.check_combat_end();
            if self.is_over {
                return;
            }
        }
        self.finish_enemy_turn(resume_player_turn);
    }

    fn finish_enemy_move_after_choice(&mut self, enemy: CId, mv: u8, next_index: usize) {
        if let Some(ai) = self.cr_mut(enemy).ai.as_mut() {
            ai.on_move_performed(mv);
        }
        self.check_combat_end();
        if self.is_over {
            return;
        }
        self.continue_enemy_moves(next_index, true);
    }

    /// r7 `_finish_enemy_turn`.
    fn finish_enemy_turn(&mut self, resume_player_turn: bool) {
        self.between_stage("enemy_end", None);
        self.fire_before_turn_end(Side::Enemy);
        self.check_combat_end();
        if !self.is_over {
            self.cleanup_cards_end_of_turn();
            self.fire_after_turn_end(Side::Enemy);
            // fire: core/combat.py:2099
            self.check_combat_end();
        }
        if self.is_over {
            // CombatManager.EndEnemyTurn calls CheckWinCondition, then SwitchSides:
            // IsCombatEnding returns false once IsInProgress is false. A side-end
            // victory (e.g. Doom) advances the side/round, but does not StartTurn.
            self.side = Side::Player;
            self.round += 1;
            for player in self.players.0.iter_mut() { player.turn_number += 1; }
            return;
        }
        if self.pending_choice.is_some() || !resume_player_turn {
            return;
        }
        self.round += 1;
        self.start_player_turn();
    }

    /// `MoveState.perform`.
    fn perform_enemy_move(&mut self, e: CId, mv: u8) {
        if let Some(ai) = self.cr_mut(e).ai.as_mut() {
            ai.mark_performing(mv);
        }
        self.flush_pending_attack();
        if mv == STUNNED_MOVE {
            // fire: core/combat.py:3830-3833
            self.power_on_stunned_move(e);
        } else {
            self.monster_move_effect(e, mv);
        }
        self.flush_pending_attack();
    }
}

pub const STUNNED_MOVE: u8 = crate::monster::STUNNED;

#[cfg(test)]
mod ball_transfer_tests {
    use super::*;
    use crate::run::{RunManager, RunOptions};
    use crate::rng::Rng;

    fn combat() -> Combat {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(80, 80, 3, 0, &[]);
        c.add_player(80, 80, 3, 0, &[]);
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(7));
        c
    }

    #[test]
    fn transferred_exhaust_is_not_an_exhaust_event_and_history_keeps_actor() {
        let mut c = combat();
        let ball = c.new_card(0, CardId::TheBall, false).unwrap();
        let midnight = c.new_card(1, CardId::Midnight, false).unwrap();
        c.players[1].hand.push(midnight);
        c.finish_card_play(ball);
        c.move_to_play_result(ball, PlayResult { pile: PileKind::Exhaust, pos: PilePos::Bottom, owner: 1 });
        assert_eq!(c.cards[ball as usize].owner, 1);
        assert!(c.players[1].exhaust.contains(&ball));
        assert_eq!(c.hist.exhausted_this_turn, 0);
        assert_eq!(c.hist.exhausted_combat, 0);
        assert_eq!(c.cards[midnight as usize].cost, 12);
        assert_eq!(c.hist.played_this_turn, vec![(0, ball)]);
        // Finisher must count the attack for the caster, never for the receiver.
        let enemy = c.enemies[0];
        let f = c.new_card(1, CardId::Finisher, false).unwrap();
        let hp = c.cr(enemy).hp;
        c.play_card_effect(f, Some(enemy));
        c.flush_pending_attack();
        assert_eq!(c.cr(enemy).hp, hp);
        c.cards[f as usize].owner = 0;
        c.play_card_effect(f, Some(enemy));
        c.flush_pending_attack();
        assert!(c.cr(enemy).hp < hp);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn play_result_to_a_full_hand_goes_to_discard_like_card_pile_cmd_add() {
        let mut c = combat();
        for _ in 0..MAX_HAND_SIZE {
            let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
            c.players[0].hand.push(k);
        }
        let played = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.finish_card_play(played);
        // Feral's ModifyCardPlayResultLocation: Hand/Top; the hand filled during the play.
        c.move_to_play_result(played, PlayResult { pile: PileKind::Hand, pos: PilePos::Top, owner: 0 });
        assert_eq!(c.players[0].hand.len(), MAX_HAND_SIZE);
        assert!(!c.players[0].hand.contains(&played));
        assert_eq!(c.players[0].discard.first(), Some(&played));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn none_and_failed_give_keep_original_owner_and_exhaust_fallback_fires() {
        let mut c = combat();
        let ball = c.new_card(0, CardId::TheBall, false).unwrap();
        c.move_to_play_result(ball, PlayResult { pile: PileKind::None, pos: PilePos::None, owner: 1 });
        assert_eq!(c.cards[ball as usize].owner, 0);
        c.cr_mut(1).hp = 0;
        c.move_to_play_result(ball, PlayResult { pile: PileKind::Draw, pos: PilePos::Random, owner: 1 });
        assert!(c.players[0].draw.contains(&ball));
        assert_eq!(c.cards[ball as usize].owner, 0);
        c.players[0].draw.clear();
        c.move_to_play_result(ball, PlayResult { pile: PileKind::Exhaust, pos: PilePos::Bottom, owner: 1 });
        assert!(c.players[0].exhaust.contains(&ball));
        assert_eq!(c.hist.exhausted_combat, 1);
        let another = c.new_card(0, CardId::TheBall, false).unwrap();
        c.is_ending = true;
        let shuffle = c.run.rngs.shuffle.counter;
        c.move_to_play_result(another, PlayResult { pile: PileKind::Draw, pos: PilePos::Random, owner: 2 });
        assert_eq!(c.cards[another as usize].owner, 0);
        assert!(c.players[0].play.contains(&another));
        assert!(!c.players[0].draw.contains(&another));
        assert_eq!(c.run.rngs.shuffle.counter, shuffle);
    }

    #[test]
    fn ball_selects_among_multiple_living_teammates_in_seat_order() {
        let mut c = combat();
        let ball = c.new_card(0, CardId::TheBall, false).unwrap();
        let mut rng = c.run.rngs.combat_targets.clone();
        let expected = [1, 2][rng.choice_index(2)];
        let result = c.card_play_result(0, ball, false, false, 1);
        assert_eq!(result.owner, expected);
        assert_eq!(result.pile, PileKind::Draw);
        assert_eq!(result.pos, PilePos::Random);
        assert_eq!(c.run.rngs.combat_targets.counter, rng.counter);
    }
}
