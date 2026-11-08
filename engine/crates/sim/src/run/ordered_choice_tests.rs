use crate::identity::EventId;
use super::*;
use crate::tables::{CardId, card_info};

#[test]
fn ordered_export_retains_toggle_order_without_changing_progress_contract() {
    let mut run = RunManager::new(16000358, RunOptions::default());
    let cards = &run.run_ctx().deck;
    let options = cards.iter().take(3).map(|c| RunChoiceCard {
        instance_id: c.instance_id, card_id: c.id, source_pile: "deck",
    }).collect();
    // Construction in this unit test is not evidence of natural reachability.
    run.pending_run_choice = Some(RunCardChoice {
        wire_source: Some("test:ordered"), prompt: "Choose two", options,
        selected: vec![2, 0], min_choices: 2, max_choices: 2,
        allow_skip: false, single_select: false,
        effect: RunChoiceEffect::RemoveAndAddCard(CardId::UltimateStrike),
        resume: ChoiceResume::MapChoice, reward_cards: Vec::new(),
    });
    let before = run.observation_selector_shape();
    let mut exported = run.observation_ordered_run_choice().unwrap();
    assert_eq!(exported.selected_in_order, [2, 0]);
    assert_eq!(run.observation_progress_run_choice().unwrap().unwrap().0, [0, 2]);
    assert!(exported.can_confirm && exported.is_multi);
    assert!(!exported.allow_skip && !exported.single_select);
    assert_eq!((exported.min_choices, exported.max_choices), (2, 2));
    assert_eq!(exported.options[0].card_id, run.run_ctx().deck[0].id);
    assert!(card_info(exported.options[0].card_id, false).is_some());
    exported.selected_in_order.reverse();
    exported.options.clear();
    assert_eq!(run.observation_ordered_run_choice().unwrap().selected_in_order, [2, 0]);
    assert_eq!(before, run.observation_selector_shape());
}

#[test]
fn ordered_choice_transform_effects_follow_returned_order_not_option_order() {
    for effect in [RunChoiceEffect::TransformEventRng,
        RunChoiceEffect::TransformEventRngAndDamage(9),
        RunChoiceEffect::Transform,RunChoiceEffect::TransformUpgrade] {
        let mut run=RunManager::new(7,RunOptions::default());
        let old=run.run_ctx().deck.clone();
        run.run_ctx_mut().deck=vec![old[9].clone(),old[5].clone(),old[0].clone()];
        run.current_event=Some(EventState::new(EventId::MorphicGrove,Rng::new(91)));
        run.unported=None;
        let mut expected=run.clone();
        let ids=[old[0].instance_id,old[9].instance_id];
        // Original event/relic foreach, invoking the same per-card transform
        // once per returned selection; not an option-set enumeration.
        match effect {
            RunChoiceEffect::TransformUpgrade=>expected.apply_astrolabe_choice(&ids).unwrap(),
            RunChoiceEffect::Transform=>{
                for id in ids {expected.transform_deck_card(id).unwrap();}
            }
            _=>{
                let mut rng=expected.current_event.as_ref().unwrap().rng.clone();
                for id in ids {expected.transform_deck_card_with_rng(id,&mut rng).unwrap();}
                expected.current_event.as_mut().unwrap().rng=rng;
                if let RunChoiceEffect::TransformEventRngAndDamage(n)=effect{
                    expected.apply_event_effect(EventEffect::LoseHp(n)).unwrap();
                }
            }
        }
        run.begin_run_card_choice("test:ordered","Choose two",&[0,1,2],2,2,false,
            effect,ChoiceResume::MapChoice);
        run.resolve_run_choice(2).unwrap();
        run.resolve_run_choice(0).unwrap();
        assert_eq!(format!("{:?}",run.run_ctx()),format!("{:?}",expected.run_ctx()),"{effect:?}");
        if matches!(effect,RunChoiceEffect::TransformEventRng|RunChoiceEffect::TransformEventRngAndDamage(_)){
            assert_eq!(run.replay_event.as_ref().unwrap().rng,expected.current_event.as_ref().unwrap().rng);
        }
    }
}

#[test]
fn ordered_choice_deselect_reselect_moves_card_to_returned_tail() {
    let mut run=RunManager::new(7,RunOptions::default());
    let ids=[run.run_ctx().deck[2].instance_id,run.run_ctx().deck[0].instance_id];
    run.unported=None;
    run.begin_run_card_choice("test:ordered","Duplicate two",&[0,1,2],2,3,false,
        RunChoiceEffect::Duplicate,ChoiceResume::MapChoice);
    for index in [0,2,0,0]{run.resolve_run_choice(index).unwrap();}
    assert_eq!(run.pending_run_choice.as_ref().unwrap().selected,[2,0]);
    // Give the originals distinct mutable state so append order is observable.
    run.run_ctx_mut().upgrade_deck_card(2);
    run.confirm_pending_run_choice().unwrap();
    let deck=&run.run_ctx().deck;
    let originals:Vec<_>=ids.iter().map(|id|deck.iter().find(|c|c.instance_id==*id).unwrap().upgraded).collect();
    assert_eq!(deck[deck.len()-2..].iter().map(|c|c.upgraded).collect::<Vec<_>>(),originals);
}
