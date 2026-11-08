use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c.start_combat();
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    c
}
#[test]
fn b8_boot_raises_only_owners_nonzero_powered_hp_loss_after_block() {
    for owner in 0..4 {
        for (base, block, powered, own, expected) in [
            (0,0,true,true,0), (1,0,true,true,5), (4,0,true,true,5),
            (5,0,true,true,5), (4,2,true,true,5), (4,4,true,true,0),
            (4,0,false,true,4), (4,0,true,false,4),
        ] {
            let mut c = setup(owner, RelicId::TheBoot); let target = c.enemies[0];
            c.cr_mut(target).block = block;
            let dealer = if own { owner } else { (owner + 1) % 4 };
            c.deal_damage(Some(dealer), target, base, if powered { Props::MOVE } else { Props::UNPOWERED });
            assert_eq!(200 - c.cr(target).hp, expected);
            assert!(c.fault.is_none());
        }
        let mut c = setup(owner, RelicId::TheBoot);
        let hp = c.cr(owner).hp;
        c.deal_damage(Some(owner), owner, 2, Props::MOVE);
        assert_eq!(hp - c.cr(owner).hp, 2);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b8_boot_recognizes_only_owners_osty_as_a_qualified_pet_dealer() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TheBoot); let enemy = c.enemies[0];
        let own = c.summon_osty(owner, 10).unwrap();
        let foreign = c.summon_osty((owner + 1) % 4, 10).unwrap();
        c.deal_damage(Some(foreign), enemy, 2, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 198);
        c.deal_damage(Some(own), enemy, 2, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 193);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b8_boot_intangible_cap_precedes_block_and_buffer_precedes_final_minimum() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TheBoot); let enemy = c.enemies[0];
        c.apply_power_to(enemy, PowerId::Intangible, 1, None);
        c.cr_mut(enemy).block = 1;
        c.deal_damage(Some(owner), enemy, 8, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 200);
        assert_eq!(c.cr(enemy).block, 0);
        c.deal_damage(Some(owner), enemy, 8, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 195);
        c.deal_damage(Some((owner + 1) % 4), enemy, 8, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 194);
        c.apply_power_to(enemy, PowerId::Buffer, 1, None);
        c.deal_damage(Some(owner), enemy, 8, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 194);
        assert_eq!(c.power_amount(enemy, PowerId::Buffer), 0);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b8_noncombat_models_never_replay_pickups_rewards_or_treasure_in_combat() {
    for owner in 0..4 {
        for id in [RelicId::WongoCustomerAppreciationBadge, RelicId::TouchOfOrobas,
            RelicId::NewLeaf, RelicId::WhiteBeastStatue, RelicId::TheCourier,
            RelicId::SilverCrucible, RelicId::SmallCapsule] {
            let mut c = setup(owner, id);
            let card = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].hand.push(card);
            let cards = c.cards.len(); let niche = c.run.rngs.niche.counter;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            c.fire_relics(Hook::AfterCombatVictory, Ev::None);
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert_eq!(c.cards.len(), cards); assert_eq!(c.run.rngs.niche.counter, niche);
            assert_eq!(c.run.relics[0].id, id); assert_eq!(c.run.relics.len(), 1);
            assert!(!c.cards[card as usize].upgraded);
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn b8_boot_does_not_raise_unpowered_thorns_damage_through_intangible() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TheBoot); let enemy = c.enemies[0];
        c.apply_power_to(owner, PowerId::Thorns, 3, None);
        c.apply_power_to(enemy, PowerId::Intangible, 1, None);
        c.deal_damage(Some(enemy), owner, 8, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, 199);
        assert!(c.fault.is_none());
    }
}
