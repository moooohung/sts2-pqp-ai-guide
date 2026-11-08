//! Original WellLaidPlans.OnPlay and WellLaidPlansPower.ShouldFlush/StackType.
use super::*;
use crate::rng::Rng;
use crate::run::{RunManager, RunOptions};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 0..3 { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for seat in 0..4 {
        c.players[seat].hand.clear();
        c.players[seat].draw.clear();
        c.players[seat].discard.clear();
    }
    c
}

#[test]
fn normal_card_play_applies_one_and_upgrade_changes_only_cost() {
    assert_eq!(power_meta(PowerId::WellLaidPlans).stack_type, PowerStackType::Single);
    for owner in 0..4 {
        for upgraded in [false, true] {
            let mut c = combat();
            let card = c.new_card(owner, CardId::WellLaidPlans, upgraded).unwrap();
            c.players[owner].hand.push(card);
            c.players[owner].energy = 3;
            assert!(c.play_card(owner, 0, None), "owner {owner}: {:?}", c.fault);
            assert_eq!(c.power_amount(owner, PowerId::WellLaidPlans), 1);
            assert_eq!(c.players[owner].energy, if upgraded { 2 } else { 1 });
            for seat in 0..4 {
                assert_eq!(c.should_flush(seat), seat != owner);
            }
            assert!(c.pending_choice.is_none());
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn whole_hand_is_kept_in_original_order_without_a_retain_selector() {
    for owner in 0..4 {
        let mut c = combat();
        c.apply_power_to(owner, PowerId::WellLaidPlans, 1, Some(owner)).unwrap();
        let mut hands = Vec::new();
        for seat in 0..4 {
            let a = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
            let b = c.new_card(seat, CardId::DefendIronclad, false).unwrap();
            let d = c.new_card(seat, CardId::Bash, false).unwrap();
            // An interleaved intrinsic Retain must not move in front of a.
            c.cards[b as usize].keywords |= KW_RETAIN;
            c.players[seat].hand.extend([a, b, d]);
            hands.push(vec![a, b, d]);
        }
        for seat in 0..4 {
            c.flush_hand(seat);
            if seat == owner {
                assert_eq!(c.players[seat].hand.as_slice(), hands[seat as usize]);
                assert!(c.players[seat].discard.is_empty());
            } else {
                assert_eq!(c.players[seat].hand.as_slice(), &[hands[seat as usize][1]]);
                assert_eq!(c.players[seat].discard.as_slice(), &[hands[seat as usize][0], hands[seat as usize][2]]);
            }
            assert!(c.pending_choice.is_none());
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn reapplication_uses_one_instance_and_inactive_owner_does_not_veto_flush() {
    for owner in 0..4 {
        let mut c = combat();
        let first = c.apply_power_to(owner, PowerId::WellLaidPlans, 1, Some(owner)).unwrap();
        let second = c.apply_power_to(owner, PowerId::WellLaidPlans, 1, Some(owner)).unwrap();
        assert_eq!(first, second);
        assert!(!c.should_flush(owner));
        c.players[owner].hooks_active = false;
        assert!(c.should_flush(owner));
        for seat in 0..4 {
            if seat != owner { assert!(c.should_flush(seat)); }
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn ethereal_is_still_exhausted_before_the_owner_skips_flush() {
    for owner in 0..4 {
        let mut c = combat();
        c.apply_power_to(owner, PowerId::WellLaidPlans, 1, Some(owner)).unwrap();
        let keep = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let ethereal = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.cards[ethereal as usize].keywords |= KW_ETHEREAL;
        c.players[owner].hand.extend([keep, ethereal]);
        assert!(c.resolve_end_of_turn_hand(owner));
        c.flush_hand(owner);
        assert_eq!(c.players[owner].hand.as_slice(), &[keep]);
        assert_eq!(c.players[owner].exhaust.as_slice(), &[ethereal]);
        assert!(c.pending_choice.is_none());
        assert!(c.fault.is_none());
    }
}

#[test]
fn existing_whole_hand_flush_skips_also_preserve_interleaved_retain_order() {
    for relic in [None, Some(RelicId::RunicPyramid), Some(RelicId::RingingTriangle)] {
        let mut c = combat();
        c.round = 1;
        c.players[0].turn_number = 1;
        if let Some(id) = relic {
            c.run.relics.push(crate::relic::Relic::new(id));
        } else {
            c.apply_power_to(0, PowerId::RetainHand, 1, Some(0)).unwrap();
        }
        let a = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        let b = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        let d = c.new_card(0, CardId::Bash, false).unwrap();
        c.cards[b as usize].keywords |= KW_RETAIN;
        c.players[0].hand.extend([a, b, d]);
        c.flush_hand(0);
        assert_eq!(c.players[0].hand.as_slice(), &[a, b, d]);
        assert!(c.players[0].discard.is_empty());
        assert!(c.fault.is_none());
    }
}
