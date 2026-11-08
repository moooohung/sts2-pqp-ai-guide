//! Models/Relics/{MembershipCard,MrStruggles,LargeCapsule,LordsParasol,
//! MeatCleaver,Mango,LuckyFysh,MassiveScroll}.cs.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(seats: usize, owner: CId, id: RelicId) -> Combat {
    let mut c = Combat::new(combat().run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c
}

#[test]
fn h4_struggles_uses_starting_owners_turn_number_and_unpowered_damage() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::MrStruggles);
        let other = (owner + 1) % seats as CId;
        c.round = 99;
        c.players[owner].turn_number = 3;
        c.players[other].turn_number = 7;
        c.apply_power_to(owner, PowerId::Strength, 20, Some(owner));
        let rng = c.run.rngs.combat_targets.counter;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(other));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 197); }
        c.round = 1;
        c.players[owner].turn_number = 4;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 193); }
        assert_eq!(c.run.rngs.combat_targets.counter, rng);
        for seat in 0..seats as CId { assert_eq!(c.cr(seat).hp, 50); }
        c.run.relics[0].enabled = false;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        c.run.relics[0].enabled = true;
        c.players[owner].hooks_active = false;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 193); }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }}
}

#[test]
fn h4_run_only_relics_do_not_reapply_pickups_or_interpret_combat_piles_as_deck() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        for id in [RelicId::MembershipCard, RelicId::LargeCapsule, RelicId::LordsParasol,
            RelicId::MeatCleaver, RelicId::Mango, RelicId::LuckyFysh, RelicId::MassiveScroll] {
            let mut c = setup(seats, owner, id);
            assert!(crate::relic::is_ported(id));
            assert!(c.relic_owner_has_enabled(owner, id));
            c.run.gold = 99;
            for seat in 0..seats as CId {
                c.players[seat].gold = Some(99);
                c.cr_mut(seat).hp = 30 + seat as i32;
            }
            let before = (c.run.rngs.rewards.counter, c.run.rngs.niche.counter,
                c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter,
                c.run.next_card_id, c.run.deck.len());
            for hook in [Hook::BeforeCombatStart, Hook::BeforeCombatStartLate,
                Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            for seat in 0..seats as CId {
                c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(seat));
                c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(seat));
                assert_eq!((c.cr(seat).hp, c.cr(seat).max_hp), (30 + seat as i32, 50));
                assert_eq!(c.relic_owner_gold(seat), Some(99));
                assert!(c.players[seat].hand.is_empty());
            }
            assert_eq!(before, (c.run.rngs.rewards.counter, c.run.rngs.niche.counter,
                c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter,
                c.run.next_card_id, c.run.deck.len()));
            // LuckyFysh.AfterCardChangedPiles requires PileType.Deck, not any
            // combat hand/draw/discard transition for either owner's cards.
            for seat in 0..seats as CId {
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                c.players[seat].hand.push(k);
                c.fire_after_card_changed_piles(k, PileKind::None, None);
                assert_eq!(c.relic_owner_gold(seat), Some(99));
            }
            assert!(c.fault.is_none(), "{id:?}, owner {owner}: {:?}", c.fault);
        }
    }}
}
