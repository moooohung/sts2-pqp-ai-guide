use sts2_sim::{
    combat::{Combat, Props},
    powers::PORTED_POWERS,
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId, power_meta},
};

fn combat() -> Combat {
    combat_for_players(1)
}

fn combat_for_players(count: usize) -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    for _ in 1..count { c.add_player(100, 100, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let enemy = c.enemies[0];
    c.creatures[enemy as usize].hp = 1000;
    c.creatures[enemy as usize].max_hp = 1000;
    c
}

#[test]
fn capacity_forge_updates_every_owned_non_dupe_blade_across_piles() {
    let mut c = combat_for_players(2);
    let mut blades = Vec::new();
    for i in 0..20 {
        let k = c.new_card(0, CardId::SovereignBlade, false).unwrap();
        if i % 2 == 0 { c.players[0].discard.push(k); }
        else { c.players[0].exhaust.push(k); }
        blades.push(k);
    }
    let dupe = c.new_card(0, CardId::SovereignBlade, false).unwrap();
    c.cards[dupe as usize].cv.is_dupe = true;
    c.players[0].discard.push(dupe);
    let other = c.new_card(1, CardId::SovereignBlade, false).unwrap();
    c.players[1].discard.push(other);
    let base = c.cards[blades[0] as usize].damage.unwrap();
    c.forge(0, 7, None);
    assert_eq!(c.fault, None);
    for k in blades { assert_eq!(c.cards[k as usize].damage, Some(base + 7)); }
    assert_eq!(c.cards[dupe as usize].damage, Some(base));
    assert_eq!(c.cards[other as usize].damage, Some(base));
}

#[test]
fn capacity_card_draw_listener_after_sixteen_other_players_cards_still_runs() {
    let mut c = combat_for_players(2);
    for _ in 0..16 {
        let k = c.new_card(0, CardId::KinglyPunch, false).unwrap();
        c.players[0].draw.push(k);
    }
    let drawn = c.new_card(1, CardId::KinglyPunch, false).unwrap();
    c.players[1].draw.push(drawn);
    let base = c.cards[drawn as usize].damage.unwrap();
    c.draw_cards(1, 1, false);
    assert_eq!(c.fault, None);
    assert!(c.players[1].hand.contains(&drawn));
    assert_eq!(c.cards[drawn as usize].damage, Some(base + 4));
}

#[test]
fn capacity_player_keeps_more_than_twenty_four_distinct_powers() {
    let mut c = combat();
    let ids: Vec<_> = PORTED_POWERS.iter().copied()
        .filter(|&id| !power_meta(id).instanced).take(25).collect();
    assert_eq!(ids.len(), 25);
    for &id in &ids {
        assert!(c.apply_power_to(0, id, 1, Some(0)).is_some(), "{id:?}");
    }
    assert_eq!(c.fault, None);
    for id in ids { assert!(c.has_power(0, id), "{id:?}"); }
}

#[test]
fn capacity_player_keeps_ten_independent_bombs() {
    let mut c = combat();
    let mut bombs = Vec::new();
    for _ in 0..10 {
        bombs.push(c.apply_power_to(0, PowerId::TheBomb, 2, Some(0)).unwrap());
    }
    assert_eq!(c.fault, None);
    for p in bombs { assert!(c.cr(0).power_list().contains(&p)); }
}

#[test]
fn capacity_power_list_preserves_application_order_past_thirty_two_instances() {
    let mut c = combat();
    let mut bombs = Vec::new();
    for _ in 0..34 {
        bombs.push(c.apply_power_to(0, PowerId::TheBomb, 2, Some(0)).unwrap());
    }
    assert_eq!(c.cr(0).power_list().as_slice(), bombs.as_slice());
}

#[test]
fn capacity_turn_end_ticks_every_bomb_past_thirty_two_listeners() {
    let mut c = combat();
    let mut bombs = Vec::new();
    for _ in 0..34 {
        bombs.push(c.apply_power_to(0, PowerId::TheBomb, 2, Some(0)).unwrap());
    }
    c.end_player_turn();
    assert_eq!(c.fault, None);
    for p in bombs { assert_eq!(c.powers[p as usize].amount, 1); }
}

#[test]
fn capacity_death_removes_every_power_past_thirty_two_instances() {
    let mut c = combat();
    let enemy = c.enemies[0];
    for _ in 0..34 {
        c.apply_power_to(enemy, PowerId::TheBomb, 2, Some(0)).unwrap();
    }
    c.deal_damage(Some(0), enemy, 1000, Props::UNPOWERED);
    assert_eq!(c.fault, None);
    assert!(c.cr(enemy).is_dead());
    assert!(c.cr(enemy).power_list().is_empty());
}

#[test]
fn capacity_every_tag_team_modifier_is_counted_and_consumed() {
    let mut c = combat_for_players(2);
    let enemy = c.enemies[0];
    for _ in 0..34 {
        c.apply_power_to(enemy, PowerId::TagTeam, 1, Some(1)).unwrap();
    }
    let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    c.active_target = Some(enemy);
    let (count, modifiers) = c.modify_card_play_count(1, k);
    assert_eq!(count, 35);
    c.fire_after_modifying_card_play_count(k, modifiers);
    assert_eq!(c.fault, None);
    assert!(!c.has_power(enemy, PowerId::TagTeam));
}

#[test]
fn capacity_card_factory_materializes_the_full_requested_batch() {
    let mut c = combat();
    c.set_character_pool(0, sts2_sim::character::Character::Defect).unwrap();
    let repeated = c.generate_pool_cards(0, &[CardId::Anger], 17, false);
    assert_eq!(repeated.len(), 17);
    assert!(repeated.iter().all(|&k| c.cards[k as usize].id == CardId::Anger));
    let distinct = c.generate_character_cards(0, None, None, 17, true);
    assert_eq!(distinct.len(), 17);
    let ids: std::collections::HashSet<_> =
        distinct.iter().map(|&k| c.cards[k as usize].id).collect();
    assert_eq!(ids.len(), 17);
    assert_eq!(c.fault, None);
}

#[test]
fn capacity_hello_world_generates_eleven_cards_including_hand_overflow() {
    let mut c = combat();
    c.set_character_pool(0, sts2_sim::character::Character::Defect).unwrap();
    c.apply_power_to(0, PowerId::HelloWorld, 11, Some(0)).unwrap();
    c.start_player_turn();
    assert_eq!(c.fault, None);
    assert_eq!(c.hist.generated_combat, 11);
    assert_eq!(c.players[0].hand.len(), 10);
    assert_eq!(c.players[0].discard.len(), 1);
}
