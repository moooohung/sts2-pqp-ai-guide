//! Frozen-r7 treasure-room helpers.

use crate::combat::RunCtx;
use crate::tables::*;

pub(super) fn should_generate(run: &RunCtx) -> bool {
    run.owned_relics(crate::combat::PLAYER)
        .find(|r| r.id == RelicId::SilverCrucible)
        .is_none_or(|r| r.counter2 > 1)
}

pub(super) fn chest_gold(run: &mut RunCtx, relic_id: RelicId) -> i32 {
    if relic_id == RelicId::CursedPearl {
        return 0;
    }
    let mut gold = run.rngs.rewards.next_int_exclusive(42, 53) as i32;
    if run.ascension >= 3 {
        gold = (gold as f64 * 0.75) as i32;
    }
    gain_gold(run, gold)
}

/// PlayerCmd.cs:107-132 applies ModifyGoldGained before the final truncation.
pub(super) fn gain_gold(run: &mut RunCtx, amount: i32) -> i32 {
    run.gain_run_gold(amount)
}
