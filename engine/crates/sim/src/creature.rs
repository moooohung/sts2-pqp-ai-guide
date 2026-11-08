//! r7 `core/creature.py` `Creature` and `powers/base.py` `PowerInstance` data.

use crate::card::CId;
use crate::monster::MonsterAi;
use crate::tables::*;
use smallvec::SmallVec;

/// Index of a power in `Combat::powers`. Removed powers stay in the arena, so a listener
/// snapshot taken before a removal still reaches them exactly like r7's list copies do.
pub type PRef = u16;

pub const BLOCK_CAP: i32 = 999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side { Player, Enemy }

#[derive(Clone, Debug)]
pub struct Power {
    pub id: PowerId,
    pub amount: i32,
    /// Class-specific scalar payload for r7 powers whose instances carry extra state.
    // fire: powers/turn_effects.py:654-659
    pub payload: i32,
    pub skip_next_tick: bool,
    pub applier: Option<CId>,
    pub ignore_next_instance: bool,
    pub owner: CId,
    /// Per-instance fields of the power's class (C# instance fields such as counters); they die
    /// with the instance like the game's.
    pub vars: [i32; 4],
    /// C# `PowerModel.Target`: the player creature an instanced per-player power is bound to
    /// (Thievery, Heist, Withering Presence); `None` for untargeted powers.
    pub target: Option<CId>,
}

impl Power {
    #[inline]
    pub fn meta(&self) -> &'static PowerMeta { power_meta(self.id) }
    /// r7 removal rule after hooks: `amount == 0` if negatives are allowed, else `<= 0`.
    #[inline]
    pub fn should_remove(&self) -> bool {
        if self.meta().allow_negative { self.amount == 0 } else { self.amount <= 0 }
    }
}

#[derive(Clone, Debug)]
pub struct Creature {
    pub max_hp: i32,
    pub hp: i32,
    pub block: i32,
    /// C# Creature.HpDisplay.IsInfinite: Hellraiser reads this visual state.
    pub hp_display_infinite: bool,
    /// Primary instance per PowerId, in Python dict order.
    pub powers: SmallVec<[PRef; 24]>,
    /// `_extra_power_instances` (instanced powers beyond the first).
    pub extra: SmallVec<[PRef; 8]>,
    pub side: Side,
    pub is_player: bool,
    // relic fire: core/creature.py:71-72
    pub is_pet: bool,
    pub pet_owner: Option<CId>,
    pub monster_id: &'static str,
    pub combat_id: u8,
    pub slot: Option<&'static str>,
    pub stars: i32,
    pub escaped: bool,
    pub death_processed: bool,
    /// Initial HP range of ranged-HP monsters (`initial_min_hp`/`initial_max_hp`).
    pub hp_range: Option<(i32, i32)>,
    pub ai: Option<MonsterAi>,
    /// False once `combat_state` is cleared (removed from combat after death or escape).
    pub in_combat: bool,
    /// OR of `PowerMeta::hooks` over every power ever attached (never cleared on removal, so it
    /// only over-approximates): hook dispatch skips a creature whose mask lacks the hook bit.
    pub hook_mask: u128,
}

impl Creature {
    pub fn player(max_hp: i32, hp: i32) -> Creature {
        Creature {
            max_hp, hp, block: 0, hp_display_infinite: false, powers: SmallVec::new(), extra: SmallVec::new(), side: Side::Player,
            is_player: true, is_pet: false, pet_owner: None, monster_id: "", combat_id: 0,
            slot: None, stars: 0, escaped: false,
            death_processed: false, hp_range: None, ai: None, in_combat: true, hook_mask: 0,
        }
    }

    /// Attach arena power `p` (of class `id`): instanced repeats go to `extra`.
    #[inline]
    pub fn attach_power(&mut self, p: PRef, id: PowerId, extra: bool) {
        if extra { self.extra.push(p) } else { self.powers.push(p) }
        self.hook_mask |= power_meta(id).hooks;
    }

    #[inline] pub fn is_alive(&self) -> bool { self.hp > 0 && !self.escaped }
    #[inline] pub fn is_dead(&self) -> bool { self.hp <= 0 }

    pub fn gain_block(&mut self, amount: i32) {
        if amount > 0 {
            self.block = (self.block + amount).min(BLOCK_CAP);
        }
    }

    /// Block absorbs damage; returns the amount blocked.
    pub fn damage_block(&mut self, amount: i32, unblockable: bool) -> i32 {
        if unblockable || self.block <= 0 {
            return 0;
        }
        let blocked = self.block.min(amount);
        self.block -= blocked;
        blocked
    }

    /// Power handles in C# `Creature.Powers` order: application order, instanced repeats
    /// interleaved where they were applied (arena handles grow with every new instance).
    #[inline]
    pub fn power_list(&self) -> SmallVec<[PRef; 32]> {
        let mut out = SmallVec::with_capacity(self.powers.len() + self.extra.len());
        out.extend(self.powers.iter().copied());
        out.extend(self.extra.iter().copied());
        // Removing an instance can promote a newer handle into an older primary slot.
        out.sort_unstable();
        out
    }
}
