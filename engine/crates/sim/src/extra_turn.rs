//! C# CombatManager.SwitchFromPlayerToEnemySide / SwitchSides extra-player list.

use crate::card::CId;
use crate::combat::Combat;
use crate::tables::PowerId;

impl Combat {
    pub fn is_player_taking_turn(&self, owner: CId) -> bool {
        self.players.is_player(owner)
            && (self.extra_turn_players == 0 || self.extra_turn_players & (1 << owner) != 0)
    }

    /// Evaluate every player before dispatching any AfterTakingExtraTurn hook.
    pub(crate) fn should_take_extra_turn_now(&mut self) -> bool {
        let mut mask = 0;
        for owner in 0..self.players.len() as CId {
            let ambergris = self.find_power(owner, PowerId::Ambergris)
                .is_some_and(|p| self.powers[p as usize].amount > 0 && self.players[owner].hooks_active);
            let relic = (0..self.run.relics.len())
                .any(|r| self.relic_should_take_extra_turn(r, owner) == Some(true));
            if ambergris || relic { mask |= 1 << owner; }
        }
        self.extra_turn_players = mask;
        mask != 0
    }

    pub(crate) fn fire_after_taking_extra_turn_now(&mut self) {
        for owner in 0..self.players.len() as CId {
            if !self.is_player_taking_turn(owner) { continue; }
            if let Some(p) = self.find_power(owner, PowerId::Ambergris) {
                if self.powers[p as usize].amount > 0 {
                    self.modify_power_instance(p, -1, None, None);
                }
            }
            for r in 0..self.run.relics.len() {
                self.relic_after_taking_extra_turn(r, owner);
            }
        }
    }
}
