use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}

#[test]
fn wide08_antler_uses_normal_before_draw_dispatch_and_generated_card_lifecycle() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BlessedAntler);
        c.round = 99; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature((owner + 1) % 4));
        assert!(c.players[owner].draw.is_empty());
        assert_eq!(c.max_energy(owner), 4);
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].draw.len(), 3);
        assert_eq!(c.hist.generated_combat, 3);
        assert_eq!(c.hist.generated_by_player_combat, 3);
        for &k in &c.players[owner].draw {
            assert_eq!(c.cards[k as usize].owner, owner);
            assert_eq!(c.cards[k as usize].id, CardId::Dazed);
        }
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].draw.len(), 3);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide08_bowler_modifies_only_owner_positive_gold_gains_and_truncates_fraction() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BowlerHat);
        c.run.gold = 100;
        for seat in 1..4 { c.players[seat].gold = Some(100); }
        for seat in 0..4 {
            c.change_player_gold(seat, 3); // 3.75 -> 3
            c.change_player_gold(seat, 4); // 5 for Owner; 4 otherwise
            c.change_player_gold(seat, -2);
            assert_eq!(c.relic_owner_gold(seat), Some(if seat == owner { 106 } else { 105 }));
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn reviewer_fake_dummy_real_damage_dispatch_checks_dealer_or_card_owner() {
    for owner in 0..4 {
        let other = (owner + 1) % 4;
        for (dealer, card_owner, powered, bonus) in [
            (owner, other, true, 1), (other, owner, true, 1),
            (other, other, true, 0), (owner, owner, false, 0),
        ] {
            let mut c = setup(owner, RelicId::FakeStrikeDummy);
            let k = c.new_card(card_owner, CardId::StrikeIronclad, false).unwrap();
            let enemy = c.enemies[0];
            c.cr_mut(enemy).hp = 200; c.cr_mut(enemy).max_hp = 200;
            c.active_card = Some(k);
            c.deal_damage(Some(dealer), enemy, 6, if powered { Props::MOVE } else { Props::UNPOWERED });
            assert_eq!(c.cr(enemy).hp, 200 - 6 - bonus);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide08_equipped_snapshot_only_models_do_not_repeat_pickup_or_deck_effects() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::CallingBell, RelicId::FragrantMushroom, RelicId::Circlet,
            RelicId::BingBong, RelicId::BookOfFiveRings, RelicId::AmethystAubergine] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.start_combat();
        assert_eq!(c.cr(owner).hp, 50);
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.players[owner].hand.is_empty());
        let card = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.add_generated_card_by(card, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.players[owner].hand.len(), 1); // BingBong clones Deck, not Hand.
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
