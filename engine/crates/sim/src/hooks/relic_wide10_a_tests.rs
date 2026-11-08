use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, ids: &[RelicId]) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for &id in ids { let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); }
    c
}

#[test]
fn wide10_ectoplasm_vetoes_owner_gold_before_bowler_or_dragon_in_either_order() {
    for owner in 0..4 {
        for ids in [
            [RelicId::Ectoplasm, RelicId::BowlerHat, RelicId::DragonFruit],
            [RelicId::BowlerHat, RelicId::DragonFruit, RelicId::Ectoplasm],
        ] {
            let mut c = setup(owner, &ids);
            c.run.gold = 100;
            for seat in 1..4 { c.players[seat].gold = Some(100); }
            c.change_player_gold(owner, 20);
            assert_eq!(c.relic_owner_gold(owner), Some(100));
            assert_eq!(c.cr(owner).max_hp, 50);
            let other = (owner + 1) % 4;
            c.change_player_gold(other, 20);
            assert_eq!(c.relic_owner_gold(other), Some(120));
            assert_eq!(c.max_energy(owner), 4);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide10_fake_tea_consumes_saved_flag_only_on_owners_reset() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::FakeVenerableTeaSet]);
        c.run.relics[0].flag1 = true;
        c.fire_after_energy_reset((owner + 1) % 4);
        assert!(c.run.relics[0].flag1);
        c.fire_after_energy_reset(owner);
        assert_eq!(c.players[owner].energy, 1);
        assert!(!c.run.relics[0].flag1);
        c.fire_after_energy_reset(owner);
        assert_eq!(c.players[owner].energy, 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide10_fiddle_uses_real_draw_dispatch_for_owner_and_other_seats() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::Fiddle]);
        for seat in 0..4 {
            let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
            c.players[seat].draw.push(k);
        }
        c.draw_cards(owner, 1, false);
        assert!(c.players[owner].hand.is_empty());
        let other = (owner + 1) % 4;
        c.draw_cards(other, 1, false);
        assert_eq!(c.players[other].hand.len(), 1);
        c.draw_cards(owner, 1, true);
        assert_eq!(c.players[owner].hand.len(), 1);
        assert_eq!(c.relic_modify_hand_draw(0, owner, 5, true), 7);
        c.side = Side::Enemy;
        assert!(c.should_draw(owner, false));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide10_chemical_x_boosts_real_owner_x_play_without_extra_energy_payment() {
    for owner in 0..4 {
        for player in [owner, (owner + 1) % 4] {
            let mut c = setup(owner, &[RelicId::ChemicalX]);
            let enemy = c.enemies[0]; c.cr_mut(enemy).hp = 200; c.cr_mut(enemy).max_hp = 200;
            let k = c.new_card(player, CardId::Whirlwind, false).unwrap();
            let damage = c.cards[k as usize].damage.or(c.cards[k as usize].var(VarKey::Damage)).unwrap();
            c.players[player].hand.push(k); c.players[player].energy = 3;
            assert!(c.play_card(player, 0, None), "{:?}", c.fault);
            assert_eq!(c.players[player].energy, 0);
            assert_eq!(c.cr(enemy).hp, 200 - damage * if owner == player { 5 } else { 3 });
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide10_equipped_noncombat_models_preserve_existing_hp_and_deck() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::GlassEye, RelicId::Glitter,
            RelicId::Driftwood, RelicId::EternalFeather]);
        c.cr_mut(owner).hp = 40;
        c.start_combat();
        assert_eq!(c.cr(owner).hp, 40);
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.players[owner].hand.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
