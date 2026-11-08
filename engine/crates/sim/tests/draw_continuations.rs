use sts2_sim::{combat::{Combat, ChoiceScheduling}, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId}};

#[test]
fn hellraiser_pillage_finishes_against_hardened_shell_with_each_owners_original_turn_cap() {
    use sts2_sim::{combat::Props, creature::Side, hooks::Ev, tables::Hook};
    for seats in 1..=4 { for owner in 0..seats {
        let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
        run.deck.clear();run.relics.clear();
        let mut c=Combat::new(run,1);
        for _ in 1..seats {c.add_player(50,50,3,0,&[]);}
        c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
        c.start_combat();
        let enemy=c.enemies[0];
        c.cr_mut(enemy).hp=10000;c.cr_mut(enemy).max_hp=10000;
        c.apply_power_to(enemy,PowerId::HardenedShell,1,Some(enemy));
        c.apply_power_to(owner,PowerId::Hellraiser,1,Some(owner));
        c.apply_damage(enemy,c.power_amount(enemy,PowerId::HardenedShell),Props::MOVE,Some(owner));
        for _ in 0..3 {
            let strike=c.new_card(owner,CardId::StrikeIronclad,false).unwrap();
            c.players[owner].draw.push(strike);
        }
        // Original HellraiserPower auto-plays nine Strikes against infinite
        // HP displays; the tenth stays in hand, so Pillage can finish drawing.
        c.draw_cards(owner,10,false);
        assert_eq!(c.players[owner].hand.len(),1,"{seats} seats, owner {owner}: tenth Strike must remain in hand");
        assert_eq!(c.hist.played_combat.len(),9);
        c.fire_after_turn_end(Side::Enemy);
        c.draw_cards(owner,1,false);
        assert_eq!(c.hist.played_combat.len(),9,"another side's turn must not reset the owner");
        c.fire_after_turn_end(Side::Player);
        let hand=std::mem::take(&mut c.players[owner].hand);
        c.players[owner].draw.extend(hand);
        let pillage=c.new_card(owner,CardId::Pillage,false).unwrap();
        c.players[owner].hand.push(pillage);
        c.players[owner].energy=30;
        let before=c.hist.played_combat.len();
        assert!(c.play_card(owner,0,Some(0)));
        assert_eq!(c.fault,None);
        assert_eq!(c.hist.played_combat.len()-before,10,"nine Strikes and the completed Pillage");
        assert_eq!(c.players[owner].hand.len(),3);
        for seat in 0..seats {if seat!=owner {assert!(c.players[seat].hand.is_empty());}}
        // HardenedShellPower restores the normal display at either side's
        // turn start; ordinary targets must not inherit the nine-play limit.
        let shell=c.find_power(enemy,PowerId::HardenedShell).unwrap();
        c.powers[shell as usize].amount=10000;
        c.fire(Hook::BeforeSideTurnStart,Ev::Side(Side::Enemy),None);
        c.players[owner].discard.retain(|&k|k!=pillage);
        let hand=std::mem::take(&mut c.players[owner].hand);
        c.players[owner].draw.extend(hand);
        let before=c.hist.played_combat.len();
        c.draw_cards(owner,10,false);
        assert_eq!(c.hist.played_combat.len()-before,10);
        assert!(c.players[owner].hand.is_empty());
        assert_eq!(c.fault,None);
    }}
}

#[test]
fn prepared_awaits_shuffle_choice_then_discards_its_original_draw_count() {
    for scheduling in [ChoiceScheduling::Sequential,ChoiceScheduling::Live] {
    for owner in 0..4 {for upgraded in [false,true] {
        let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
        run.choice_scheduling=scheduling;
        run.deck.clear();
        let mut c=Combat::new(run,1);
        for _ in 1..4 {c.add_player(50,50,3,0,&[]);}
        c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.apply_power_to(owner,PowerId::Stratagem,1,Some(owner));
        for _ in 0..5 {
            let k=c.new_card(owner,CardId::DefendIronclad,false).unwrap();
            c.players[owner].discard.push(k);
        }
        let prepared=c.new_card(owner,CardId::Prepared,upgraded).unwrap();
        c.players[owner].hand.push(prepared);
        assert!(c.play_card(owner,0,None));
        assert_eq!(c.fault,None);
        assert!(c.pending_choice_for_player(owner).is_some());
        assert!(c.resolve_pending_choice_for_player(owner,Some(0)));
        assert_eq!(c.fault,None);
        assert!(c.pending_choice_for_player(owner).is_some());
        assert!(c.resolve_pending_choice_for_player(owner,Some(0)));
        if upgraded {
            assert!(c.resolve_pending_choice_for_player(owner,Some(1)));
            assert!(c.resolve_pending_choice_for_player(owner,None));
        }
        assert_eq!(c.fault,None);
        assert!(c.pending_choice_for_player(owner).is_none());
        assert_eq!(c.players[owner].hand.len(),1);
        assert_eq!(c.hist.played_combat.len(),1);
        assert!(c.players[owner].discard.contains(&prepared));
        for seat in 0..4 {if seat!=owner {assert!(c.players[seat].hand.is_empty());}}
    }}}
}

#[test]
fn soul_awaits_stratagem_then_finishes_draw_and_exhaust_once_in_every_seat() {
    for scheduling in [ChoiceScheduling::Sequential, ChoiceScheduling::Live] {
    for owner in 0..4 { for upgraded in [false, true] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.choice_scheduling = scheduling;
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        // Two mandatory selections exercise the explicit multi-selector
        // confirmation path rather than the single-selection fast path.
        c.apply_power_to(owner, PowerId::Stratagem, 2, Some(owner));
        for _ in 0..5 {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(k);
        }
        let soul = c.new_card(owner, CardId::Soul, upgraded).unwrap();
        c.players[owner].hand.push(soul);
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.fault, None);
        assert!(c.hist.played_combat.is_empty());
        assert!(!c.players[owner].exhaust.contains(&soul));
        assert!(c.pending_choice_for_player(owner).is_some());
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert!(c.pending_choice_for_player(owner).is_some());
        assert!(c.resolve_pending_choice_for_player(owner, Some(1)));
        assert!(c.resolve_pending_choice_for_player(owner, None));
        assert_eq!(c.players[owner].hand.len(), if upgraded { 5 } else { 4 });
        assert!(c.players[owner].exhaust.contains(&soul));
        assert_eq!(c.hist.played_combat.len(), 1);
        for seat in 0..4 { if seat != owner { assert!(c.players[seat].hand.is_empty()); } }
        assert!(c.pending_choice_for_player(owner).is_none());
        assert_eq!(c.fault, None);
    }}}
}

#[test]
fn huddle_waits_for_its_owner_before_starting_later_draw_branches() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        for p in 0..4 {
            if p == owner { c.apply_power_to(p, PowerId::Stratagem, 1, Some(p)); }
            for _ in 0..4 {
                let k = c.new_card(p, CardId::DefendIronclad, false).unwrap();
                c.players[p].discard.push(k);
            }
        }
        let k = c.new_card(owner, CardId::HuddleUp, false).unwrap();
        c.players[owner].hand.push(k);
        assert!(c.play_card(owner, 0, None));
        for p in owner + 1..4 {
            assert!(c.pending_choice_for_player(p).is_none());
            assert_eq!(c.players[p].discard.len(), 4);
        }
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.hist.played_combat.len(), 1);
        assert_eq!(c.fault, None);
        for p in 0..4 { assert_eq!(c.players[p].hand.len(), if p == owner { 3 } else { 2 }); }
    }
}

#[test]
fn huddle_teammate_choices_run_after_parent_and_resume_in_any_seat_order() {
    for seats in 2..=4 {
    for owner in 0..seats {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.choice_scheduling = ChoiceScheduling::Live;
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        for p in 0..seats {
            if p != owner { c.apply_power_to(p, PowerId::Stratagem, 1, Some(p)); }
            for _ in 0..4 {
                let k = c.new_card(p, CardId::DefendIronclad, false).unwrap();
                c.players[p].discard.push(k);
            }
        }
        let k = c.new_card(owner, CardId::HuddleUp, false).unwrap();
        c.players[owner].hand.push(k);
        c.on_choice_presented = Some(|c| {
            assert_eq!(c.hist.played_combat.len(), 1);
            assert!(c.pending_play.is_empty());
            assert!(c.players.0.iter().any(|p| p.exhaust.iter().any(|&k| c.cards[k as usize].id == CardId::HuddleUp)));
        });
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.fault, None);
        assert_eq!(c.hist.played_combat.len(), 1);
        assert_eq!(c.players[owner].hand.len(), 2);
        for p in (0..seats).rev() {
            if p == owner { continue; }
            assert!(c.pending_choice_for_player(p).is_some());
            assert!(c.resolve_pending_choice_for_player(p, Some(0)));
            assert_eq!(c.players[p].hand.len(), 3);
        }
        assert_eq!(c.hist.played_combat.len(), 1);
        assert_eq!(c.fault, None);
    }
    }
}

#[test]
fn master_of_strategy_and_autoplay_collection_await_shuffle_for_all_seats() {
    for owner in 0..4 {
        for batch in [false, true] {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
            for _ in 0..4 {
                let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                c.players[owner].discard.push(k);
            }
            if batch { c.auto_play_from_draw(owner, 3, false); }
            else {
                let k = c.new_card(owner, CardId::MasterOfStrategy, false).unwrap();
                c.players[owner].hand.push(k);
                assert!(c.play_card(owner, 0, None));
            }
            assert_eq!(c.fault, None);
            assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
            assert!(c.hist.played_combat.is_empty());
            assert!(c.resolve_pending_choice(Some(0)));
            if c.pending_choice.is_some() { assert!(c.resolve_pending_choice(None)); }
            assert_eq!(c.fault, None);
            assert_eq!(c.players[owner].hand.len(), if batch { 1 } else { 4 });
            assert_eq!(c.hist.played_combat.len(), if batch { 3 } else { 1 });
        }
    }
}

#[test]
fn card_tail_draws_await_stratagem_and_finish_for_every_seat() {
    for owner in 0..4 {
        for id in [
            CardId::SpoilsOfBattle,
            CardId::Offering,
            CardId::Finesse,
            CardId::Impatience,
            CardId::RocketPunch,
            CardId::Scrawl,
        ] {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
            for _ in 0..4 {
                let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                c.players[owner].discard.push(q);
            }
            let card = c.new_card(owner, id, false).unwrap();
            c.players[owner].hand.push(card);
            c.players[owner].energy = 99;

            let target = (id == CardId::RocketPunch).then_some(0);
            assert!(c.play_card(owner, 0, target), "{id:?} seat {owner}");
            assert_eq!(c.fault, None, "{id:?} seat {owner}");
            assert!(c.pending_choice_for_player(owner).is_some(), "{id:?} seat {owner}");
            assert!(c.hist.played_combat.is_empty(), "{id:?} seat {owner}");
            assert!(c.players[owner].play.contains(&card), "{id:?} seat {owner}");

            assert!(c.resolve_pending_choice_for_player(owner, Some(0)), "{id:?} seat {owner}");
            assert_eq!(c.fault, None, "{id:?} seat {owner}");
            assert!(c.pending_choice_for_player(owner).is_none(), "{id:?} seat {owner}");
            assert_eq!(c.hist.played_combat.as_slice(), &[(owner, card)], "{id:?} seat {owner}");
            assert!(!c.players[owner].play.contains(&card), "{id:?} seat {owner}");
        }
    }
}

#[test]
fn modded_runs_post_draw_cost_change_only_after_stratagem_resumes() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        for _ in 0..4 {
            let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(q);
        }
        let card = c.new_card(owner, CardId::Modded, false).unwrap();
        c.players[owner].hand.push(card);
        c.players[owner].energy = 99;

        assert_eq!(c.modified_card_cost(owner, card), 0);
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.fault, None);
        assert!(c.pending_choice_for_player(owner).is_some());
        assert_eq!(c.modified_card_cost(owner, card), 0);
        assert!(c.hist.played_combat.is_empty());

        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.fault, None);
        assert_eq!(c.modified_card_cost(owner, card), 1);
        assert_eq!(c.hist.played_combat.as_slice(), &[(owner, card)]);
    }
}
