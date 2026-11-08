//! EventSynchronizer.cs: all-seat votes, random vote winner, one shared page.
use super::{MpRun,MpPhase};
use crate::identity::{EventId, EventOptionId};
impl MpRun {
    pub(super) fn vote_shared_event_option(&mut self,net:u64,seat:usize,option:usize)
        ->Result<(),&'static str>{
        self.roster.actor(net,seat)?;
        if self.phase!=MpPhase::Event {return Err("not at event");}
        if option>=self.event_options(net,seat)?.len() {return Err("invalid event option");}
        let mut next=self.clone();
        next.shared_event_votes[seat]=Some(option);
        if next.shared_event_votes.iter().all(Option::is_some) {
            let winner=next.shared_event_selection_rng.choice_index(next.shared_event_votes.len());
            let choice=next.shared_event_votes[winner].unwrap();
            next.shared_event_votes.fill(None);
            let event=next.events[seat].as_ref().ok_or("shared event missing")?;
            let id=event.id;
            let option=event.options.iter().filter(|option|option.enabled).nth(choice)
                .ok_or("invalid event option")?.option_id;
            // These reducers already dispatch the resolved page to the whole
            // party, or enter one shared combat. Do not invoke them twice.
            let whole_party=matches!(id,EventId::TheLanternKey|EventId::BattlewornDummy)
                || (id==EventId::PunchOff && option.is(EventOptionId::PunchOffICanTakeThemFight))
                || (id==EventId::DenseVegetation && option.is(EventOptionId::DenseVegetationRestFight));
            if whole_party {
                next.choose_event_option_now(net,seat,choice)?;
                if id==EventId::PunchOff {
                    next.punch_off_owners=(0..next.roster.players().len()).collect();
                }
            } else {
                for owner in 0..next.roster.players().len() {
                    let net=next.roster.player(owner).unwrap().net_id;
                    next.choose_event_option_now(net,owner,choice)?;
                }
            }
        }
        *self=next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character,PlayerSpec};
    #[test]
    fn dense_fallback_votes_once_per_seat_before_advancing_the_page() {
        let specs=[PlayerSpec{net_id:100,character:Character::Silent},
            PlayerSpec{net_id:101,character:Character::Defect}];
        let mut run=MpRun::new("20261008020007",&specs,10).unwrap();
        run.phase=MpPhase::Event;run.world.current_room=Some("DENSE_VEGETATION");
        run.begin_generic_event(crate::identity::EventId::DenseVegetation).unwrap();
        let counter=run.shared_event_selection_rng.counter;
        let hp:Vec<_>=run.roster.players().iter().map(|p|p.hp).collect();
        for seat in 0..2 {
            assert!(run.event_is_shared());
            run.choose_event_option(specs[seat].net_id,seat,0).unwrap();
            if seat==0 {
                assert_eq!(run.phase,MpPhase::Event);
                assert_eq!(run.shared_event_selection_rng.counter,counter);
                assert_eq!(run.roster.player(0).unwrap().hp,hp[0]);
            }
        }
        assert_eq!(run.phase,MpPhase::Map);
        assert!(!run.event_is_shared());
        assert_eq!(run.shared_event_selection_rng.counter,counter+1);
        for seat in 0..2 {assert_eq!(run.roster.player(seat).unwrap().hp,hp[seat]-8);}
    }
    #[test]
    fn every_shared_event_waits_for_other_players_without_advancing_rng() {
        use crate::{rng::Rng,run::events::{EventState,EventOption}};
        for (id,option) in [(crate::identity::EventId::BattlewornDummy,"setting_1"),(crate::identity::EventId::DenseVegetation,"trudge_on"),(crate::identity::EventId::FakeMerchant,"leave"),
            (crate::identity::EventId::JungleMazeAdventure,"solo"),(crate::identity::EventId::MorphicGrove,"group"),(crate::identity::EventId::PunchOff,"nab"),
            (crate::identity::EventId::TheLanternKey,"return_key"),(crate::identity::EventId::WarHistorianRepy,"unlock_cage")] {
            let specs=[PlayerSpec{net_id:100,character:Character::Ironclad},
                PlayerSpec{net_id:101,character:Character::Silent}];
            let mut run=MpRun::new("73003",&specs,0).unwrap();
            run.phase=MpPhase::Event;run.world.current_room=Some(id.name());
            // Fake Merchant and War Historian are not legal MP room draws.
            // Inject their page to verify transport also enforces their IsShared.
            for seat in 0..2 {
                let mut event=EventState::new(id,Rng::new(1));
                event.options.push(EventOption::new(option,"Probe",""));
                run.events[seat]=Some(event);
            }
            let roster=format!("{:?}",run.roster);
            let counter=run.shared_event_selection_rng.counter;
            run.choose_event_option(100,0,0).unwrap();
            assert_eq!(run.phase,MpPhase::Event,"{id}");
            assert_eq!(format!("{:?}",run.roster),roster,"{id}");
            assert_eq!(run.shared_event_selection_rng.counter,counter,"{id}");
            assert_eq!(run.shared_event_votes,vec![Some(0),None],"{id}");
        }
    }

    #[test]
    fn lantern_return_requires_both_votes_then_consumes_one_shared_rng_draw() {
        let specs=[PlayerSpec{net_id:100,character:Character::Ironclad},
            PlayerSpec{net_id:101,character:Character::Silent}];
        let mut run=MpRun::new("73003",&specs,0).unwrap();
        run.phase=MpPhase::Event;run.world.current_room=Some("THE_LANTERN_KEY");
        run.begin_generic_event(crate::identity::EventId::TheLanternKey).unwrap();
        let gold:Vec<_>=run.roster.players().iter().map(|p|p.gold).collect();
        let index=run.event_options(100,0).unwrap().iter().position(|&id|id=="return_key").unwrap();
        let counter=run.shared_event_selection_rng.counter;
        run.choose_event_option(100,0,index).unwrap();
        assert_eq!(run.phase,MpPhase::Event);
        assert_eq!(run.roster.player(1).unwrap().gold,gold[1]);
        assert_eq!(run.shared_event_selection_rng.counter,counter);
        run.choose_event_option(101,1,index).unwrap();
        assert_eq!(run.phase,MpPhase::Map);
        for (seat,before) in gold.into_iter().enumerate() {
            assert_eq!(run.roster.player(seat).unwrap().gold,before+100);
        }
        assert_eq!(run.shared_event_selection_rng.counter,counter+1);
    }
    #[test]
    fn opposing_punch_off_votes_cannot_leave_a_nab_pickup_inside_combat(){
        let specs=[PlayerSpec{net_id:100,character:Character::Regent},
            PlayerSpec{net_id:101,character:Character::Regent}];
        let mut run=MpRun::new("7300000875",&specs,3).unwrap();
        run.phase=MpPhase::Event;run.world.current_room=Some("PUNCH_OFF");
        run.begin_generic_event(crate::identity::EventId::PunchOff).unwrap();
        let decks:Vec<_>=run.roster.players().iter().map(|p|p.deck.len()).collect();
        let mut rng=run.shared_event_selection_rng.clone();
        let winner=rng.choice_index(2);
        run.choose_event_option(100,0,0).unwrap(); // Nab vote, not an acquisition.
        assert_eq!(run.roster.player(0).unwrap().deck.len(),decks[0]);
        assert!(run.relic_rewards.iter().all(Option::is_none));
        run.choose_event_option(101,1,1).unwrap();
        if winner==0 {
            assert!(run.relic_rewards.iter().all(Option::is_some));
            for seat in 0..2 {
                run.select_event_relic_reward(100+seat as u64,seat,false).unwrap();
            }
            assert_eq!(run.phase,MpPhase::Map);
        } else {
            assert!(run.relic_rewards.iter().all(Option::is_none));
            run.choose_event_option(100,0,0).unwrap();
            assert_eq!(run.phase,MpPhase::Event);
            run.choose_event_option(101,1,0).unwrap();
            assert_eq!(run.phase,MpPhase::Combat);
            assert_eq!(run.punch_off_owners,vec![0,1]);
        }
    }
}
