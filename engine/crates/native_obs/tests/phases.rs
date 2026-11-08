use sts2_native_obs::*;
use sts2_sim::{card::Card, run::{Coord, RunManager, RunOptions, RunPhase,
    mp::{Character, MpPhase, MpRun, PlayerRewards, PlayerSpec}}, tables::{CardId, MapPointType, PotionId}};

fn decision(seed: u64, actor: u8) -> DecisionId { DecisionId { episode: seed, generation: 0, actor } }
fn select(obs: &NativeObsV1, current: DecisionId, net: u64, predicate: impl Fn(Command) -> bool) -> usize {
    let v = obs.view().unwrap();
    (0..v.candidate_len).find(|&i| obs.binding(i, current, net).is_ok_and(|b| predicate(b.command))).unwrap()
}
fn mp(seats: usize, seed: u64) -> MpRun {
    let characters = [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent];
    let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec { net_id: 10 + seat as u64,
        character: characters[(seed as usize + seat) % 5] }).collect();
    MpRun::new(&seed.to_string(), &specs, 0).unwrap()
}
fn enter_room(seats: usize, seed: u64, kind: MapPointType) -> MpRun {
    let mut run = mp(seats, seed); run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = kind;
    let target = run.world.map.points[child].coord;
    for seat in 0..seats { run.vote(10 + seat as u64, seat, target).unwrap(); }
    run
}

#[test]
fn merchant_pending_pickups_are_owned_decisions_and_seeded_runs_complete_without_refusals() {
    seeded_merchant_runs(true);
}

#[test]
fn fixed_count_live_selection_seeded_runs_complete_without_refusals() {
    seeded_merchant_runs(false);
}

fn seeded_merchant_runs(legacy_journal: bool) {
    for (seed, scenario, expected) in [
        (202611030012u64, 12usize, sts2_sim::tables::RelicId::DollysMirror),
        (202611030313, 313, sts2_sim::tables::RelicId::RoyalStamp),
    ] {
        let characters = [Character::Ironclad, Character::Silent, Character::Defect,
            Character::Necrobinder, Character::Regent];
        let seats = 2 + scenario % 3;
        let specs: Vec<_> = (0..seats).map(|s| PlayerSpec { net_id: 101 + s as u64 * 17,
            character: characters[(scenario + s) % 5] }).collect();
        let mut run = MpRun::new(&seed.to_string(), &specs, if scenario % 2 == 0 {0}else{10}).unwrap();
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        let mut current = decision(seed, 0); let mut rng = seed ^ 0xd1b54a32d192ed03;
        let mut reached = false; let mut completed = false;
        for tick in 0..20_000 {
            NativeEncoder::advance_multiplayer(&mut run, &mut current).unwrap();
            let seat = tick % seats; current.actor = seat as u8; let net = specs[seat].net_id;
            let before = format!("{run:?}");
            encoder.encode_multiplayer(&run, current, net, &mut obs).unwrap();
            assert_eq!(format!("{run:?}"), before);
            if !reached && run.phase == MpPhase::Shop && run.merchant_pickup(net, seat).unwrap().is_some() {
                assert!(run.roster.player(seat).unwrap().relics.iter().any(|r| r.id == expected));
                assert!(run.merchant_offers(net, seat).unwrap().is_empty());
                let Some(sts2_sim::run::mp::MerchantPickupChoice::Deck { relic, cards, .. }) =
                    run.merchant_pickup(net, seat).unwrap() else { panic!("expected deck selector"); };
                assert_eq!(*relic, expected);
                let view = obs.view().unwrap();
                assert_eq!(view.status, DecisionStatus::Actionable);
                let choice = view.rows.kind.iter().position(|&kind| kind == RowKind::Choice).unwrap();
                assert_eq!(view.rows.model[choice], ContentId::from_index(Namespace::RelicId, expected.index()).unwrap());
                let mut offered = vec![false; cards.len()];
                for index in 0..view.candidate_len {
                    if !view.candidates.sampleable(index, view.candidate_len) { continue; }
                    let binding = obs.binding(index, current, net).unwrap();
                    match binding.command {
                        Command::MpMerchantPickup { index: Some(card_index) } => {
                            let card_index = card_index as usize;
                            assert!(run.native_can_select_merchant_pickup(net, seat, &[card_index]));
                            assert_eq!(binding.source_instance, Some(cards[card_index].instance_id));
                            offered[card_index] = true;
                            // Revalidation rejects a changed decision generation.
                            assert!(obs.binding(index, DecisionId { generation: current.generation + 1, ..current }, net).is_err());
                        }
                        other => panic!("{seed}: non-selector action during owned pickup: {other:?}"),
                    }
                }
                assert!(offered.into_iter().enumerate().all(|(index, present)|
                    present == run.native_can_select_merchant_pickup(net, seat, &[index])));
                // Other seats may still buy from their own inventories. Leaving
                // the shared room is blocked by the simulator's pickup barrier.
                for other in 0..seats {
                    if other == seat { continue; }
                    let other_net = specs[other].net_id;
                    let mut other_encoder = NativeEncoder::new(); let mut other_obs = NativeObsV1::new();
                    let other_decision = DecisionId { actor: other as u8, ..current };
                    other_encoder.encode_multiplayer(&run, other_decision, other_net, &mut other_obs).unwrap();
                    let other_view = other_obs.view().unwrap();
                    let legal = run.merchant_offers(other_net, other).unwrap();
                    for index in 0..other_view.candidate_len {
                        if !other_view.candidates.sampleable(index, other_view.candidate_len) { continue; }
                        match other_obs.binding(index, other_decision, other_net).unwrap().command {
                            Command::MpLeaveShop => panic!("shared-room leave was exposed during pickup"),
                            Command::MpMerchant(command) => {
                                use sts2_sim::run::mp::MerchantAction as A;
                                let action = match command {
                                    MerchantCommand::BuyCard { index } => A::BuyCard(index as usize),
                                    MerchantCommand::BuyRelic { index } => A::BuyRelic(index as usize),
                                    MerchantCommand::BuyPotion { index } => A::BuyPotion(index as usize),
                                    MerchantCommand::RemoveCard { instance } => A::RemoveCard(instance),
                                };
                                assert!(legal.iter().any(|offer| offer.action == action && offer.enabled));
                            }
                            _ => {}
                        }
                    }
                    assert!(!run.native_can_leave_shop(other_net, other));
                }
                reached = true;
            }
            let view = obs.view().unwrap();
            if view.status == DecisionStatus::Terminal { completed = true; break; }
            if view.status == DecisionStatus::Waiting { continue; }
            assert_eq!(view.status, DecisionStatus::Actionable);
            rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
            let count = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len)).count();
            let index = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
                .nth(rng as usize % count).unwrap();
            if legacy_journal {
                encoder.dispatch_multiplayer_journal(&mut run, &mut current, net, &mut obs, index).unwrap();
            } else {
                encoder.dispatch_multiplayer(&mut run, &mut current, net, &mut obs, index).unwrap();
            }
        }
        // These merchant inventories were reached by the historical native UI
        // click stream. Automatic fixed-count completion consumes fewer policy
        // decisions, hence the same random draws take a different live path.
        if legacy_journal { assert!(reached, "{seed}: merchant pickup not reached"); }
        assert!(completed, "{seed}: run did not complete");
    }
}

#[test]
fn multiplayer_potion_discard_mask_matches_run_query_and_roundtrips() {
    for seats in 2..=4 {
        for combat in [false, true] {
            let seed = 73001;
            let mut run = mp(seats, seed);
            for seat in 0..seats {
                run.roster.player_mut(seat).unwrap().potions = vec![Some(PotionId::Firepotion), None];
            }
            if combat {
                run.phase = MpPhase::Map;
                let (x, y) = run.world.run.coord.unwrap();
                let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
                run.world.map.points[child].point_type = MapPointType::Monster;
                let target = run.world.map.points[child].coord;
                for seat in 0..seats { run.vote(10 + seat as u64, seat, target).unwrap(); }
                assert_eq!(run.phase, MpPhase::Combat);
            }
            for seat in 0..seats {
                let net = 10 + seat as u64;
                let mut current = decision(seed, seat as u8);
                let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
                let before = format!("{run:?}");
                encoder.encode_multiplayer(&run, current, net, &mut obs).unwrap();
                assert_eq!(format!("{run:?}"), before);
                let candidate = select(&obs, current, net, |c| matches!(c, Command::MpDiscardPotion { slot: 0 }));
                let view = obs.view().unwrap();
                assert_eq!(view.candidates.sampleable(candidate, view.candidate_len),
                    run.can_discard_potion(net, seat, 0));
                assert!(run.can_discard_potion(net, seat, 0));
                assert!(!run.can_discard_potion(net, seat, 1));
                assert!(!run.can_discard_potion(net + 99, seat, 0));
                let mut stepped = run.clone();
                encoder.dispatch_multiplayer(&mut stepped, &mut current, net, &mut obs, candidate).unwrap();
                assert!(!stepped.can_discard_potion(net, seat, 0));
            }
        }
    }
}

#[test]
fn multiplayer_combat_shop_rest_treasure_and_terminal_cover_two_to_four_seats() {
    for seats in 2..=4 {
        for (kind, phase) in [(MapPointType::Monster, MpPhase::Combat), (MapPointType::Shop, MpPhase::Shop),
            (MapPointType::RestSite, MpPhase::RestSite), (MapPointType::Treasure, MpPhase::Treasure)] {
            let mut run = enter_room(seats, 73001, kind); assert_eq!(run.phase, phase);
            let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new(); let mut current = decision(73001, 0);
            let before = format!("{run:?}");
            encoder.encode_multiplayer(&run, current, 10, &mut obs).unwrap();
            assert_eq!(format!("{run:?}"), before);
            assert_eq!(obs.view().unwrap().status, DecisionStatus::Actionable);
            let candidate = select(&obs, current, 10, |command| match phase {
                MpPhase::Combat => matches!(command, Command::MpPlayCard { .. } | Command::MpEndTurn),
                MpPhase::Shop => command == Command::MpLeaveShop,
                MpPhase::RestSite => matches!(command, Command::MpRestOption { index: 0 }),
                MpPhase::Treasure => command == Command::MpOpenTreasure,
                _ => false,
            });
            encoder.dispatch_multiplayer(&mut run, &mut current, 10, &mut obs, candidate).unwrap();
            assert_eq!(current.generation, 1); assert!(obs.view().is_err());
            encoder.encode_multiplayer(&run, current, 10, &mut obs).unwrap();
        }
        let mut run = mp(seats, 73001); run.phase = MpPhase::RunOver;
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        encoder.encode_multiplayer(&run, decision(73001, 0), 10, &mut obs).unwrap();
        assert_eq!(obs.view().unwrap().status, DecisionStatus::Terminal);
        assert_eq!(obs.view().unwrap().candidate_len, 0);
    }
}

#[test]
fn multiplayer_event_phase_has_owned_typed_options() {
    for seats in 2..=4 {
        let mut found = false;
        for seed in 73001..73033 {
            let run = enter_room(seats, seed, MapPointType::Unknown);
            if run.phase != MpPhase::Event { continue; }
            let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
            encoder.encode_multiplayer(&run, decision(seed, 0), 10, &mut obs).unwrap();
            assert!(obs.view().unwrap().rows.kind.contains(&RowKind::Event));
            assert!(obs.view().unwrap().rows.kind.contains(&RowKind::EventOption));
            assert_eq!(obs.view().unwrap().status, DecisionStatus::Actionable);
            found = true; break;
        }
        assert!(found, "no event fixture for {seats} seats");
    }
}

#[test]
fn every_character_has_an_owned_combat_projection_at_two_to_four_seats() {
    let mut covered = [false; 5];
    for seats in 2..=4 {
        for seed in 73000..73005 {
            let run = enter_room(seats, seed, MapPointType::Monster);
            assert_eq!(run.phase, MpPhase::Combat);
            let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
            for actor in 0..seats {
                let current = decision(seed, actor as u8);
                let before = format!("{run:?}");
                encoder.encode_multiplayer(&run, current, 10 + actor as u64, &mut obs).unwrap();
                assert_eq!(format!("{run:?}"), before);
                let character = run.roster.players()[actor].character;
                covered[character as usize] = true;
                let view = obs.view().unwrap();
                let row = view.rows.kind.iter().enumerate().find_map(|(i, &kind)|
                    (kind == RowKind::Seat && view.rows.integers[i][0] == actor as i32).then_some(i)).unwrap();
                assert_eq!(view.rows.detail[row][0], ContentId::from_index(Namespace::Character, character as usize).unwrap());
                let candidate = select(&obs, current, 10 + actor as u64, |c| matches!(c, Command::MpPlayCard { .. } | Command::MpEndTurn));
                let mut stepped = run.clone(); let mut next = current;
                encoder.dispatch_multiplayer(&mut stepped, &mut next, 10 + actor as u64, &mut obs, candidate).unwrap();
                assert_eq!(next.generation, 1);
            }
        }
    }
    assert!(covered.into_iter().all(|value| value));
}

#[test]
fn dead_seat_readiness_is_automatic_and_encoding_is_readonly() {
    // Original CreatureCmd.Kill automatically ends a dead player's turn.
    // Build the dead state directly through the public run setup;
    // a sampled 1000-action trajectory changes when legal menus expand.
    let seed = 73001;
    let mut run = mp(3, seed);
    run.roster.player_mut(0).unwrap().hp = 0;
    run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Monster;
    let target = run.world.map.points[child].coord;
    for seat in 0..3 { run.vote(10 + seat as u64, seat, target).unwrap(); }
    assert_eq!(run.phase, MpPhase::Combat);
    assert!(run.combat().unwrap().cr(0).is_dead());
    // flow::start_player_side_turn already marks nonparticipants ready.
    assert!(run.combat().unwrap().is_player_ready_to_end_turn(0));
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
    let mut current = decision(seed, 0);
    let before = format!("{run:?}");
    encoder.encode_multiplayer(&run, current, 10, &mut obs).unwrap();
    assert_eq!(obs.view().unwrap().status, DecisionStatus::Waiting);
    assert_eq!(obs.view().unwrap().candidate_len, 0);
    assert_eq!(format!("{run:?}"), before);
    for seat in 1..3 { run.end_turn_for(10 + seat as u64, seat).unwrap(); }
    assert!(!NativeEncoder::advance_multiplayer(&mut run, &mut current).unwrap());
    assert_eq!(current.generation, 0);
    let combat = run.combat().unwrap();
    assert!(combat.is_player_ready_to_end_turn(0));
    assert!(combat.needs_enemy_turn_acknowledgement(1));
    current.actor = 1;
    encoder.encode_multiplayer(&run, current, 11, &mut obs).unwrap();
    let candidate = select(&obs, current, 11, |c| c == Command::MpReadyForEnemyTurn);
    assert!(obs.view().unwrap().candidates.sampleable(candidate, obs.view().unwrap().candidate_len));
}

#[test]
fn closed_boss_rewards_advance_act_before_exposing_the_next_native_decision() {
    for seats in 2..=4 {
        let seed=73001;
        let mut run=mp(seats,seed);
        run.phase=MpPhase::Map;
        let boss=run.world.map.points.iter().find(|p|
            p.point_type==MapPointType::Boss && p.children.is_empty()).unwrap().coord;
        run.world.run.coord=Some((boss.col,boss.row));
        let act=run.world.run.act_index;
        let mut current=decision(seed,0);
        assert!(NativeEncoder::advance_multiplayer(&mut run,&mut current).unwrap());
        assert_eq!(run.world.run.act_index,act+1);
        assert_eq!(current.generation,1);
        assert!(!NativeEncoder::advance_multiplayer(&mut run,&mut current).unwrap());
        let mut encoder=NativeEncoder::new();
        let mut obs=NativeObsV1::new();
        encoder.encode_multiplayer(&run,current,10,&mut obs).unwrap();
        assert_eq!(obs.view().unwrap().status,DecisionStatus::Actionable);
    }
}

#[test]
fn special_card_reward_roundtrips_exact_owner_instance_and_modifiers() {
    let mut run = mp(2, 73001); run.phase = MpPhase::Rewards;
    let mut card = Card::from_factory(CardId::StrikeIronclad, true, 10001).unwrap(); card.owner = 1;
    card.enchantments.push((EnchantmentId::TezcatarasEmber, 1));
    let empty = PlayerRewards { gold: vec![0], potion: Some(PotionId::Firepotion), cards: Vec::new(),
        extra_cards: Vec::new(), extra_relics: Vec::new(), extra_potions: Vec::new(), special_cards: vec![Some(card.clone())],
        gold_taken: false, potion_taken: true, card_taken: true, completed: false, card_removals: Vec::new() };
    run.rewards = Some(vec![empty.clone(), empty]);
    let before_other = run.roster.player(0).unwrap().deck.len();
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new(); let mut current = decision(73001, 1);
    encoder.encode_multiplayer(&run, current, 11, &mut obs).unwrap();
    let candidate = select(&obs, current, 11, |c| matches!(c, Command::MpTakeSpecialCard { .. }));
    let binding = obs.binding(candidate, current, 11).unwrap();
    assert_eq!(binding.source_instance, Some(card.instance_id));
    assert_eq!(binding.command, Command::MpTakeSpecialCard { reward_index: 2 });
    assert_eq!(obs.binding(candidate, current, 10), Err(ContractError::WrongOwner));
    encoder.dispatch_multiplayer(&mut run, &mut current, 11, &mut obs, candidate).unwrap();
    assert_eq!(run.roster.player(0).unwrap().deck.len(), before_other);
    let received = run.roster.player(1).unwrap().deck.last().unwrap();
    assert_eq!(received.id, card.id); assert_eq!(received.upgraded, card.upgraded);
    assert_eq!(received.enchantments, card.enchantments);
    assert!(run.rewards.as_ref().unwrap()[1].special_cards[0].is_none());
}

#[test]
fn solo_seeded_transitions_visit_every_decision_phase() {
    let phases = [RunPhase::Event, RunPhase::MapChoice, RunPhase::Combat, RunPhase::CardReward,
        RunPhase::Shop, RunPhase::RestSite, RunPhase::Treasure, RunPhase::RunOver];
    for character in [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent] {
        let mut seen = [false; 8]; let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        for offset in 0..128u64 {
            let seed = 202610300000 + offset;
            let mut run = RunManager::new_for_character(seed, RunOptions { hp_cheat: (offset >= 16).then_some(10000),
                saved_wongo_points: Some(0), ..RunOptions::default() }, character);
            let mut current = decision(seed, 0); let mut rng = seed;
            for _ in 0..1500 {
                match encoder.encode_solo(&run, current, &mut obs) {
                    Ok(()) => {}, Err(EncodeError::Gameplay(_)) => break, Err(error) => panic!("{character:?} {seed}: {error:?}"),
                }
                let at = phases.iter().position(|phase| *phase == run.phase()).unwrap(); seen[at] = true;
                let view = obs.view().unwrap();
                let seat = view.rows.kind.iter().position(|&kind| kind == RowKind::Seat).unwrap();
                assert_eq!(view.rows.detail[seat][0], ContentId::from_index(Namespace::Character, character as usize).unwrap());
                if seen.iter().all(|&value| value) { break; }
                if view.status != DecisionStatus::Actionable { break; }
                rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
                let count = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len)).count();
                let candidate = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
                    .nth(rng as usize % count).unwrap();
                match encoder.dispatch_solo(&mut run, &mut current, &mut obs, candidate) {
                    Ok(()) => {}, Err(DispatchError::Gameplay(_)) => break, Err(error) => panic!("{character:?} {seed}: {error:?}"),
                }
            }
            if seen.iter().all(|&value| value) { break; }
        }
        assert!(seen.iter().all(|&value| value), "{character:?} phase coverage: {seen:?}");
    }
}
