use super::MpRun;
use crate::{identity::EventOptionId, run::events::{doll_room, EventEffect, EventOptionIdentity, EventState}};

impl MpRun {
    pub(super) fn choose_doll_room(&mut self, seat: usize, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        let relic = if let Some(i) = option.offer_index(EventOptionId::DollRoomTakeTake) {
            Some(*event.relics.get(i).ok_or("invalid doll choice")?)
        } else if option.is(EventOptionId::DollRoomInitialRandom) {
            Some(doll_room::random(&mut event))
        } else {
            let take_time = option.is(EventOptionId::DollRoomInitialTakeSomeTime);
            self.apply_event_effect(seat, EventEffect::LoseHp(if take_time {5} else {15}), &mut event)?;
            if self.roster.player(seat).unwrap().hp > 0 {
                doll_room::offer(&mut event, if take_time {2} else {3});
                self.events[seat] = Some(event);
            } else { self.finish_event_seat(seat); }
            return Ok(());
        };
        self.attach_event_relic(seat, relic.unwrap())?;
        self.record_relic_seen(seat, relic.unwrap());
        self.finish_event_seat(seat);
        Ok(())
    }
}
