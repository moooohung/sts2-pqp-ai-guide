use super::{MpRun, events::EventCardAction};
use crate::{identity::EventOptionId, run::events::{EventOptionIdentity, EventState, EventEffect, spirit_grafter::{accept, REJECTION_DAMAGE}}};

impl MpRun {
    pub(super) fn choose_spirit_grafter(&mut self, seat: usize, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        if option.is(EventOptionId::SpiritGrafterInitialLetItIn) {
            for effect in accept() { self.apply_event_effect(seat, effect, &mut event)?; }
            self.finish_event_seat(seat);
        } else {
            let cards = self.upgradable_event_cards(seat);
            if cards.is_empty() {
                self.apply_event_effect(seat, EventEffect::LoseHp(REJECTION_DAMAGE), &mut event)?;
                self.finish_event_seat(seat);
            } else {
                let all = cards.len() == 1;
                self.events[seat] = Some(event);
                self.begin_event_card_modal(seat, cards, 1, 1, false, EventCardAction::UpgradeAndDamage(REJECTION_DAMAGE))?;
                if all { self.select_event_cards(self.roster.player(seat).unwrap().net_id, seat, &[0])?; }
            }
        }
        Ok(())
    }
}
