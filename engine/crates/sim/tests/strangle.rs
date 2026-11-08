use sts2_sim::{
    combat::Combat, creature::Side, hooks::Ev, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, Hook, PowerId, VarKey},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.creatures[e as usize].hp = 1000;
    c.creatures[e as usize].max_hp = 1000;
    c
}

#[test]
fn nested_snapshots_are_per_card_and_per_applier_for_all_seats() {
    let mut c = combat();
    let e = c.enemies[0];
    for owner in 0..4 {
        let p = c.apply_power_to(e, PowerId::Strangle, 2, Some(owner)).unwrap();
        assert_eq!(c.apply_power_to(e, PowerId::Strangle, 1, Some(owner)), Some(p));
        let outer = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let nested = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let hp = c.cr(e).hp;
        c.fire(Hook::BeforeCardPlayed, Ev::Card(outer), None);
        c.apply_power_to(e, PowerId::Strangle, 2, Some(owner));
        c.fire(Hook::BeforeCardPlayed, Ev::Card(nested), None);
        c.fire(Hook::AfterCardPlayed, Ev::Card(nested), None);
        c.fire(Hook::AfterCardPlayed, Ev::Card(outer), None);
        assert_eq!(hp - c.cr(e).hp, 3 + 5);
    }
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert!(c.has_power(e, PowerId::Strangle));
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert!(!c.has_power(e, PowerId::Strangle));
    assert!(c.strangle_cards.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn choking_applies_after_damage_and_does_not_trigger_on_its_own_play() {
    let mut c = combat();
    let e = c.enemies[0];
    let k = c.new_card(3, CardId::MadScience, false).unwrap();
    c.cards[k as usize].vars.iter_mut().find(|(key, _)| *key == VarKey::Rider).unwrap().1 = 3;
    c.fire(Hook::BeforeCardPlayed, Ev::Card(k), None);
    c.play_card_effect(k, Some(e));
    let hp = c.cr(e).hp;
    c.fire(Hook::AfterCardPlayed, Ev::Card(k), None);
    assert_eq!(c.cr(e).hp, hp);
    assert_eq!(c.power_amount(e, PowerId::Strangle), 6);
    assert_eq!(c.fault, None);
}
