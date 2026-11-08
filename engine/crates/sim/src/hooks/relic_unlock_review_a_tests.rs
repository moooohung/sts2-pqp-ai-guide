use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng, orbs::OrbKind};

#[test]
fn crossbow_legacy_solo_generates_same_character_pool_with_missing_unlocks() {
    for unlocked in [None, Some(false), Some(true)] {
        for character in [None, Some(crate::character::Character::Ironclad), Some(crate::character::Character::Silent)] {
            if unlocked == Some(true) && character.is_some() { continue; }
            let mut c = Combat::new(combat().run, 1);
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            c.run.all_cards_unlocked = unlocked; c.players[0].character = character;
            c.run.relics.push(Relic::new(RelicId::Crossbow));
            let pool = crate::cardgen::character_pool(character.unwrap_or(crate::character::Character::Ironclad).into(), false, Some(CardType::Attack), None).unwrap();
            let mut ids = pool.to_vec();
            let mut expected = c.run.rngs.combat_card_generation.clone();
            expected.shuffle(&mut ids);
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            let k = c.players[0].hand[0];
            assert_eq!(c.cards[k as usize].id, ids[0]);
            assert_eq!(c.cards[k as usize].local_energy_cost(), 0);
            assert_eq!(c.run.rngs.combat_card_generation.counter, expected.counter);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn potion_factories_refuse_unknown_multiplayer_epochs_before_rng_for_every_seat() {
    for owner in 0..4 {
        for unlocked in [None, Some(false)] {
            for frond in [false, true] {
                let mut c = combat();
                c.run.all_cards_unlocked = unlocked;
                c.run.max_potion_slots = 3; c.run.potions = vec![None; 3];
                c.players[owner].character = Some(Character::Ironclad);
                if owner > 0 { c.players[owner].potions = Some(vec![None; 3]); }
                if frond {
                    let mut r = Relic::new(RelicId::DelicateFrond); r.owner = owner; c.run.relics.push(r);
                    c.fire_relics(Hook::BeforeCombatStart, Ev::None);
                } else {
                    let k = c.new_card(owner, CardId::Alchemize, false).unwrap();
                    c.play_card_effect(k, None);
                }
                assert_eq!(c.fault, Some("potion generation requires explicit all-unlocked multiplayer configuration"));
                assert_eq!(c.run.rngs.combat_potion_generation.counter, 0);
                assert!(c.relic_owner_potions(owner).unwrap().iter().all(Option::is_none));
            }
        }
    }
}

#[test]
fn emotion_chip_passives_include_cables_extra_front_trigger() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for id in [RelicId::EmotionChip, RelicId::GoldPlatedCables] {
        let mut r = Relic::new(id); r.owner = 1; c.run.relics.push(r);
    }
    c.channel_orb(1, OrbKind::Frost);
    c.players[1].turn_number = 1;
    c.apply_damage(1, 1, Props::UNPOWERED, None);
    c.players[1].turn_number = 2;
    c.fire_after_player_turn_start(1);
    assert_eq!(c.cr(1).block, 4); // two Frost passives, not r7's direct one.
    assert!(c.fault.is_none(), "{:?}", c.fault);
}
