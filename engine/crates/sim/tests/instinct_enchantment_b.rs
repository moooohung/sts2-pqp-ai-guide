use sts2_sim::{
    combat::{Combat, Props}, rng::Rng, run::{RunManager, RunOptions}, tables::CardId,
};

#[test]
fn live_instinct_preserves_cost_and_doubles_only_powered_attack_damage_for_every_seat() {
    for owner in 0..4 {
        for upgraded in [false, true] {
            let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
            let enemy = c.enemies[0];
            c.cr_mut(enemy).hp = 500;
            c.cr_mut(enemy).max_hp = 500;
            for seat in 0..4 { c.players[seat].energy = 3; }
            let k = c.new_card(owner, CardId::StrikeIronclad, upgraded).unwrap();
            let base = c.cards[k as usize].damage.unwrap();
            let cost = c.cards[k as usize].cost;
            let original_cost = c.cards[k as usize].original_cost;
            assert!(c.cards[k as usize].apply_static_enchantment("Instinct", 1));
            assert_eq!((c.cards[k as usize].cost, c.cards[k as usize].original_cost), (cost, original_cost));
            c.players[owner].hand.push(k);
            // C# Instinct.cs:13-20 doubles powered attacks only.
            assert!(c.play_card(owner, 0, Some(0)));
            assert_eq!(c.cr(enemy).hp, 500 - base * 2);
            for seat in 0..4 {
                assert_eq!(c.players[seat].energy, if seat == owner { 3 - cost } else { 3 });
            }
            c.active_card = Some(k);
            c.deal_damage(Some(owner), enemy, base, Props::UNPOWERED);
            assert_eq!(c.cr(enemy).hp, 500 - base * 3);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}
