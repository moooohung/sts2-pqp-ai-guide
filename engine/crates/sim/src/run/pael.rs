//! C# Pael.GenerateInitialOptions and Pael relic AfterObtained hooks.
use super::*;

impl RunManager {
    pub(super) fn initialize_pael(&mut self) -> Result<(), &'static str> {
        self.heal_before_ancient_event()?;
        let mut event = self.current_event.take().ok_or("Pael event missing")?;
        Self::populate_pael_options(&mut event,&self.run_ctx().deck,
            self.removable_deck_indices().len(),self.has_event_pet());
        self.current_event=Some(event);
        Ok(())
    }

    pub(in crate::run) fn populate_pael_options(event:&mut EventState,deck:&[Card],removable:usize,has_pet:bool) {
        let first = [
            (RelicId::PaelsFlesh, "Paels Flesh"),
            (RelicId::PaelsHorn, "Paels Horn"),
            (RelicId::PaelsTears, "Paels Tears"),
        ];
        let a = first[event.rng.choice_index(first.len())];
        let mut second = vec![(RelicId::PaelsWing, "Paels Wing")];
        if deck.iter().filter(|c| Self::event_can_enchant(c, "Goopy")).count() >= 3 {
            second.push((RelicId::PaelsClaw, "Paels Claw"));
        }
        if removable >= 5 {
            second.push((RelicId::PaelsTooth, "Paels Tooth"));
        }
        // Pael.cs: list.AddRange(list), then add Growth. Preserve multiplicity.
        second.extend_from_within(..);
        second.push((RelicId::PaelsGrowth, "Paels Growth"));
        let b = second[event.rng.choice_index(second.len())];
        let mut third = vec![(RelicId::PaelsEye, "Paels Eye"), (RelicId::PaelsBlood, "Paels Blood")];
        if !has_pet {
            third.push((RelicId::PaelsLegion, "Paels Legion"));
        }
        let c = third[event.rng.choice_index(third.len())];
        event.options.clear();
        event.relics.clear();
        for (option, (id, label)) in ["relic_1", "relic_2", "relic_3"].into_iter().zip([a,b,c]) {
            event.relics.push(id);
            event.options.push(super::super::events::EventOption::new(option, label, "Gain a Pael relic"));
        }
    }

    pub(super) fn choose_pael(&mut self, event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        let index = event.options.iter().position(|o| o.option_id == option).ok_or("Pael option missing")?;
        let id = *event.relics.get(index).ok_or("Pael relic missing")?;
        self.remove_relic_from_bags(id)?;
        shop::attach_relic(self.run_ctx_mut(),id);
        self.current_event = None;
        self.unported = None;
        self.phase = RunPhase::MapChoice;
        match id {
            RelicId::PaelsHorn => {
                // PaelsHorn.AfterObtained: two newly created Relax cards.
                for _ in 0..2 { self.add_card_to_deck(CardId::Relax)?; }
            }
            RelicId::PaelsClaw => {
                // PaelsClaw.AfterObtained enchants ALL eligible cards at amount 1,
                // despite the hover-tip's amount 2 and the eligibility count 3.
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)| Self::event_can_enchant(c, "Goopy").then_some(i)).collect();
                for i in candidates {
                    if !crate::card::enchant::apply_static_enchantment(&mut self.run_ctx_mut().deck[i], "Goopy", 1) {
                        return Err("PaelsClaw enchantment capacity exceeded");
                    }
                }
            }
            RelicId::PaelsGrowth => {
                // PaelsGrowth.AfterObtained: one Clone enchantment, amount FOUR.
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)| Self::event_can_enchant(c, "Clone").then_some(i)).collect();
                if !candidates.is_empty() {
                    self.begin_run_card_choice("relic.paels_growth.pickup", "Choose a card to enchant with Clone", &candidates,
                        1, 1, false, RunChoiceEffect::Enchant { name: crate::identity::EnchantmentId::Clone, amount: 4 }, ChoiceResume::MapChoice);
                    if candidates.len() == 1 { self.resolve_run_choice(0)?; }
                }
            }
            RelicId::PaelsTooth => {
                // PaelsTooth.AfterObtained: removal selector additionally filters
                // IsUpgradable; stored-card ordering/storage uses the existing resolver.
                let removable = self.removable_deck_indices();
                let upgradable = self.upgradable_deck_indices();
                let candidates: Vec<_> = removable.into_iter().filter(|i| upgradable.contains(i)).collect();
                let count = candidates.len().min(5);
                if count > 0 && candidates.len() <= 5 {
                    // CardSelectCmd.FromDeckGeneric returns all candidates
                    // without a selector when count <= MinSelect.
                    self.run_ctx_mut().paels_tooth_store_selected(&candidates);
                } else if count > 0 {
                    // PaelsTooth.cs:82-90 keeps the removal selector semantics.
                    // Wire prompt follows frozen shop_event.py:2703.
                    self.begin_run_card_choice("relic.paels_tooth.pickup", "Choose 5 cards for Pael's Tooth", &candidates,
                        count, count, false, RunChoiceEffect::StorePaelsTooth, ChoiceResume::MapChoice);
                } else {
                    self.run_ctx_mut().owned_relics_mut(crate::combat::PLAYER).last().unwrap().enabled = false;
                }
            }
            // Flesh, Tears, Blood and Wing have no upon-obtain gameplay mutation.
            // Eye.AfterObtained is outside combat (false), matching new flags.
            // Legion.AfterObtained draws skin from a separate relic-local RNG,
            // not any shared gameplay RNG, and summons only inside combat.
            _ => {}
        }
        if self.pending_run_choice.is_some() {
            // PaelsGrowth.AfterObtained / PaelsTooth.cs:82-90 await the
            // deck selector inside the ancient's RelicOption callback.
            // This is still the event room, not a RewardsCmd screen.
            self.current_event = Some(event);
            self.phase = RunPhase::Event;
        }
        Ok(())
    }
}
