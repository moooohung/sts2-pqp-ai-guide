use sts2_sim::{
    choice::{ChoiceSrc, Resolver}, combat::Combat,
    run::{RunManager, RunOptions}, tables::{CardId, PowerId},
};

#[test]
fn charge_transforms_draw_selection_into_dive_bomb_then_upgrades_if_requested() {
    for upgraded in [false, true] {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.players[0].draw.clear();
        c.players[0].hand.clear();
        let old = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].draw.push(old);
        let charge = c.new_card(0, CardId::Charge, upgraded).unwrap();
        c.apply_power_to(0, PowerId::Arsenal, 2, Some(0));
        c.resolve_choice_effect(Resolver {
            src: ChoiceSrc::Card(CardId::Charge), k: charge, target: None, step: 0, arg: 0,
        }, &[old]);
        let replacement = c.players[0].draw[0];
        assert_ne!(old, replacement);
        assert_eq!(c.cards[replacement as usize].id, CardId::MinionDiveBomb);
        assert_eq!(c.cards[replacement as usize].upgraded, upgraded);
        assert_eq!(c.power_amount(0, PowerId::Strength), 2);
        assert_eq!(c.hist.generated_combat, 1);
        assert_eq!(c.fault, None);
    }
}
