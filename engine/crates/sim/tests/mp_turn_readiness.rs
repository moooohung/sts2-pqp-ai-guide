use sts2_sim::{
    combat::Combat,
    creature::Side,
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::PowerId,
};

fn multiplayer_combat(seats: usize) -> Combat {
    let mut run = RunManager::new(17, RunOptions::default()).run_ctx().clone();
    run.deck.clear();
    let mut c = Combat::new(run, 19);
    for _ in 1..seats {
        c.add_player(80, 80, 3, 0, &[]);
    }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(23));
    c.start_combat();
    c
}

#[test]
fn per_seat_end_turn_waits_for_both_csharp_readiness_barriers() {
    let mut c = multiplayer_combat(3);
    assert_eq!(c.player_turn_participants(), 0b111);

    c.end_player_turn_for(1);
    assert!(c.is_player_ready_to_end_turn(1));
    assert!(!c.is_player_ready_to_end_turn(0));
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 1);

    c.end_player_turn_for(0);
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 1);

    c.end_player_turn_for(2);
    // EndPlayerTurnPhaseOne has run, but C# still waits for every
    // ReadyToBeginEnemyTurnAction before flushing/switching sides.
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 1);
    for owner in 0..3 {
        assert!(c.is_player_ready_to_end_turn(owner));
        assert!(!c.is_player_ready_to_begin_enemy_turn(owner));
    }

    c.set_ready_to_begin_enemy_turn(2);
    c.set_ready_to_begin_enemy_turn(0);
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 1);
    assert!(c.is_player_ready_to_begin_enemy_turn(0));
    assert!(c.is_player_ready_to_begin_enemy_turn(2));
    assert!(!c.is_player_ready_to_begin_enemy_turn(1));

    c.set_ready_to_begin_enemy_turn(1);
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 2);
    for owner in 0..3 {
        assert_eq!(c.players[owner].turn_number, 2);
        assert!(!c.is_player_ready_to_end_turn(owner));
        assert!(!c.is_player_ready_to_begin_enemy_turn(owner));
    }
    assert_eq!(c.fault, None);
}

#[test]
fn extra_turn_auto_readies_nonparticipants_but_begin_enemy_requires_all_seats() {
    let mut c = multiplayer_combat(4);
    c.apply_power_to(2, PowerId::Ambergris, 1, Some(2));

    // Preserve the existing oracle path for the ordinary turn.  Ambergris then
    // starts an extra turn owned only by seat 2.
    c.end_player_turn();
    assert_eq!(c.round, 1);
    assert_eq!(c.player_turn_participants(), 0b0100);
    for owner in [0, 1, 3] {
        assert!(c.is_player_ready_to_end_turn(owner));
    }
    assert!(!c.is_player_ready_to_end_turn(2));

    c.end_player_turn_for(2);
    assert_eq!(c.round, 1);
    for owner in 0..4 {
        assert!(c.is_player_ready_to_end_turn(owner));
    }

    for owner in 0..3 {
        c.set_ready_to_begin_enemy_turn(owner);
        assert_eq!(c.round, 1);
    }
    c.set_ready_to_begin_enemy_turn(3);

    assert_eq!(c.round, 2);
    assert_eq!(c.player_turn_participants(), 0b1111);
    for owner in 0..4 {
        assert_eq!(c.players[owner].turn_number, if owner == 2 { 3 } else { 2 });
    }
    assert_eq!(c.fault, None);
}

