//! Tezcatara.cs:40-95,181-195 and its offered relics' AfterObtained hooks.
use super::*;

const FIRST: &[(RelicId, &str)] = &[
    (RelicId::VeryHotCocoa, "Very Hot Cocoa"), (RelicId::YummyCookie, "Yummy Cookie"),
];
const SECOND: &[(RelicId, &str)] = &[
    (RelicId::BiiigHug, "Biiig Hug"), (RelicId::Storybook, "Storybook"),
    (RelicId::ToastyMittens, "Toasty Mittens"),
];
const THIRD: &[(RelicId, &str)] = &[
    (RelicId::GoldenCompass, "Golden Compass"), (RelicId::PumpkinCandle, "Pumpkin Candle"),
    (RelicId::ToyBox, "Toy Box"), (RelicId::SealOfGold, "Seal Of Gold"),
];

impl RunManager {
    pub(super) fn initialize_tezcatara(&mut self) -> Result<(), &'static str> {
        self.heal_before_ancient_event()?;
        let soup=self.run_ctx().deck.iter().any(|c|c.rarity==CardRarity::Basic&&c.has_tag(CardTag::Strike));
        Self::populate_tezcatara_options(self.current_event.as_mut().ok_or("Tezcatara event missing")?,soup);
        Ok(())
    }
    pub(in crate::run) fn populate_tezcatara_options(event:&mut EventState,soup:bool){
        let mut first = FIRST.to_vec();
        if soup {
            first.push((RelicId::NutritiousSoup, "Nutritious Soup"));
        }
        event.options.clear();
        event.relics.clear();
        for (option, pool) in [("comfort_1", first.as_slice()), ("comfort_2", SECOND), ("comfort_3", THIRD)] {
            let (id, label) = pool[event.rng.choice_index(pool.len())];
            event.relics.push(id);
            // Frozen shared.py Tezcatara uses these exact public menu strings.
            event.options.push(EventOption::new(option, label, "Gain a comfort relic"));
        }
    }

    pub(super) fn choose_tezcatara<S: crate::run::events::native_identity::EventOptionSelector>(&mut self, event: EventState, option: S) -> Result<(), &'static str> {
        let option = option.resolve(&event);
        let index = event.options.iter().position(|o| o.option_id == option).ok_or("Tezcatara option missing")?;
        let id = *event.relics.get(index).ok_or("Tezcatara relic missing")?;
        self.phase = RunPhase::MapChoice;
        self.unported = None;
        self.remove_relic_from_bags(id)?;
        let obtained=shop::attach_relic(self.run_ctx_mut(),id);
        match id {
            // These models have no pickup callback. PumpkinCandle.cs:52-55
            // Rekindle adds five to the raw model's zero (Relic::new is post-pickup).
            RelicId::VeryHotCocoa | RelicId::ToastyMittens | RelicId::SealOfGold | RelicId::PumpkinCandle => {}
            // Storybook.cs:17-21 creates one unupgraded BrightestFlame in Deck.
            RelicId::Storybook => self.add_card_to_deck(CardId::BrightestFlame)?,
            // NutritiousSoup.cs:21-36 enchants eligible Basic Strikes in place.
            RelicId::NutritiousSoup => {
                for card in &mut self.run_ctx_mut().deck {
                    if card.rarity == CardRarity::Basic && card.has_tag(CardTag::Strike)
                        && Self::event_can_enchant(card, "TezcatarasEmber")
                        && !crate::card::enchant::apply_static_enchantment(card, "TezcatarasEmber", 1) {
                        return Err("Tezcatara unsupported Ember enchantment");
                    }
                }
            }
            // YummyCookie.cs:70-77 / BiiigHug.cs:26-29: clamp the requested
            // four to eligible cards and preserve the shared selector protocol.
            RelicId::YummyCookie | RelicId::BiiigHug => {
                let candidates = if id == RelicId::YummyCookie { self.upgradable_deck_indices() }
                    else { self.removable_deck_indices() };
                let count = candidates.len().min(4);
                if count > 0 {
                    let (prompt, effect, source) = if id == RelicId::YummyCookie {
                        ("Choose 4 cards to upgrade", RunChoiceEffect::Upgrade, "relic:YUMMY_COOKIE")
                    } else {
                        ("Choose 4 cards to remove", RunChoiceEffect::Remove { damage_after: 0 }, "relic:BIIIG_HUG")
                    };
                    self.begin_run_card_choice(source, prompt, &candidates, count, count, false, effect, ChoiceResume::MapChoice);
                    // CardSelectCmd.cs:623-625 auto-selects when all eligible
                    // cards are required; do not invent another policy decision.
                    if candidates.len() <= 4 {
                        let choice = self.pending_run_choice.take().unwrap();
                        self.apply_run_choice(choice, &(0..count).collect::<Vec<_>>())?;
                    }
                }
            }
            RelicId::GoldenCompass => self.replace_with_tezcatara_golden_path(obtained)?,
            // ToyBox.cs:87-97 pulls all five before offering the set. The
            // container preserves wax provenance across skips and nested pickups.
            RelicId::ToyBox => {
                let mut rewards = RewardState::default();
                rewards.wax_relics = true;
                rewards.stay_in_container = true;
                for _ in 0..5 {
                    let relic = self.pull_player_relic(None, false);
                    rewards.items.push(RewardItem {
                        kind: RewardItemKind::Relic(relic), consumed: false,
                        skippable: true, skip_completes: true, add_injury_after: false,
                        neows_bones_final: false,
                    });
                }
                self.reward = rewards;
                self.phase = RunPhase::CardReward;
            }
            _ => return Err("Tezcatara invalid relic"),
        }
        Ok(())
    }

    fn replace_with_tezcatara_golden_path(&mut self,obtained:usize) -> Result<(), &'static str> {
        // GoldenCompass.cs:33-45 / GoldenPathActMap.cs:10-67.
        // GenerateMap still builds the standard graph before the replacement
        // hook, preserving the map RNG stream (run_state.py:1857-1872).
        let act = self.current_act;
        let old = self.acts[act].map.as_ref().ok_or("GoldenCompass map missing")?;
        let current_coord = old.points[self.current_point as usize].coord;
        let visited: Vec<_> = self.visited_points.iter().map(|&i| old.points[i as usize].coord).collect();
        let generated = generate_act_map(self.acts[act].def.num_rooms, &mut self.map_rngs[act],
            self.options.ascension, act);
        // An existing start point is live (not removed); clone its storage, then
        // set every public MapPoint field to the C# constructor defaults.
        let mut template = generated.points[generated.start as usize].clone();
        template.children.clear(); template.parents.clear(); template.can_be_modified = true;
        let mut kinds = vec![MapPointType::Monster, MapPointType::Unknown, MapPointType::Monster,
            MapPointType::RestSite, MapPointType::Monster, MapPointType::RestSite,
            MapPointType::Unknown, MapPointType::Treasure, MapPointType::Unknown,
            MapPointType::Treasure, MapPointType::Unknown, MapPointType::Shop,
            MapPointType::Elite, MapPointType::RestSite, MapPointType::Elite, MapPointType::RestSite];
        if self.is_multiplayer() { kinds.remove(2); }
        let mut points = Vec::new();
        for (row, kind) in std::iter::once((0, MapPointType::Ancient))
            .chain(std::iter::once((kinds.len() as i32 + 1, MapPointType::Boss)))
            .chain(kinds.iter().enumerate().map(|(i, &kind)| (i as i32 + 1, kind))) {
            let mut point = template.clone();
            point.coord = Coord::new(3, row); point.point_type = kind;
            points.push(point);
        }
        let chain: Vec<u8> = std::iter::once(0).chain(2..points.len() as u8).chain(std::iter::once(1)).collect();
        for pair in chain.windows(2) {
            points[pair[0] as usize].children.push(pair[1]);
            points[pair[1] as usize].parents.push(pair[0]);
        }
        let map = ActMap { map_length: kinds.len() as i32 + 1, points, start: 0, boss: 1, second_boss: None };
        self.current_point = map.point_index(current_coord).ok_or("GoldenCompass current coordinate missing")?;
        self.visited_points = visited.into_iter().filter_map(|coord| map.point_index(coord)).collect();
        self.acts[act].map = Some(map);
        let relic = self.run_ctx_mut().relics.get_mut(obtained).ok_or("GoldenCompass relic missing")?;
        relic.flag1 = true; // acquisition act supplied
        relic.counter2 = act as i32;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mandatory_small_decks_resolve_without_a_spurious_selector() {
        for id in [RelicId::BiiigHug, RelicId::YummyCookie] {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.run_ctx_mut().deck.truncate(2);
            let mut event = EventState::new(crate::identity::EventId::Tezcatara, Rng::new(1));
            event.relics.push(id);
            event.options.push(EventOption::new("comfort_1", "Relic", "Gain a comfort relic"));
            mgr.choose_tezcatara(event, "comfort_1").unwrap();
            assert!(mgr.pending_run_choice.is_none());
            if id == RelicId::BiiigHug { assert!(mgr.run_ctx().deck.is_empty()); }
            else { assert_eq!(mgr.run_ctx().deck.len(), 2); assert!(mgr.run_ctx().deck.iter().all(|c| c.upgraded)); }
        }
    }

    fn choose(id: RelicId) -> RunManager {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.unported = None;
        let mut event = EventState::new(crate::identity::EventId::Tezcatara, Rng::new(1));
        event.relics.push(id);
        event.options.push(EventOption::new("comfort_1", "Relic", "Gain a comfort relic"));
        mgr.current_event = Some(event);
        mgr.phase = RunPhase::Event;
        mgr.take_event_choice("comfort_1").unwrap();
        mgr
    }

    #[test]
    fn pickup_applies_soup_storybook_and_explicit_four_card_choices() {
        let soup = choose(RelicId::NutritiousSoup);
        for card in &soup.run_ctx().deck {
            assert_eq!(card.enchantments.contains(&(crate::identity::EnchantmentId::TezcatarasEmber, 1)),
                card.rarity == CardRarity::Basic && card.has_tag(CardTag::Strike));
        }
        let story = choose(RelicId::Storybook);
        assert_eq!(story.run_ctx().deck.last().unwrap().id, CardId::BrightestFlame);
        for id in [RelicId::BiiigHug, RelicId::YummyCookie] {
            let mgr = choose(id);
            let choice = mgr.pending_run_choice.as_ref().unwrap();
            assert_eq!((choice.min_choices, choice.max_choices, choice.allow_skip), (4, 4, false));
            assert!(mgr.unported.is_none());
        }
    }

    #[test]
    fn golden_path_replaces_graph_and_wax_set_retains_provenance_on_pickup() {
        let mgr = choose(RelicId::GoldenCompass);
        let map = mgr.visible_map().unwrap();
        assert_eq!(map.points.len(), 18);
        assert_eq!(map.points[map.start as usize].coord, Coord::new(3, 0));
        assert_eq!(map.points[map.boss as usize].coord, Coord::new(3, 17));
        for row in 0..17 {
            let i = map.point_index(Coord::new(3, row)).unwrap();
            assert_eq!(map.points[i as usize].children.len(), 1);
            assert_eq!(map.points[map.points[i as usize].children[0] as usize].coord, Coord::new(3, row + 1));
        }
        let mut mgr = choose(RelicId::ToyBox);
        assert_eq!(mgr.reward.items.len(), 5);
        assert!(mgr.reward.wax_relics);
        mgr.reward.items[0].kind = RewardItemKind::Relic(RelicId::Lantern);
        mgr.pick_relic_reward(Some(0)).unwrap();
        assert!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::Lantern).unwrap().is_wax);
        assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
    }

    #[test]
    fn offers_use_three_source_ordered_pools_and_only_basic_strikes_enable_soup() {
        for strikes in [false, true] {
            for seed in 0..64 {
                let mut mgr = RunManager::new(4100000, RunOptions::default());
                if !strikes { mgr.run_ctx_mut().deck.clear(); }
                let mut expected = Rng::new(seed);
                let mut first = FIRST.to_vec();
                if strikes { first.push((RelicId::NutritiousSoup, "Nutritious Soup")); }
                let ids = [first[expected.choice_index(first.len())].0,
                    SECOND[expected.choice_index(SECOND.len())].0, THIRD[expected.choice_index(THIRD.len())].0];
                mgr.current_event = Some(EventState::new(crate::identity::EventId::Tezcatara, Rng::new(seed)));
                mgr.initialize_tezcatara().unwrap();
                let event = mgr.current_event.as_ref().unwrap();
                assert_eq!(event.relics.as_slice(), &ids);
                assert_eq!(event.rng.counter, expected.counter);
                assert_eq!(event.options.iter().map(|o| o.option_id).collect::<Vec<_>>(),
                    ["comfort_1", "comfort_2", "comfort_3"]);
            }
        }
    }
}
