//! N-R run listeners kept out of the shared manager's dispatch bodies.
use super::*;

impl RunManager {
    pub(super) fn apply_byrdpip_pickup(&mut self)->Result<(), &'static str> {
        // Byrdpip.AfterObtained selects a cosmetic skin on its OWN model RNG.
        // This never advances run streams. Persistent combat summons already
        // use Byrdpip.BeforeCombatStart; acquisition here is outside combat.
        let mut cosmetic=Rng::for_model(self.run_ctx().rngs.seed,0,"BYRDPIP",0);
        let _skin=cosmetic.next_int_exclusive(0,4);
        let eggs:Vec<_>=self.run_ctx().deck.iter()
            .filter(|c|c.id==CardId::ByrdonisEgg).map(|c|c.instance_id).collect();
        for original in eggs {
            let index=self.run_ctx().deck.iter().position(|c|c.instance_id==original)
                .ok_or("Byrdpip egg missing")?;
            let owner=self.run_ctx().deck[index].owner;
            let iid=self.run_ctx_mut().new_card_id();
            let mut replacement=Card::from_factory(CardId::ByrdSwoop,false,iid)
                .ok_or("Byrd Swoop factory missing")?;
            replacement.owner=owner;
            replacement.floor_added_to_deck=Some(self.run_ctx().total_floor);
            self.run_ctx_mut().remove_run_deck_card(index);
            self.add_card_instance_to_deck(replacement)?;
        }
        Ok(())
    }
    /// Shared card population for the base room reward, Prayer Wheel and Orrery.
    pub(super) fn make_nr_card_reward(&mut self, room: Option<RoomType>) -> Option<RewardItem> {
        let mut chosen = ArrayVec::new();
        let mut cards = ArrayVec::new();
        for _ in 0..3 {
            let rarity = match room {
                Some(room) => self.roll_card_rarity(room),
                None => self.roll_base_card_rarity(),
            };
            let card = self.generate_reward_card(rarity, &chosen)?;
            chosen.push(card.id);
            cards.push(card);
        }
        if let Some(room) = room {
            self.apply_encounter_reward_relic_hooks(&mut cards, room);
        } else {
            self.apply_card_reward_relic_hooks(&mut cards, true);
        }
        if self.unported.is_some() { return None; }
        Some(RewardItem {
            kind: RewardItemKind::Card {
                cards, closed: false,
                reroll: Some(match room {
                    Some(room) => CardRewardRerollOptions::Encounter(room),
                    None => CardRewardRerollOptions::Noncombat {
                        character: self.character.into(), count: 3, rarity: None, roll_upgrade: true,
                        modify_pool: true,
                    },
                }),
                progress: Default::default(),
            },
            consumed: false, skippable: true, skip_completes: room.is_none(),
            add_injury_after: false, neows_bones_final: false,
        })
    }

    pub(super) fn apply_prayer_wheel_rewards(&mut self, room: RoomType) {
        // Models/Relics/PrayerWheel.cs:14-25: Owner only, Monster only.
        if room != RoomType::Monster { return; }
        let count = self.run_ctx().owned_relics(crate::combat::PLAYER)
            .filter(|r| r.id == RelicId::PrayerWheel && r.enabled).count();
        for _ in 0..count {
            let Some(item) = self.make_nr_card_reward(Some(RoomType::Monster)) else { return; };
            self.reward.items.push(item);
        }
    }

    pub(super) fn offer_nr_pickup(&mut self, relic: RelicId) -> Result<bool, &'static str> {
        match relic {
            RelicId::Cauldron => {
                // Cauldron.AfterObtained offers five ordinary PotionRewards,
                // populated before the custom reward container is shown.
                self.reward = RewardState::default();
                self.reward.stay_in_container = true;
                for _ in 0..5 {
                    let Some(potion) = self.roll_potion() else { return Ok(true); };
                    self.reward.items.push(RewardItem {
                        kind: RewardItemKind::Potion(potion), consumed: false,
                        skippable: true, skip_completes: true, add_injury_after: false,
                        neows_bones_final: false,
                    });
                }
                self.phase = RunPhase::CardReward;
            }
            RelicId::Byrdpip => self.apply_byrdpip_pickup()?,
            RelicId::DollysMirror => {
                // C# Filter excludes Quest only; curses, enchanted and
                // upgraded cards remain valid. No random selector ordering.
                let candidates:Vec<_>=self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)|(c.card_type!=CardType::Quest).then_some(i)).collect();
                if candidates.is_empty() {return Ok(true);}
                let resume=if self.phase==RunPhase::Shop {ChoiceResume::Shop}
                    else {ChoiceResume::MapChoice};
                self.begin_run_card_choice("relic:DOLLYS_MIRROR","Choose a card to duplicate",
                    &candidates,1,1,false,RunChoiceEffect::Duplicate,resume);
            }
            RelicId::RoyalStamp => {
                // RoyalStamp.AfterObtained: shuffle the FULL eligible list on
                // Niche before the mandatory one-card enchant selector.
                let mut candidates:Vec<_>=self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)|Self::event_can_enchant(c,"RoyallyApproved").then_some(i))
                    .collect();
                self.run_ctx_mut().rngs.niche.shuffle(&mut candidates);
                if candidates.is_empty() {return Ok(true);}
                let resume=if self.phase==RunPhase::Shop {ChoiceResume::Shop}
                    else {ChoiceResume::MapChoice};
                self.begin_run_card_choice("relic:ROYAL_STAMP","Choose a card to enchant",
                    &candidates,1,1,false,RunChoiceEffect::Enchant {
                        name:crate::identity::EnchantmentId::RoyallyApproved,amount:1},resume);
            }
            RelicId::PunchDagger => {
                // C# PunchDagger.AfterObtained: one eligible deck attack,
                // mandatory selection, enchanted with Momentum 5.
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i, card)| Self::event_can_enchant(card, "Momentum").then_some(i))
                    .collect();
                if candidates.is_empty() { return Ok(true); }
                let resume = if self.phase == RunPhase::Shop {
                    ChoiceResume::Shop
                } else {
                    ChoiceResume::MapChoice
                };
                self.begin_run_card_choice("relic:PUNCH_DAGGER", "Choose a card to enchant",
                    &candidates, 1, 1, false,
                    RunChoiceEffect::Enchant { name: crate::identity::EnchantmentId::Momentum, amount: 5 }, resume);
            }
            RelicId::Kifuda => {
                // Kifuda.cs: noncancelable manual selector, up to three deck
                // cards that Adroit can enchant, then Adroit(3) on each.
                // Frozen r7 models the zero-minimum selection explicitly.
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i, card)| Self::event_can_enchant(card, "Adroit").then_some(i))
                    .collect();
                if candidates.is_empty() {
                    return Ok(true);
                }
                let resume = if self.phase == RunPhase::Shop {
                    ChoiceResume::Shop
                } else {
                    ChoiceResume::MapChoice
                };
                let max = candidates.len().min(3);
                self.begin_run_card_choice("relic:KIFUDA", "Choose 3 cards to enchant with Adroit",
                    &candidates, 0, max, true,
                    RunChoiceEffect::Enchant { name: crate::identity::EnchantmentId::Adroit, amount: 3 }, resume);
            }
            RelicId::Orrery => {
                // Models/Relics/Orrery.cs:19-27 / Runs/CardCreationOptions.cs:29:
                // five separate Other/RegularEncounter rewards, normal upgrade
                // rolls (no NoUpgradeRoll flag), all populated before offering.
                self.reward = RewardState::default();
                let mut reward = RewardState::default();
                reward.stay_in_container = true;
                for _ in 0..5 {
                    let Some(item) = self.make_nr_card_reward(None) else { return Ok(true); };
                    reward.items.push(item);
                }
                self.reward = reward;
                self.phase = RunPhase::CardReward;
            }
            RelicId::PreservedFog => {
                // Models/Relics/PreservedFog.cs:20-26: remove three eligible
                // cards, then add Folly through the ordinary deck-entry hooks.
                let candidates = self.removable_deck_indices();
                let count = candidates.len().min(3);
                if count == 0 {
                    self.add_card_to_deck(CardId::Folly)?;
                } else {
                    self.begin_run_card_choice("relic.preserved_fog.pickup", "Choose 3 cards to remove", &candidates,
                        count, count, false, RunChoiceEffect::RemoveAndAddCard(CardId::Folly),
                        ChoiceResume::MapChoice);
                }
                self.unported = None;
            }
            _ => return Ok(false),
        }
        // CardSelectCmd.FromDeckGeneric / FromDeckForEnchantment return all
        // candidates immediately when count <= MinSelect. Kifuda has min=0,
        // so its nonempty optional selector remains manual.
        let automatic=self.pending_run_choice.as_ref().is_some_and(|c|
            matches!(c.wire_source,Some("relic:DOLLYS_MIRROR"|"relic:ROYAL_STAMP"|
                "relic:PUNCH_DAGGER"|"relic.preserved_fog.pickup"))
                && c.options.len()<=c.min_choices);
        if automatic {
            let choice=self.pending_run_choice.take().unwrap();
            let indices:Vec<_>=(0..choice.options.len()).collect();
            self.apply_run_choice(choice,&indices)?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod cauldron_tests {
    use super::*;

    #[test]
    fn cauldron_pickup_offers_five_potions_and_returns_to_shop() {
        let mut run = RunManager::new(42, RunOptions::default());
        run.unported = None;
        run.phase = RunPhase::Shop;
        run.run_ctx_mut().room_type = RoomType::Shop;
        let before = run.run_ctx().rngs.rewards.counter;
        run.obtain_event_relic(RelicId::Cauldron).unwrap();
        assert_eq!(run.unported, None);
        assert_eq!(run.phase(), RunPhase::CardReward);
        assert_eq!(run.reward.items.len(), 5);
        assert!(run.reward.items.iter().all(|item| matches!(item.kind, RewardItemKind::Potion(_))));
        assert_eq!(run.run_ctx().rngs.rewards.counter - before, 10);
        run.skip_reward_set().unwrap();
        assert_eq!(run.phase(), RunPhase::Shop);
    }
}
