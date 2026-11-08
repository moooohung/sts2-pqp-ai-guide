use crate::identity::EventId;
use super::{Character, MpPhase, MpRun, PlayerSpec};
use crate::tables::{CardId, CardType};

fn party(id: crate::identity::EventId) -> MpRun {
    let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
        PlayerSpec { net_id: 42, character: Character::Silent }];
    let mut run = MpRun::new("73003", &specs, 0).unwrap();
    run.world.run.act_index = 1;
    run.phase = MpPhase::Event;
    run.world.current_room=Some(id.name());
    run.event_done.fill(false);
    run.begin_generic_event(id).unwrap();
    run
}

#[test]
fn act2_owned_holes_remove_and_enchant_only_the_actor() {
    let mut run = party(EventId::FieldOfManSizedHoles);
    assert!(run.choose_event_option(7, 1, 0).is_err());
    let before = run.roster.player(1).unwrap().deck.len();
    run.choose_event_option(42, 1, 0).unwrap();
    assert!(run.choose_event_option(42, 1, 0).is_err());
    assert_eq!(run.event_card_modal(42, 1).unwrap().1, 2);
    run.select_event_cards(42, 1, &[0, 1]).unwrap();
    assert_eq!(run.roster.player(1).unwrap().deck.len(), before - 1);
    assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, CardId::Normality);
    assert_eq!(run.phase, MpPhase::Event);
    run.choose_event_option(7, 0, 1).unwrap();
    let iid = run.event_card_modal(7, 0).unwrap().0[0].instance_id;
    run.select_event_cards(7, 0, &[0]).unwrap();
    assert!(run.roster.player(0).unwrap().deck.iter().find(|c| c.instance_id == iid)
        .unwrap().enchantments.contains(&(crate::identity::EnchantmentId::PerfectFit, 1)));
    assert_eq!(run.phase, MpPhase::Map);
}

#[test]
fn act2_owned_amalgamator_combines_two_basics_without_affecting_other_seats() {
    for (option, target) in [(0, CardId::UltimateStrike), (1, CardId::UltimateDefend)] {
        let mut run = party(EventId::Amalgamator);
        let before = run.roster.player(1).unwrap().deck.len();
        let other: Vec<_> = run.roster.player(0).unwrap().deck.iter().map(|c| c.instance_id).collect();
        run.choose_event_option(42, 1, option).unwrap();
        assert!(run.select_event_cards(42, 1, &[0]).is_err());
        run.select_event_cards(42, 1, &[0, 1]).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.len(), before - 1);
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, target);
        assert_eq!(run.roster.player(0).unwrap().deck.iter().map(|c| c.instance_id).collect::<Vec<_>>(), other);
    }
}

#[test]
fn act2_owned_automaton_uses_owner_rewards_not_event_rng() {
    for option in 0..2 {
        let mut run = party(EventId::InfestedAutomaton);
        let before = run.roster.player(1).unwrap().deck.len();
        let owner_rng = run.roster.player(1).unwrap().rngs.rewards.counter;
        let other_rng = run.roster.player(0).unwrap().rngs.rewards.counter;
        run.choose_event_option(42, 1, option).unwrap();
        let player = run.roster.player(1).unwrap();
        assert_eq!(player.deck.len(), before + 1);
        let card = player.deck.last().unwrap();
        if option == 0 { assert_eq!(card.card_type, CardType::Power); }
        else { assert_eq!(card.cost, 0); }
        assert_eq!(card.owner, 1);
        assert_eq!(player.rngs.rewards.counter, owner_rng + 2);
        assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter, other_rng);
        assert_eq!(run.phase, MpPhase::Event);
    }
    let mut run = party(EventId::InfestedAutomaton);
    for id in [crate::tables::RelicId::WingCharm, crate::tables::RelicId::FrozenEgg] {
        let mut relic = crate::relic::Relic::new(id);
        relic.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(relic);
    }
    run.choose_event_option(42, 1, 0).unwrap();
    let card = run.roster.player(1).unwrap().deck.last().unwrap();
    assert_eq!(card.owner, 1);
    assert!(card.upgraded);
    assert!(card.enchantments.contains(&(crate::identity::EnchantmentId::Swift, 1)));
}
