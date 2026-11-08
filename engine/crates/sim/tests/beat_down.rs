use sts2_sim::{
    combat::Combat, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

#[test]
fn beat_down_finishes_the_frozen_discard_snapshot_after_headbutt_for_all_seats() {
    for owner in 0..4 {
        for seed in 1..=12 {
            let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 1);
            c.players[0].hand.clear();
            c.players[0].draw.clear();
            for _ in 1..4 { c.add_player(1000, 1000, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            let enemy = c.enemies[0];
            c.creatures[enemy as usize].hp = 1000;
            c.creatures[enemy as usize].max_hp = 1000;
            c.run.rngs.shuffle = Rng::new(seed);
            let beat_down = c.new_card(owner, CardId::BeatDown, false).unwrap();
            c.players[owner].hand.push(beat_down);
            for id in [CardId::Headbutt, CardId::Bash, CardId::Bash] {
                let card = c.new_card(owner, id, false).unwrap();
                c.players[owner].discard.push(card);
            }
            let mut expected_order = c.players[owner].discard.to_vec();
            expected_order.sort_by_key(|&k| c.cards[k as usize].id.name());
            c.run.rngs.shuffle.clone().shuffle(&mut expected_order);
            let headbutt_position = expected_order.iter().position(|&k|
                c.cards[k as usize].id == CardId::Headbutt).unwrap();
            // C# BeatDown.OnPlay explicitly rolls CombatTargets once per selected attack.
            let mut expected_targets = c.run.rngs.combat_targets.clone();
            for _ in 0..3 { expected_targets.choice_index(1); }
            let mut expected_before_answer = c.run.rngs.combat_targets.clone();
            for _ in 0..=headbutt_position { expected_before_answer.choice_index(1); }
            c.players[owner].energy = 3;
            assert!(c.play_card(owner, 0, None));
            assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
            assert!(c.players[owner].play.contains(&beat_down));
            assert_eq!(c.run.rngs.combat_targets.counter, expected_before_answer.counter);
            assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
            assert!(c.pending_choice.is_none(), "owner {owner} seed {seed}");
            assert!(c.pending_play.is_empty());
            assert!(c.players[owner].play.is_empty());
            assert!(c.players[owner].discard.contains(&beat_down));
            assert_eq!(c.power_amount(enemy, PowerId::Vulnerable), 4, "owner {owner} seed {seed}");
            assert_eq!(c.run.rngs.combat_targets.counter, expected_targets.counter);
            assert_eq!(c.fault, None);
        }
    }
}
