//! Audited Models/Relics/<Name>.cs; normal hook/modifier dispatch at every seat.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c
}

#[test]
fn h3_hand_drill_only_owner_or_owner_pet_breaks_nonplayer_block() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::HandDrill);
        let other = (owner + 1) % 4;
        let e = c.enemies[0];
        c.cr_mut(e).block = 5;
        c.deal_damage(Some(other), e, 5, Props::UNPOWERED);
        assert_eq!(c.power_amount(e, PowerId::Vulnerable), 0);
        c.cr_mut(e).block = 5;
        c.deal_damage(Some(owner), e, 4, Props::UNPOWERED);
        assert_eq!(c.power_amount(e, PowerId::Vulnerable), 0);
        c.deal_damage(Some(owner), e, 1, Props::UNPOWERED);
        assert_eq!(c.power_amount(e, PowerId::Vulnerable), 2);
        let pet = c.summon_osty(owner, 10).unwrap();
        c.cr_mut(e).block = 5;
        c.deal_damage(Some(pet), e, 5, Props::UNPOWERED);
        assert_eq!(c.power_amount(e, PowerId::Vulnerable), 4);
        c.cr_mut(other).block = 5;
        c.deal_damage(Some(owner), other, 5, Props::UNPOWERED);
        assert_eq!(c.power_amount(other, PowerId::Vulnerable), 0);
        c.cr_mut(e).block = 5;
        c.run.relics[0].enabled = false;
        c.deal_damage(Some(owner), e, 5, Props::UNPOWERED);
        assert_eq!(c.power_amount(e, PowerId::Vulnerable), 4);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h3_mystic_lighter_uses_enchanted_source_owner_not_dealer_or_card_type() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::MysticLighter);
        let other = (owner + 1) % 4;
        let e = c.enemies[0];
        let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        assert_eq!(c.modify_damage(4, Some(owner), e, Props::MOVE, Some(own)), 4);
        for k in [own, foreign] { c.cards[k as usize].enchantments.push((crate::identity::EnchantmentId::Imbued, 1)); }
        c.cards[own as usize].card_type = CardType::Skill;
        assert_eq!(c.modify_damage(4, Some(other), e, Props::MOVE, Some(own)), 13);
        assert_eq!(c.modify_damage(4, None, e, Props::MOVE, Some(own)), 13);
        assert_eq!(c.modify_damage(4, Some(owner), e, Props::MOVE, Some(foreign)), 4);
        assert_eq!(c.modify_damage(4, Some(owner), e, Props::UNPOWERED, Some(own)), 4);
        assert_eq!(c.modify_damage(4, Some(owner), e, Props::MOVE, None), 4);
        c.run.relics[0].enabled = false;
        assert_eq!(c.modify_damage(4, Some(owner), e, Props::MOVE, Some(own)), 4);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h3_run_only_relics_preserve_each_seats_snapshot_and_rng() {
    for owner in 0..4 {
        for id in [RelicId::MiniatureTent, RelicId::JewelryBox, RelicId::LostCoffer,
            RelicId::Kaleidoscope, RelicId::JuzuBracelet, RelicId::LavaRock] {
            let mut c = setup(owner, id);
            assert!(crate::relic::is_ported(id));
            assert!(c.relic_owner_has_enabled(owner, id));
            let before = (c.run.rngs.rewards.counter, c.run.rngs.niche.counter,
                c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter,
                c.run.next_card_id, c.run.deck.len());
            c.run.relics[0].flag1 = true; // LavaRock.HasTriggered must survive combat.
            for hook in [Hook::BeforeCombatStart, Hook::BeforeCombatStartLate,
                Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
            for seat in 0..4 {
                c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(seat));
                c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(seat));
                assert_eq!((c.cr(seat).hp, c.cr(seat).max_hp), (50, 50));
                assert!(c.players[seat].hand.is_empty());
            }
            assert_eq!(before, (c.run.rngs.rewards.counter, c.run.rngs.niche.counter,
                c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter,
                c.run.next_card_id, c.run.deck.len()));
            assert!(c.run.relics[0].flag1);
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}
