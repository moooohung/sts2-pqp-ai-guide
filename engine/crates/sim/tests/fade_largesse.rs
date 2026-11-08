//! C# Fade.OnPlay/TemporaryDexterityPower and Largesse.OnPlay generation contracts.
use sts2_sim::combat::{Combat, Props};
use sts2_sim::creature::Side;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId, CARD_MP};

fn combat(seats: usize) -> Combat {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(80, 80, 3, 0, &[]); }
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    c
}

#[test]
fn fade_buffs_each_target_stacks_and_expires_without_touching_other_dexterity() {
    for seats in 2..=4 {
        for target in 0..seats as u8 {
            let mut c = combat(seats);
            c.apply_power_to(target, PowerId::Dexterity, 2, Some(target));
            for up in [false, true] {
                let k = c.new_card(0, CardId::Fade, up).unwrap();
                c.play_card_effect(k, Some(target));
            }
            assert_eq!(c.power_amount(target, PowerId::Fade), 15);
            assert_eq!(c.power_amount(target, PowerId::Dexterity), 17);
            assert_eq!(c.calculate_block(4, target, Props::MOVE, None), 21);
            c.fire_after_turn_end(Side::Enemy);
            assert_eq!(c.power_amount(target, PowerId::Fade), 15);
            c.fire_after_turn_end(Side::Player);
            assert!(!c.has_power(target, PowerId::Fade));
            assert_eq!(c.power_amount(target, PowerId::Dexterity), 2);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn largesse_uses_multiplayer_colorless_order_and_target_ownership() {
    let pool = sts2_sim::cardgen::character_pool(sts2_sim::character::CardPool::Colorless, true, None, None).unwrap();
    // C# ColorlessCardPool: 65 canonical entries, three forbid combat generation.
    assert_eq!(pool.len(), 62);
    assert!(pool.contains(&CardId::CoordinateCard));
    assert!(!pool.contains(&CardId::Alchemize));
    assert!(!pool.contains(&CardId::HandOfGreed));
    assert!(!pool.contains(&CardId::HiddenGem));
    assert!(pool.iter().all(|id| CARD_MP[id.index()] != 2));
    for target in [0, 1] {
        let mut c = combat(2);
        let before = c.run.rngs.combat_card_generation.counter;
        let k = c.new_card(0, CardId::Largesse, true).unwrap();
        c.play_card_effect(k, Some(target));
        assert_eq!(c.players[target].hand.len(), 1);
        assert!(c.players[1 - target].hand.is_empty());
        let generated = c.players[target].hand[0];
        assert_eq!(c.cards[generated as usize].owner, target);
        // C# TakeRandom shuffles the filtered 62-card list (61 RNG calls).
        assert_eq!(c.run.rngs.combat_card_generation.counter - before, 61);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn jack_of_all_trades_filters_itself_from_multiplayer_pool_before_shuffling() {
    let mut c = combat(2);
    let before = c.run.rngs.combat_card_generation.counter;
    let k = c.new_card(1, CardId::JackOfAllTrades, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.players[1].hand.len(), 2);
    assert!(c.players[0].hand.is_empty());
    assert!(c.players[1].hand.iter().all(|&q| c.cards[q as usize].id != CardId::JackOfAllTrades));
    assert_ne!(c.cards[c.players[1].hand[0] as usize].id, c.cards[c.players[1].hand[1] as usize].id);
    assert_eq!(c.run.rngs.combat_card_generation.counter - before, 60);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}
