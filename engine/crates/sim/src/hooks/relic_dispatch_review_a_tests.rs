use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.start_combat();
    c
}

#[test]
fn reviewer_bookmark_normal_end_turn_rng_pick_whole_hand_and_until_played_expiry() {
    for owner in 0..4 {
        for whole_hand in [false, true] {
            let mut c = setup(owner, RelicId::Bookmark);
            if whole_hand { c.apply_power_to(owner, PowerId::WellLaidPlans, 1, Some(owner)); }
            let mut cards = Vec::new();
            for id in [CardId::DefendIronclad, CardId::StrikeIronclad, CardId::Bash] {
                let k = c.new_card(owner, id, false).unwrap();
                if !whole_hand { c.cards[k as usize].keywords |= KW_RETAIN; }
                c.players[owner].hand.push(k); cards.push(k);
            }
            let originals: Vec<_> = cards.iter().map(|&k| c.cards[k as usize].local_energy_cost()).collect();
            let mut expected_rng = c.run.rngs.combat_card_selection.clone();
            let picked = expected_rng.choice_index(cards.len());
            c.end_player_turn();
            assert!(c.fault.is_none(), "{:?}", c.fault);
            assert_eq!(c.run.rngs.combat_card_selection.counter, expected_rng.counter);
            for (i, &k) in cards.iter().enumerate() {
                assert!(c.players[owner].hand.contains(&k));
                assert_eq!(c.cards[k as usize].local_energy_cost(), originals[i] - i32::from(i == picked));
            }
            let k = cards[picked];
            let index = c.players[owner].hand.iter().position(|&q| q == k).unwrap();
            let target = (c.cards[k as usize].card_type == CardType::Attack).then_some(0);
            assert!(c.play_card(owner, index, target), "{:?}", c.fault);
            assert_eq!(c.cards[k as usize].local_energy_cost(), originals[picked]);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn reviewer_scarf_real_fifth_play_uses_free_energy_and_star_costs() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BrilliantScarf);
        c.players[owner].energy = 4;
        c.gain_stars(owner, 5);
        for _ in 0..4 {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].hand.push(k);
            assert!(c.play_card(owner, 0, None));
        }
        let fifth = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.cards[fifth as usize].set_energy_cost(3, crate::card::CostExpiry::Combat, false);
        c.cards[fifth as usize].star_cost = 2;
        c.players[owner].hand.push(fifth);
        assert!(c.play_card(owner, 0, None), "{:?}", c.fault);
        assert_eq!(c.players[owner].energy, 0);
        assert_eq!(c.players[owner].stars, 5);
        assert_eq!(c.run.relics[0].counter, 5);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn reviewer_fur_coat_real_enemy_insertion_dispatches_marked_hp_hook() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::FurCoat);
        c.run.coord = Some((2, 3));
        c.run.relics[0].set_fur_coat_coords(&[(2, 3)]).unwrap();
        let mut creature = c.cr(c.enemies[0]).clone();
        let ai = creature.ai.take().unwrap();
        let added = c.prepare_enemy_in_slot(creature, ai, None);
        assert!(c.cr(added).hp > 1);
        c.insert_prepared_enemy(added);
        assert!(c.enemies.contains(&added));
        assert_eq!(c.cr(added).hp, 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn reviewer_cables_front_passive_can_end_combat_before_extra_pass() {
    let mut c = setup(0, RelicId::GoldPlatedCables);
    c.cr_mut(c.enemies[0]).hp = 1;
    c.channel_orb(0, crate::orbs::OrbKind::Lightning);
    c.trigger_first_orb_passive(0);
    assert!(c.is_over_or_ending());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}
