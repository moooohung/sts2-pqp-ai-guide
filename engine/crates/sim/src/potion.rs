//! r7 combat potion use (core/combat.py `can_use_potion`, `_resolve_potion_target`,
//! `use_potion`). Potion effects live in `content/potions.rs`.

use crate::card::CId;
use crate::combat::{Combat, PLAYER};
use crate::creature::Side;
use crate::hooks::Ev;
use crate::tables::*;

impl Combat {
    /// PotionCmd.Discard -> PotionModel.Discard -> AfterPotionDiscarded.
    /// Not potion use: no effect, target, energy, or AfterPotionUsed callback.
    pub fn discard_potion_for_player(&mut self,owner:CId,slot:usize)->Result<(),&'static str>{
        if !self.can_discard_potion_for_player(owner,slot){return Err("potion discard not legal in owner play phase");}
        if owner==PLAYER{self.run.potions[slot]=None;}
        else{self.players[owner].potions.as_mut().unwrap()[slot]=None;}
        self.fire_relic_after_potion_discarded(owner);
        Ok(())
    }
    /// DiscardPotionGameAction.ActionType is CombatPlayPhaseOnly; end-turn
    /// cancellation uses the same owner scheduling barrier as potion/card use.
    pub fn can_discard_potion_for_player(&self,owner:CId,slot:usize)->bool{
        !self.is_over&&self.side==Side::Player
            &&if self.players.len()==1 {self.pending_choice.is_none()}else{self.pending_choice_for_player(owner).is_none()}
            &&self.players.is_player(owner)&&self.cr(owner).is_alive()
            &&self.is_player_taking_turn(owner)
            &&self.players_ready_to_end_turn&(1<<owner)==0
            &&!self.player_turn_phase_one_done
            &&self.potion_slots_for_player(owner).and_then(|p|p.get(slot)).is_some_and(Option::is_some)
    }
    #[inline]
    fn potion_info(id: PotionId) -> &'static PotionInfo {
        &POTION_INFO[id.index()]
    }

    /// PotionModel.Owner selects inventory; IsValidTarget independently selects the recipient.
    pub fn potion_slots_for_player(&self, owner: CId) -> Option<&[Option<PotionId>]> {
        if !self.players.is_player(owner) {
            return None;
        }
        if owner == PLAYER {
            Some(&self.run.potions)
        } else {
            self.players[owner].potions.as_deref()
        }
    }

    fn resolve_potion_target(&self, owner: CId, id: PotionId, target_index: Option<usize>) -> Option<CId> {
        match Self::potion_info(id).target {
            PotionTargetType::Self_ => Some(owner).filter(|_| target_index.is_none_or(|i| i == usize::from(owner))),
            PotionTargetType::AnyPlayer => {
                let seat = target_index.unwrap_or(usize::from(owner));
                (seat < self.players.len()).then_some(seat as CId)
                    .filter(|&p| self.cr(p).is_alive())
            }
            PotionTargetType::AnyEnemy => match target_index {
                Some(i) if i < self.enemies.len() => Some(self.enemies[i]).filter(|&e| self.can_hit(e)),
                Some(_) => None,
                None => self.hittable_enemies().first().copied(),
            },
            _ => None,
        }
    }

    pub fn can_use_potion(&self, slot: usize, target_index: Option<usize>) -> bool {
        self.can_use_potion_for_player(PLAYER, slot, target_index)
    }

    pub fn can_use_potion_for_player(&self, owner: CId, slot: usize, target_index: Option<usize>) -> bool {
        if self.is_over || self.side != Side::Player
            || if self.players.len() == 1 { self.pending_choice.is_some() }
                else { self.pending_choice_for_player(owner).is_some() } {
            return false;
        }
        if !self.players.is_player(owner) || !self.cr(owner).is_alive() || !self.is_player_taking_turn(owner) {
            return false;
        }
        let Some(Some(id)) = self.potion_slots_for_player(owner).and_then(|slots| slots.get(slot)).copied() else {
            return false;
        };
        let info = Self::potion_info(id);
        if !matches!(info.usage, PotionUsageType::CombatOnly | PotionUsageType::AnyTime) {
            return false;
        }
        match info.target {
            PotionTargetType::Self_ | PotionTargetType::AnyPlayer | PotionTargetType::AnyEnemy => self.resolve_potion_target(owner, id, target_index).is_some(),
            _ => true,
        }
    }

    /// `add_potion` (core/combat.py:2199): first empty slot below `max_potion_slots`, growing
    /// the slot list as r7 does. Returns false when every slot is full.
    pub fn add_potion(&mut self, id: PotionId) -> bool {
        for i in 0..self.run.max_potion_slots.max(0) as usize {
            if i >= self.run.potions.len() {
                self.run.potions.push(None);
            }
            if self.run.potions[i].is_none() {
                self.run.potions[i] = Some(id);
                return true;
            }
        }
        false
    }

    /// `use_potion(slot, target_index)`.
    pub fn use_potion(&mut self, slot: usize, target_index: Option<usize>) -> bool {
        self.use_potion_for_player(PLAYER, slot, target_index)
    }

    pub fn use_potion_for_player(&mut self, owner: CId, slot: usize, target_index: Option<usize>) -> bool {
        self.with_live_queue_action(|c| c.use_potion_action(owner, slot, target_index))
    }

    fn use_potion_action(&mut self, owner: CId, slot: usize, target_index: Option<usize>) -> bool {
        if !self.can_use_potion_for_player(owner, slot, target_index) {
            return false;
        }
        let id = if owner == PLAYER { self.run.potions[slot].take().unwrap() }
            else { self.players[owner].potions.as_mut().unwrap()[slot].take().unwrap() };
        let target = self.resolve_potion_target(owner, id, target_index);
        // fire: core/combat.py before_potion_used target context for SurroundedPower.
        let previous_target = self.active_target;
        self.active_target = target;
        self.fire(Hook::BeforePotionUsed, Ev::None, None);
        self.active_target = previous_target;
        let play_depth = self.pending_play.len();
        self.potion_effect_for_player(owner, id, target);
        // Every owner's selector blocks PotionModel.OnUseWrapper; its
        // AfterPotionUsed hook cannot run ahead of the suspended effect.
        // Teammate choices branch away and retain the early after-potion hook.
        // Frozen r7 core/combat.py:2390-2392 fires after_potion_used immediately
        // after potion.use(), even with a pending self-choice. C# awaits it
        // (PotionModel.cs:301-317); Reptile Trinket exposes the ordering difference.
        if self.pending_choice.as_ref().is_some_and(|choice| choice.player == owner) {
            self.pending_potion_after = Some((play_depth, owner, id, target));
            return true;
        }
        self.finish_potion_after_use(owner, id, target);
        true
    }

    pub(crate) fn finish_potion_after_use(&mut self, owner: CId, id: PotionId, target: Option<CId>) {
        // PotionModel.OnUseWrapper:301-317 awaits the owner's effect before
        // AfterPotionUsed, and skips owner callbacks if the effect killed them.
        // relic fire: core/hooks.py:880
        if self.cr(owner).is_alive() {
            self.fire_relic_after_potion_used(owner, id, target);
        }
        self.check_combat_end();
    }
}
