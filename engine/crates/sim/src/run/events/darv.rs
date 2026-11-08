//! Darv.cs:151-198: source-ordered singleton offers, shuffle, optional DustyTome.
use super::*;

const RELICS: &[(RelicId, &str)] = &[
    (RelicId::Astrolabe, "Astrolabe"), (RelicId::BlackStar, "Black Star"),
    (RelicId::CallingBell, "Calling Bell"), (RelicId::EmptyCage, "Empty Cage"),
    (RelicId::PandorasBox, "Pandoras Box"), (RelicId::RunicPyramid, "Runic Pyramid"),
    (RelicId::SneckoEye, "Snecko Eye"), (RelicId::Ectoplasm, "Ectoplasm"),
    (RelicId::Sozu, "Sozu"), (RelicId::PhilosophersStone, "Philosophers Stone"),
    (RelicId::VelvetChoker, "Velvet Choker"),
];
const TRANSCENDENCE: &[&str] = &["BREAK", "SUPPRESS", "PROTECTOR", "METEOR_SHOWER", "QUADCAST"];
impl RunManager {
    pub(super) fn initialize_darv(&mut self) -> Result<(), &'static str> {
        self.heal_before_ancient_event()?;
        let mut event = self.current_event.take().ok_or("Darv event missing")?;
        let character=self.character;
        let act=self.current_act;
        let multiplayer=self.is_multiplayer();
        Self::populate_darv_options(&mut event,character,act,multiplayer,&mut self.run_ctx_mut().rngs.rewards)?;
        self.current_event=Some(event);Ok(())
    }
    pub(in crate::run) fn populate_darv_options(event:&mut EventState,character:crate::character::Character,act:usize,multiplayer:bool,
        rewards:&mut Rng)->Result<(),&'static str>{
        // The native run profile has no deck-clearing run modifiers. Darv.cs:187
        // therefore includes Pandora's Box; the remaining filters use act index.
        let mut source = Vec::new();
        for &(id, label) in RELICS {
            if matches!(id, RelicId::Ectoplasm | RelicId::Sozu) && act != 1 { continue; }
            if matches!(id, RelicId::PhilosophersStone | RelicId::VelvetChoker) && act < 1 { continue; }
            event.rng.choice_index(1); // C# NextItem executes even for singleton sets.
            source.push((id, label));
        }
        event.rng.shuffle(&mut source);
        let dusty = event.rng.next_bool();
        source.truncate(if dusty { 2 } else { 3 });
        event.cards.clear();
        if dusty {
            // DustyTome.cs:50-56 / ArchaicTooth.cs:26-51. Setup consumes the
            // player's Rewards stream, NOT the ancient event's RNG.
            let pool: Vec<_> = super::super::card_pools::cards(character.into(), multiplayer)?
                .into_iter().filter(|id| !TRANSCENDENCE.contains(&id.name()))
                .filter(|id| card_info(*id, false).is_some_and(|c| c.rarity == CardRarity::Ancient)).collect();
            if pool.is_empty() { return Err("Darv DustyTome ancient card pool empty"); }
            let card = pool[rewards.choice_index(pool.len())];
            event.cards.push(card);
            source.push((RelicId::DustyTome, "Dusty Tome"));
        }
        event.options.clear(); event.relics.clear();
        for (option, (id, label)) in ["relic_1", "relic_2", "relic_3"].into_iter().zip(source) {
            event.relics.push(id);
            event.options.push(EventOption::new(option, label, "Gain a boss relic"));
        }
        Ok(())
    }

    pub(super) fn choose_darv<S: crate::run::events::native_identity::EventOptionSelector>(&mut self, event: EventState, option: S) -> Result<(), &'static str> {
        let option = option.resolve(&event);
        let index = event.options.iter().position(|o| o.option_id == option).ok_or("Darv option missing")?;
        let id = *event.relics.get(index).ok_or("Darv relic missing")?;
        self.phase = RunPhase::MapChoice;
        self.unported = None;
        self.remove_relic_from_bags(id)?;
        let mut relic = shop::obtained_relic(self.run_ctx(),id);
        if id == RelicId::DustyTome { relic.ancient_card = event.cards.first().copied(); }
        self.run_ctx_mut().relics.push(relic);
        match id {
            RelicId::Astrolabe | RelicId::EmptyCage => {
                // Astrolabe.cs:20-28 / EmptyCage.cs:18-23.
                let (candidates, requested, effect, prompt, source) = if id == RelicId::Astrolabe {
                    (self.transformable_deck_indices(), 3, RunChoiceEffect::TransformUpgrade,
                        // r7's selector wire names both operations; C# uses
                        // TransformSelectionPrompt and upgrades before Transform.
                        "Choose 3 cards to transform and upgrade", "relic:ASTROLABE")
                } else {
                    (self.removable_deck_indices(), 2, RunChoiceEffect::Remove { damage_after: 0 },
                        "Choose 2 cards to remove", "relic:EMPTY_CAGE")
                };
                let count = candidates.len().min(requested);
                if count > 0 {
                    self.begin_run_card_choice(source, prompt, &candidates, count, count, false, effect, ChoiceResume::MapChoice);
                    // CardSelectCmd.cs:623-625: forced all-candidate selections
                    // resolve without presenting an unnecessary selector.
                    if candidates.len() <= requested {
                        let choice = self.pending_run_choice.take().unwrap();
                        self.apply_run_choice(choice, &(0..count).collect::<Vec<_>>())?;
                    }
                }
            }
            RelicId::PandorasBox => {
                // PandorasBox.cs:21-25: removable Basic Strikes/Defends only.
                let removable = self.removable_deck_indices();
                let ids: Vec<_> = removable.into_iter().filter_map(|i| {
                    let c = &self.run_ctx().deck[i];
                    (c.rarity == CardRarity::Basic && (c.has_tag(CardTag::Strike) || c.has_tag(CardTag::Defend)))
                        .then_some(c.instance_id)
                }).collect();
                self.transform_deck_cards_batch(&ids)?;
            }
            RelicId::CallingBell => {
                // CallingBell.cs:27-31,53-63: curse first, then a populated
                // Common/Uncommon/Rare RewardsSet (not three random rarities).
                self.add_card_to_deck(CardId::CurseOfTheBell)?;
                let mut reward = RewardState::default();
                reward.stay_in_container = true;
                for rarity in 0..3 {
                    let relic = self.pull_player_relic(Some(rarity), false);
                    reward.items.push(RewardItem { kind: RewardItemKind::Relic(relic),
                        consumed: false, skippable: true, skip_completes: true,
                        add_injury_after: false, neows_bones_final: false });
                }
                self.reward = reward;
                self.phase = RunPhase::CardReward;
            }
            RelicId::DustyTome => {
                // DustyTome.cs:58-63: use the previously configured card; never
                // roll again when selecting the offer.
                let id = event.cards.first().copied().ok_or("Darv DustyTome setup missing")?;
                let iid = self.run_ctx_mut().new_card_id();
                let mut card = Card::from_factory(id, false, iid).ok_or("Darv DustyTome card factory")?;
                shop::upgrade_template(self.run_ctx_mut(), &mut card);
                self.add_card_instance_to_deck(card)?;
            }
            // These models have no AfterObtained override. Their combat/reward
            // hooks already use the audited owner dispatch.
            RelicId::BlackStar | RelicId::RunicPyramid | RelicId::SneckoEye |
            RelicId::Ectoplasm | RelicId::Sozu | RelicId::PhilosophersStone | RelicId::VelvetChoker => {}
            _ => return Err("Darv invalid relic"),
        }
        Ok(())
    }

    pub(super) fn apply_astrolabe_choice(&mut self, ids: &[u32]) -> Result<(), &'static str> {
        // C# Astrolabe.cs:23-28 upgrades each replacement before deck entry.
        // Frozen run_state.py:974-987 creates all replacements before allocating
        // upgrade templates. Reserve those wire-only identities first without
        // delaying C#'s upgrade-before-insertion callbacks.
        let replacement_ids: Vec<_> = ids.iter().map(|_| self.run_ctx_mut().new_card_id()).collect();
        let mut rng = self.run_ctx().rngs.niche.clone();
        for (&id, iid) in ids.iter().zip(replacement_ids) {
            self.transform_deck_card_with_rng_upgrade(id, &mut rng, Some(iid))?;
        }
        self.run_ctx_mut().rngs.niche = rng;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn astrolabe_with_eggs_upgrades_replacements_before_deck_entry() {
        for id in [RelicId::FrozenEgg, RelicId::MoltenEgg, RelicId::ToxicEgg] {
            for (owner, enabled) in [(0, true), (1, true), (0, false)] {
                let mut mgr = RunManager::new(4100000, RunOptions::default());
                mgr.unported = None;
                mgr.current_act = 1;
                mgr.run_ctx_mut().act_index = 1;
                mgr.run_ctx_mut().total_floor = 18;
                let mut egg = Relic::new(id); egg.owner = owner; egg.enabled = enabled;
                mgr.run_ctx_mut().relics.push(egg);
                let mut event = EventState::new(crate::identity::EventId::Darv, Rng::new(1));
                event.relics.push(RelicId::Astrolabe);
                event.options.push(EventOption::new("relic_1", "Astrolabe", "Gain a boss relic"));
                mgr.current_event = Some(event); mgr.phase = RunPhase::Event;
                let action = mgr.available_actions().into_iter().find(|a| a.option_id == Some("relic_1")).unwrap();
                mgr.take_action(action).unwrap();
                let choice=mgr.pending_run_choice.take().unwrap();
                let removed:Vec<_>=choice.options.iter().take(3).map(|c|c.instance_id).collect();
                mgr.apply_run_choice(choice,&[0,1,2]).unwrap();
                assert!(mgr.unported.is_none());
                assert!(mgr.run_ctx().deck.iter().all(|c|!removed.contains(&c.instance_id)));
                assert!(mgr.run_ctx().deck.iter().rev().take(3).all(|c|c.upgraded));
            }
        }
    }

    #[test]
    fn mandatory_small_decks_resolve_without_a_spurious_selector() {
        for id in [RelicId::Astrolabe, RelicId::EmptyCage] {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.run_ctx_mut().deck.truncate(2);
            let mut event = EventState::new(crate::identity::EventId::Darv, Rng::new(1));
            event.relics.push(id);
            event.options.push(EventOption::new("relic_1", "Relic", "Gain a boss relic"));
            mgr.choose_darv(event, "relic_1").unwrap();
            assert!(mgr.pending_run_choice.is_none());
            if id == RelicId::EmptyCage { assert!(mgr.run_ctx().deck.is_empty()); }
            else { assert_eq!(mgr.run_ctx().deck.len(), 2); assert!(mgr.run_ctx().deck.iter().all(|c| c.upgraded)); }
        }
    }

    fn pick(id: RelicId) -> RunManager {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.unported = None;
        let mut event = EventState::new(crate::identity::EventId::Darv, Rng::new(1));
        event.relics.push(id);
        event.options.push(EventOption::new("relic_1", "Relic", "Gain a boss relic"));
        mgr.current_event = Some(event); mgr.phase = RunPhase::Event;
        mgr.take_event_choice("relic_1").unwrap();
        mgr
    }

    #[test]
    fn cage_astrolabe_bell_and_pandora_dispatch_their_actual_pickup_effects() {
        let cage = pick(RelicId::EmptyCage);
        let choice = cage.pending_run_choice.as_ref().unwrap();
        assert_eq!((choice.min_choices, choice.max_choices), (2, 2));
        let mut astro = pick(RelicId::Astrolabe);
        let choice = astro.pending_run_choice.take().unwrap();
        assert_eq!((choice.min_choices, choice.max_choices), (3, 3));
        let old: Vec<_> = choice.options.iter().take(3).map(|c| c.instance_id).collect();
        let count = astro.run_ctx().deck.len();
        let next_id = astro.run_ctx().next_card_id;
        astro.apply_run_choice(choice, &[0, 1, 2]).unwrap();
        assert_eq!(astro.run_ctx().deck.len(), count);
        assert!(astro.run_ctx().deck.iter().all(|c| !old.contains(&c.instance_id)));
        assert!(astro.run_ctx().deck.iter().rev().take(3).all(|c| c.upgraded));
        assert_eq!(astro.run_ctx().deck.iter().rev().take(3).map(|c| c.instance_id).collect::<Vec<_>>(),
            vec![next_id + 3, next_id + 2, next_id + 1]);
        let bell = pick(RelicId::CallingBell);
        assert_eq!(bell.run_ctx().deck.last().unwrap().id, CardId::CurseOfTheBell);
        assert_eq!(bell.reward.items.len(), 3);
        for (item, rarity) in bell.reward.items.iter().zip([RelicRarity::Common, RelicRarity::Uncommon, RelicRarity::Rare]) {
            let RewardItemKind::Relic(id) = item.kind else { panic!("relic reward required") };
            assert_eq!(RELIC_INFO[id.index()].rarity, rarity);
        }
        let pandora = pick(RelicId::PandorasBox);
        assert!(pandora.run_ctx().deck.iter().all(|c| c.rarity != CardRarity::Basic
            || !(c.has_tag(CardTag::Strike) || c.has_tag(CardTag::Defend))));
        assert!(pandora.run_ctx().deck.iter().any(|c| c.id == CardId::Bash));
    }

    #[test]
    fn dusty_tome_uses_the_offer_setup_without_a_second_rewards_draw() {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.unported = None;
        for seed in 0..64 {
            mgr.current_event = Some(EventState::new(crate::identity::EventId::Darv, Rng::new(seed)));
            mgr.initialize_darv().unwrap();
            if mgr.current_event.as_ref().unwrap().relics[2] != RelicId::DustyTome { continue; }
            let expected = mgr.current_event.as_ref().unwrap().cards[0];
            let counter = mgr.run_ctx().rngs.rewards.counter;
            mgr.take_event_choice("relic_3").unwrap();
            assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter);
            let card = mgr.run_ctx().deck.last().unwrap();
            assert_eq!(card.id, expected); assert!(card.upgraded);
            assert_eq!(mgr.run_ctx().relics.last().unwrap().ancient_card, Some(expected));
            return;
        }
        panic!("expected an offered Dusty Tome among these source RNG seeds");
    }

    #[test]
    fn singleton_draws_act_filters_shuffle_and_dusty_setup_use_their_own_streams() {
        for act in 0..3 {
            for seed in 0..32 {
                let mut mgr = RunManager::new(4100000, RunOptions::default());
                mgr.current_act = act;
                mgr.current_event = Some(EventState::new(crate::identity::EventId::Darv, Rng::new(seed)));
                let mut rng = Rng::new(seed);
                let mut allowed = RELICS.iter().copied().filter(|(id,_)|
                    (!matches!(id, RelicId::Ectoplasm | RelicId::Sozu) || act == 1)
                    && (!matches!(id, RelicId::PhilosophersStone | RelicId::VelvetChoker) || act >= 1)).collect::<Vec<_>>();
                for _ in &allowed { rng.choice_index(1); }
                rng.shuffle(&mut allowed);
                let dusty = rng.next_bool();
                mgr.initialize_darv().unwrap();
                let event = mgr.current_event.as_ref().unwrap();
                assert_eq!(event.rng.counter, rng.counter);
                assert_eq!(event.relics[..2], [allowed[0].0, allowed[1].0]);
                assert_eq!(event.relics[2], if dusty { RelicId::DustyTome } else { allowed[2].0 });
                assert_eq!(event.cards.len(), usize::from(dusty));
                assert!(event.cards.iter().all(|id| card_info(*id,false).unwrap().rarity == CardRarity::Ancient
                    && !TRANSCENDENCE.contains(&id.name())));
            }
        }
    }
}
