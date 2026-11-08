//! StoneOfAllTime.cs: owner-local potion sacrifice and Vigorous selection.
use super::*;
use crate::{identity::EventOptionId, run::events::EventOptionIdentity};

impl MpRun {
    pub(super) fn initialize_stone_event(&self, seat: usize, event: &mut EventState) {
        let slots: Vec<_> = self.roster.player(seat).unwrap().potions.iter().enumerate()
            .filter_map(|(i, p)| p.is_some().then_some(i)).collect();
        event.ints[0] = if slots.is_empty() { -1 } else {
            slots[event.rng.choice_index(slots.len())] as i32
        };
        event.options.replace([
            EventOption::new("lift", "Lift", "Discard a potion, gain 10 Max HP"),
            EventOption::new("push", "Push", "Take 6 damage, enchant a card with Vigorous +8"),
        ]);
        event.options[0].enabled = event.ints[0] >= 0;
        event.options[1].enabled = !self.enchantable_event_cards(seat, "Vigorous", None).is_empty();
    }

    pub(super) fn choose_stone_event(&mut self, seat: usize, mut event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        if option.is(EventOptionId::StoneOfAllTimeInitialLift) {
            self.roster.player_mut(seat).unwrap().potions[event.ints[0] as usize] = None;
            self.apply_event_effect(seat, EventEffect::GainMaxHp(10), &mut event)?;
        } else {
            self.apply_event_effect(seat, EventEffect::LoseHp(6), &mut event)?;
            let cards = self.enchantable_event_cards(seat, "Vigorous", None);
            if self.roster.player(seat).unwrap().hp > 0 && !cards.is_empty() {
                let all = cards.len() == 1;
                self.events[seat] = Some(event);
                self.begin_event_card_modal(seat, cards, 1, 1, false,
                    EventCardAction::Enchant(crate::identity::EnchantmentId::Vigorous, 8))?;
                if all {
                    let net_id = self.roster.player(seat).unwrap().net_id;
                    self.select_event_cards(net_id, seat, &[0])?;
                }
                return Ok(());
            }
        }
        event.rng.next_int_exclusive(0, 100);
        self.finish_event_seat(seat);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    #[test]
    fn stone_options_resolve_for_each_owner_without_touching_other_seats() {
        let specs = [
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ];
        let mut run = MpRun::new("73003", &specs, 0).unwrap();
        run.phase = MpPhase::Event;
        run.world.current_room = Some("STONE_OF_ALL_TIME");
        for seat in 0..2 {
            run.roster.player_mut(seat).unwrap().potions =
                vec![None, Some(PotionId::Firepotion), None];
        }
        run.begin_generic_event(crate::identity::EventId::StoneOfAllTime).unwrap();
        assert_eq!(run.event_options(7, 0).unwrap(), vec!["lift", "push"]);
        assert_eq!(run.event_options(42, 1).unwrap(), vec!["lift", "push"]);
        assert!(!run.can_discard_potion(7, 0, 1));
        assert!(run.discard_potion(7, 0, 1).is_err());
        let other = format!("{:?}", run.roster.player(1).unwrap());
        let max_hp = run.roster.player(0).unwrap().max_hp;
        run.choose_event_option(7, 0, 0).unwrap();
        assert_eq!(run.roster.player(0).unwrap().max_hp, max_hp + 10);
        assert!(run.roster.player(0).unwrap().potions.iter().all(Option::is_none));
        assert_eq!(other, format!("{:?}", run.roster.player(1).unwrap()));
        let hp = run.roster.player(1).unwrap().hp;
        let counter = run.events[1].as_ref().unwrap().rng.counter;
        run.choose_event_option(42, 1, 1).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, hp - 6);
        assert_eq!(run.events[1].as_ref().unwrap().rng.counter, counter);
        run.select_event_cards(42, 1, &[0]).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.enchantments.contains(&(crate::identity::EnchantmentId::Vigorous, 8))).count(), 1);
        assert_eq!(run.phase, MpPhase::Map);
        assert_eq!(run.roster.player(1).unwrap().potions[1], Some(PotionId::Firepotion));
        assert!(run.can_discard_potion(42, 1, 1));
    }
}
