use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, ids: &[RelicId]) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for &id in ids { let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); }
    c
}

#[test]
fn wide04_fake_orichalcum_latches_owner_zero_block_not_later_block() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::FakeOrichalcum]);
        c.cr_mut((owner + 1) % 4).block = 20;
        c.fire_relics(Hook::BeforeTurnEndVeryEarly, Ev::Side(Side::Player));
        c.cr_mut(owner).block = 7;
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 10);
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 10);
        assert_eq!(c.cr((owner + 1) % 4).block, 20);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide04_mask_first_owner_draw_and_core_first_owner_turn() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::FuneraryMask, RelicId::CrackedCore]);
        c.players[owner].turn_number = 1; c.round = 99;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature((owner + 1) % 4));
        assert!(c.players[owner].draw.is_empty());
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].draw.len(), 3);
        for &k in &c.players[owner].draw {
            assert_eq!(c.cards[k as usize].id, CardId::Soul);
            assert_eq!(c.cards[k as usize].owner, owner);
        }
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].orbs.orbs.len(), 1);
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].draw.len(), 3);
        assert_eq!(c.players[owner].orbs.orbs.len(), 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide04_phylactery_energy_reset_late_and_flute_attack_use_pet_owner() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::BoundPhylactery, RelicId::BoneFlute]);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        let pet = c.osty(owner).unwrap();
        assert_eq!(c.cr(pet).hp, 1);
        c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterEnergyResetLate, Ev::Creature(owner));
        assert_eq!(c.cr(pet).hp, 1);
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterEnergyResetLate, Ev::Creature((owner + 1) % 4));
        assert_eq!(c.cr(pet).hp, 1);
        c.fire_relics(Hook::AfterEnergyResetLate, Ev::Creature(owner));
        assert_eq!(c.cr(pet).hp, 2);
        c.fire_relics(Hook::AfterAttack, Ev::Attack { attacker: Some((owner + 1) % 4),
            target: None, props: Props::MOVE, card: None });
        assert_eq!(c.cr(owner).block, 0);
        c.fire_relics(Hook::AfterAttack, Ev::Attack { attacker: Some(pet),
            target: None, props: Props::MOVE, card: None });
        assert_eq!(c.cr(owner).block, 2);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide04_star_listeners_keep_owner_remainder_and_reset_mini_only_on_owner_side() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::GalacticDust, RelicId::MiniRegent]);
        c.fire_relics(Hook::OnStarsSpent, Ev::Stars { owner: (owner + 1) % 4, amount: 25 });
        assert_eq!(c.run.relics[0].counter, 0);
        c.fire_relics(Hook::OnStarsSpent, Ev::Stars { owner, amount: 25 });
        assert_eq!(c.cr(owner).block, 20);
        assert_eq!(c.run.relics[0].counter, 5);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
        c.fire_relics(Hook::OnStarsSpent, Ev::Stars { owner, amount: 4 });
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        c.fire_relics(Hook::OnStarsSpent, Ev::Stars { owner, amount: 1 });
        assert_eq!(c.cr(owner).block, 30);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.fire_relics(Hook::OnStarsSpent, Ev::Stars { owner, amount: 1 });
        assert_eq!(c.power_amount(owner, PowerId::Strength), 2);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide04_ivory_tile_uses_real_play_energy_value_not_printed_cost() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::IvoryTile]);
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.cards[k as usize].set_energy_cost(3, crate::card::CostExpiry::Combat, false);
        c.players[owner].hand.push(k); c.players[owner].energy = 3;
        c.play_card(owner, 0, None);
        assert_eq!(c.players[owner].energy, 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
