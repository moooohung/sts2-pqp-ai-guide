use sts2_sim::character::Character;
use sts2_sim::{
    combat::Combat, hooks::Ev, run::{RunManager, RunOptions},
    tables::{CardId, CardType, Hook, PotionId, PowerId, KW_ETHEREAL},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.run.all_cards_unlocked = Some(true);
    c
}

#[test]
fn nested_fetch_hellraiser_iron_club_draws_finish_every_awaited_callback() {
    use sts2_sim::{relic::Relic,tables::RelicId};
    let mut c=combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut sts2_sim::rng::Rng::new(1));
    c.combat_started=true;
    c.summon_osty(0,10);
    let fetch=c.new_card(0,CardId::Fetch,false).unwrap();
    let defend=c.new_card(0,CardId::DefendNecrobinder,false).unwrap();
    let other=c.new_card(0,CardId::DefendNecrobinder,false).unwrap();
    let strike=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
    let sculpt=c.new_card(0,CardId::SculptingStrike,false).unwrap();
    c.players[0].hand.extend([fetch,defend,other]);
    c.players[0].draw.extend([strike,sculpt]);
    let mut club=Relic::new(RelicId::IronClub);club.counter=3;
    c.run.relics.push(club);
    c.apply_power_to(0,PowerId::Hellraiser,1,Some(0));
    c.apply_power_to(0,PowerId::Automation,1,Some(0));
    assert!(c.play_card(0,0,Some(0)));
    assert!(c.pending_choice.is_some());
    assert_eq!(c.fault,None);
    let automation=c.find_power(0,PowerId::Automation).unwrap();
    assert_eq!(c.powers[automation as usize].vars[0],10);
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.fault,None);
    assert_eq!(c.hist.cards_drawn_combat,2);
    assert_eq!(c.powers[automation as usize].vars[0],8);
    assert!(c.pending_play.is_empty());
    for card in [fetch,strike,sculpt] {assert!(c.players[0].discard.contains(&card));}
}

#[test]
fn fetch_awaits_hellraiser_choice_before_draw_hooks_and_parent_completion() {
    let mut c=combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut sts2_sim::rng::Rng::new(1));
    c.summon_osty(0,10);
    let fetch=c.new_card(0,CardId::Fetch,false).unwrap();
    let defend=c.new_card(0,CardId::DefendNecrobinder,false).unwrap();
    let other=c.new_card(0,CardId::DefendNecrobinder,false).unwrap();
    let strike=c.new_card(0,CardId::SculptingStrike,false).unwrap();
    c.players[0].hand.extend([fetch,defend,other]);
    c.players[0].draw.push(strike);
    c.apply_power_to(0,PowerId::Hellraiser,1,Some(0));
    c.apply_power_to(0,PowerId::Automation,1,Some(0));
    assert!(c.play_card(0,0,Some(0)));
    assert!(c.pending_choice.is_some());
    assert_eq!(c.fault,None);
    let automation=c.find_power(0,PowerId::Automation).unwrap();
    assert_eq!(c.powers[automation as usize].vars[0],10);
    assert!(!c.hist.played_combat.contains(&(0,fetch)));
    assert!(!c.hist.played_combat.contains(&(0,strike)));
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.fault,None);
    assert_eq!(c.powers[automation as usize].vars[0],9);
    assert!(c.cards[defend as usize].keywords & KW_ETHEREAL != 0);
    assert_eq!(c.cards[other as usize].keywords & KW_ETHEREAL,0);
    assert!(c.hist.played_combat.contains(&(0,strike)));
    assert!(c.hist.played_combat.contains(&(0,fetch)));
    assert!(c.players[0].discard.contains(&strike));
    assert!(c.players[0].discard.contains(&fetch));
    assert!(c.pending_play.is_empty());
    assert_eq!(c.hist.cards_drawn_combat,1);
}

#[test]
fn audited_character_generation_handles_all_seats_and_constraints() {
    for mp in [false, true] {
        for character in Character::ALL {
            let mut c = combat();
            let owner = if mp { c.add_player(50,50,3,0,&[]) } else { 0 };
            c.players[owner].character = Some(character);
            let cards = c.generate_unlocked_owner_cards(owner, Some(CardType::Power), 3);
            assert_eq!(cards.len(), 3, "{character} mp={mp}: {:?}", c.fault);
            assert!(cards.iter().all(|&q| c.cards[q as usize].owner == owner && c.cards[q as usize].card_type == CardType::Power));
            assert_eq!(c.fault, None);
        }
    }
}

#[test]
fn call_of_void_generates_ethereal_for_only_owner_and_partial_unlocks_fail_closed() {
    let mut c = combat();
    let p = c.add_player(50,50,3,0,&[]);
    c.players[p].character = Some(Character::Necrobinder);
    c.apply_power_to(p, PowerId::CallOfTheVoid, 2, Some(p));
    c.fire(Hook::BeforeHandDraw, Ev::Creature(0), None);
    assert!(c.players[p].hand.is_empty());
    c.fire(Hook::BeforeHandDraw, Ev::Creature(p), None);
    assert_eq!(c.players[p].hand.len(), 2);
    assert!(c.players[p].hand.iter().all(|&q| c.cards[q as usize].keywords & KW_ETHEREAL != 0));
    assert_eq!(c.fault, None);
    let mut partial = combat();
    partial.run.all_cards_unlocked = Some(false);
    assert!(partial.generate_unlocked_owner_cards(0, None, 1).is_empty());
    assert!(partial.fault.is_some());
}

#[test]
fn abundance_upgrades_all_options_before_selection_and_only_chosen_card_enters() {
    let mut c = combat();
    let k = c.new_card(0, CardId::Abundance, false).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.choice_options.len(), 3);
    assert!(c.choice_options.iter().all(|&q| c.cards[q as usize].upgraded));
    let selected = c.choice_options[1];
    assert!(c.players[0].hand.is_empty());
    assert!(c.resolve_pending_choice(Some(1)));
    assert_eq!(c.players[0].hand.as_slice(), &[selected]);
    assert_eq!(c.modified_card_cost(0, selected), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn eidolon_awaits_child_choice_before_playing_remaining_snapshot_and_finishing_parent() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
    let pact = c.new_card(0, CardId::BurningPact, false).unwrap();
    let strike = c.new_card(0, CardId::StrikeNecrobinder, false).unwrap();
    for q in [pact, strike] {
        c.cards[q as usize].keywords |= KW_ETHEREAL;
        c.players[0].exhaust.push(q);
    }
    let eidolon = c.new_card(0, CardId::Eidolon, false).unwrap();
    c.players[0].hand.push(eidolon);
    for _ in 0..2 {
        let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].hand.push(q);
    }
    c.players[0].energy = 3;
    assert!(c.play_card(0, 0, None));
    assert!(c.pending_choice.is_some());
    assert!(!c.hist.played_combat.contains(&(0, strike)));
    assert!(!c.hist.played_combat.contains(&(0, eidolon)));
    assert!(c.resolve_pending_choice(Some(0)));
    assert!(c.hist.played_combat.contains(&(0, pact)));
    assert!(c.hist.played_combat.contains(&(0, strike)));
    assert!(c.hist.played_combat.contains(&(0, eidolon)));
    assert_eq!(c.fault, None);
}

#[test]
fn mayhem_autoplays_after_setup_not_before_hand_draw() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
    let q = c.new_card(0, CardId::StrikeNecrobinder, false).unwrap();
    c.players[0].draw.push(q);
    c.apply_power_to(0, PowerId::Mayhem, 1, Some(0));
    c.fire(Hook::BeforeHandDrawLate, Ev::Creature(0), None);
    assert!(!c.hist.played_combat.contains(&(0, q)));
    c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(0), None);
    assert!(c.hist.played_combat.contains(&(0, q)));
    assert_eq!(c.fault, None);
}

#[test]
fn stratagem_awaits_draw_selector_then_resumes_the_original_draw_count() {
    let mut c = combat();
    for _ in 0..4 {
        let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(q);
    }
    c.apply_power_to(0, PowerId::Stratagem, 1, Some(0));
    c.draw_cards(0, 2, true); // turn-start hand draw has an exact continuation
    assert!(c.players[0].hand.is_empty());
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[0].hand.len(), 3); // selected card, then the two original draws
    assert_eq!(c.hist.cards_drawn_combat, 2); // selection is not a draw
    assert_eq!(c.fault, None);
}

#[test]
fn standalone_autoplay_batch_continues_after_unplayable_card() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
    for id in [CardId::AscendersBane, CardId::DefendIronclad, CardId::DefendIronclad] {
        let q = c.new_card(0, id, false).unwrap();
        c.players[0].draw.push(q);
    }
    c.auto_play_from_draw(0, 3, false);
    assert!(c.players[0].play.is_empty());
    assert_eq!(c.cr(0).block, 10);
    assert_eq!(c.players[0].discard.len(), 3);
    assert_eq!(c.fault, None);
}

#[test]
fn shrug_it_off_awaits_shuffle_choice_before_after_play_and_result_for_every_seat() {
    for owner in 0..4 {
        let mut c = combat();
        for _ in 0..owner { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
        let shrug = c.new_card(owner, CardId::ShrugItOff, false).unwrap();
        c.players[owner].hand.push(shrug);
        for _ in 0..3 {
            let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(q);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        c.players[owner].energy = 3;
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert!(c.players[owner].play.contains(&shrug));
        assert!(c.players[owner].hand.is_empty());
        assert_eq!(c.cr(owner).block, 8);
        assert_eq!(c.fault, None);
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        // StratagemPower.AfterShuffle adds its selection, then Shrug draws one more.
        assert_eq!(c.players[owner].hand.len(), 2);
        assert!(c.players[owner].play.is_empty());
        assert!(c.players[owner].discard.contains(&shrug));
        assert_eq!(c.fault, None);
    }
}

#[test]
fn battle_trance_awaits_shuffle_selection_before_applying_no_draw() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
    let trance = c.new_card(0, CardId::BattleTrance, false).unwrap();
    c.players[0].hand.push(trance);
    let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].draw.push(q);
    for _ in 0..3 {
        let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(q);
    }
    c.apply_power_to(0, PowerId::Stratagem, 1, Some(0));
    c.players[0].energy = 3;
    assert!(c.play_card(0, 0, None));
    assert_eq!(c.fault,None);
    assert_eq!(c.power_amount(0,PowerId::NoDraw),0);
    assert!(c.resolve_pending_choice(Some(0)));
    // BattleTrance.OnPlay awaits Draw before applying NoDraw.
    assert_eq!(c.players[0].hand.len(), 4);
    assert_eq!(c.power_amount(0,PowerId::NoDraw),1);
    assert_eq!(c.fault,None);
}

#[test]
fn slimed_awaits_shuffle_selection_and_draw_before_exhaust_for_every_seat() {
    for owner in 0..4 {
        let mut c = combat();
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
        let slimed = c.new_card(owner, CardId::Slimed, false).unwrap();
        c.players[owner].hand.push(slimed);
        for _ in 0..3 {
            let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(q);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        c.players[owner].energy = 3;
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
        assert!(c.players[owner].play.contains(&slimed));
        assert!(!c.players[owner].exhaust.contains(&slimed));
        assert!(c.hist.played_combat.is_empty());
        assert_eq!(c.fault, None);
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.players[owner].hand.len(), 2);
        assert_eq!(c.hist.cards_drawn_combat, 1);
        assert!(c.players[owner].play.is_empty());
        assert!(c.players[owner].exhaust.contains(&slimed));
        assert_eq!(c.hist.played_combat.as_slice(), &[(owner, slimed)]);
        for seat in 0..4 {
            if seat != owner {
                assert!(c.players[seat].hand.is_empty());
                assert!(c.players[seat].exhaust.is_empty());
            }
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn snecko_oil_suspended_draw_without_active_card_fails_closed() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
    let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].draw.push(q);
    for _ in 0..3 {
        let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(q);
    }
    c.apply_power_to(0, PowerId::Stratagem, 1, Some(0));
    assert!(c.active_card.is_none());
    c.potion_effect(PotionId::Sneckooil, None);
    assert!(c.pending_choice.is_some());
    assert_eq!(c.players[0].hand.len(), 1);
    assert_eq!(c.fault, Some("caller awaiting suspended draw continuation"));
}

