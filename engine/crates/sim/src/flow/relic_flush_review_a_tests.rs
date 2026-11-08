use super::*;
use crate::run::{RunManager, RunOptions};

#[test]
fn bookmark_after_flush_is_cancelled_when_combat_has_ended() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.run.relics.push(crate::relic::Relic::new(RelicId::Bookmark));
    let keep = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.cards[keep as usize].keywords |= KW_RETAIN;
    c.players[0].hand.push(keep);
    let rng = c.run.rngs.combat_card_selection.counter;
    c.is_over = true;
    c.flush_hand(0);
    assert_eq!(c.cards[keep as usize].local_energy_cost(), 1);
    assert_eq!(c.run.rngs.combat_card_selection.counter, rng);
}
