use crate::identity::EventId;
use super::*;

#[test]
fn unvisited_act_start_has_no_public_current_map_coord() {
    // C# RunState.CurrentActIndex clears VisitedMapCoords; CurrentMapCoord
    // returns null until the first visit, not the new map's start coordinate.
    let mut run=RunManager::new(1,RunOptions::default());
    assert!(run.observation_map_position().is_some());
    run.run_ctx_mut().coord=None;
    assert_eq!(run.observation_map_position(),None);
}

#[test]
fn touch_of_orobas_exposes_the_same_starter_pair_it_replaces() {
    let mut run=RunManager::new(1,RunOptions::default());
    let mut event=EventState::new(EventId::Orobas,crate::rng::Rng::new(7));
    event.options.push(super::super::events::EventOption::new("relic_1","Relic","Gain a relic"));
    event.relics.push(RelicId::TouchOfOrobas);
    run.current_event=Some(event);
    run.phase=RunPhase::Event;
    assert_eq!(run.observation_orobas_starter_swap("relic_1"),
        Some((RelicId::BurningBlood,RelicId::BlackBlood)));
    run.take_event_choice("relic_1").unwrap();
    assert!(run.run_ctx().has_owned_relic(crate::combat::PLAYER,RelicId::BlackBlood));
    assert!(!run.run_ctx().has_owned_relic(crate::combat::PLAYER,RelicId::BurningBlood));
}
