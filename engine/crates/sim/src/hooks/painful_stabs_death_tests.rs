use super::*;
use crate::{rng::Rng, run::{RunManager, RunOptions}};

#[test]
fn painful_stabs_does_not_allocate_status_cards_for_a_dead_receiver() {
    for seats in 2..=4 {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        let attacker = c.enemies[0];
        c.apply_power_to(attacker, PowerId::PainfulStabs, 2, Some(attacker));
        for owner in 0..seats {
            c.players[owner].discard.clear();
            c.completed_attack_results.push((owner, 5));
        }
        // The completed attack contains a lethal DamageResult; death handling
        // has already removed seat zero's cards before AfterAttack runs.
        c.creatures[0].hp = 0;
        let before = c.cards.len();
        c.fire(Hook::AfterAttack, Ev::Attack {
            attacker: Some(attacker), target: None, props: Props::MOVE, card: None,
        }, None);
        assert!(c.players[0].discard.is_empty());
        assert_eq!(c.cards.len() - before, 2 * (seats as usize - 1));
        for owner in 1..seats { assert_eq!(c.players[owner].discard.len(), 2); }
        assert_eq!(c.fault, None);
    }
}
