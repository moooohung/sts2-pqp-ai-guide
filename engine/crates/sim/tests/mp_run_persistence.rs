use sts2_sim::{
    run::{Coord, RunManager, RunOptions, mp::{Character, MpRoster, PlayerSpec, MapVote, MapVoting}},
    tables::{CardId, PotionId, RoomType},
};

#[test]
fn every_seat_carries_permanent_state_across_two_combats_without_arena_cards() {
    for seats in 2..=4 {
        let specs: Vec<_> = (0..seats).map(|i| PlayerSpec {
            net_id: 100 + i as u64, character: Character::Ironclad
        }).collect();
        let mut roster = MpRoster::new("73001", &specs).unwrap();
        roster.player_mut(0).unwrap().extra_fields.damage_dealt = 17;
        roster.player_mut(0).unwrap().extra_fields.debuffs_applied = 3;
        roster.player_mut(1).unwrap().hp = 60;
        roster.player_mut(1).unwrap().gold = 37;
        roster.player_mut(1).unwrap().potions[0] = Some(PotionId::Starpotion);
        let shared = RunManager::new(1, RunOptions { ascension: 0, ..Default::default() }).run_ctx().clone();
        let mut c = roster.enter_combat(shared, 1, "SLUDGE_SPINNER_WEAK", RoomType::Monster).unwrap();
        assert_eq!(c.players.len(), seats);
        assert_eq!(c.cr(1).hp, 60);
        assert!(c.use_potion_for_player(1, 0, None));
        c.players[1].persistent_deck.remove(0);
        let generated = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].hand.push(generated);
        let enemy = c.enemies[0];
        c.cr_mut(enemy).hp = 1;
        let hand = c.players[0].hand.len() - 1;
        assert!(c.play_card(0, hand, Some(0)));
        assert!(c.is_over && c.won);
        assert_eq!(c.player_damage_dealt[0], 18, "only the last enemy HP counts, not overkill");
        let hp = c.cr(1).hp;
        let shared = roster.leave_combat(c).unwrap();
        assert_eq!(roster.player(0).unwrap().extra_fields.damage_dealt, 18);
        assert_eq!(roster.player(0).unwrap().extra_fields.debuffs_applied, 3);
        assert_eq!(roster.player(0).unwrap().deck.len(), 10);
        assert_eq!(roster.player(1).unwrap().deck.len(), 9);
        assert_eq!(roster.player(1).unwrap().hp, hp);
        assert_eq!(roster.player(1).unwrap().gold, 37);
        assert_eq!(roster.player(1).unwrap().potions[0], None);
        let c = roster.enter_combat(shared, 2, "SLUDGE_SPINNER_WEAK", RoomType::Monster).unwrap();
        assert_eq!(c.player_damage_dealt[0], 18);
        assert_eq!(c.player_debuffs_applied[0], 3);
        assert_eq!(c.cr(1).hp, hp);
        assert_eq!(c.player_deck(0).len(), 10);
        assert_eq!(c.player_deck(1).len(), 9);
        assert_eq!(c.potion_slots_for_player(1).unwrap()[0], None);
    }
}

#[test]
fn ascension_applies_once_to_every_owner_without_changing_base_roster_contract() {
    for level in 0..=10 {
        let specs = [PlayerSpec { net_id: 10, character: Character::Ironclad },
            PlayerSpec { net_id: 20, character: Character::Silent }];
        let mut r = MpRoster::new("73001", &specs).unwrap();
        r.apply_starting_ascension(level).unwrap();
        for (seat, p) in r.players().iter().enumerate() {
            assert_eq!(p.max_potion_slots, if level >= 4 { 2 } else { 3 });
            assert_eq!(p.deck.iter().filter(|c| c.id == CardId::AscendersBane).count(), usize::from(level >= 5));
            for card in p.deck.iter().filter(|c| c.id == CardId::AscendersBane) {
                assert_eq!(card.floor_added_to_deck, Some(1), "A{level} seat {seat}");
            }
            assert!(p.deck.iter().all(|c| c.owner as usize == seat));
        }
        assert!(r.apply_starting_ascension(level).is_err());
    }
}

#[test]
fn map_vote_is_a_seeded_player_vote_not_majority_and_rejects_stale_sources() {
    for seats in 2..=4 {
        let source = (0, Coord::new(3, 0));
        let mut votes = MapVoting::new("73001", seats, source.0, source.1).unwrap();
        votes.before_map_generated();
        let mut reference = votes.rng.clone();
        let chosen_seat = reference.choice_index(seats);
        let target = |seat| Coord::new(seat as i32, 1);
        for seat in 0..seats {
            let result = votes.vote(seat, source, Some(MapVote { generation: 1, coord: target(seat) })).unwrap();
            assert_eq!(result, if seat + 1 == seats { Some(target(chosen_seat)) } else { None });
        }
        assert_eq!(votes.rng, reference);
        let state = votes.rng.clone();
        assert_eq!(votes.vote(0, source, None).unwrap(), None);
        assert_eq!(votes.rng, state);
        votes.on_location_changed(0, target(chosen_seat));
        assert!(votes.votes().iter().all(Option::is_none));
        votes.before_map_generated();
        assert_eq!(votes.vote(0, (0, target(chosen_seat)), Some(MapVote { generation: 1, coord: Coord::new(2, 2) })).unwrap(), None);
        assert!(votes.votes().iter().all(Option::is_none));
    }
}

#[test]
fn legacy_map_votes_use_decoded_shared_seed_without_changing_player_rng_seeding() {
    use sts2_sim::rng::{Rng, deterministic_hash_code_old};
    let decoded = deterministic_hash_code_old("73001") as u32 as u64;
    let votes = MapVoting::new("old73001", 2, 0, Coord::new(3, 0)).unwrap();
    assert_eq!(votes.rng, Rng::named(decoded, "map_point_selection"));
}

#[test]
fn act_floor_tracks_entered_coordinate_not_number_of_votes() {
    use sts2_sim::run::mp::{MpRun, MpPhase};
    use sts2_sim::tables::MapPointType;
    for seats in 2..=4 {
        let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
            net_id: 100 + seat as u64, character: Character::Ironclad,
        }).collect();
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        let (col, row) = run.world.run.coord.unwrap();
        assert_eq!(run.world.act_floor, row + 1);
        let source = run.world.map.point(Coord::new(col, row)).unwrap();
        let destination = source.children.iter().map(|&i| &run.world.map.points[i as usize])
            .find(|point| point.point_type == MapPointType::Monster).unwrap().coord;
        run.phase = MpPhase::Map;
        for (seat, spec) in specs.iter().enumerate() {
            run.vote(spec.net_id, seat, destination).unwrap();
            assert_eq!(run.world.act_floor, if seat + 1 == seats { destination.row + 1 } else { row + 1 });
        }
        assert_eq!(run.phase, MpPhase::Combat);
    }
}

#[test]
fn debuffer_badge_counts_owner_changes_not_stacks_and_excludes_self_and_temporary() {
    use sts2_sim::tables::{PowerId, PowerType, power_meta};
    use sts2_sim::run::mp::MpRun;
    let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
        PlayerSpec { net_id: 2, character: Character::Ironclad }];
    let run = MpRun::new("73001", &specs, 0).unwrap();
    let mut c = run.roster.enter_combat(run.world.run, 1, "SLUDGE_SPINNER_WEAK", RoomType::Monster).unwrap();
    let enemy = c.enemies[0];
    c.after_power_amount_changed(enemy, PowerId::Weak, 2, Some(1), None);
    c.after_power_amount_changed(enemy, PowerId::Weak, 3, Some(1), None);
    c.after_power_amount_changed(enemy, PowerId::Strength, -1, Some(1), None);
    assert_eq!(c.player_debuffs_applied[1], 3);
    c.after_power_amount_changed(1, PowerId::Weak, 2, Some(1), None);
    c.after_power_amount_changed(enemy, PowerId::Strength, 1, Some(1), None);
    c.after_power_amount_changed(enemy, PowerId::Weak, 2, None, None);
    let temporary = PowerId::ALL.into_iter().find(|id| {
        let m = power_meta(*id);
        m.temporary && m.power_type == PowerType::Debuff
    }).unwrap();
    c.after_power_amount_changed(enemy, temporary, 2, Some(1), None);
    assert_eq!(c.player_debuffs_applied[1], 3);
    c.after_power_amount_changed(0, PowerId::Weak, 2, Some(1), None);
    assert_eq!(c.player_debuffs_applied[1], 4);
    assert_eq!(c.player_debuffs_applied[0], 0);
}
