use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn wide02_simple_start_effects_are_owner_scoped() {
    for owner in 0..4 {
        for id in [RelicId::FakeAnchor, RelicId::FakeSneckoEye, RelicId::GhostSeed] {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            for seat in 0..4 {
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                c.players[seat].draw.push(k);
            }
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..4 {
                assert_eq!(c.cr(seat).block, if seat == owner && id == RelicId::FakeAnchor { 4 } else { 0 });
                assert_eq!(c.has_power(seat, PowerId::Confused), seat == owner && id == RelicId::FakeSneckoEye);
                let k = c.players[seat].draw[0];
                assert_eq!(c.cards[k as usize].keywords & KW_ETHEREAL != 0, seat == owner && id == RelicId::GhostSeed);
            }
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide02_lava_lamp_ignores_teammate_damage_and_music_box_clones_owner_attack() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::LavaLamp, RelicId::MusicBox] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.deal_damage(Some(c.enemies[0]), (owner + 1) % 4, 1, Props::MOVE);
        assert!(!c.run.relics[0].flag1);
        c.deal_damage(Some(c.enemies[0]), owner, 1, Props::MOVE);
        assert!(c.run.relics[0].flag1);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert!(!c.run.relics[0].flag1);
        let other = c.new_card((owner + 1) % 4, CardId::StrikeIronclad, false).unwrap();
        c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(other));
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(other));
        assert!(c.run.relics[1].card.is_none());
        let k = c.new_card(owner, CardId::StrikeIronclad, true).unwrap();
        c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(k));
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        let clone = *c.players[owner].hand.last().unwrap();
        assert_eq!(c.cards[clone as usize].owner, owner);
        assert!(c.cards[clone as usize].upgraded);
        assert!(c.cards[clone as usize].keywords & KW_ETHEREAL != 0);
        let count = c.cards.len();
        c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(k));
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.cards.len(), count);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
