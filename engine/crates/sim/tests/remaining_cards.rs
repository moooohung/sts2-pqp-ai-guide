use sts2_sim::{
    combat::{Combat, PileKind, PilePos},
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].draw.clear();
    c.players[0].hand.clear();
    c.add_player(50, 50, 3, 0, &[]);
    c
}

#[test]
fn midnight_counts_all_exhaustions_before_entry_and_ignores_entry_for_clones() {
    let mut c = combat();
    let exhausted = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    c.players[1].exhaust.push(exhausted);
    c.fire_after_card_exhausted(exhausted);
    let midnight = c.new_card(0, CardId::Midnight, false).unwrap();
    c.add_generated_card(midnight, PileKind::Hand, PilePos::Bottom, true);
    assert_eq!(c.modified_card_cost(0, midnight), 11);
    c.fire_after_card_exhausted(exhausted);
    assert_eq!(c.modified_card_cost(0, midnight), 10);
    let mut cloned = c.cards[midnight as usize].clone_with_id(999);
    cloned.cv.is_clone = true;
    let clone = c.cards.len() as u16;
    c.cards.push(cloned);
    c.add_generated_card(clone, PileKind::Hand, PilePos::Bottom, true);
    assert_eq!(c.modified_card_cost(0, clone), 10);
    c.fire_after_card_exhausted(exhausted);
    assert_eq!(c.modified_card_cost(0, clone), 9);
    assert_eq!(c.modified_card_cost(0, midnight), 9);
    assert_eq!(c.fault, None);
}

#[test]
fn stack_counts_only_owners_discard_and_caltrops_uses_canonical_upgrade() {
    for upgraded in [false, true] {
        let mut c = combat();
        for owner in [0, 1, 1] {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(k);
        }
        c.apply_power_to(1, PowerId::Dexterity, 2, Some(1));
        let stack = c.new_card(1, CardId::Stack, upgraded).unwrap();
        c.play_card_effect(stack, None);
        assert_eq!(c.cr(1).block, 2 + 2 + if upgraded { 3 } else { 0 });
        assert_eq!(c.cr(0).block, 0);
        let caltrops = c.new_card(1, CardId::Caltrops, upgraded).unwrap();
        c.play_card_effect(caltrops, None);
        assert_eq!(c.power_amount(1, PowerId::Thorns), if upgraded { 5 } else { 3 });
        assert_eq!(c.power_amount(0, PowerId::Thorns), 0);
        assert_eq!(c.fault, None);
    }
}
