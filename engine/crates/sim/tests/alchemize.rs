use sts2_sim::character::Character;
use sts2_sim::{combat::Combat, run::{RunManager, RunOptions}, tables::{CardId, PotionId, POTION_INFO}};

#[test]
fn alchemize_all_characters_and_seats_respect_slots_and_combat_eligibility() {
    for character in Character::ALL {
        for owner in 0..4 {
            let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.players[owner].character = Some(character);
            c.players[owner].potions = Some(vec![None; 3]);
            c.run.potions = vec![None; 3];
            c.run.max_potion_slots = 3;
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut sts2_sim::rng::Rng::new(1));
            let card = c.new_card(owner, CardId::Alchemize, false).unwrap();
            for _ in 0..4 { c.play_card_effect(card, None); }
            let slots = if owner == 0 { &c.run.potions } else { c.players[owner].potions.as_ref().unwrap() };
            assert_eq!(slots.iter().flatten().count(), 3);
            assert!(slots.iter().flatten().all(|id| POTION_INFO[id.index()].gen_combat));
            assert_eq!(c.run.rngs.combat_potion_generation.counter, 8);
            if owner != 0 { assert!(c.run.potions.iter().all(Option::is_none)); }
            assert_eq!(c.fault, None);
        }
    }
}

#[test]
fn alchemize_card_unlock_option_does_not_disable_owner_potion_generation() {
    for unlocked in [None, Some(false)] {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.run.all_cards_unlocked = unlocked;
        c.players[0].character = Some(Character::Silent);
        c.run.potions = vec![Some(PotionId::Firepotion), None, None];
        let k = c.new_card(0, CardId::Alchemize, false).unwrap();
        c.play_card_effect(k, None);
        assert_eq!(c.run.potions[0], Some(PotionId::Firepotion));
        assert!(c.run.potions[1].is_some());
        assert!(c.run.potions[2].is_none());
        assert_eq!(c.run.rngs.combat_potion_generation.counter, 2);
        assert_eq!(c.fault, None);
    }
}
