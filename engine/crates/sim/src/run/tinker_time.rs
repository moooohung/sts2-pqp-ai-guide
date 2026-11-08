use super::*;
use crate::run::events::tinker_time::{self, Step};

impl RunManager {
    pub(super) fn choose_tinker_time(&mut self, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        match tinker_time::choose(&mut event, option)? {
            Step::Types | Step::Riders => {
                // GetCardTypeHoverTip creates all three cards before TakeRandom;
                // GetRiderHoverTip creates the two offered cards after that draw.
                let count = if event.ints[0] == 1 { 3 } else { 2 };
                for _ in 0..count { self.run_ctx_mut().new_card_id(); }
                self.current_event = Some(event);
            }
            Step::Complete => {
                let instance = self.run_ctx_mut().new_card_id();
                self.add_card_instance_to_deck(tinker_time::card(&event, instance)?)?;
                self.phase = RunPhase::MapChoice;
            }
        }
        Ok(())
    }
}
