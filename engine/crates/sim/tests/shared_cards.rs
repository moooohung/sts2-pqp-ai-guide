//! C# shared/event card regression coverage.
use sts2_sim::combat::{Combat, RunCtx, RunRngs, REF_CACHE_WORDS};
use sts2_sim::rng::Rng;
use sts2_sim::tables::{CardId, CardType, PowerId, RoomType, VarKey};
use sts2_sim::card::{Card, CostExpiry};

fn combat() -> Combat {
    let rng = Rng::new(1);
    let run = RunCtx {
        choice_scheduling: sts2_sim::combat::ChoiceScheduling::Sequential,
        all_cards_unlocked: None,
        rngs: RunRngs {
            seed: 1, up_front: rng.clone(), shuffle: rng.clone(),
            unknown_map_point: rng.clone(), combat_card_generation: rng.clone(),
            combat_potion_generation: rng.clone(), combat_card_selection: rng.clone(),
            combat_energy_costs: rng.clone(), combat_targets: rng.clone(),
            monster_ai: rng.clone(), niche: rng.clone(), combat_orbs: rng.clone(),
            treasure_room: rng.clone(), rewards: rng.clone(), shops: rng.clone(),
            transformations: rng,
        },
        ascension: 0, act_index: 0, total_floor: 1, coord: None,
        room_type: RoomType::Monster, gold: 0, extra_gold_reward: 0,
        max_hp: 50, hp: 50, max_energy: 3,
        max_potion_slots: 0, base_orb_slots: 0, deck: vec![],
        paels_tooth_cards: vec![], relics: vec![], potions: vec![],
        next_card_id: 0, ref_cache: [0; REF_CACHE_WORDS],
    };
    Combat::new(run, 1)
}

#[test]
fn feeding_frenzy_applies_its_temporary_strength_power_to_the_card_owner() {
    // C# FeedingFrenzy: StrengthPower 5, upgraded by 2; no target is used.
    for (upgraded, amount) in [(false, 5), (true, 7)] {
        for owner in 0..2 {
            let mut c = combat();
            c.add_player(50, 50, 3, 0, &[]);
            let card = c.new_card(owner, CardId::FeedingFrenzyCard, upgraded).unwrap();
            c.play_card_effect(card, None);
            assert_eq!(c.fault, None);
            assert_eq!(c.power_amount(owner, PowerId::FeedingFrenzy), amount);
            assert_eq!(c.power_amount(owner, PowerId::Strength), amount);
            assert_eq!(c.power_amount(1-owner, PowerId::FeedingFrenzy), 0);
            assert_eq!(c.power_amount(1-owner, PowerId::Strength), 0);
        }
    }
}

#[test]
fn wish_selects_attacks_and_skills_from_draw_without_generation() {
    let mut c = combat();
    let wish = c.new_card(0, CardId::Wish, false).unwrap();
    let attack = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let skill = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].draw.extend([attack, skill]);
    c.play_card_effect(wish, None);
    assert_eq!(c.choice_options.len(), 2);
    assert!(c.choice_options.contains(&attack));
    assert!(c.choice_options.contains(&skill));
    let pending = c.pending_choice.take().unwrap();
    c.resolve_choice_effect(pending.resolver, &[skill]);
    assert_eq!(c.players[0].draw.as_slice(), &[attack]);
    assert_eq!(c.players[0].hand.as_slice(), &[skill]);
    assert_eq!(c.hist.generated_combat, 0);
    assert_eq!(c.fault, None);
}

#[test]
fn metamorphosis_uses_the_bound_character_pool_and_combat_free_attacks() {
    for character in [sts2_sim::character::Character::Silent, sts2_sim::character::Character::Defect] {
      for upgraded in [false, true] {
        let mut c = combat();
        c.set_character_pool(0, character).unwrap();
        let k = c.new_card(0, CardId::Metamorphosis, upgraded).unwrap();
        c.play_card_effect(k, None);
        assert_eq!(c.players[0].draw.len(), if upgraded { 5 } else { 3 });
        let pool = sts2_sim::cardgen::character_pool(character.into(), false, Some(CardType::Attack), None).unwrap();
        for &q in &c.players[0].draw {
            let card = &mut c.cards[q as usize];
            assert!(pool.contains(&card.id));
            assert_eq!(card.cost, 0);
            card.after_card_played_cost_cleanup();
            card.end_of_turn_cleanup();
            assert_eq!(card.cost, 0);
        }
        assert_eq!(c.fault, None);
      }
    }
}

#[test]
fn abundance_fails_closed_before_generation_or_choice() {
    for character in sts2_sim::character::Character::ALL {
      for upgraded in [false, true] {
        let mut c = combat();
        c.set_character_pool(0, character).unwrap();
        let k = c.new_card(0, CardId::Abundance, upgraded).unwrap();
        c.play_card_effect(k, None);
        assert!(c.fault.is_some());
        assert!(c.choice_options.is_empty());
        assert!(c.pending_choice.is_none());
        assert!(c.players[0].hand.is_empty());
        assert_eq!(c.cards.len(), 1);
        assert_eq!(c.hist.generated_combat, 0);
      }
    }
}


#[test]
fn entrench_uses_raw_block_ignoring_dexterity_and_frail_and_respects_cap() {
    let mut c = combat();
    let k = c.new_card(0, CardId::Entrench, false).unwrap();
    c.apply_power_to(0, PowerId::Dexterity, 10, Some(0));
    c.apply_power_to(0, PowerId::Frail, 2, Some(0));
    c.cr_mut(0).block = 200;
    for expected in [400, 800, sts2_sim::creature::BLOCK_CAP] {
        c.play_card_effect(k, None);
        assert_eq!(c.cr(0).block, expected);
    }
    assert_eq!(c.fault, None);
}

#[test]
fn fuel_and_outmaneuver_use_the_nonzero_card_owner_and_upgraded_energy() {
    for upgraded in [false, true] {
        let mut c = combat();
        c.add_player(50, 50, 3, 0, &[]);
        let fuel = c.new_card(1, CardId::Fuel, upgraded).unwrap();
        let outmaneuver = c.new_card(1, CardId::Outmaneuver, upgraded).unwrap();
        let before = [c.players[0].energy, c.players[1].energy];
        c.play_card_effect(fuel, None);
        assert_eq!(c.players[0].energy, before[0]);
        assert_eq!(c.players[1].energy, before[1] + if upgraded { 2 } else { 1 });
        c.play_card_effect(outmaneuver, None);
        assert_eq!(c.power_amount(1, PowerId::EnergyNextTurn), if upgraded { 3 } else { 2 });
        assert_eq!(c.power_amount(0, PowerId::EnergyNextTurn), 0);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn whistle_hits_then_stuns_and_rebound_arms_its_owner() {
    for id in [CardId::Whistle, CardId::Rebound] {
        let mut c = combat();
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(2));
        let e = c.enemies[0];
        c.cr_mut(e).hp = 1000;
        c.cr_mut(e).max_hp = 1000;
        let k = c.new_card(0, id, false).unwrap();
        c.play_card_effect(k, Some(e));
        assert_eq!(c.cr(e).hp, 1000 - if id == CardId::Whistle { 33 } else { 9 });
        if id == CardId::Whistle {
            assert_eq!(c.cr(e).ai.as_ref().unwrap().current, sts2_sim::monster::STUNNED);
        } else {
            assert_eq!(c.power_amount(0, PowerId::Rebound), 1);
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn enlightenment_reduces_only_energy_and_expires_at_the_csharp_boundaries() {
    for upgraded in [false, true] {
        let mut c = combat();
        let enlightenment = c.new_card(0, CardId::Enlightenment, upgraded).unwrap();
        let mut cards = Vec::new();
        for id in [CardId::Anger, CardId::StrikeIronclad, CardId::Bash, CardId::Whirlwind, CardId::AscendersBane] {
            let k = c.new_card(0, id, false).unwrap();
            c.cards[k as usize].star_cost = 2;
            c.players[0].hand.push(k);
            cards.push(k);
        }
        let old: Vec<_> = cards.iter().map(|&k| c.cards[k as usize].cost).collect();
        c.play_card_effect(enlightenment, None);
        for (&k, &cost) in cards.iter().zip(&old) {
            let card = &mut c.cards[k as usize];
            let expected = if card.x_cost || cost < 0 { cost } else { cost.min(1) };
            assert_eq!(card.cost, expected);
            assert_eq!(card.star_cost, 2);
            let mut unplayed = card.clone();
            unplayed.end_of_turn_cleanup();
            assert_eq!(unplayed.cost, if upgraded { expected } else { cost });
            card.after_card_played_cost_cleanup();
            assert_eq!(card.cost, if upgraded { expected } else { cost });
            card.end_of_turn_cleanup();
            assert_eq!(card.cost, if upgraded { expected } else { cost });
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn energy_modifiers_preserve_order_and_expire_independently() {
    // CardEnergyCost applies the complete ordered list before clamping.
    let mut card = Card::from_factory(CardId::Bludgeon, false, 1).unwrap();
    assert_eq!(card.cost, 3);
    card.set_energy_cost(1, CostExpiry::Combat, true);
    card.add_combat_cost(-2);
    assert_eq!(card.cost, 0);
    assert_eq!(card.local_energy_cost(), -1);
    card.set_energy_cost(2, CostExpiry::Turn, false);
    assert_eq!(card.cost, 2);
    card.set_energy_cost(1, CostExpiry::TurnOrPlayed, true);
    assert_eq!(card.cost, 1);
    let mut clone = card.clone_with_id(2);
    clone.after_card_played_cost_cleanup();
    assert_eq!(clone.cost, 2);
    assert_eq!(card.cost, 1);
    clone.end_of_turn_cleanup();
    assert_eq!(clone.cost, 0);
    clone.upgrade_energy_cost(0);
    assert_eq!(clone.cost, 0);
}

#[test]
fn mad_science_skill_uses_saved_type_and_energized_rider() {
    let mut c = combat();
    let k = c.new_card(0, CardId::MadScience, false).unwrap();
    let card = &mut c.cards[k as usize];
    card.card_type = CardType::Skill;
    card.block = Some(8);
    card.vars.iter_mut().find(|(key, _)| *key == VarKey::Rider).unwrap().1 = 4;
    let before = c.players[0].energy;
    c.upgrade_card(k);
    assert_eq!(c.cards[k as usize].card_type, CardType::Skill);
    assert_eq!(c.cards[k as usize].var(VarKey::Rider), Some(4));
    c.play_card_effect(k, None);
    assert_eq!(c.cr(0).block, 8);
    assert_eq!(c.players[0].energy, before + 2);
    assert_eq!(c.fault, None);
}

#[test]
fn apparition_grants_one_intangible() {
    for upgraded in [false, true] {
        let mut c = combat();
        let k = c.new_card(0, CardId::Apparition, upgraded).unwrap();
        c.play_card_effect(k, None);
        assert_eq!(c.power_amount(0, PowerId::Intangible), 1);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn apotheosis_upgrades_only_other_cards_in_owners_combat_piles() {
    let mut c = combat();
    let apotheosis = c.new_card(0, CardId::Apotheosis, false).unwrap();
    let strike = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let defend = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    let curse = c.new_card(0, CardId::AscendersBane, false).unwrap();
    let outside_piles = c.new_card(0, CardId::Bash, false).unwrap();
    c.players[0].play.push(apotheosis);
    c.players[0].hand.extend([strike, curse]);
    c.players[0].exhaust.push(defend);
    c.play_card_effect(apotheosis, None);
    assert!(c.cards[strike as usize].upgraded);
    assert!(c.cards[defend as usize].upgraded);
    assert!(!c.cards[apotheosis as usize].upgraded);
    assert!(!c.cards[curse as usize].upgraded);
    assert!(!c.cards[outside_piles as usize].upgraded);
    assert_eq!(c.fault, None);
}

#[test]
fn mad_science_expertise_applies_strength_then_dexterity() {
    let mut c = combat();
    let k = c.new_card(0, CardId::MadScience, false).unwrap();
    c.cards[k as usize].card_type = CardType::Power;
    c.cards[k as usize].vars.iter_mut().find(|(key, _)| *key == VarKey::Rider).unwrap().1 = 7;
    c.play_card_effect(k, None);
    assert_eq!(c.power_amount(0, PowerId::Strength), 2);
    assert_eq!(c.power_amount(0, PowerId::Dexterity), 2);
    assert_eq!(c.fault, None);
}

