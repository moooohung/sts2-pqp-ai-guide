use super::MpRun;
use super::events::EventCardAction;
use crate::{run::events::EventState, tables::CardId};

impl MpRun {
    pub(super) fn confront_forgotten_grave(&mut self, seat: usize, event: EventState)
        -> Result<(), &'static str> {
        self.add_event_card(seat, CardId::Decay)?;
        let cards = self.enchantable_event_cards(seat, "SoulsPower", None);
        if cards.is_empty() {
            self.finish_event_seat(seat);
        } else {
            self.events[seat] = Some(event);
            self.begin_event_card_modal(seat, cards, 1, 1, false,
                EventCardAction::Enchant(crate::identity::EnchantmentId::SoulsPower, 1))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Character, MpPhase, PlayerSpec};
    #[test]
    fn confront_adds_owner_decay_before_selecting_one_souls_power() {
        let mut run = MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap();
        run.phase = MpPhase::Event;
        run.add_event_card(1, CardId::from_name("BACKSTAB").unwrap()).unwrap();
        run.begin_generic_event(crate::identity::EventId::GraveOfTheForgotten).unwrap();
        run.choose_event_option(42, 1, 0).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, CardId::Decay);
        assert_eq!(run.roster.player(0).unwrap().deck.len(), 10);
        assert!(run.select_event_cards(7, 1, &[0]).is_err());
        run.select_event_cards(42, 1, &[0]).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.enchantments.iter().any(|(name, amount)| *name == crate::identity::EnchantmentId::SoulsPower && *amount == 1)).count(), 1);
        assert!(!run.roster.player(1).unwrap().deck.iter()
            .find(|c| c.id == CardId::from_name("BACKSTAB").unwrap()).unwrap()
            .has_kw(crate::tables::KW_EXHAUST));
        assert!(run.event_done[1]);
        assert!(!run.event_done[0]);
    }
}
