use crate::identity::EventId;
use super::*;

#[test]
fn owned_events_mad_science_deck_upgrade_preserves_customization() {
    let mut run = event_run(EventId::TinkerTime, 73000);
    let mut event = run.current_event.take().unwrap();
    event.ints[1] = 1;
    event.ints[2] = 6;
    let card = super::super::events::tinker_time::card(&event, 900).unwrap();
    run.run_ctx_mut().deck.push(card.clone());
    let index = run.run_ctx().deck.len() - 1;
    run.run_ctx_mut().upgrade_deck_card(index);
    let upgraded = &run.run_ctx().deck[index];
    assert_eq!(upgraded.card_type, CardType::Skill);
    assert_eq!(upgraded.vars, card.vars);
    assert_eq!(upgraded.target, card.target);
    assert!(upgraded.upgraded && upgraded.has_kw(KW_INNATE));
}

fn event_run(id: crate::identity::EventId, seed: u64) -> RunManager {
    let mut run = RunManager::new(4100000, RunOptions::default());
    run.current_event = Some(super::super::events::initialize(id, seed, 100, 20, 30, 80)
        .expect("owned event must be enterable"));
    run.phase = RunPhase::Event;
    run.unported = None;
    run.run_ctx_mut().hp = 30;
    run.run_ctx_mut().gold = 100;
    run
}

#[test]
fn owned_events_tinker_time_retains_selected_type_rider_and_owner_egg_upgrade() {
    let mut seen = std::collections::HashSet::new();
    for seed in 0..24 {
        let mut run = event_run(EventId::TinkerTime, seed);
        let next_id = run.run_ctx().next_card_id;
        run.take_event_choice("choose_card_type").unwrap();
        assert_eq!(run.current_event.as_ref().unwrap().options.len(), 2);
        assert_eq!(run.run_ctx().next_card_id, next_id + 3);
        for option in run.current_event.as_ref().unwrap().options.to_vec() {
            let mut typed = run.clone();
            typed.take_event_choice(option.option_id.legacy_id()).unwrap();
            let (kind, target, egg) = match option.option_id.legacy_id() {
                "attack" => (CardType::Attack, TargetType::AnyEnemy, RelicId::MoltenEgg),
                "skill" => (CardType::Skill, TargetType::Self_, RelicId::ToxicEgg),
                "power" => (CardType::Power, TargetType::Self_, RelicId::FrozenEgg),
                _ => panic!("unexpected card type"),
            };
            assert_eq!(typed.run_ctx().next_card_id, next_id + 5);
            for rider in typed.current_event.as_ref().unwrap().options.to_vec() {
                let expected = match rider.option_id.legacy_id() {
                    "sapping" => 1, "violence" => 2, "choking" => 3,
                    "energized" => 4, "wisdom" => 5, "chaos" => 6,
                    "expertise" => 7, "curious" => 8, "improvement" => 9,
                    _ => panic!("unexpected rider"),
                };
                seen.insert(expected);
                let mut final_run = typed.clone();
                final_run.run_ctx_mut().relics.push(Relic::new(egg));
                let before = final_run.run_ctx().deck.len();
                final_run.take_event_choice(rider.option_id.legacy_id()).unwrap();
                assert_eq!(final_run.phase, RunPhase::MapChoice);
                assert_eq!(final_run.run_ctx().deck.len(), before + 1);
                let card = final_run.run_ctx().deck.last().unwrap();
                assert_eq!((card.id, card.card_type, card.target), (CardId::MadScience, kind, target));
                assert_eq!(card.var(VarKey::Rider), Some(expected));
                assert!(card.upgraded && card.has_kw(KW_INNATE));
                assert_eq!(card.var(VarKey::Damage), Some(12));
                assert_eq!(card.var(VarKey::Block), Some(8));
            }
        }
    }
    assert_eq!(seen.len(), 9);
}

#[test]
fn owned_events_trial_judgments_apply_curse_before_owned_rewards_and_event_rng_transforms() {
    let mut seen = std::collections::HashSet::new();
    for seed in 0..24 {
        let mut initial = event_run(EventId::Trial, seed);
        initial.take_event_choice("reject").unwrap();
        initial.take_event_choice("double_down").unwrap();
        assert_eq!(initial.phase, RunPhase::Event);
        initial.take_event_choice("cancel_abandon").unwrap();
        initial.take_event_choice("accept").unwrap();
        let page = initial.current_event.as_ref().unwrap().ints[0];
        for verdict in ["guilty", "innocent"] {
            let mut run = initial.clone();
            let deck = run.run_ctx().deck.len();
            let rewards_rng = run.run_ctx().rngs.rewards.counter;
            seen.insert((page, verdict));
            run.take_event_choice(verdict).unwrap();
            match (page, verdict) {
                (2, "guilty") => {
                    assert_eq!(run.run_ctx().deck[deck].id, CardId::Regret);
                    assert_eq!(run.run_ctx().relics.len(), initial.run_ctx().relics.len() + 2);
                }
                (2, "innocent") => {
                    assert_eq!(run.run_ctx().deck[deck].id, CardId::Shame);
                    let choice = run.pending_run_choice.take().unwrap();
                    assert_eq!((choice.min_choices, choice.max_choices), (2, 2));
                    run.apply_run_choice(choice, &[1, 0]).unwrap();
                    assert_eq!(run.run_ctx().deck.iter().filter(|c| c.upgraded).count(), 2);
                }
                (3, "guilty") => {
                    assert_eq!(run.run_ctx().hp, 40);
                    assert_eq!(run.run_ctx().deck.len(), deck);
                }
                (3, "innocent") => {
                    assert_eq!(run.run_ctx().gold, 400);
                    assert_eq!(run.run_ctx().deck[deck].id, CardId::Regret);
                }
                (4, "guilty") => {
                    assert_eq!(run.run_ctx().deck[deck].id, CardId::Doubt);
                    assert_eq!(run.reward.items.len(), 2);
                    assert_eq!(run.run_ctx().rngs.rewards.counter, rewards_rng + 12);
                    for reward in &run.reward.items {
                        let RewardItemKind::Card { cards, .. } = &reward.kind else { panic!("card reward"); };
                        assert_eq!(cards.len(), 3);
                        assert!(cards.iter().all(|c| !c.upgraded));
                    }
                }
                (4, "innocent") => {
                    assert_eq!(run.run_ctx().deck[deck].id, CardId::Doubt);
                    let niche = run.run_ctx().rngs.niche.counter;
                    let choice = run.pending_run_choice.take().unwrap();
                    run.apply_run_choice(choice, &[1, 0]).unwrap();
                    assert_eq!(run.run_ctx().rngs.niche.counter, niche);
                    assert_eq!(run.run_ctx().rngs.rewards.counter, rewards_rng);
                    assert_eq!(run.run_ctx().deck.len(), deck + 1);
                }
                _ => panic!("unexpected trial story"),
            }
            assert!(run.unported.is_none());
        }
    }
    assert_eq!(seen.len(), 6);
    let mut run = event_run(EventId::Trial, 0);
    run.current_act = run.acts.len() - 1;
    run.take_event_choice("reject").unwrap();
    run.take_event_choice("double_down").unwrap();
    run.take_event_choice("confirm_abandon").unwrap();
    assert_eq!(run.phase, RunPhase::RunOver);
    assert_eq!(run.run_ctx().hp, 0);
    assert!(!run.observation_full_run_victory());
}

