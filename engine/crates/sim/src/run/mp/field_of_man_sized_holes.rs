use super::{MpRun, events::EventCardAction};
use crate::{identity::EventOptionId, run::events::EventOptionIdentity};

impl MpRun {
    pub(super) fn choose_holes(&mut self, net_id: u64, seat: usize, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let resist = option.is(EventOptionId::FieldOfManSizedHolesInitialResist);
        let (cards, requested, action) = if resist {
            (self.removable_event_cards(seat), 2,
                EventCardAction::RemoveAndAdd(crate::tables::CardId::Normality))
        } else {
            (self.enchantable_event_cards(seat, "PerfectFit", None), 1,
                EventCardAction::Enchant(crate::identity::EnchantmentId::PerfectFit, 1))
        };
        let count = requested.min(cards.len());
        if count == 0 {
            if resist { self.add_event_card(seat, crate::tables::CardId::Normality)?; }
            self.finish_event_seat(seat);
            return Ok(());
        }
        let all = count == cards.len();
        self.begin_event_card_modal(seat, cards, count, count, false, action)?;
        if all { self.select_event_cards(net_id, seat, &(0..count).collect::<Vec<_>>())?; }
        Ok(())
    }
}
