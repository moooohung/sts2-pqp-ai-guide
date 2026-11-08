use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng, orbs::OrbKind};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}

#[test]
fn wide05_emotion_chip_uses_real_receiver_history_including_unblocked_zero_damage() {
    for owner in 0..4 {
        for blocked in [false, true] {
            let mut c = setup(owner, RelicId::EmotionChip);
            c.players[owner].turn_number = 1;
            c.channel_orb(owner, OrbKind::Frost);
            let enemy = c.enemies[0];
            if blocked { c.cr_mut(owner).block = 1; }
            c.apply_damage(owner, 0, Props::UNPOWERED, Some(enemy));
            c.players[owner].turn_number = 2;
            c.fire_after_player_turn_start((owner + 1) % 4);
            assert_eq!(c.cr(owner).block, i32::from(blocked));
            c.fire_after_player_turn_start(owner);
            assert_eq!(c.cr(owner).block, if blocked { 1 } else { 2 });
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide05_flower_energy_modifier_and_dragon_fruit_are_owner_scoped() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::FakeHappyFlower);
        c.run.relics[0].counter = 4;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        for seat in 0..4 { assert_eq!(c.players[seat].energy, i32::from(seat == owner)); }
        let mut c = setup(owner, RelicId::BloodSoakedRose);
        for seat in 0..4 { assert_eq!(c.max_energy(seat), if seat == owner { 4 } else { 3 }); }
        let mut c = setup(owner, RelicId::DragonFruit);
        if owner > 0 { c.players[owner].gold = Some(0); }
        c.cr_mut(owner).hp = 40;
        c.change_player_gold(owner, 5);
        assert_eq!((c.cr(owner).hp, c.cr(owner).max_hp), (41, 51));
        c.change_player_gold(owner, -1);
        assert_eq!(c.cr(owner).max_hp, 51);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide05_crossbow_generates_owner_attack_and_rod_upgrades_owner_deck_only() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::Crossbow);
        c.run.all_cards_unlocked = Some(true); c.players[owner].character = Some(Character::Silent);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        let k = c.players[owner].hand[0];
        assert_eq!(c.cards[k as usize].owner, owner);
        assert_eq!(c.cards[k as usize].card_type, CardType::Attack);
        assert_eq!(c.cards[k as usize].local_energy_cost(), 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
        let mut c = setup(owner, RelicId::FishingRod);
        for seat in 0..4 {
            let mut card = crate::card::Card::from_factory(CardId::StrikeIronclad, false, seat as u32).unwrap();
            card.owner = seat; c.relic_owner_deck_mut(seat).push(card);
        }
        c.run.relics[0].counter = 2;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        for seat in 0..4 { assert_eq!(c.relic_owner_deck(seat)[0].upgraded, seat == owner); }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
