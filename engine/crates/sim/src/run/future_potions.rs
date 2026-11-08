use crate::identity::EventId;
use super::*;

impl RunManager {
    pub(super) fn generate_future_potion_cards(
        &mut self, rarity: CardRarity, kind: CardType,
    ) -> Result<ArrayVec<Card, 4>, &'static str> {
        // TheFutureOfPotions.cs:123-138; CardReward.cs:98-103,123-137:
        // retain owner pool + type/rarity filter, suppress pool modifications
        // and upgrade rolls; model/reward hooks precede AfterGenerated upgrades.
        // Frozen events/act2.py:1381-1398 retains these options on the reward.
        let mut cards: ArrayVec<Card, 4> = self.generate_filtered_uniform_cards(
            3, self.character.into(), rarity, false, true, Some(kind), false)?.into_iter().collect();
        self.apply_card_reward_relic_hooks(&mut cards, true);
        if let Some(reason) = self.unported { return Err(reason); }
        for card in &mut cards {
            if !card.upgraded {
                let owner = card.owner;
                let enchantments = card.enchantments.clone();
                shop::upgrade_template(self.run_ctx_mut(), card);
                card.owner = owner;
                for (name, amount) in enchantments {
                    if !crate::card::enchant::apply_static_enchantment(card, name, amount) {
                        return Err("unsupported Future of Potions upgrade enchantment");
                    }
                }
            }
        }
        Ok(cards)
    }

    pub(super) fn trade_future_potion(&mut self, option: EventOptionIdentity) -> Result<(), &'static str> {
        let event = self.current_event.as_ref().unwrap();
        let index = event.options.iter().position(|o| o.option_id == option).unwrap();
        let slot = (event.ints[index] >> 2) as usize;
        let id = self.run_ctx().potions.get(slot).copied().flatten()
            .ok_or("invalid Future of Potions offer")?;
        let kind = [CardType::Attack, CardType::Skill, CardType::Power][(event.ints[index] & 3) as usize];
        let rarity = match POTION_INFO[id.index()].rarity {
            PotionRarity::Common | PotionRarity::Token => CardRarity::Common,
            PotionRarity::Uncommon => CardRarity::Uncommon,
            _ => CardRarity::Rare,
        };
        // Discard the exact offered instance before populating the reward.
        self.run_ctx_mut().potions[slot] = None;
        self.current_event = None;
        let cards = self.generate_future_potion_cards(rarity, kind)?;
        self.reward = RewardState::default();
        self.reward.items.push(RewardItem {
            kind: RewardItemKind::Card { cards, closed: false, progress: Default::default(),
                reroll: Some(CardRewardRerollOptions::FuturePotions { rarity, kind }) },
            consumed: false, skippable: true, skip_completes: true,
            add_injury_after: false, neows_bones_final: false,
        });
        self.advance_rewards();
        Ok(())
    }

    pub(super) fn initialize_future_potions(&mut self) -> Result<(), &'static str> {
        // Original-DLL TheFutureOfPotions.cs:34-62,102-120: lazy map is
        // populated for ALL held potions, in inventory order, on first offer.
        // Frozen r7 events/act2.py:1349-1367 mirrors the same draws and top 3.
        let held: Vec<_> = self.run_ctx().potions.iter().enumerate()
            .filter_map(|(slot, id)| id.map(|id| (slot, id))).collect();
        if held.len() < 2 { return Err("Future of Potions needs two held potions"); }
        let event = self.current_event.as_mut().ok_or("Future of Potions event missing")?;
        for (i, (slot, id)) in held.into_iter().enumerate() {
            let count = match POTION_INFO[id.index()].rarity {
                PotionRarity::Common | PotionRarity::Token => 2,
                _ => 3,
            };
            let card_type = event.rng.next_int_exclusive(0, count);
            if i < 3 {
                event.ints[i] = ((slot as i32) << 2) | card_type as i32;
                event.options.push(EventOption::new(["trade_0", "trade_1", "trade_2"][i],
                    "Trade", "Discard potion for upgraded card reward"));
            }
        }
        Ok(())
    }

    /// Typed inventory offer, never guessed from an ordinal or displayed label.
    pub fn observation_future_potion(&self, option: &str) -> Option<(usize, PotionId)> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::TheFutureOfPotions { return None; }
        let index = event.options.iter().position(|o| o.option_id.legacy_id() == option)?;
        let slot = (event.ints[index] >> 2) as usize;
        Some((slot, self.run_ctx().potions.get(slot).copied().flatten()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> RunManager {
        let mut run = RunManager::new(3006115, RunOptions::default());
        run.run_ctx_mut().total_floor = 4;
        run.run_ctx_mut().potions.push(Some(PotionId::Bonebrew));
        run.run_ctx_mut().potions.push(Some(PotionId::Foulpotion));
        run.current_event = Some(EventState::new(EventId::TheFutureOfPotions, Rng::new(123)));
        run.initialize_future_potions().unwrap();
        run.phase = RunPhase::Event;
        run.unported = None;
        run
    }

    #[test]
    fn trades_use_retained_filter_discard_one_slot_and_offer_upgraded_distinct_cards() {
        for option in ["trade_0", "trade_1"] {
            let mut run = menu();
            let (slot, potion) = run.observation_future_potion(option).unwrap();
            let index = run.current_event.as_ref().unwrap().options.iter()
                .position(|o| o.option_id == option).unwrap();
            let kind = [CardType::Attack, CardType::Skill, CardType::Power]
                [(run.current_event.as_ref().unwrap().ints[index] & 3) as usize];
            let before = run.run_ctx().potions.clone();
            let counter = run.run_ctx().rngs.rewards.counter;
            let action = run.available_actions().into_iter()
                .find(|a| a.option_id == Some(option)).unwrap();
            run.take_action(action).unwrap();
            assert_eq!(run.phase, RunPhase::CardReward);
            assert!(run.current_event.is_none());
            assert_eq!(run.run_ctx().rngs.rewards.counter, counter + 3);
            for (i, id) in before.iter().enumerate() {
                assert_eq!(run.run_ctx().potions[i], if i == slot { None } else { *id });
            }
            let RewardItemKind::Card { cards, .. } = &run.reward.items[0].kind else { panic!("card reward"); };
            assert_eq!(cards.len(), 3);
            assert!(cards.iter().all(|c| c.upgraded && c.card_type == kind));
            let rarity = POTION_INFO[potion.index()].rarity;
            assert!(cards.iter().all(|c| c.rarity == match rarity {
                PotionRarity::Common | PotionRarity::Token => CardRarity::Common,
                PotionRarity::Uncommon => CardRarity::Uncommon,
                _ => CardRarity::Rare,
            }));
            assert_eq!(cards.iter().map(|c| c.id).collect::<std::collections::HashSet<_>>().len(), 3);
            run.skip_card_reward().unwrap();
            assert_eq!(run.phase, RunPhase::MapChoice);
        }
    }

    #[test]
    fn invalid_trade_does_not_discard_or_consume_reward_rng() {
        let mut run = menu();
        let potions = run.run_ctx().potions.clone();
        let counter = run.run_ctx().rngs.rewards.counter;
        assert!(run.take_event_choice("trade_2").is_err());
        assert_eq!(run.run_ctx().potions, potions);
        assert_eq!(run.run_ctx().rngs.rewards.counter, counter);
        assert!(run.current_event.is_some());
    }
}
