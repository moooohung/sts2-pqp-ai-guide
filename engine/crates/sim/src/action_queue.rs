//! Deterministic owner FIFOs and host-order execution for multiplayer combat.
use crate::{card::{CId, KRef}, combat::Combat, creature::Side, tables::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatAction {
    PlayCard { card: KRef, target: Option<CId> },
    UsePotion { slot: usize, target: Option<CId> },
    EndTurn,
    UnendTurn,
    ReadyForEnemyTurn,
    Hook,
}
impl CombatAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::PlayCard { .. } => "play_card", Self::UsePotion { .. } => "use_potion",
            Self::EndTurn => "end_turn", Self::UnendTurn => "unend_turn",
            Self::ReadyForEnemyTurn => "ready_for_enemy_turn", Self::Hook => "hook",
        }
    }
    fn player_driven(self) -> bool { !matches!(self, Self::Hook | Self::ReadyForEnemyTurn) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State { Waiting, Choice, Resume }
#[derive(Clone, Debug)]
struct Entry {
    id: u64, original: u64, owner: CId, turn: i32, action: CombatAction,
    parent: Option<u64>,
    state: State, answer: Option<Option<usize>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueEvent {
    pub sequence: u64, pub id: Option<u64>, pub original_id: Option<u64>, pub owner: Option<CId>,
    pub parent: Option<u64>, pub event: &'static str, pub action: Option<CombatAction>,
    pub turn: Option<i32>, pub round: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunQueueEvent { pub combat: u64, pub event: QueueEvent }
#[derive(Clone, Debug, Default)]
pub struct CombatQueue {
    pub(crate) run_combat_id: Option<u64>,
    next_id: u64, next_sequence: u64, entries: Vec<Entry>,
    pub events: Vec<QueueEvent>,
    pub(crate) run_sequence_offset: u64,
    executing: Option<(CId, u64)>,
    ending_turn: Option<i32>,
    ended: bool,
}
impl CombatQueue {
    pub(crate) fn next_sequence(&self) -> u64 { self.next_sequence }
    fn record(&mut self, entry: &Entry, event: &'static str) {
        self.events.push(QueueEvent { sequence: self.next_sequence, id: Some(entry.id),
            original_id: Some(entry.original), owner: Some(entry.owner), parent: entry.parent,
            event, action: Some(entry.action), turn: Some(entry.turn), round: None });
        self.next_sequence += 1;
    }
    pub(crate) fn phase(&mut self, event: &'static str, round: i32) {
        self.events.push(QueueEvent {sequence:self.next_sequence,id:None,original_id:None,owner:None,
            parent:self.executing.map(|(_,id)|id),event,action:None,turn:None,round:Some(round)});
        self.next_sequence+=1;
    }
    pub(crate) fn combat_ended(&mut self, round:i32) {
        if !self.ended {self.ended=true;self.phase("combat_end",round);}
    }
    fn id(&mut self) -> u64 { let id = self.next_id; self.next_id += 1; id }
    fn enqueue(&mut self, owner: CId, turn: i32, action: CombatAction, state: State) {
        let id = self.id();
        let entry = Entry { id, original: id, owner, turn, action, state, answer: None,
            parent:self.executing.map(|(_,id)|id) };
        self.record(&entry, "enqueue");
        self.entries.push(entry);
    }
    fn ready(&self) -> Option<usize> {
        self.entries.iter().enumerate().filter(|(i, entry)| entry.state != State::Choice
            && !self.entries[..*i].iter().any(|prior| prior.owner == entry.owner))
            .min_by_key(|(_, entry)| entry.id).map(|(i, _)| i)
    }
    fn cancel_waiting(&mut self, mut predicate: impl FnMut(&Entry) -> bool) {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].state == State::Waiting && predicate(&self.entries[i]) {
                let entry = self.entries.remove(i); self.record(&entry, "cancel");
            } else { i += 1; }
        }
    }
}

impl Combat {
    pub fn accepts_combat_play_requests(&self) -> bool {
        self.side==Side::Player && !self.is_over && !self.is_ending && self.fault.is_none()
            && self.action_queue.ending_turn!=Some(self.turn_count)
    }
    pub fn can_unend_player_turn(&self, owner: CId) -> bool {
        self.players.is_player(owner) && self.cr(owner).is_alive()
            && self.is_player_taking_turn(owner) && self.is_player_ready_to_end_turn(owner)
            && self.side == Side::Player && !self.is_over && self.fault.is_none()
            && !self.player_turn_phase_one_done && self.action_queue.ending_turn != Some(self.turn_count)
    }

    pub(crate) fn register_queue_choices(&mut self) {
        for owner in 0..self.players.len() as CId {
            let choices = self.pending_choice_count_for_player(owner);
            if choices == 0 { continue; }
            // The original CardSelectCmd reopens an owner who had already marked
            // themselves ready whenever a selector is raised during player-side
            // combat. This must happen before the all-ready barrier is claimed.
            if self.side == Side::Player && self.is_player_ready_to_end_turn(owner)
                && self.action_queue.ending_turn != Some(self.turn_count)
            {
                self.players_ready_to_end_turn &= !(1 << owner);
            }
            let mut queued = self.action_queue.entries.iter()
                .filter(|entry| entry.owner == owner && entry.state == State::Choice).count()
                + usize::from(self.action_queue.executing.is_some_and(|(actor, _)| actor == owner));
            while queued < choices {
                let turn = self.players[owner].turn_number;
                self.action_queue.enqueue(owner, turn, CombatAction::Hook, State::Choice);
                let entry = self.action_queue.entries.last().unwrap().clone();
                self.action_queue.record(&entry, "pause");
                queued += 1;
            }
        }
    }

    pub fn enqueue_player_action(&mut self, owner: CId, action: CombatAction) -> Result<(), &'static str> {
        if !self.players.is_player(owner) { return Err("invalid queue owner"); }
        if self.is_over || self.fault.is_some() { return Err("combat queue is closed"); }
        self.register_queue_choices();
        let turn = self.players[owner].turn_number;
        self.action_queue.enqueue(owner, turn, action, State::Waiting);
        if self.action_queue.ending_turn == Some(self.turn_count) && action.player_driven() {
            self.action_queue.cancel_waiting(|entry| entry.action.player_driven());
        }
        self.drain_combat_queue();
        self.fault.map_or(Ok(()), Err)
    }

    pub fn resume_queued_choice(&mut self, owner: CId, index: Option<usize>) -> Result<(), &'static str> {
        if self.is_over || self.fault.is_some() { return Err("combat queue is closed"); }
        if self.action_queue.executing.is_some() { return Err("cannot submit a choice inside execution"); }
        self.register_queue_choices();
        let choice = self.pending_choice_for_player(owner).ok_or("owner has no queue choice")?;
        let selected = self.choice_selected_for_player(owner).ok_or("queue selection missing")?;
        let options = self.choice_options_for_player(owner).ok_or("queue options missing")?;
        let completes = if !choice.is_multi() {
            index.is_some_and(|i| i < options.len()) || index.is_none() && choice.allow_skip
        } else {
            index.is_none() && (selected.len() >= choice.min as usize || choice.allow_skip && selected.is_empty())
                || matches!(choice.resolver.src, crate::choice::ChoiceSrc::Card(CardId::Dredge))
                    && index.is_some_and(|i| i < options.len() && !selected.contains(&(i as u16)))
                    && selected.len() + 1 == choice.max as usize
        };
        if !completes {
            if !self.resolve_pending_choice_for_player(owner, index) { return Err("queue choice refused"); }
            return self.fault.map_or(Ok(()), Err);
        }
        let at = self.action_queue.entries.iter().position(|entry| entry.owner == owner && entry.state == State::Choice)
            .ok_or("paused queue action missing")?;
        let id = self.action_queue.id();
        let entry = &mut self.action_queue.entries[at];
        entry.id = id; entry.state = State::Resume; entry.answer = Some(index);
        let entry = entry.clone(); self.action_queue.record(&entry, "resume");
        self.drain_combat_queue();
        self.fault.map_or(Ok(()), Err)
    }

    fn execute_queue_entry(&mut self, entry: &Entry) -> bool {
        if entry.state == State::Resume {
            return self.resolve_pending_choice_for_player(entry.owner, entry.answer.unwrap());
        }
        if self.players[entry.owner].turn_number != entry.turn { return false; }
        match entry.action {
            CombatAction::PlayCard { card, target } => {
                if !self.cr(entry.owner).is_alive() || self.is_player_ready_to_end_turn(entry.owner)
                    || self.side != Side::Player { return false; }
                let Some(hand) = self.players[entry.owner].hand.iter().position(|&k| k == card) else { return false; };
                let target_index = match self.card_target_type(card) {
                    TargetType::AnyEnemy => {
                        let Some(index) = target.and_then(|id| self.enemies.iter().position(|&enemy| enemy == id && self.can_hit(id)))
                            else { return false; }; Some(index)
                    }
                    TargetType::AnyAlly => {
                        let Some(index) = target.and_then(|id| self.player_side().iter().position(|&ally| ally == id && self.cr(id).is_alive()))
                            else { return false; }; Some(index)
                    }
                    _ => None,
                };
                self.play_card(entry.owner, hand, target_index)
            }
            CombatAction::UsePotion { slot, target } => {
                let Some(potion) = self.potion_slots_for_player(entry.owner)
                    .and_then(|slots| slots.get(slot)).copied().flatten() else { return false; };
                if self.is_player_ready_to_end_turn(entry.owner) && POTION_INFO[potion.index()].usage != PotionUsageType::AnyTime {
                    return false;
                }
                let target = match POTION_INFO[potion.index()].target {
                    PotionTargetType::AnyEnemy => {
                        let Some(index) = target.and_then(|id| self.enemies.iter().position(|&enemy| enemy == id))
                            else { return false; }; Some(index)
                    }
                    PotionTargetType::AnyPlayer => {
                        let Some(id) = target.filter(|&id| self.players.is_player(id)) else { return false; };
                        Some(id as usize)
                    }
                    _ => None,
                };
                self.use_potion_for_player(entry.owner, slot, target)
            }
            CombatAction::EndTurn => {
                self.end_player_turn_for(entry.owner); true
            }
            CombatAction::UnendTurn => {
                if self.side != Side::Player || self.player_turn_phase_one_done
                    || self.action_queue.ending_turn == Some(self.turn_count) { return false; }
                self.players_ready_to_end_turn &= !(1 << entry.owner); true
            }
            CombatAction::ReadyForEnemyTurn => {
                if !self.needs_enemy_turn_acknowledgement(entry.owner) { return false; }
                self.set_ready_to_begin_enemy_turn(entry.owner); true
            }
            CombatAction::Hook => false,
        }
    }

    fn drain_combat_queue(&mut self) {
        if self.action_queue.executing.is_some() { return; }
        while let Some(at) = self.action_queue.ready() {
            let entry = self.action_queue.entries[at].clone();
            self.action_queue.executing = Some((entry.owner, entry.original));
            self.action_queue.record(&entry, "execute");
            // A Resume entry is paired with the currently active selector for
            // this owner. Parking it here would make a later selector for the
            // same owner jump the FIFO and receive the older resume answer.
            if entry.state != State::Resume { self.park_live_choice(); }
            let accepted = self.execute_queue_entry(&entry);
            // A selector already owned by another paused entry of this owner is
            // not a new choice raised by this action; re-pausing would orphan
            // that entry at the head of the owner's FIFO.
            let owned_elsewhere = self.action_queue.entries.iter().enumerate()
                .filter(|&(i, queued)| i != at && queued.owner == entry.owner && queued.state == State::Choice)
                .count();
            let suspended = accepted && self.pending_choice_for_player(entry.owner).is_some()
                && self.pending_choice_count_for_player(entry.owner) > owned_elsewhere
                && (matches!(entry.action, CombatAction::Hook) || self.has_executing_player_effect(entry.owner));
            if suspended {
                self.action_queue.entries[at].state = State::Choice;
                self.action_queue.entries[at].answer = None;
                self.action_queue.record(&entry, "pause");
                if self.pending_choice_for_player(entry.owner).is_some_and(|choice| choice.source == crate::combat::PileKind::Hand) {
                    self.action_queue.cancel_waiting(|queued| queued.owner == entry.owner
                        && matches!(queued.action, CombatAction::PlayCard { .. }));
                }
            } else {
                self.action_queue.entries.remove(at);
                self.action_queue.record(&entry, if accepted { "finish" } else { "cancel" });
            }
            // BranchingPlayerChoiceContext puts a choice requested for another
            // player on that player's hook queue. If that owner already has a
            // paused head, keep the older selector visible and park the newly
            // created selector behind it. The new hook still gets its enqueue
            // event now, with the executing action as parent.
            if let Some(chooser) = self.pending_choice.as_ref().map(|choice| choice.player)
                .filter(|&chooser| chooser != entry.owner
                    && self.action_queue.entries.iter().any(|queued|
                        queued.owner == chooser && queued.state == State::Choice))
            {
                self.park_live_choice();
                self.activate_choice_for_player(chooser);
            }
            self.register_queue_choices();
            self.action_queue.executing = None;
            if (0..self.players.len() as CId).all(|owner| self.is_player_ready_to_end_turn(owner))
                && self.side == Side::Player {
                self.action_queue.ending_turn = Some(self.turn_count);
                self.action_queue.cancel_waiting(|queued| queued.action.player_driven());
            }
            self.restore_first_live_choice();
            if self.is_over {
                self.action_queue.combat_ended(self.round);
                let remaining = std::mem::take(&mut self.action_queue.entries);
                for queued in remaining { self.action_queue.record(&queued, "cancel"); }
                break;
            }
            if self.fault.is_some() { break; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rng::Rng, run::{RunManager, RunOptions}};
    fn combat() -> Combat {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        c.add_player(50, 50, 3, 0, &[]);
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        c
    }
    fn play(c: &mut Combat, owner: CId, id: CardId) {
        let card = c.new_card(owner, id, false).unwrap();
        c.players[owner].hand.push(card);
        c.enqueue_player_action(owner, CombatAction::PlayCard { card, target: Some(c.enemies[0]) }).unwrap();
    }
    #[test]
    fn teammate_debuff_and_attacks_execute_in_chosen_order_not_seat_order() {
        let mut c = combat(); let enemy = c.enemies[0]; let hp = c.cr(enemy).hp;
        play(&mut c, 1, CardId::Bash);
        play(&mut c, 0, CardId::StrikeIronclad);
        play(&mut c, 1, CardId::StrikeIronclad);
        assert_eq!(c.cr(enemy).hp, hp - 26);
        assert_eq!(c.action_queue.events.iter().filter(|e| e.event == "execute")
            .map(|e| e.owner).collect::<Vec<_>>(), vec![Some(1), Some(0), Some(1)]);
    }
    #[test]
    fn helix_drill_counts_only_energy_its_owner_spent_this_turn() {
        // C# HelixDrill sums EnergySpentEntry rows whose actor is the card owner.
        let mut c = combat(); let enemy = c.enemies[0]; let hp = c.cr(enemy).hp;
        play(&mut c, 0, CardId::StrikeIronclad);
        play(&mut c, 1, CardId::HelixDrill);
        assert_eq!(c.cr(enemy).hp, hp - 6);
        play(&mut c, 1, CardId::StrikeIronclad);
        play(&mut c, 1, CardId::HelixDrill);
        assert_eq!(c.cr(enemy).hp, hp - 6 - 6 - 3);
        assert_eq!(c.fault, None);
    }
    #[test]
    fn ending_one_owner_does_not_flush_hand_and_undo_reopens_only_that_owner() {
        let mut c = combat();
        let card = c.new_card(0, CardId::DefendIronclad, false).unwrap(); c.players[0].hand.push(card);
        c.enqueue_player_action(0, CombatAction::EndTurn).unwrap();
        assert_eq!(c.players[0].hand.as_slice(), &[card]);
        assert!(!c.is_player_ready_to_end_turn(1));
        play(&mut c, 1, CardId::StrikeIronclad);
        c.enqueue_player_action(0, CombatAction::UnendTurn).unwrap();
        assert!(!c.is_player_ready_to_end_turn(0));
        c.enqueue_player_action(0, CombatAction::EndTurn).unwrap();
        c.enqueue_player_action(1, CombatAction::EndTurn).unwrap();
        assert_eq!(c.side, Side::Player);
        c.enqueue_player_action(0, CombatAction::ReadyForEnemyTurn).unwrap();
        assert_eq!(c.side, Side::Player);
        c.enqueue_player_action(1, CombatAction::ReadyForEnemyTurn).unwrap();
        assert_eq!(c.round, 2);
    }
    #[test]
    fn a_paused_owner_does_not_block_teammates_and_resume_gets_a_new_order_id() {
        let mut c = combat();
        let discarded = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(discarded);
        let alternative = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].discard.push(alternative);
        play(&mut c, 0, CardId::Headbutt);
        assert!(c.pending_choice_for_player(0).is_some());
        play(&mut c, 1, CardId::StrikeIronclad);
        assert!(c.pending_choice_for_player(0).is_some());
        c.resume_queued_choice(0, Some(0)).unwrap();
        assert!(c.pending_choice_for_player(0).is_none());
        assert_eq!(c.players[0].draw.last(), Some(&discarded));
        let resumes: Vec<_> = c.action_queue.events.iter().filter(|e| e.event == "resume").collect();
        assert_eq!(resumes.len(), 1);
        assert!(resumes[0].id > resumes[0].original_id);
        assert_eq!(c.fault, None);
    }
    #[test]
    fn card_selection_for_an_ended_owner_reopens_that_owner() {
        let mut c = combat();
        c.players[1].potions = Some(vec![Some(PotionId::Attackpotion)]);
        c.enqueue_player_action(0, CombatAction::EndTurn).unwrap();
        assert!(c.is_player_ready_to_end_turn(0));
        c.enqueue_player_action(1, CombatAction::UsePotion { slot: 0, target: Some(0) }).unwrap();
        assert!(c.pending_choice_for_player(0).is_some());
        assert!(!c.is_player_ready_to_end_turn(0));
    }
    #[test]
    fn teammate_targeted_selector_waits_behind_that_owners_existing_choice() {
        let mut c = combat();
        c.run.all_cards_unlocked = Some(true);
        c.players[1].character = Some(crate::character::Character::Necrobinder);
        for id in [CardId::DefendIronclad, CardId::StrikeIronclad] {
            let card = c.new_card(1, id, false).unwrap();
            c.players[1].hand.push(card);
        }
        play(&mut c, 1, CardId::SculptingStrike);
        let first = c.choice_options_for_player(1).unwrap().to_vec();
        assert_eq!(first.len(), 2);

        c.run.potions = vec![Some(PotionId::Skillpotion)];
        assert!(c.can_use_potion_for_player(0, 0, Some(1)));
        c.enqueue_player_action(0, CombatAction::UsePotion { slot: 0, target: Some(1) }).unwrap();

        assert_eq!(c.potion_slots_for_player(0).unwrap()[0], None);
        assert_eq!(c.fault, None);
        assert_eq!(c.pending_choice_count_for_player(1), 2);
        assert_eq!(c.choice_options_for_player(1).unwrap(), first.as_slice());
        assert_eq!(c.action_queue.entries.iter()
            .filter(|entry| entry.owner == 1 && entry.state == State::Choice).count(), 2);

        c.resume_queued_choice(1, Some(0)).unwrap();
        assert_eq!(c.pending_choice_count_for_player(1), 1);
        let next = c.pending_choice_for_player(1).unwrap();
        assert!(next.allow_skip);
        assert_eq!(c.choice_options_for_player(1).unwrap().len(), 3);
        c.resume_queued_choice(1, None).unwrap();
        assert_eq!(c.pending_choice_count_for_player(1), 0);
        assert!(!c.action_queue.entries.iter()
            .any(|entry| entry.owner == 1 && entry.state == State::Choice));
        c.enqueue_player_action(1, CombatAction::EndTurn).unwrap();
        assert!(c.is_player_ready_to_end_turn(1));
        assert_eq!(c.fault, None);
    }
    #[test]
    fn two_stacked_teammate_selectors_leave_no_paused_entry_behind() {
        let mut c = combat();
        c.run.all_cards_unlocked = Some(true);
        c.players[1].character = Some(crate::character::Character::Necrobinder);
        c.run.potions = vec![Some(PotionId::Skillpotion), Some(PotionId::Skillpotion)];
        for slot in 0..2 {
            c.enqueue_player_action(0, CombatAction::UsePotion { slot, target: Some(1) }).unwrap();
        }
        assert_eq!(c.pending_choice_count_for_player(1), 2);
        c.resume_queued_choice(1, Some(0)).unwrap();
        c.resume_queued_choice(1, Some(0)).unwrap();
        assert_eq!(c.pending_choice_count_for_player(1), 0);
        assert!(!c.action_queue.entries.iter().any(|entry| entry.owner == 1 && entry.state == State::Choice));
        let enemy = c.enemies[0]; let hp = c.cr(enemy).hp;
        play(&mut c, 1, CardId::StrikeIronclad);
        assert_eq!(c.cr(enemy).hp, hp - 6);
        assert_eq!(c.fault, None);
    }
    #[test]
    fn combat_end_cancels_another_owners_suspended_action() {
        let mut c = combat();
        for id in [CardId::DefendIronclad, CardId::StrikeIronclad] {
            let card = c.new_card(0, id, false).unwrap(); c.players[0].discard.push(card);
        }
        play(&mut c, 0, CardId::Headbutt);
        assert!(c.pending_choice_for_player(0).is_some());
        let enemy = c.enemies[0]; c.cr_mut(enemy).hp = 1;
        play(&mut c, 1, CardId::StrikeIronclad);
        assert!(c.is_over);
        assert!(c.action_queue.events.iter().any(|event| event.owner == Some(0) && event.event == "cancel"));
        assert!(c.resume_queued_choice(0, Some(0)).is_err());
    }
}
