//! r7 `monsters/state_machine.py` `MonsterAI`, driven by the generated `MONSTER_DEFS` tables.
//! Move effects and conditional/callable branches are hand-ported per monster in `moves.rs`.

use crate::rng::Rng;
use crate::tables::*;
use arrayvec::ArrayVec;

/// Sentinel state index for r7's injected `STUNNED` move (`combat.stun_enemy`).
pub const STUNNED: u8 = u8::MAX;
/// Sentinel state index for IllusionPower's injected `REVIVE_MOVE`.
pub const ILLUSION_REVIVE: u8 = u8::MAX - 2;
const LOG_CAP: usize = 32;

/// Body of an injected stun move (C# `CreatureCmd.Stun` takes the move to perform).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StunMove {
    /// Plain stun: does nothing.
    #[default]
    Nothing,
    /// `SlumberingBeetle.WakeUpMove` (Monsters/SlumberingBeetle.cs): removes Plating.
    BeetleWakeUp,
    /// `BowlbugRock.DizzyMove` (Monsters/BowlbugRock.cs): `IsOffBalance = false`.
    BowlbugRockDizzy,
}

#[derive(Clone, Debug)]
pub struct MonsterAi {
    pub kind: MonsterKind,
    /// 0: A0 export, 1: Deadly/A10 export, 2: Tough-only/A8 export.
    pub band: u8,
    pub current: u8,
    /// Most recent `state_log` entries (oldest first); `used` remembers every logged state.
    pub log: ArrayVec<u8, LOG_CAP>,
    pub used: u64,
    pub performed_first: bool,
    pub next_unset: bool,
    /// Last roll directly resumed an injected stun's follow-up, without evaluating
    /// the canonical move's conditional branch (relevant to natural sleep wake).
    pub rolled_stun_follow_up: bool,
    /// `MoveState._performed_at_least_once`, one bit per state index.
    pub performed_once: u64,
    /// Follow-up of an injected stun; `current == STUNNED` while it is pending.
    pub stun_follow_up: u8,
    /// Move body of the pending stun (`CreatureCmd.Stun(creature, move, next)`).
    pub stun_move: StunMove,
    /// Follow-up selected when IllusionPower injects REVIVE_MOVE.
    pub revive_follow_up: u8,
    pub revive_performed_once: bool,
    /// Per-monster construction flags read by conditional branches (`is_alone`, `is_front`, ...).
    pub flags: u32,
    pub rng: Rng,
}

impl MonsterAi {
    #[inline]
    pub fn def(&self) -> &'static MonsterDef {
        monster_def(self.kind, self.band as usize)
    }

    #[inline]
    pub fn state(&self, index: u8) -> &'static StateDef {
        &self.def().states[index as usize]
    }

    pub fn is_move(&self, index: u8) -> bool {
        index == STUNNED || index == ILLUSION_REVIVE || matches!(self.state(index).kind, StateKind::Move { .. })
    }

    /// Move state id as r7 names it.
    pub fn state_id(&self, index: u8) -> &'static str {
        match index {
            STUNNED => "STUNNED",
            ILLUSION_REVIVE => "REVIVE_MOVE",
            _ => self.state(index).id,
        }
    }

    /// `MonsterAI.peek_move`: the selected move, without resolving branches.
    pub fn peek_move(&self) -> Option<u8> {
        if self.next_unset || !self.is_move(self.current) { None } else { Some(self.current) }
    }

    pub(crate) fn can_transition_away(&self, index: u8) -> bool {
        match index {
            STUNNED => self.performed_once & (1 << 63) != 0,
            ILLUSION_REVIVE => self.revive_performed_once,
            _ => match self.state(index).kind {
                StateKind::Move { must_perform_once: true, .. } => self.performed_once & (1u64 << index) != 0,
                _ => true,
            },
        }
    }

    pub(crate) fn follow_up(&self, index: u8) -> u8 {
        if index == STUNNED {
            return self.stun_follow_up;
        }
        if index == ILLUSION_REVIVE {
            return self.revive_follow_up;
        }
        match self.state(index).kind {
            StateKind::Move { follow_up: Some(f), .. } => f,
            _ => panic!("MoveState '{}' has no follow_up_id", self.state_id(index)),
        }
    }

    fn on_exit_state(&mut self, index: u8) {
        if index == ILLUSION_REVIVE {
            self.revive_performed_once = false;
            return;
        }
        let bit = if index == STUNNED { 63 } else { index as u32 };
        self.performed_once &= !(1u64 << bit);
    }

    /// `WeightedBranch.get_weight`; `None` weight = callable resolved by the caller.
    fn branch_weight(&self, b: &BranchDef, callable: &dyn Fn(u8) -> f64) -> f64 {
        let target = b.target;
        match b.repeat {
            MoveRepeatType::UseOnlyOnce if self.used & (1u64 << target) != 0 => return 0.0,
            MoveRepeatType::CannotRepeat if self.log.last() == Some(&target) => return 0.0,
            MoveRepeatType::CanRepeatXTimes => {
                let consecutive = self.log.iter().rev().take_while(|&&s| s == target).count() as i32;
                debug_assert!(consecutive < LOG_CAP as i32);
                if consecutive >= b.max_times {
                    return 0.0;
                }
            }
            _ => {}
        }
        if b.cooldown > 0 {
            debug_assert!((b.cooldown as usize) <= LOG_CAP);
            let start = self.log.len().saturating_sub(b.cooldown as usize);
            if self.log[start..].contains(&target) {
                return 0.0;
            }
        }
        b.weight.unwrap_or_else(|| callable(target))
    }

    /// One branch step from a non-move state.
    fn branch_step(&self, index: u8, rng: &mut Rng, ctx: &BranchCtx) -> u8 {
        match self.state(index).kind {
            StateKind::Random { branches } => {
                let weights: ArrayVec<f64, 16> =
                    branches.iter().map(|b| self.branch_weight(b, &|t| (ctx.weight)(self, t))).collect();
                let total: f64 = weights.iter().sum();
                // C# RandomBranchState.GetNextState consumes NextFloat even when cooldowns
                // leave every weight at zero (Flyconid), then selects the first branch.
                let roll = rng.next_float(total);
                if total <= 0.0 {
                    return branches[0].target;
                }
                let mut cumulative = 0.0;
                for (b, &w) in branches.iter().zip(&weights) {
                    if w <= 0.0 {
                        continue;
                    }
                    cumulative += w;
                    if roll < cumulative {
                        return b.target;
                    }
                }
                branches.iter().zip(&weights).rev().find(|(_, w)| **w > 0.0).map(|(b, _)| b.target).unwrap()
            }
            StateKind::Conditional { targets } => (ctx.condition)(self, targets),
            StateKind::Move { .. } => unreachable!(),
        }
    }

    fn resolve_to_move(&mut self, rng: &mut Rng, ctx: &BranchCtx) {
        let mut safety = 100;
        while !self.is_move(self.current) {
            assert!(safety > 0, "Infinite loop in state machine resolution");
            safety -= 1;
            self.current = self.branch_step(self.current, rng, ctx);
        }
    }

    /// `MonsterAI.roll_move`.
    pub fn roll_move(&mut self, rng: &mut Rng, ctx: &BranchCtx) -> u8 {
        self.next_unset = false;
        let current = self.current;
        self.rolled_stun_follow_up = false;
        if !self.is_move(current) {
            self.resolve_to_move(rng, ctx);
            self.log_selected_move(self.current);
            return self.current;
        }
        if !self.performed_first || !self.can_transition_away(current) {
            return current;
        }
        let next = self.follow_up(current);
        self.rolled_stun_follow_up = current == STUNNED;
        self.on_exit_state(current);
        self.current = next;
        self.resolve_to_move(rng, ctx);
        // C# FindNextMoveState appends the selected move before it is performed.
        // This also logs the follow-up of an injected stun, even if it is the
        // same canonical move that was interrupted.
        self.log_selected_move(self.current);
        self.current
    }

    /// `MoveState.perform` bookkeeping (before the effect runs).
    pub fn mark_performing(&mut self, index: u8) {
        if index == ILLUSION_REVIVE {
            self.revive_performed_once = true;
            return;
        }
        let bit = if index == STUNNED { 63 } else { index as u32 };
        self.performed_once |= 1u64 << bit;
    }

    /// `MonsterAI.on_move_performed(performed_move)`.
    pub fn on_move_performed(&mut self, _index: u8) {
        self.performed_first = true;
    }

    /// C# MonsterMoveStateMachine constructor / FindNextMoveState StateLog.Add.
    /// ForceCurrentState (including injected stuns) never appends to this log.
    pub(crate) fn log_selected_move(&mut self, index: u8) {
        let in_logs = match index {
            STUNNED => false,
            ILLUSION_REVIVE => true,
            _ => match self.state(index).kind {
                StateKind::Move { in_logs, .. } => in_logs,
                _ => false,
            },
        };
        // Keep injected stuns out of no-repeat/cooldown history.
        if in_logs {
            if self.log.is_full() {
                self.log.remove(0);
            }
            self.log.push(index);
            if index != STUNNED && index != ILLUSION_REVIVE {
                self.used |= 1u64 << index;
            }
        }
    }

    pub fn intents(&self, index: u8) -> &'static [IntentDef] {
        static STUN: [IntentDef; 1] = [IntentDef { kind: IntentType::Stun, damage: 0, hits: 1, cards: 0 }];
        static HEAL: [IntentDef; 1] = [IntentDef { kind: IntentType::Heal, damage: 0, hits: 1, cards: 0 }];
        if index == STUNNED {
            return &STUN;
        }
        if index == ILLUSION_REVIVE {
            return &HEAL;
        }
        match self.state(index).kind {
            StateKind::Move { intents, .. } => intents,
            _ => &[],
        }
    }
}

/// Monster-specific branch logic: callable weights and conditional predicates.
pub struct BranchCtx<'a> {
    pub weight: &'a dyn Fn(&MonsterAi, u8) -> f64,
    pub condition: &'a dyn Fn(&MonsterAi, &'static [u8]) -> u8,
}
