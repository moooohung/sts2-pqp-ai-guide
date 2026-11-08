use super::{MpRun, MpPhase};
use crate::{rng::{Rng,deterministic_hash64},run::events::{EventState,EventOption}};

impl MpRun {
    pub(super) fn enter_architect(&mut self) {
        self.world.current_room=Some("THE_ARCHITECT");
        self.phase=MpPhase::Event;
        for seat in 0..self.roster.players().len() {
            // EventModel.BeginEvent: nonshared events use the owner slot.
            let mut event=EventState::new(crate::identity::EventId::TheArchitect,Rng::new(
                self.world.run.rngs.seed.wrapping_add(deterministic_hash64("THE_ARCHITECT"))
                    .wrapping_add(seat as u64)));
            event.rng.choice_index(1);
            event.ints[0]=match self.roster.player(seat).unwrap().character {
                super::Character::Silent=>1,
                super::Character::Ironclad|super::Character::Necrobinder=>2,
                _=>3,
            };
            event.options.push(EventOption::new(if event.ints[0]>1{"advance"}else{"proceed"},"Continue",""));
            self.events[seat]=Some(event);
            self.event_done[seat]=false;
        }
    }

    pub(super) fn choose_architect(&mut self,seat:usize,mut event:EventState) {
        if event.ints[0]>1 {
            event.ints[0]-=1;
            event.options.clear();
            event.options.push(EventOption::new(if event.ints[0]>1{"advance"}else{"proceed"},"Continue",""));
            self.events[seat]=Some(event);
        }else{
            self.event_done[seat]=true;
            if self.event_done.iter().all(|done|*done) {self.phase=MpPhase::RunOver;}
        }
    }
}
