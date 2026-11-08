//! Models/Relics/{Kifuda,LeadPaperweight,MoltenEgg,LeesWaffle}.cs.
//! These four models act before the combat-entry snapshot, never during combat.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn check_run_only_relic(id: RelicId) {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        let mut relic = Relic::new(id);
        relic.owner = owner;
        c.run.relics.push(relic);
        assert!(crate::relic::is_ported(id));
        assert!(c.relic_owner_has_enabled(owner, id));
        assert!(!c.relic_owner_has_enabled((owner + 1) % 4, id));
        for seat in 0..4 {
            let k = c.new_card(seat, CardId::StrikeIronclad, seat == owner).unwrap();
            // Kifuda's acquisition result is already present in the snapshot.
            // Its enchantment must survive without enchanting any new cards.
            if seat == owner && id == RelicId::Kifuda {
                c.cards[k as usize].enchantments.push((crate::identity::EnchantmentId::Adroit, 3));
            }
            let card = c.cards[k as usize].clone();
            c.relic_owner_deck_mut(seat).push(card);
            c.players[seat].draw.push(k);
            c.cr_mut(seat).hp = 30 + seat as i32;
        }
        let before: Vec<_> = (0..4).map(|seat| (
            c.cr(seat).hp, c.cr(seat).max_hp,
            c.relic_owner_deck(seat).iter().map(|card|
                (card.id, card.upgraded, card.enchantments.clone())).collect::<Vec<_>>(),
        )).collect();
        let rng = (c.run.rngs.rewards.counter, c.run.rngs.combat_card_selection.counter,
            c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            c.players[seat].turn_number = 2 + seat as i32;
            c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(seat));
            c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(seat));
            let k = c.players[seat].draw[0];
            c.fire_relics(Hook::AfterCardEnteredCombat, Ev::Card(k));
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        }
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        c.fire_relics(Hook::AfterCombatVictory, Ev::None);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        for seat in 0..4 {
            let after = (c.cr(seat).hp, c.cr(seat).max_hp,
                c.relic_owner_deck(seat).iter().map(|card|
                    (card.id, card.upgraded, card.enchantments.clone())).collect::<Vec<_>>());
            assert_eq!(after, before[seat as usize], "{id:?}, owner {owner}, seat {seat}");
            let card = &c.cards[c.players[seat].draw[0] as usize];
            assert_eq!(card.upgraded, seat == owner,
                "MoltenEgg must not upgrade newly entered combat cards");
        }
        assert_eq!(rng, (c.run.rngs.rewards.counter, c.run.rngs.combat_card_selection.counter,
            c.run.rngs.combat_card_generation.counter, c.run.rngs.shuffle.counter));
        assert!(c.fault.is_none(), "{id:?}, owner {owner}: {:?}", c.fault);
    }
}

#[test]
fn h2_kifuda_preserves_owner_enchantments_without_repeating_acquisition() {
    check_run_only_relic(RelicId::Kifuda);
}

#[test]
fn h2_lead_paperweight_never_regenerates_rewards_in_combat() {
    check_run_only_relic(RelicId::LeadPaperweight);
}

#[test]
fn h2_molten_egg_never_upgrades_combat_cards_or_teammate_decks() {
    check_run_only_relic(RelicId::MoltenEgg);
}

#[test]
fn h2_lees_waffle_never_repeats_owner_max_hp_gain_or_heal() {
    check_run_only_relic(RelicId::LeesWaffle);
}
