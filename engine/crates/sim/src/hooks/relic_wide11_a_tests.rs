use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}

#[test]
fn wide11_diadem_targets_owner_only_on_owners_first_turn_not_round() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::DiamondDiadem);
        c.round = 99; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
        assert_eq!(c.cr(owner).block, 0);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        for seat in 0..4 {
            assert_eq!(c.cr(seat).block, if seat == owner { 20 } else { 0 });
            assert_eq!(c.power_amount(seat, PowerId::Blur), i32::from(seat == owner));
        }
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 20);
        assert_eq!(c.power_amount(owner, PowerId::Blur), 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide11_forgotten_soul_uses_owner_exhaust_and_shared_target_rng_once() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::ForgottenSoul);
        for enemy in c.enemies.clone() { c.cr_mut(enemy).hp = 200; c.cr_mut(enemy).max_hp = 200; }
        let other = (owner + 1) % 4;
        let foreign = c.new_card(other, CardId::Dazed, false).unwrap();
        let mut rng = c.run.rngs.combat_targets.clone();
        c.fire_after_card_exhausted(foreign);
        assert_eq!(c.run.rngs.combat_targets.counter, rng.counter);
        let targets = c.hittable_enemies();
        let target = targets[rng.choice_index(targets.len())];
        let own = c.new_card(owner, CardId::Dazed, false).unwrap();
        c.active_card = Some(foreign); // enclosing teammate play is not the damage source.
        c.fire_after_card_exhausted(own);
        for enemy in targets {
            assert_eq!(c.cr(enemy).hp, if enemy == target { 199 } else { 200 });
        }
        let damage = c.hist.damage.last().unwrap();
        assert_eq!(damage.dealer, Some(owner));
        assert_eq!(c.active_card, Some(foreign));
        assert_eq!(c.run.rngs.combat_targets.counter, rng.counter);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide11_equipped_acquisition_and_reward_models_do_not_replay_run_effects() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::FakeLeesWaffle, RelicId::FrozenEgg, RelicId::ElectricShrymp,
            RelicId::DingyRug, RelicId::DustyTome, RelicId::GoldenPearl] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.run.gold = 99; c.cr_mut(owner).hp = 40;
        c.start_combat();
        assert_eq!(c.run.gold, 99);
        assert_eq!(c.cr(owner).hp, 40);
        assert!(c.relic_owner_deck(owner).is_empty());
        let power = c.new_card(owner, CardId::Inflame, false).unwrap();
        c.add_generated_card_by(power, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert!(!c.cards[power as usize].upgraded); // FrozenEgg upgrades Deck, not Hand generation.
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
