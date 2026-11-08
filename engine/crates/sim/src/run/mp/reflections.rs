use super::MpRun;
use crate::{identity::EventOptionId, run::{events::{EventOptionIdentity, EventState}, manager::reflections}, tables::CardId, card::Card};

impl MpRun {
    pub(super) fn resolve_reflections(&mut self, seat: usize, event: &mut EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        match option.id {
            EventOptionId::ReflectionsInitialTouchAMirror => {
                let mut run = self.rest_context(seat);
                reflections::touch(&mut run, &mut event.rng)?;
                self.carry_rest_context(seat, run);
            }
            EventOptionId::ReflectionsInitialShatter => {
                let originals = self.roster.player(seat).unwrap().deck.clone();
                for card in originals {
                    self.roster.next_card_id += 1;
                    let copy = card.clone_with_id(self.roster.next_card_id);
                    super::cards::add(&mut self.roster, &mut self.world.run, seat, copy)?;
                }
                self.roster.next_card_id += 1;
                let mut curse = Card::from_factory(CardId::BadLuck, false, self.roster.next_card_id)
                    .ok_or("Bad Luck factory missing")?;
                curse.owner = seat as u8;
                super::cards::add(&mut self.roster, &mut self.world.run, seat, curse)?;
            }
            _ => return Err("invalid Reflections option"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, MpPhase, PlayerSpec};

    #[test]
    fn reflections_shatter_and_touch_are_owner_local() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.world.current_room = Some("REFLECTIONS");
        run.phase = MpPhase::Event;
        run.begin_generic_event(crate::identity::EventId::Reflections).unwrap();
        let untouched = run.roster.player(0).unwrap().deck.clone();
        let count = run.roster.player(1).unwrap().deck.len();
        assert!(run.choose_event_option(7, 1, 1).is_err());
        run.choose_event_option(42, 1, 1).unwrap();
        let deck = &run.roster.player(1).unwrap().deck;
        assert_eq!(deck.len(), count * 2 + 1);
        assert_eq!(deck.last().unwrap().id, CardId::BadLuck);
        assert!(deck.iter().all(|c| c.owner == 1));
        assert_eq!(run.roster.player(0).unwrap().deck.iter().map(|c| c.instance_id).collect::<Vec<_>>(),
            untouched.iter().map(|c| c.instance_id).collect::<Vec<_>>());
        assert_eq!(run.phase, MpPhase::Event);
        run.choose_event_option(7, 0, 0).unwrap();
        assert_eq!(run.roster.player(0).unwrap().deck.iter().filter(|c| c.upgraded).count(), 4);
        assert_eq!(run.phase, MpPhase::Map);
    }
}
