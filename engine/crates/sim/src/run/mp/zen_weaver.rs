//! ZenWeaver.RemoveCardsAndProceed removes cards before charging their owner.
use super::*;
use crate::{identity::EventOptionId, run::events::EventOptionIdentity};

impl MpRun {
    pub(super) fn choose_zen_weaver(&mut self, seat: usize, mut event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        let (wanted, cost) = if option.is(EventOptionId::ZenWeaverInitialEmotionalAwareness) { (1, 125) } else { (2, 250) };
        let cards = self.removable_event_cards(seat);
        let count = wanted.min(cards.len());
        if count == 0 {
            self.apply_event_effect(seat, EventEffect::LoseGold(cost), &mut event)?;
            self.finish_event_seat(seat);
        } else {
            let all = count == cards.len();
            self.events[seat] = Some(event);
            self.begin_event_card_modal(seat, cards, count, count, false,
                EventCardAction::RemoveAndLoseGold(cost))?;
            if all {
                let net_id = self.roster.player(seat).unwrap().net_id;
                self.select_event_cards(net_id, seat, &(0..count).collect::<Vec<_>>())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    #[test]
    fn new_mp_zen_weaver_removes_before_charging_only_its_owner() {
        for (option, count, cost) in [(1, 1, 125), (2, 2, 250)] {
            let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
                PlayerSpec { net_id: 42, character: Character::Silent }];
            let mut run = MpRun::new("73003", &specs, 0).unwrap();
            run.phase = MpPhase::Event;
            run.world.current_room = Some("ZEN_WEAVER");
            run.roster.player_mut(1).unwrap().gold = 300;
            run.begin_generic_event(crate::identity::EventId::ZenWeaver).unwrap();
            let other = format!("{:?}", run.roster.player(0).unwrap());
            let size = run.roster.player(1).unwrap().deck.len();
            run.choose_event_option(42, 1, option).unwrap();
            assert_eq!(run.roster.player(1).unwrap().gold, 300);
            let (_, min, max, skip) = run.event_card_modal(42, 1).unwrap();
            assert_eq!((min, max, skip), (count, count, false));
            assert!(run.select_event_cards(7, 1, &[0]).is_err());
            run.select_event_cards(42, 1, &(0..count).collect::<Vec<_>>()).unwrap();
            assert_eq!(run.roster.player(1).unwrap().gold, 300 - cost);
            assert_eq!(run.roster.player(1).unwrap().deck.len(), size - count);
            assert_eq!(other, format!("{:?}", run.roster.player(0).unwrap()));
            assert!(run.event_done[1]);
        }
    }
}
