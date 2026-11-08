use super::MpRun;
use crate::{identity::EventOptionId, run::events::{EventOptionIdentity, EventState}, tables::{PotionId, PotionRarity, POTION_INFO}};

impl MpRun {
    pub(super) fn choose_potion_courier(&mut self, seat: usize, mut event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        let potions = if option.is(EventOptionId::PotionCourierInitialGrabPotions) {
            let foul = PotionId::Foulpotion;
            [Some(foul); 3]
        } else if option.is(EventOptionId::PotionCourierInitialRansack) {
            let player = self.roster.player_mut(seat).unwrap();
            let pool: Vec<_> = super::super::potion_pools::cards(player.character)
                .ok_or("courier potion pool missing")?.into_iter()
                .filter(|id| POTION_INFO[id.index()].rarity == PotionRarity::Uncommon).collect();
            if pool.is_empty() {
                self.finish_event_seat(seat);
                return Ok(());
            }
            let potion = pool[player.rngs.rewards.choice_index(pool.len())];
            [Some(potion), None, None]
        } else {
            return Err("invalid Potion Courier option");
        };
        event.options.clear();
        self.events[seat] = Some(event);
        self.event_potion_rewards[seat] = Some(potions);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Character, MpPhase, PlayerSpec};
    #[test]
    fn courier_offers_three_foul_or_one_owner_rewards_rng_uncommon() {
        let mut run = MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap();
        run.phase = MpPhase::Event;
        run.begin_generic_event(crate::identity::EventId::PotionCourier).unwrap();
        let rewards_rng = run.roster.player(0).unwrap().rngs.rewards.counter;
        let event_rng = run.events[1].as_ref().unwrap().rng.counter;
        let mut expected_rng = run.roster.player(1).unwrap().rngs.rewards.clone();
        expected_rng.choice_index(1);
        run.choose_event_option(7, 0, 0).unwrap();
        let foul = PotionId::from_name("FoulPotion").unwrap();
        for i in 0..3 {
            assert_eq!(run.event_potion_rewards(7, 0).unwrap()[i], Some(foul));
            assert!(run.take_event_potion_reward(7, 0, i).unwrap());
        }
        run.finish_event_potion_rewards(7, 0).unwrap();
        assert_eq!(run.roster.player(0).unwrap().potions, vec![Some(foul); 3]);
        assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter, rewards_rng);
        run.choose_event_option(42, 1, 1).unwrap();
        let potion = run.event_potion_rewards(42, 1).unwrap()[0].unwrap();
        assert_eq!(POTION_INFO[potion.index()].rarity, PotionRarity::Uncommon);
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, expected_rng.counter);
        assert_eq!(run.events[1].as_ref().unwrap().rng.counter, event_rng);
        assert!(run.take_event_potion_reward(7, 1, 0).is_err());
        run.skip_event_potion_reward(42, 1, 0).unwrap();
        run.finish_event_potion_rewards(42, 1).unwrap();
        assert!(run.event_done[1]);
    }
}
