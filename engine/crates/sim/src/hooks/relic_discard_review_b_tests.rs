use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId) -> Combat {
    let mut c = combat();
    for id in [RelicId::Tingsha, RelicId::ToughBandages] {
        let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    }
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    c
}

#[test]
fn gambling_chip_normal_choice_fires_discard_history_and_relics_on_every_seat() {
    for owner in 0..4 {
        let mut c = setup(owner);
        let mut r = Relic::new(RelicId::GamblingChip); r.owner = owner; c.run.relics.push(r);
        for _ in 0..2 {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].hand.push(k);
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].draw.push(k);
        }
        c.round = 1; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert!(c.pending_choice.is_some());
        assert!(c.resolve_pending_choice(Some(0)));
        assert!(c.resolve_pending_choice(Some(1)));
        assert!(c.resolve_pending_choice(None));
        assert_eq!(c.hist.discarded_this_turn, 2);
        assert_eq!(c.cr(owner).block, 6);
        assert_eq!(c.enemies.iter().map(|&e| c.cr(e).hp).sum::<i32>(), c.enemies.len() as i32 * 200 - 6);
        assert_eq!(c.players[owner].hand.len(), 2);
        assert!(c.fault.is_none());
    }
}

#[test]
fn scrape_normal_card_play_dispatches_all_four_cardcmd_discards() {
    for owner in 0..4 {
        let mut c = setup(owner);
        for _ in 0..4 {
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].draw.push(k);
        }
        let k = c.new_card(owner, CardId::Scrape, false).unwrap();
        c.players[owner].hand.push(k);
        c.execute_card_play(k, Some(c.enemies[0]), false, false);
        assert_eq!(c.hist.discarded_this_turn, 4);
        assert_eq!(c.cr(owner).block, 12);
        assert_eq!(c.enemies.iter().map(|&e| c.cr(e).hp).sum::<i32>(), c.enemies.len() as i32 * 200 - 19);
        assert!(c.fault.is_none());
    }
}

#[test]
fn untargeted_preview_has_no_sigil_recipient_even_for_player_zero() {
    for owner in 0..4 {
        let mut c = combat();
        let other = (owner + 1) % 4;
        let mut r = Relic::new(RelicId::UndyingSigil); r.owner = owner; c.run.relics.push(r);
        c.cr_mut(other).hp = 20;
        c.apply_power_to(other, PowerId::Doom, 20, Some(owner));
        let k = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        assert_eq!(c.modify_untargeted_card_damage(20, other, k), 20);
        c.active_card = Some(k);
        assert_eq!(c.calculate_damage(20, Some(other), owner, Props::MOVE), 10);
        assert!(c.fault.is_none());
    }
}

#[test]
fn untargeted_weak_preview_has_no_paper_krane_recipient_in_any_seat() {
    for owner in 0..4 {
        let mut c = combat();
        let other = (owner + 1) % 4;
        let mut r = Relic::new(RelicId::PaperKrane); r.owner = owner; c.run.relics.push(r);
        c.apply_power_to(other, PowerId::Weak, 2, Some(owner));
        let k = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        // WeakPower.cs:35 target?.Player is null in an untargeted preview.
        assert_eq!(c.modify_untargeted_card_damage(100, other, k), 75);
        c.active_card = Some(k);
        assert_eq!(c.calculate_damage(100, Some(other), owner, Props::MOVE), 60);
        c.apply_power_to(other, PowerId::Debilitate, 1, Some(owner));
        assert_eq!(c.modify_untargeted_card_damage(100, other, k), 50);
        assert_eq!(c.calculate_damage(100, Some(other), owner, Props::MOVE), 20);
        assert!(c.fault.is_none());
    }
}

#[test]
fn scrape_discards_entire_batch_before_a_sly_memento_mori_autoplays() {
    for owner in 0..4 {
        let mut c = setup(owner);
        // MementoMori.cs CanonicalVars: 9 + 4 * all four discards.
        let k = c.new_card(owner, CardId::MementoMori, false).unwrap();
        c.cards[k as usize].cv.sly_this_turn = true;
        c.players[owner].draw.push(k);
        for _ in 0..3 {
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].draw.push(k);
        }
        let k = c.new_card(owner, CardId::Scrape, false).unwrap();
        c.players[owner].hand.push(k);
        let target = c.enemies[0];
        c.execute_card_play(k, Some(target), false, false);
        assert_eq!(c.hist.discarded_this_turn, 4);
        // Scrape 7 + four Tingsha hits 12 + Memento Mori 25.
        assert_eq!(c.enemies.iter().map(|&e| c.cr(e).hp).sum::<i32>(),
            c.enemies.len() as i32 * 200 - 44);
        assert!(c.fault.is_none());
    }
}
