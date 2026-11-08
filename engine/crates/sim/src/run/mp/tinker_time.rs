use super::*;
use crate::run::events::tinker_time::{self, Step};

impl MpRun {
    pub(super) fn choose_tinker_time(&mut self, seat: usize, mut event: EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        match tinker_time::choose(&mut event, option)? {
            Step::Types | Step::Riders => {
                let count = if event.ints[0] == 1 { 3 } else { 2 };
                self.roster.next_card_id = self.roster.next_card_id.max(self.world.run.next_card_id) + count;
                self.events[seat] = Some(event);
            }
            Step::Complete => {
                self.roster.next_card_id = self.roster.next_card_id.max(self.world.run.next_card_id) + 1;
                let mut card = tinker_time::card(&event, self.roster.next_card_id)?;
                card.owner = seat as u8;
                super::super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?;
                self.finish_event_seat(seat);
            }
        }
        Ok(())
    }
}
