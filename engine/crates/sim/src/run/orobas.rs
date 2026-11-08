//! C# Orobas.GenerateInitialOptions and the offered relics' AfterObtained hooks.
use super::*;

// ModelDb.AllCharacters / UnlockState.Characters, fully unlocked profile.
pub(in crate::run) const CHARACTERS: [crate::character::Character; 5] = [crate::character::Character::Ironclad, crate::character::Character::Silent, crate::character::Character::Regent, crate::character::Character::Necrobinder, crate::character::Character::Defect];
pub(in crate::run) const STARTER_CARDS: [(&str, &str); 5] = [
    ("BASH", "BREAK"), ("NEUTRALIZE", "SUPPRESS"), ("UNLEASH", "PROTECTOR"),
    ("FALLING_STAR", "METEOR_SHOWER"), ("DUALCAST", "QUADCAST"),
];

pub(in crate::run) fn upgraded_starter_relic(id:RelicId)->RelicId {
    match id {
        RelicId::BurningBlood=>RelicId::BlackBlood,
        RelicId::RingOfTheSnake=>RelicId::RingOfTheDrake,
        RelicId::DivineRight=>RelicId::DivineDestiny,
        RelicId::BoundPhylactery=>RelicId::PhylacteryUnbound,
        RelicId::CrackedCore=>RelicId::InfusedCore,
        _=>RelicId::Circlet,
    }
}

impl RunManager {
    /// C# TouchOfOrobas.SetupForPlayer's two public offer identities.
    pub fn observation_orobas_starter_swap(&self,option:&str)->Option<(RelicId,RelicId)> {
        if self.observation_ancient_relic(option)!=Some(RelicId::TouchOfOrobas) {return None;}
        let starter=self.run_ctx().owned_relics(crate::combat::PLAYER)
            .find(|r|RELIC_INFO[r.id.index()].rarity==RelicRarity::Starter)?.id;
        Some((starter,upgraded_starter_relic(starter)))
    }
    pub(super) fn initialize_orobas(&mut self) -> Result<(), &'static str> {
        self.heal_before_ancient_event()?;
        self.initialize_orobas_options()
    }

    pub(super) fn heal_before_ancient_event(&mut self) -> Result<(), &'static str> {
        // C# AncientEventModel.BeforeEventStarted / Creature.SetCurrentHpInternal:
        // heal missing HP (80% at WearyTraveler, ascension 2), truncating decimals.
        let missing = self.run_ctx().max_hp - self.run_ctx().hp;
        let heal = if self.options.ascension >= 2 { missing * 4 / 5 } else { missing };
        self.apply_event_effect(EventEffect::Heal(heal))?;
        Ok(())
    }

    fn initialize_orobas_options(&mut self) -> Result<(), &'static str> {
        let mut event = self.current_event.take().ok_or("Orobas event missing")?;
        Self::populate_orobas_options(&mut event,self.character,&self.run_ctx().deck,&self.run_ctx().relics);
        self.current_event=Some(event);Ok(())
    }
    pub(in crate::run) fn populate_orobas_options(event:&mut EventState,owner_character:crate::character::Character,deck:&[Card],relics:&[Relic]){
        let others: Vec<_> = CHARACTERS.iter().copied().filter(|&c| c != owner_character).collect();
        let character = others[event.rng.choice_index(others.len())];
        event.ints[0] = CHARACTERS.iter().position(|&c| c == character).unwrap() as i32;
        let mut first = vec![(RelicId::ElectricShrymp, "Electric Shrymp"), (RelicId::GlassEye, "Glass Eye")];
        first.push(if event.rng.next_float(1.0) < 0.3333333_f32 as f64 {
            (RelicId::PrismaticGem, "Prismatic Gem")
        } else { (RelicId::SeaGlass, "Sea Glass") });
        let second = [(RelicId::AlchemicalCoffer, "Alchemical Coffer"),
            (RelicId::Driftwood, "Driftwood"), (RelicId::RadiantPearl, "Radiant Pearl"),
            (RelicId::SandCastle, "Sand Castle")];
        let a = first[event.rng.choice_index(first.len())];
        let b = second[event.rng.choice_index(second.len())];
        // C# TouchOfOrobas.SetupForPlayer / ArchaicTooth.SetupForPlayer.
        let mut third = Vec::new();
        if relics.iter().any(|r| RELIC_INFO[r.id.index()].rarity == RelicRarity::Starter) {
            third.push((RelicId::TouchOfOrobas, "Touch Of Orobas"));
        }
        if deck.iter().any(|c| STARTER_CARDS.iter().any(|&(from, _)| c.id.name() == from)) {
            third.push((RelicId::ArchaicTooth, "Archaic Tooth"));
        }
        event.options.clear();
        event.relics.clear();
        for (option, (id, label)) in ["relic_1", "relic_2"].into_iter().zip([a, b]) {
            event.relics.push(id);
            event.options.push(super::super::events::EventOption::new(option, label, "Gain a relic"));
        }
        let index = event.rng.choice_index(third.len().max(1)); // empty pool becomes C#'s singleton Locked option
        if let Some(&(id, label)) = third.get(index) {
            event.relics.push(id);
            event.options.push(super::super::events::EventOption::new("relic_3", label, "Gain a relic"));
        } else {
            event.options.push(super::super::events::EventOption::new("relic_3", "Locked", "No valid relic").disabled());
        }
    }

    pub(super) fn offer_card_grid(&mut self, source: &'static str, cards: Vec<Card>, min: usize, max: usize,
        allow_skip: bool, prompt: &'static str)
    {
        let options = cards.iter().map(|card| super::super::events::RunChoiceCard {
            instance_id: card.instance_id, card_id: card.id, source_pile: "reward",
        }).collect();
        assert!(!source.is_empty(), "selector site must have a stable name");
        self.pending_run_choice = Some(RunCardChoice {
            wire_source: Some(source),
            prompt, options, selected: Vec::new(), min_choices: min, max_choices: max,
            allow_skip, single_select: false, effect: RunChoiceEffect::AddCards,
            resume: ChoiceResume::MapChoice, reward_cards: cards,
        });
        self.phase = RunPhase::CardReward;
    }

    pub(super) fn choose_orobas(&mut self, event: EventState, option: crate::run::events::EventOptionIdentity) -> Result<(), &'static str> {
        let index = event.options.iter().position(|o| o.option_id == option).ok_or("Orobas option missing")?;
        let id = *event.relics.get(index).ok_or("Orobas option locked")?;
        self.current_event = None;
        self.unported = None;
        self.phase = RunPhase::MapChoice;
        self.remove_relic_from_bags(id)?;
        let obtained=shop::attach_relic(self.run_ctx_mut(),id);
        match id {
            RelicId::ElectricShrymp => {
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter(|(_, c)| Self::event_can_enchant(c, "Imbued")).map(|(i, _)| i).collect();
                if !candidates.is_empty() {
                    self.begin_run_card_choice("event.orobas.enchant", "Choose a card to enchant", &candidates, 1, 1, false,
                        RunChoiceEffect::Enchant { name: crate::identity::EnchantmentId::Imbued, amount: 1 }, ChoiceResume::MapChoice);
                }
            }
            RelicId::GlassEye => {
                self.reward = RewardState::default();
                for rarity in [CardRarity::Common, CardRarity::Common, CardRarity::Uncommon,
                    CardRarity::Uncommon, CardRarity::Rare]
                {
                    let mut cards: ArrayVec<Card, 4> = self.generate_uniform_cards(3, self.character.into(), rarity, false, true)?
                        .into_iter().collect();
                    self.apply_card_reward_relic_hooks(&mut cards, true);
                    self.reward.items.push(RewardItem {
                        kind: RewardItemKind::Card { cards, closed: false,
                            progress: Default::default(),
                            reroll: Some(CardRewardRerollOptions::Uniform {
                                character: self.character.into(), count: 3, rarity,
                            }) }, consumed: false,
                        skippable: true, skip_completes: false, add_injury_after: false, neows_bones_final: false,
                    });
                }
                self.advance_rewards();
            }
            RelicId::SeaGlass => {
                let character = *CHARACTERS.get(event.ints[0] as usize).ok_or("SeaGlass character missing")?;
                let mut cards = Vec::new();
                for rarity in [CardRarity::Common, CardRarity::Uncommon, CardRarity::Rare] {
                    let mut group = self.generate_uniform_cards(5, character.into(), rarity, false, false)?;
                    for chunk in group.chunks_mut(3) {
                        let mut options: ArrayVec<Card, 4> = chunk.iter().cloned().collect();
                        self.apply_card_reward_relic_hooks(&mut options, false);
                        chunk.clone_from_slice(&options);
                    }
                    cards.extend(group);
                }
                self.offer_card_grid("event.orobas.add", cards, 0, 15, true, "Choose up to 15 cards");
            }
            RelicId::AlchemicalCoffer => {
                let mut pool = super::super::potion_pools::cards(self.character).ok_or("AlchemicalCoffer potion pool missing")?;
                let original_slots = self.run_ctx().max_potion_slots as usize;
                self.run_ctx_mut().max_potion_slots += 4;
                self.run_ctx_mut().potions.resize(original_slots + 4, None);
                for i in 0..4 {
                    let rarity = shop::roll_potion_rarity(&mut self.run_ctx_mut().rngs.combat_potion_generation);
                    let options: Vec<_> = pool.iter().copied().filter(|id| POTION_INFO[id.index()].rarity == rarity).collect();
                    if options.is_empty() { return Err("AlchemicalCoffer potion rarity empty"); }
                    let pick = self.run_ctx_mut().rngs.combat_potion_generation.choice_index(options.len());
                    let potion = options[pick];
                    pool.retain(|id| *id != potion);
                    if !self.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Sozu) {
                        self.run_ctx_mut().potions[original_slots + i] = Some(potion);
                    }
                }
            }
            RelicId::SandCastle => {
                let mut rng = self.run_ctx().rngs.niche.clone();
                self.upgrade_random_event_cards(&mut rng, 6);
                self.run_ctx_mut().rngs.niche = rng;
            }
            RelicId::TouchOfOrobas => {
                let index = self.run_ctx().relics.iter()
                    .position(|r| r.owner == crate::combat::PLAYER && RELIC_INFO[r.id.index()].rarity == RelicRarity::Starter)
                    .ok_or("TouchOfOrobas starter missing")?;
                let old = self.run_ctx().relics[index].id;
                let replacement = upgraded_starter_relic(old);
                self.remove_relic_from_bags(replacement)?;
                let replacement=shop::obtained_relic(self.run_ctx(),replacement);
                self.run_ctx_mut().relics[index] = replacement;
            }
            RelicId::ArchaicTooth => {
                let (index, target) = self.run_ctx().deck.iter().enumerate().find_map(|(i, c)|
                    STARTER_CARDS.iter().find(|&&(from, _)| c.id.name() == from).map(|&(_, to)| (i, to)))
                    .ok_or("ArchaicTooth starter missing")?;
                if !self.transformable_deck_indices().contains(&index) {
                    return Err("ArchaicTooth starter is not transformable");
                }
                let old = self.run_ctx_mut().remove_run_deck_card(index);
                let target = CardId::from_name(target).ok_or("ArchaicTooth target missing")?;
                let iid = self.run_ctx_mut().new_card_id();
                let mut replacement = Card::from_factory(target, old.upgraded, iid).ok_or("ArchaicTooth factory missing")?;
                replacement.owner = old.owner;
                for &(name, amount) in &old.enchantments {
                    if !crate::card::enchant::apply_static_enchantment(&mut replacement, name, amount) {
                        return Err("ArchaicTooth enchantment unsupported");
                    }
                }
                self.run_ctx_mut().relics.get_mut(obtained)
                    .ok_or("ArchaicTooth relic missing")?.saved_cards =
                    Some(Box::new([old, replacement.clone()]));
                // C# CardCmd.Transform removes the old deck card and appends the
                // replacement through ModifyCardBeingAddedToDeck.
                self.add_card_instance_to_deck(replacement)?;
            }
            RelicId::PrismaticGem | RelicId::RadiantPearl | RelicId::Driftwood => {}
            _ => return Err("Orobas relic unknown"),
        }
        Ok(())
    }
}
