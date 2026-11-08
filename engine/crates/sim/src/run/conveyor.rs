use crate::character::CardPool;
use crate::identity::{EventId, EventOptionId};
use super::*;

// C# Models/Events/EndlessConveyor.cs RollDish: source-ordered weights,
// no consecutive repeat; the fifth offered dish is forced without an RNG draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dish { Caviar, Spicy, Jelly, Eel, Condiment, Clam, Fysh, Salad }

impl Dish {
    pub(crate) fn from_index(index: i32) -> Self {
        [Self::Caviar, Self::Spicy, Self::Jelly, Self::Eel,
            Self::Condiment, Self::Clam, Self::Fysh, Self::Salad][index as usize]
    }
}

/// EndlessConveyor.cs:109-147,211-263. Roll the next dish and rebuild the two
/// visible options. Callers provide the owner-local predicates so solo and MP
/// share the exact event RNG/state transition.
pub(crate) fn resume_conveyor_state(
    event: &mut EventState,
    gold: i32,
    can_offer_condiment: bool,
    missing_hp: bool,
) {
    let previous = (event.ints[0] > 0).then(|| Dish::from_index(event.ints[1]));
    event.ints[0] += 1;
    let dish = if event.ints[0] % 5 == 0 {
        Dish::Salad
    } else {
        let mut dishes = vec![(Dish::Caviar, 6f32), (Dish::Spicy, 3.0),
            (Dish::Jelly, 3.0), (Dish::Eel, 3.0)];
        if can_offer_condiment {
            dishes.push((Dish::Condiment, 3.0));
        }
        if missing_hp {
            dishes.push((Dish::Clam, 6.0));
        }
        if event.ints[0] > 1 { dishes.push((Dish::Fysh, 1.0)); }
        dishes.retain(|(dish, _)| Some(*dish) != previous);
        let roll = event.rng.next_float(1.0) as f32
            * dishes.iter().map(|(_, weight)| weight).sum::<f32>();
        let mut cumulative = 0.0;
        dishes.into_iter().find_map(|(dish, weight)| {
            cumulative += weight;
            (roll < cumulative).then_some(dish)
        }).expect("NextFloat weighted dish")
    };
    event.ints[1] = dish as i32;
    event.options.clear();
    if event.ints[0] > 1 {
        event.options.set_page(crate::identity::EventPageId::EndlessConveyorGrabSomethingOffTheBelt,
            super::super::events::native_identity::EventParameter::None);
    }
    let mut grab = EventOption::new("grab",
        if event.ints[0] == 1 { "Grab Something (40g)" } else { "Grab Another (40g)" },
        "Random dish");
    if gold < 40 {
        grab = EventOption::new("grab", "Locked", "Cannot afford another dish").disabled();
    }
    event.options.push(grab);
    event.options[0].option_id.parameter =
        super::super::events::native_identity::EventParameter::ConveyorDish(dish);
    event.options.push(if event.ints[0] == 1 {
        EventOption::new("observe", "Observe Chef", "Upgrade 1 random card (free)")
    } else {
        EventOption::new("leave", "Leave", "Done eating")
    });
}

impl RunManager {
    pub(super) fn resume_conveyor(&mut self) {
        let can_offer_condiment = self.can_procure_potion();
        let missing_hp = self.run_ctx().hp != self.run_ctx().max_hp;
        let gold = self.run_ctx().gold;
        let Some(mut event) = self.current_event.take() else { return };
        if event.id != EventId::EndlessConveyor {
            self.current_event = Some(event);
            return;
        }
        resume_conveyor_state(&mut event, gold, can_offer_condiment, missing_hp);
        self.current_event = Some(event);
    }

    fn conveyor_upgrade(&mut self, event: &mut EventState) {
        // ObserveChef and SpicySnappy use NextItem in deck order, not StableShuffle.
        let candidates = self.upgradable_deck_indices();
        if !candidates.is_empty() {
            let index = candidates[event.rng.choice_index(candidates.len())];
            self.run_ctx_mut().upgrade_deck_card(index);
        }
    }

    pub(super) fn choose_conveyor(&mut self, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        if !option.is(EventOptionId::EndlessConveyorAllId) {
            if option.is(EventOptionId::EndlessConveyorInitialObserveChef) { self.conveyor_upgrade(&mut event); }
            self.phase = RunPhase::MapChoice;
            return Ok(());
        }
        let dish = Dish::from_index(event.ints[1]);
        if dish != Dish::Fysh { self.apply_event_effect(EventEffect::LoseGold(40))?; }
        match dish {
            Dish::Caviar => self.apply_event_effect(EventEffect::GainMaxHp(4))?,
            Dish::Clam => self.apply_event_effect(EventEffect::Heal(10))?,
            Dish::Fysh => self.apply_event_effect(EventEffect::GainGold(75))?,
            Dish::Spicy => self.conveyor_upgrade(&mut event),
            Dish::Salad => self.add_card_to_deck(CardId::FeedingFrenzyCard)?,
            Dish::Eel => {
                // FriedEel: Other source, default odds, NoUpgradeRoll, direct add.
                let rarity = self.roll_base_card_rarity();
                let card = self.generate_noncombat_card(CardPool::Colorless, rarity, &[], false)?;
                self.add_card_instance_to_deck(card)?;
            }
            Dish::Jelly => {
                let candidates = self.transformable_deck_indices();
                if candidates.len() > 1 {
                    self.current_event = Some(event);
                    self.begin_run_card_choice("event:ENDLESS_CONVEYOR", "Choose a card to transform", &candidates, 1, 1,
                        false, RunChoiceEffect::TransformEventRng, ChoiceResume::Event);
                    return Ok(());
                }
                for index in candidates {
                    let id = self.run_ctx().deck[index].instance_id;
                    self.transform_deck_card_with_rng(id, &mut event.rng)?;
                }
            }
            Dish::Condiment => {
                // SuspiciousCondiment: uniform character+shared pool, not rarity roll.
                let id = self.roll_event_potion().ok_or("unsupported conveyor potion pool")?;
                self.reward = RewardState::default();
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Potion(id), consumed: false, skippable: true,
                    skip_completes: true, add_injury_after: false, neows_bones_final: false,
                });
                self.reward.stay_in_container = true;
                self.current_event = Some(event);
                self.phase = RunPhase::CardReward;
                return Ok(());
            }
        }
        self.current_event = Some(event);
        if self.unported.is_none() { self.resume_conveyor(); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> RunManager {
        let mut mgr = RunManager::new(4100103, RunOptions::default());
        mgr.run_ctx_mut().gold = 1000;
        mgr.phase = RunPhase::Event;
        mgr.current_event = super::super::super::events::initialize(EventId::EndlessConveyor,
            mgr.run_ctx().rngs.seed, 1000, 24, 80, 80);
        mgr.resume_conveyor();
        mgr
    }

    #[test]
    fn conveyor_fifth_offer_is_forced_and_consumes_no_draw() {
        let mut mgr = manager();
        let event = mgr.current_event.as_mut().unwrap();
        event.ints[0] = 4;
        let counter = event.rng.counter;
        mgr.resume_conveyor();
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(Dish::from_index(event.ints[1]), Dish::Salad);
        assert_eq!(event.rng.counter, counter);
        mgr.take_event_choice("grab").unwrap();
        assert!(mgr.run_ctx().deck.iter().any(|c| c.id == CardId::FeedingFrenzyCard));
        assert_eq!(mgr.run_ctx().gold, 960);
    }

    #[test]
    fn conveyor_potion_modal_waits_for_claim_before_reroll_and_returns_to_event() {
        let mut mgr = manager();
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Condiment as i32;
        let counter = mgr.current_event.as_ref().unwrap().rng.counter;
        let rewards = mgr.run_ctx().rngs.rewards.counter;
        mgr.take_event_choice("grab").unwrap();
        assert_eq!(mgr.phase, RunPhase::CardReward);
        assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, counter);
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards + 1);
        mgr.skip_reward_set().unwrap();
        assert_eq!(mgr.phase, RunPhase::Event);
        assert_eq!(mgr.current_event.as_ref().unwrap().ints[0], 2);
    }

    #[test]
    fn conveyor_gold_dish_is_free_but_still_requires_forty_gold() {
        let mut mgr = manager();
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Fysh as i32;
        mgr.take_event_choice("grab").unwrap();
        assert_eq!(mgr.run_ctx().gold, 1075);
        mgr.run_ctx_mut().gold = 39;
        mgr.resume_conveyor();
        assert!(mgr.take_event_choice("grab").is_err());
    }

    #[test]
    fn conveyor_transform_waits_for_selection_then_rolls_using_event_rng() {
        let mut mgr = manager();
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Jelly as i32;
        let counter = mgr.current_event.as_ref().unwrap().rng.counter;
        let len = mgr.run_ctx().deck.len();
        mgr.take_event_choice("grab").unwrap();
        assert_eq!(mgr.phase, RunPhase::CardReward);
        assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, counter);
        assert_eq!(mgr.pending_run_choice.as_ref().unwrap().wire_source, Some("event:ENDLESS_CONVEYOR"));
        mgr.resolve_run_choice(0).unwrap();
        assert_eq!(mgr.phase, RunPhase::Event);
        assert_eq!(mgr.current_event.as_ref().unwrap().ints[0], 2);
        assert!(mgr.current_event.as_ref().unwrap().rng.counter > counter);
        assert_eq!(mgr.run_ctx().deck.len(), len);
    }

    #[test]
    fn conveyor_caviar_clam_and_eel_apply_source_effects_and_draw_counts() {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 40;
        let max_hp = mgr.run_ctx().max_hp;
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Caviar as i32;
        mgr.take_event_choice("grab").unwrap();
        assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (44, max_hp + 4));
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Clam as i32;
        mgr.take_event_choice("grab").unwrap();
        assert_eq!(mgr.run_ctx().hp, 54);
        mgr.current_event.as_mut().unwrap().ints[1] = Dish::Eel as i32;
        let rewards = mgr.run_ctx().rngs.rewards.counter;
        let len = mgr.run_ctx().deck.len();
        mgr.take_event_choice("grab").unwrap();
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards + 2);
        assert_eq!(mgr.run_ctx().deck.len(), len + 1);
        assert!(super::super::super::card_pools::cards(CardPool::Colorless, false).unwrap()
            .contains(&mgr.run_ctx().deck.last().unwrap().id));
        assert!(!mgr.run_ctx().deck.last().unwrap().upgraded);
    }

    #[test]
    fn conveyor_observe_is_free_next_item_in_deck_order_not_a_shuffle() {
        let mut mgr = manager();
        let candidates = mgr.upgradable_deck_indices();
        let mut expected_rng = mgr.current_event.as_ref().unwrap().rng.clone();
        let expected = candidates[expected_rng.choice_index(candidates.len())];
        mgr.take_event_choice("observe").unwrap();
        assert!(mgr.run_ctx().deck[expected].upgraded);
        assert_eq!(mgr.run_ctx().gold, 1000);
        assert_eq!(mgr.phase, RunPhase::MapChoice);
        assert!(mgr.current_event.is_none());
    }
}
