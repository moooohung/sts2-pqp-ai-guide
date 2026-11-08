//! C# Tutor.OnPlay: target player chooses one card from their own draw pile.
use sts2_sim::combat::Combat;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::CardId;

fn combat() -> Combat {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.add_player(80, 80, 3, 0, &[]);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    c
}

#[test]
fn tutor_targets_other_player_and_resumes_choice_into_their_hand() {
    let mut c = combat();
    let own = c.new_card(0, CardId::Bash, false).unwrap();
    c.players[0].draw.push(own);
    let a = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
    let b = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    c.players[1].draw.extend([a, b]);
    let tutor = c.new_card(0, CardId::Tutor, false).unwrap();
    c.play_card_effect(tutor, Some(1));
    assert!(c.pending_choice.is_some());
    assert_eq!(c.choice_options.len(), 2);
    assert!(c.choice_options.contains(&a) && c.choice_options.contains(&b));
    let selected = c.choice_options[0];
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[1].hand.as_slice(), &[selected]);
    assert_eq!(c.players[1].draw.len(), 1);
    assert_eq!(c.players[0].draw.as_slice(), &[own]);
    assert!(c.players[0].hand.is_empty());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn tutor_self_target_and_empty_pile_and_missing_target() {
    for player in [0, 1] {
        let mut c = combat();
        let tutor = c.new_card(0, CardId::Tutor, true).unwrap();
        c.play_card_effect(tutor, Some(player));
        assert!(c.pending_choice.is_none());
        let k = c.new_card(player, CardId::StrikeIronclad, false).unwrap();
        c.players[player].draw.push(k);
        c.play_card_effect(tutor, Some(player));
        assert_eq!(c.players[player].hand.as_slice(), &[k]);
        assert!(c.pending_choice.is_none());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
    let mut c = combat();
    let tutor = c.new_card(0, CardId::Tutor, false).unwrap();
    c.play_card_effect(tutor, None);
    assert_eq!(c.fault, Some("Tutor requires an ally target"));
}
