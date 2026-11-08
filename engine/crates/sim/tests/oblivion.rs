use sts2_sim::{
    combat::Combat, creature::Side, hooks::Ev, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, Hook, PowerId},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.add_player(50, 50, 3, 0, &[]);
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c
}

#[test]
fn oblivion_stacks_per_applier_and_snapshots_amount_before_play() {
    let mut c = combat();
    let e = c.enemies[0];
    let card = c.new_card(0, CardId::Oblivion, false).unwrap();
    // Its first OnPlay cannot trigger its newly applied mark.
    c.fire(Hook::BeforeCardPlayed, Ev::Card(card), None);
    c.play_card_effect(card, Some(e));
    c.fire(Hook::AfterCardPlayed, Ev::Card(card), None);
    assert_eq!(c.power_amount(e, PowerId::Doom), 0);
    let p0 = c.find_power(e, PowerId::Oblivion).unwrap();
    let p1 = c.apply_power_to(e, PowerId::Oblivion, 4, Some(1)).unwrap();
    assert_ne!(p0, p1);
    assert_eq!(c.apply_power_to(e, PowerId::Oblivion, 2, Some(0)), Some(p0));
    let played = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    let nested = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.fire(Hook::BeforeCardPlayed, Ev::Card(played), None); // 5
    c.apply_power_to(e, PowerId::Oblivion, 3, Some(0));
    c.fire(Hook::BeforeCardPlayed, Ev::Card(nested), None); // 8
    c.fire(Hook::AfterCardPlayed, Ev::Card(nested), None);
    c.fire(Hook::AfterCardPlayed, Ev::Card(played), None);
    assert_eq!(c.power_amount(e, PowerId::Doom), 13);
    let ally = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    c.fire(Hook::BeforeCardPlayed, Ev::Card(ally), None);
    c.fire(Hook::AfterCardPlayed, Ev::Card(ally), None);
    assert_eq!(c.power_amount(e, PowerId::Doom), 17);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert!(c.has_power(e, PowerId::Oblivion));
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert!(!c.has_power(e, PowerId::Oblivion));
    assert!(c.oblivion_cards.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn upgraded_oblivion_canonical_amount_and_artifact_prevention() {
    let mut c = combat();
    let e = c.enemies[0];
    let artifact = c.apply_power_to(e, PowerId::Artifact, 1, Some(e)).unwrap();
    // Artifact scales in multiplayer; this test explicitly exercises one charge.
    c.powers[artifact as usize].amount = 1;
    let card = c.new_card(1, CardId::Oblivion, true).unwrap();
    c.play_card_effect(card, Some(e));
    assert!(!c.has_power(e, PowerId::Oblivion));
    c.play_card_effect(card, Some(e));
    assert_eq!(c.power_amount(e, PowerId::Oblivion), 4);
    assert_eq!(c.powers[c.find_power(e, PowerId::Oblivion).unwrap() as usize].applier, Some(1));
    assert_eq!(c.fault, None);
}
