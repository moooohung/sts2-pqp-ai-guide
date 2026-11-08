use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId}};

#[test]
fn outbreak_with_doom_applies_then_triggers_poison_without_an_instant_doom_kill() {
    for owner in 0..4 { for upgraded in [false, true] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        c.start_combat();
        for e in c.enemies.clone() {
            c.cr_mut(e).hp = 100; c.cr_mut(e).max_hp = 100;
            c.apply_power_to(e, PowerId::Doom, 99, Some(owner));
        }
        let k = c.new_card(owner, CardId::OutbreakCard, upgraded).unwrap();
        c.players[owner].hand.push(k);
        c.players[owner].energy = 3;
        assert!(c.play_card(owner, 0, None));
        let poison = if upgraded { 12 } else { 9 };
        for &e in &c.enemies {
            assert!(c.cr(e).is_alive(), "Doom must wait for its side-end callback");
            assert_eq!(c.cr(e).hp, 100 - poison);
            assert_eq!(c.power_amount(e, PowerId::Poison), poison - 1);
            assert_eq!(c.power_amount(e, PowerId::Doom), 99);
        }
        assert_eq!(c.fault, None);
    }}
}
