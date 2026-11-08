use sts2_sim::{combat::Combat, relic::Relic, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId, RelicId}};

#[test]
fn ambergris_and_paels_eye_restart_only_requesting_seats() {
    for owner in 0..4 {
        for use_eye in [false, true] {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            if use_eye {
                let mut r = Relic::new(RelicId::PaelsEye);
                r.owner = owner;
                c.run.relics.push(r);
            }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            for p in 0..4 {
                for _ in 0..12 {
                    let k = c.new_card(p, CardId::DefendIronclad, false).unwrap();
                    c.players[p].draw.push(k);
                }
                c.players[p].energy = 0;
                c.cr_mut(p).block = 7;
            }
            if !use_eye { c.apply_power_to(owner, PowerId::Ambergris, 1, Some(owner)); }
            let enemy = c.enemies[0];
            let hp = c.cr(enemy).hp;
            c.end_player_turn();
            assert_eq!(c.fault, None, "owner {owner} eye {use_eye}");
            assert_eq!(c.round, 1);
            assert_eq!(c.cr(enemy).hp, hp);
            for p in 0..4 {
                assert_eq!(c.players[p].turn_number, if p == owner { 2 } else { 1 });
                assert_eq!(c.players[p].energy, if p == owner { 3 } else { 0 });
                assert_eq!(c.players[p].hand.len(), if p == owner { 5 } else { 0 });
                assert_eq!(c.cr(p).block, if p == owner { 0 } else { 7 });
            }
            c.end_player_turn();
            assert_eq!(c.round, 2);
            for p in 0..4 {
                assert_eq!(c.players[p].turn_number, if p == owner { 3 } else { 2 });
            }
            assert_eq!(c.fault, None);
        }
    }
}
