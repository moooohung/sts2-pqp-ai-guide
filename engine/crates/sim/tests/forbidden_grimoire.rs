use sts2_sim::{
    combat::{Combat, EndReward},
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

fn combat(seats: u8) -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c
}

#[test]
fn grimoire_emits_stacked_owned_removals_at_combat_end_for_one_to_four_seats() {
    for seats in 1..=4 {
        let mut c = combat(seats);
        // Apply in reverse arena order: AfterCombatEnd must still walk player
        // listeners in seat order, keeping each Counter stack together.
        for owner in (0..seats).rev() {
            let k = c.new_card(owner, CardId::ForbiddenGrimoire, owner % 2 != 0).unwrap();
            for _ in 0..=owner { c.play_card_effect(k, None); }
            assert_eq!(c.power_amount(owner, PowerId::ForbiddenGrimoire), i32::from(owner) + 1);
            assert_eq!(c.cr(owner).power_list().iter()
                .filter(|&&p| c.powers[p as usize].id == PowerId::ForbiddenGrimoire).count(), 1);
        }
        assert!(c.end_rewards.is_empty(), "no removal before AfterCombatEnd");
        for enemy in c.enemies.clone() { c.kill_creature(enemy); }
        c.check_combat_end();
        assert!(c.is_over && c.won);
        let expected: Vec<_> = (0..seats)
            .flat_map(|owner| (0..=owner).map(move |_| owner))
            .collect();
        let owners = |c: &Combat| c.end_rewards.iter().map(|r| match r {
            EndReward::CardRemoval { owner } => *owner, _ => panic!("unexpected reward"),
        }).collect::<Vec<_>>();
        assert_eq!(owners(&c), expected);
        c.check_combat_end();
        assert_eq!(owners(&c), expected, "a completed combat cannot emit twice");
        assert_eq!(c.fault, None);
    }
}

#[test]
fn grimoire_does_not_emit_rewards_on_defeat() {
    for seats in 1..=4 {
        let mut c = combat(seats);
        for owner in 0..seats {
            let k = c.new_card(owner, CardId::ForbiddenGrimoire, false).unwrap();
            c.play_card_effect(k, None);
        }
        for owner in 0..seats { c.kill_creature(owner); }
        c.check_combat_end();
        assert!(c.is_over && !c.won);
        assert!(c.end_rewards.is_empty());
        assert_eq!(c.fault, None);
    }
}
