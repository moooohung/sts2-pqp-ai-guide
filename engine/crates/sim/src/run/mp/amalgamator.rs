use super::{MpRun, events::EventCardAction};
use crate::{identity::EventOptionId, run::events::EventOptionIdentity, tables::{CardId, CardTag}};

impl MpRun {
    pub(super) fn choose_amalgamator(&mut self, net_id: u64, seat: usize, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let (tag, target) = if option.is(EventOptionId::AmalgamatorInitialCombineStrikes) {
            (CardTag::Strike, CardId::UltimateStrike)
        } else { (CardTag::Defend, CardId::UltimateDefend) };
        let cards = self.removable_event_cards(seat).into_iter()
            .filter(|c| c.rarity == crate::tables::CardRarity::Basic && c.has_tag(tag))
            .collect::<Vec<_>>();
        let count = cards.len().min(2);
        if count == 0 {
            self.add_event_card(seat, target)?;
            self.finish_event_seat(seat);
            return Ok(());
        }
        let all = count == cards.len();
        self.begin_event_card_modal(seat, cards, count, count, false, EventCardAction::RemoveAndAdd(target))?;
        if all { self.select_event_cards(net_id, seat, &(0..count).collect::<Vec<_>>())?; }
        Ok(())
    }
}
