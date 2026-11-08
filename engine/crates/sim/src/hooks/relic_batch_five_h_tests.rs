//! Models/Relics/{LostWisp,JeweledMask,LeafyPoultice}.cs.
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
fn h5_wisp_only_owner_power_in_progress_damages_enemies_without_strength_or_rng() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::LostWisp);
        let other = (owner + 1) % seats as CId;
        let foreign = c.new_card(other, CardId::Inflame, false).unwrap();
        let skill = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let power = c.new_card(owner, CardId::Inflame, false).unwrap();
        c.apply_power_to(owner, PowerId::Strength, 20, Some(owner));
        let rng = c.run.rngs.combat_targets.counter;
        for k in [foreign, skill] { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(power));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 192); }
        c.run.relics[0].enabled = false;
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(power));
        c.run.relics[0].enabled = true;
        c.is_over = true;
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(power));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 192); }
        for seat in 0..seats as CId { assert_eq!(c.cr(seat).hp, 50); }
        assert_eq!(c.run.rngs.combat_targets.counter, rng);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }}
}

#[test]
fn h5_mask_prefers_noninnate_owner_power_on_owner_turn_one_not_global_round() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::JeweledMask);
        let other = (owner + 1) % seats as CId;
        let innate = c.new_card(owner, CardId::Inflame, false).unwrap();
        c.cards[innate as usize].keywords |= KW_INNATE;
        let selected = c.new_card(owner, CardId::DemonFormCard, false).unwrap();
        let attack = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].draw.extend([innate, attack, selected]);
        c.players[owner].turn_number = 1; c.round = 7;
        let rng = c.run.rngs.combat_card_selection.counter;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(other));
        assert_eq!(c.run.rngs.combat_card_selection.counter, rng);
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].hand.as_slice(), &[selected]);
        assert_eq!(c.players[owner].draw.as_slice(), &[innate, attack]);
        assert!(c.players[other].hand.is_empty());
        assert_eq!(c.run.rngs.combat_card_selection.counter, rng + 1);
        assert_eq!(c.cards[selected as usize].cv.turn_star_cost_override, Some(0));
        c.players[owner].turn_number = 2; c.round = 1;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].draw.as_slice(), &[innate, attack]);
        assert_eq!(c.run.rngs.combat_card_selection.counter, rng + 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }}
}

#[test]
fn h5_mask_falls_back_to_innate_powers_and_uses_normal_full_hand_destination() {
    for owner in 0..4 {
        let mut c = setup(4, owner, RelicId::JeweledMask);
        let first = c.new_card(owner, CardId::Inflame, false).unwrap();
        let second = c.new_card(owner, CardId::DemonFormCard, false).unwrap();
        for k in [first, second] { c.cards[k as usize].keywords |= KW_INNATE; }
        c.players[owner].draw.extend([first, second]);
        for _ in 0..MAX_HAND_SIZE {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].hand.push(k);
        }
        let mut rng = c.run.rngs.combat_card_selection.clone();
        let expected = [first, second][rng.choice_index(2)];
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.players[owner].hand.len(), MAX_HAND_SIZE);
        assert_eq!(c.players[owner].discard.as_slice(), &[expected]);
        assert_eq!(c.run.rngs.combat_card_selection.counter, rng.counter);
        assert_eq!(c.cards[expected as usize].cv.turn_star_cost_override, Some(0));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h5_leafy_poultice_preserves_imported_owner_hp_deck_and_transformation_rng() {
    for owner in 0..4 {
        let mut c = setup(4, owner, RelicId::LeafyPoultice);
        assert!(crate::relic::is_ported(RelicId::LeafyPoultice));
        c.cr_mut(owner).max_hp = 38; c.cr_mut(owner).hp = 25;
        let before = (c.run.deck.len(), c.run.next_card_id, c.run.rngs.transformations.counter);
        for hook in [Hook::BeforeCombatStart, Hook::BeforeCombatStartLate,
            Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
            c.fire_relics(hook, Ev::None);
        }
        assert_eq!((c.cr(owner).max_hp, c.cr(owner).hp), (38, 25));
        assert_eq!(before, (c.run.deck.len(), c.run.next_card_id, c.run.rngs.transformations.counter));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
