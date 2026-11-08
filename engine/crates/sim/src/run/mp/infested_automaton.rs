use super::MpRun;
use crate::{identity::EventOptionId, run::events::EventOptionIdentity, tables::CardType};

impl MpRun {
    pub(super) fn choose_infested_automaton(&mut self, seat: usize, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let ascension = self.world.run.ascension;
        let player = self.roster.player_mut(seat).unwrap();
        let (kind, cost, costs_x) = if option.is(EventOptionId::InfestedAutomatonInitialStudy) {
            (Some(CardType::Power), None, None)
        } else { (None, Some(0), Some(false)) };
        let id = crate::run::events::infested_automaton::draw(
            player.character, true, ascension, &mut player.rngs.rewards,
            kind, cost, costs_x)?;
        self.roster.next_card_id += 1;
        let mut cards = [crate::card::Card::from_factory(id, false, self.roster.next_card_id)
            .ok_or("Infested Automaton card factory")?];
        cards[0].owner = seat as u8;
        let mut next_id = self.roster.next_card_id;
        super::rewards::apply_card_creation_modifiers(
            self.roster.player_mut(seat).unwrap(), &mut cards, &mut next_id,
            Some(self.world.run.room_type), &mut self.world.run.rngs.niche, false)?;
        self.roster.next_card_id = next_id;
        for card in cards { super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?; }
        self.finish_event_seat(seat);
        Ok(())
    }
}
