use sts2_sim::{combat::Combat, relic::Relic, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId, RelicId}};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.run.relics.clear();
    for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn synchronous_pile_selector_finishes_each_seats_draw_before_later_setup() {
    let mut c = combat();
    c.run.choice_scheduling = sts2_sim::combat::ChoiceScheduling::LiveHandAndChooseOnly;
    c.in_play_phase = true;
    c.round = 1;
    for owner in 0..4 {
        c.players[owner].draw.clear();
        c.players[owner].discard.clear();
        c.players[owner].hand.clear();
        c.players[owner].turn_number = 1;
        for _ in 0..4 {
            let card = c.new_card(owner, CardId::Wound, false).unwrap();
            c.players[owner].hand.push(card);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
    }
    c.end_player_turn();
    for owner in 0..4 {
        assert_eq!(c.fault, None);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert_eq!(c.players[owner].draw.len(), 4);
        for later in owner + 1..4 {
            assert_eq!(c.players[later].discard.len(), 4, "later seat has not started its shuffle");
            assert!(c.players[later].draw.is_empty());
            assert!(c.players[later].hand.is_empty());
        }
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.players[owner].hand.len(), 4);
    }
    assert!(c.pending_choice.is_none());
    assert_eq!(c.fault, None);
}

#[test]
fn waterfall_stomp_tail_waits_for_every_puzzles_draw_before_weak_and_steam() {
    // C# WaterfallGiant.cs:243-250 awaits Attack.Execute before Weak/Steam.
    // Frozen r7 monsters/act4.py:2029-2038 (create_waterfall_giant.stomp) falls
    // through after a suspended draw; Sequential solo must retain the C# order.
    for (seats, owner) in [(1, 0), (4, 0), (4, 1), (4, 2), (4, 3)] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.ascension = 0;
        if seats == 1 { run.choice_scheduling = sts2_sim::combat::ChoiceScheduling::Sequential; }
        let mut c = Combat::new(run, 1);
        c.run.relics.clear();
        for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("WATERFALL_GIANT_BOSS", &mut Rng::new(1));
        let enemy = c.enemies[0];
        let mv = {
            let ai = c.cr(enemy).ai.as_ref().unwrap();
            sts2_sim::content::state_index(ai.def(), "STOMP_MOVE")
        };
        c.apply_power_to(enemy, PowerId::SteamEruption, 15, Some(enemy));
        let mut puzzle = Relic::new(RelicId::CentennialPuzzle); puzzle.owner = owner;
        c.run.relics.push(puzzle);
        c.players[owner].draw.clear();
        for _ in 0..4 {
            let card = c.new_card(owner, CardId::Wound, false).unwrap();
            c.players[owner].discard.push(card);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        c.monster_move_effect(enemy, mv);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert_eq!(c.power_amount(enemy, PowerId::SteamEruption), 15);
        for seat in 0..seats { assert_eq!(c.power_amount(seat, PowerId::Weak), 0); }
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.power_amount(enemy, PowerId::SteamEruption), 18);
        for seat in 0..seats { assert_eq!(c.power_amount(seat, PowerId::Weak), 1); }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn nested_thorns_complete_slippery_callbacks_before_the_next_seat() {
    for first in 0..4 {
        let mut c = combat();
        let enemy = c.enemies[0];
        c.cr_mut(enemy).hp = 100;
        c.cr_mut(enemy).max_hp = 100;
        c.apply_power_to(enemy, PowerId::Slippery, 1, Some(enemy));
        let slippery = c.find_power(enemy, PowerId::Slippery).unwrap();
        c.powers[slippery as usize].amount = 1; // Corpse Slug already starts with Slippery.
        for seat in [first, (first + 1) % 4] {
            c.apply_power_to(seat, PowerId::Thorns, 5, Some(seat));
        }
        c.monster_attack_players(enemy, 1, 1);
        assert_eq!(c.cr(enemy).hp, 94, "first retaliation capped at 1; second deals 5");
        assert_eq!(c.power_amount(enemy, PowerId::Slippery), 0);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn iron_club_awaits_draw_before_later_listeners_and_result_for_every_seat() {
    for owner in 0..4 {
        let mut c = combat();
        let mut club = Relic::new(RelicId::IronClub);
        club.owner = owner; club.counter = 3;
        c.run.relics.push(club);
        let mut fork = Relic::new(RelicId::TuningFork); fork.owner = owner;
        c.run.relics.push(fork);
        let zap = c.new_card(owner, CardId::Zap, false).unwrap();
        c.players[owner].hand.push(zap);
        for _ in 0..4 {
            let card = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(card);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        c.players[owner].energy = 3;
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.fault, None);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert!(c.players[owner].play.contains(&zap));
        assert_eq!(c.run.relics[0].counter, 4);
        assert_eq!(c.run.relics[1].counter, 0, "later listeners must await IronClub");
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.players[owner].hand.len(), 2);
        assert_eq!(c.run.relics[0].counter, 4, "resume must not replay IronClub");
        assert_eq!(c.run.relics[1].counter, 1);
        assert!(c.players[owner].play.is_empty());
        assert!(c.players[owner].discard.contains(&zap));
        for seat in 0..4 { if seat != owner { assert!(c.players[seat].hand.is_empty()); } }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn joss_paper_awaits_draw_before_counter_reset_later_listeners_and_cleanup() {
    for owner in 0..4 {
        let mut c = combat();
        let mut paper = Relic::new(RelicId::JossPaper); paper.owner = owner; paper.counter = 4;
        c.run.relics.push(paper);
        let mut ashes = Relic::new(RelicId::CharonsAshes); ashes.owner = owner;
        c.run.relics.push(ashes);
        let soul = c.new_card(owner, CardId::Soul, false).unwrap();
        c.players[owner].hand.push(soul);
        for i in 0..5 {
            let card = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            if i < 2 { c.players[owner].draw.push(card); }
            else { c.players[owner].discard.push(card); }
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        c.players[owner].energy = 3;
        let enemy = c.enemies[0];
        let hp = c.cr(enemy).hp;
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.fault, None);
        assert!(c.players[owner].exhaust.contains(&soul));
        assert_eq!(c.run.relics[0].counter, 5, "modulo follows the awaited draw");
        assert_eq!(c.cr(enemy).hp, hp, "later Charon listener must not run early");
        assert_eq!(c.pending_play.len(), 1, "result-pile callback still owns the play");
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.players[owner].hand.len(), 4);
        assert_eq!(c.run.relics[0].counter, 0);
        assert_eq!(c.cr(enemy).hp, hp - 3);
        assert!(c.pending_play.is_empty());
        assert_eq!(c.players[owner].exhaust.iter().filter(|&&k| k == soul).count(), 1);
        assert_eq!(c.hist.played_combat.as_slice(), &[(owner, soul)]);
        for seat in 0..4 { if seat != owner { assert!(c.players[seat].hand.is_empty()); } }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn centennial_puzzle_applies_all_targets_before_awaited_listeners_and_next_hit() {
    for owner in 0..4 {
        let mut c = combat();
        for seat in 0..4 { c.cr_mut(seat).hp = 50; c.cr_mut(seat).max_hp = 50; }
        let mut puzzle = Relic::new(RelicId::CentennialPuzzle); puzzle.owner = owner;
        c.run.relics.push(puzzle);
        let mut clay = Relic::new(RelicId::SelfFormingClay); clay.owner = owner;
        c.run.relics.push(clay);
        for _ in 0..5 {
            let card = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(card);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        let enemy = c.enemies[0];
        c.monster_attack_players(enemy, 1, 2);
        assert_eq!(c.fault, None);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert_eq!(c.power_amount(owner, PowerId::SelfFormingClay), 0);
        for seat in 0..4 {
            assert_eq!(c.cr(seat).hp, 49, "all targets take the first hit before any received callback");
        }
        assert!(!c.resolve_pending_choice_for_player((owner + 1) % 4, Some(0)));
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.players[owner].hand.len(), 4); // selection, then three distinct draws
        assert_eq!(c.hist.cards_drawn_combat, 3);
        assert_eq!(c.power_amount(owner, PowerId::SelfFormingClay), 6);
        for seat in 0..4 {
            assert_eq!(c.cr(seat).hp, 48);
            if seat != owner { assert!(c.players[seat].hand.is_empty()); }
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn exhaust_callers_await_each_joss_draw_before_block_remaining_exhausts_and_hits() {
    for owner in 0..4 {
        for id in [CardId::SecondWind, CardId::FiendFire] {
            let mut c = combat();
            let mut paper = Relic::new(RelicId::JossPaper); paper.owner = owner; paper.counter = 4;
            c.run.relics.push(paper);
            let caller = c.new_card(owner, id, false).unwrap();
            c.players[owner].hand.push(caller);
            let mut fodder = Vec::new();
            for _ in 0..2 {
                let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                fodder.push(q); c.players[owner].hand.push(q);
            }
            for _ in 0..4 {
                let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                c.players[owner].discard.push(q);
            }
            c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
            c.players[owner].energy = 3;
            let enemy = c.enemies[0];
            c.cr_mut(enemy).hp = 500; c.cr_mut(enemy).max_hp = 500;
            let damage = c.cards[caller as usize].damage.unwrap_or(0);
            assert!(c.play_card(owner, 0, (id == CardId::FiendFire).then_some(0)));
            assert_eq!(c.fault, None);
            assert!(c.players[owner].exhaust.contains(&fodder[0]));
            assert!(c.players[owner].hand.contains(&fodder[1]));
            assert_eq!(c.cr(owner).block, 0);
            assert_eq!(c.cr(enemy).hp, 500);
            assert!(c.hist.played_combat.is_empty());
            assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
            assert!(fodder.iter().all(|q| c.players[owner].exhaust.contains(q)));
            assert_eq!(c.players[owner].hand.len(), 2, "newly drawn cards are outside the snapshot");
            assert_eq!(c.cr(owner).block, if id == CardId::SecondWind { 10 } else { 0 });
            assert_eq!(c.cr(enemy).hp, 500 - damage * 2);
            assert_eq!(c.hist.played_combat.as_slice(), &[(owner, caller)]);
            assert!(c.pending_play.is_empty());
            assert_eq!(c.fault, None);
        }
    }
}

