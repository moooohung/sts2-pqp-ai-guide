use crate::character::CardPool;
use super::{MpPhase, MpRun};
use crate::{
    card::Card,
    relic::Relic,
    tables::{CardId, CardRarity, CardTag, PotionId, PotionRarity, RelicId, CARD_MP, KW_ETERNAL, POTION_INFO},
};

// Neow.AllPossibleOptions ancient relic order, matching run/manager's audited
// solo table. NeowsBones filters this through IsAllowedAtNeow before shuffling.
const NEOW_BONES_POOL: &[RelicId] = &[
    RelicId::CursedPearl, RelicId::DowsingRod, RelicId::HeftyTablet,
    RelicId::LargeCapsule, RelicId::LeafyPoultice, RelicId::NeowsBones,
    RelicId::NeowsSacrifice, RelicId::PrecariousShears, RelicId::SilkenTress,
    RelicId::SilverCrucible, RelicId::ArcaneScroll, RelicId::BoomingConch,
    RelicId::FishingRod, RelicId::GoldenPearl, RelicId::Kaleidoscope,
    RelicId::LeadPaperweight, RelicId::LostCoffer, RelicId::MassiveScroll,
    RelicId::NeowsTorment, RelicId::NewLeaf, RelicId::PhialHolster,
    RelicId::PreciseScissors, RelicId::ScrollBoxes, RelicId::WingedBoots,
    RelicId::LavaRock, RelicId::NeowsTalisman, RelicId::NutritiousOyster,
    RelicId::Pomander, RelicId::SmallCapsule, RelicId::StoneHumidifier,
];

impl MpRun {
    pub(super) fn obtain_neow_relic_effect(&mut self, seat: usize, id: RelicId)
        -> Result<(), &'static str> {
        if id == RelicId::PrecariousShears { return self.begin_precarious_shears(seat); }
        if id == RelicId::PaelsGrowth { return self.begin_paels_growth(seat); }
        if id == RelicId::NewLeaf { return self.begin_new_leaf(seat); }
        if id == RelicId::NeowsBones { return self.begin_neows_bones(seat); }
        if self.begin_extended_neow_pickup(seat, id)? { return Ok(()); }
        if id == RelicId::NeowsTalisman {
            self.roster.obtain_inventory_relic(&mut self.world.run, seat, id)?;
            self.record_relic_seen(seat, id);
            return Ok(());
        }
        if !matches!(id, RelicId::LeadPaperweight | RelicId::NeowsTorment | RelicId::CursedPearl
            | RelicId::SmallCapsule | RelicId::StoneHumidifier | RelicId::GoldenPearl | RelicId::FishingRod
            | RelicId::LavaRock | RelicId::SilkenTress | RelicId::BoomingConch
            | RelicId::NutritiousOyster)
        {
            return Err("Neow pickup continuation not yet supported");
        }
        let mut relic = Relic::new(id);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        self.record_relic_seen(seat, id);
        match id {
            RelicId::LeadPaperweight => {
                let mut cards = Vec::new();
                for _ in 0..2 {
                    let p = self.roster.player_mut(seat).unwrap();
                    let roll = p.rngs.rewards.next_float(1.0);
                    let rare = if self.world.run.ascension >= 7 { 0.0149 } else { 0.03 };
                    let mut rarity = if roll < rare { CardRarity::Rare } else if roll < rare + 0.37 {
                        CardRarity::Uncommon
                    } else { CardRarity::Common };
                    let pool = loop {
                        let pool: Vec<_> = super::super::card_pools::cards(CardPool::Colorless, true)?.into_iter()
                            .filter(|id| !cards.iter().any(|c: &Card| c.id == *id))
                            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|c| c.rarity == rarity)).collect();
                        if !pool.is_empty() || rarity == CardRarity::Rare { break pool; }
                        rarity = if rarity == CardRarity::Common { CardRarity::Uncommon } else { CardRarity::Rare };
                    };
                    let card_id = pool[p.rngs.rewards.choice_index(pool.len())];
                    p.rngs.rewards.next_float(1.0); // act-1 upgrade roll, odds zero
                    self.roster.next_card_id += 1;
                    let mut card = Card::from_factory(card_id, false, self.roster.next_card_id)
                        .ok_or("Neow card factory")?;
                    card.owner = seat as u8;
                    cards.push(card);
                }
                self.neow_cards[seat] = Some(cards);
            }
            RelicId::NeowsTorment | RelicId::CursedPearl => {
                let card_id = if id == RelicId::NeowsTorment { CardId::NeowsFury } else { CardId::Greed };
                self.add_owner_card_from_id(seat, card_id)?;
                if id == RelicId::CursedPearl { self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run,333); }
            }
            RelicId::GoldenPearl => {self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run,150);}
            RelicId::SilkenTress => self.roster.player_mut(seat).unwrap().gold = 0,
            RelicId::NutritiousOyster => {
                let player = self.roster.player_mut(seat).unwrap();
                player.max_hp += 11;
                player.hp = (player.hp + 11).min(player.max_hp);
            }
            RelicId::SmallCapsule => self.begin_small_capsule_reward(seat)?,
            _ => {}
        }
        Ok(())
    }

    fn begin_neows_bones(&mut self, seat: usize) -> Result<(), &'static str> {
        // NeowsBones.cs:35-58. Build the complete Neow relic set allowed for
        // this multiplayer owner, shuffle on PlayerRng.Rewards, and offer two
        // mandatory RelicRewards. The curse waits for both nested pickups.
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let mut relic = Relic::new(RelicId::NeowsBones);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        self.record_relic_seen(seat, RelicId::NeowsBones);

        let mut choices: Vec<_> = NEOW_BONES_POOL.iter().copied()
            .filter(|id| !matches!(id,
                RelicId::NeowsBones | RelicId::SilverCrucible | RelicId::WingedBoots))
            .collect();
        self.roster.player_mut(seat).unwrap().rngs.rewards.shuffle(&mut choices);
        if choices.len() < 2 { return Err("NeowsBones relic pool too small"); }
        self.relic_rewards[seat] = Some(choices[0]);
        self.relic_reward_skippable[seat] = false;
        self.relic_reward_queues[seat].push((choices[1], false));
        self.neows_bones_curse_pending[seat] = true;
        Ok(())
    }

    pub(super) fn maybe_finish_neows_bones(&mut self, seat: usize)
        -> Result<(), &'static str> {
        if !self.neows_bones_curse_pending[seat] { return Ok(()); }
        let modal_pending = self.relic_rewards[seat].is_some()
            || self.neow_cards[seat].is_some()
            || self.neow_potion_rewards[seat].is_some()
            || self.scroll_box_bundles[seat].is_some()
            || self.paels_growth_choices[seat].is_some()
            || self.shears_choices[seat].is_some()
            || self.new_leaf_choices[seat].is_some()
            || self.pomander_choices[seat].is_some()
            || self.precise_scissors_choices[seat].is_some();
        if modal_pending { return Ok(()); }
        if !self.relic_reward_queues[seat].is_empty() {
            let (id, skippable) = self.relic_reward_queues[seat].remove(0);
            self.relic_rewards[seat] = Some(id);
            self.relic_reward_skippable[seat] = skippable;
            return Ok(());
        }

        // NeowsBones.cs:42-58: multiplayer-filtered Curse pool, sorted by Id,
        // one RunState.Rng.Niche pick, then an ordinary Deck add.
        let mut curses: Vec<_> = super::super::card_pools::cards(CardPool::Curse, true)?
            .into_iter()
            .filter(|id| crate::tables::card_info(*id, false).is_some_and(|c| c.gen_modifiers))
            .collect();
        curses.sort_by_key(|id| id.name());
        if curses.is_empty() { return Err("NeowsBones curse pool empty"); }
        let index = self.world.run.rngs.niche.choice_index(curses.len());
        self.add_owner_card_from_id(seat, curses[index])?;
        self.neows_bones_curse_pending[seat] = false;
        Ok(())
    }

    /// Original-DLL Ancient relic pickup continuations that can be represented
    /// without a cross-owner reward queue. Returns false only for relics owned
    /// by a different continuation module or still intentionally fail-closed.
    pub(super) fn begin_extended_neow_pickup(&mut self, seat: usize, id: RelicId)
        -> Result<bool, &'static str> {
        if !matches!(id,
            RelicId::ArcaneScroll | RelicId::DowsingRod | RelicId::HeftyTablet |
            RelicId::LargeCapsule | RelicId::LeafyPoultice | RelicId::NeowsSacrifice | RelicId::PhialHolster |
            RelicId::Pomander | RelicId::PreciseScissors | RelicId::LostCoffer |
            RelicId::MassiveScroll | RelicId::ScrollBoxes | RelicId::Kaleidoscope)
        {
            return Ok(false);
        }
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let mut relic = Relic::new(id);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        self.record_relic_seen(seat, id);

        match id {
            RelicId::ArcaneScroll => self.apply_arcane_scroll(seat)?,
            RelicId::DowsingRod => {
                // DowsingRod.cs:18-22 creates one owner-bound Dowsing card.
                let id = CardId::from_name("DOWSING").ok_or("Dowsing card missing")?;
                self.add_owner_card_from_id(seat, id)?;
            }
            RelicId::HeftyTablet => self.begin_hefty_tablet(seat)?,
            RelicId::LargeCapsule => self.apply_large_capsule(seat)?,
            RelicId::LeafyPoultice => self.apply_leafy_poultice(seat)?,
            RelicId::NeowsSacrifice => self.apply_neows_sacrifice(seat)?,
            RelicId::PhialHolster => self.apply_phial_holster(seat)?,
            RelicId::Pomander => self.begin_pomander(seat)?,
            RelicId::PreciseScissors => self.begin_precise_scissors(seat)?,
            RelicId::LostCoffer => self.begin_lost_coffer(seat)?,
            RelicId::MassiveScroll => self.begin_massive_scroll(seat)?,
            RelicId::ScrollBoxes => self.begin_scroll_boxes(seat)?,
            RelicId::Kaleidoscope => self.begin_kaleidoscope(seat)?,
            _ => unreachable!(),
        }
        Ok(true)
    }

    /// CardFactory.CreateForReward with CardCreationSource.Other and regular
    /// encounter odds. CardFactory.cs:174-218 uses base odds (no pity-offset
    /// mutation), wraps to the next available rarity, blacklists prior picks,
    /// and always consumes the upgrade roll.
    pub(super) fn make_other_regular_reward_cards(&mut self, seat: usize, pool: &[CardId], count: usize,
        modify_hooks: bool) -> Result<Vec<Card>, &'static str> {
        self.make_other_regular_reward_cards_with_upgrade_roll(seat,pool,count,modify_hooks,true)
    }

    pub(super) fn make_other_regular_reward_cards_with_upgrade_roll(&mut self, seat: usize, pool: &[CardId], count: usize,
        modify_hooks: bool, roll_upgrade:bool) -> Result<Vec<Card>, &'static str> {
        let ascension = self.world.run.ascension;
        let act_index = self.world.run.act_index;
        let mut next_id = self.roster.next_card_id;
        let mut cards = Vec::with_capacity(count);
        {
            let player = self.roster.player_mut(seat).ok_or("invalid reward owner")?;
            for _ in 0..count {
                let roll = player.rngs.rewards.next_float(1.0) as f32;
                let rare = if ascension >= 7 { 0.0149f32 } else { 0.03f32 };
                let mut rarity = if roll < rare { CardRarity::Rare }
                    else if roll < 0.37f32 + rare { CardRarity::Uncommon }
                    else { CardRarity::Common };
                let has_rarity = |wanted: CardRarity, chosen: &[Card]| pool.iter().copied()
                    .filter(|id| !chosen.iter().any(|card| card.id == *id))
                    .any(|id| Card::from_factory(id, false, 0).is_some_and(|card| card.rarity == wanted));
                for _ in 0..3 {
                    if has_rarity(rarity, &cards) { break; }
                    rarity = match rarity {
                        CardRarity::Common => CardRarity::Uncommon,
                        CardRarity::Uncommon => CardRarity::Rare,
                        CardRarity::Rare => CardRarity::Common,
                        _ => return Err("invalid regular reward rarity"),
                    };
                }
                let candidates: Vec<_> = pool.iter().copied()
                    .filter(|id| !cards.iter().any(|card| card.id == *id))
                    .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|card| card.rarity == rarity))
                    .collect();
                if candidates.is_empty() { return Err("Neow regular reward pool exhausted"); }
                let id = candidates[player.rngs.rewards.choice_index(candidates.len())];
                let upgraded = if roll_upgrade {
                    let upgrade_roll = player.rngs.rewards.next_float(1.0);
                    let scaling = if ascension >= 7 { 0.125 } else { 0.25 };
                    let upgrade_odds = if rarity == CardRarity::Rare { 0.0 }
                        else { act_index as f64 * scaling };
                    upgrade_roll <= upgrade_odds
                }else{false};
                next_id += 1;
                let mut card = Card::from_factory(id, upgraded, next_id)
                    .ok_or("Neow reward card factory")?;
                card.owner = seat as u8;
                cards.push(card);
            }
            if modify_hooks {
                super::rewards::apply_card_reward_modifiers(player, &mut cards, &mut next_id,Some(self.world.run.room_type),&mut self.world.run.rngs.niche)?;
            }
        }
        self.roster.next_card_id = next_id;
        Ok(cards)
    }

    fn begin_massive_scroll(&mut self, seat: usize) -> Result<(), &'static str> {
        // MassiveScroll.cs:24-45. Pool = owner character + Colorless, predicate
        // MultiplayerOnly, three ordinary reward creations, then a skippable
        // choose-a-card screen.
        let character = self.roster.player(seat).unwrap().character;
        let mut pool = super::super::card_pools::cards(character.into(), true)?;
        pool.extend(super::super::card_pools::cards(CardPool::Colorless, true)?);
        pool.retain(|id| CARD_MP[id.index()] == 1);
        let cards = self.make_other_regular_reward_cards(seat, &pool, 3, true)?;
        self.neow_cards[seat] = Some(cards);
        self.neow_card_sources[seat] = Some(RelicId::MassiveScroll);
        Ok(())
    }

    fn begin_lost_coffer(&mut self, seat: usize) -> Result<(), &'static str> {
        // LostCoffer.cs:17-23 offers one three-card owner CardReward followed by
        // one owner PotionReward; RewardsCmd populates both before selection.
        let character = self.roster.player(seat).unwrap().character;
        let pool = super::super::card_pools::cards(character.into(), true)?;
        let cards = self.make_other_regular_reward_cards(seat, &pool, 3, true)?;
        let potion = super::rewards::generate_potions(self.roster.player_mut(seat).unwrap(), 1)?
            .into_iter().next().ok_or("Lost Coffer potion reward missing")?;
        self.neow_cards[seat] = Some(cards);
        self.neow_potion_rewards[seat] = Some(potion);
        Ok(())
    }

    fn begin_scroll_boxes(&mut self, seat: usize) -> Result<(), &'static str> {
        // ScrollBoxes.cs:48-92. Two bundles, each normally two distinct Common
        // plus one distinct Uncommon from the owner pool. Defect independently
        // has a 1% Claw/Claw/Claw replacement per bundle.
        let character = self.roster.player(seat).unwrap().character;
        let pool = super::super::card_pools::cards(character.into(), true)?;
        let common: Vec<_> = pool.iter().copied()
            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|card| card.rarity == CardRarity::Common))
            .collect();
        let uncommon: Vec<_> = pool.iter().copied()
            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|card| card.rarity == CardRarity::Uncommon))
            .collect();
        if common.len() < 4 || uncommon.len() < 2 { return Err("Scroll Boxes owner pool too small"); }
        let is_defect = character == super::Character::Defect;
        let mut used = Vec::<CardId>::new();
        let mut bundles = Vec::with_capacity(2);
        for _ in 0..2 {
            let claw = is_defect && self.roster.player_mut(seat).unwrap().rngs.rewards.next_int(0, 100) < 1;
            let mut bundle = Vec::with_capacity(3);
            if claw {
                for _ in 0..3 {
                    self.roster.next_card_id += 1;
                    let mut card = Card::from_factory(CardId::Claw, false, self.roster.next_card_id)
                        .ok_or("Scroll Boxes Claw factory")?;
                    card.owner = seat as u8;
                    bundle.push(card);
                }
            } else {
                for _ in 0..2 {
                    let candidates: Vec<_> = common.iter().copied().filter(|id| !used.contains(id)).collect();
                    if candidates.is_empty() { return Err("Scroll Boxes common pool exhausted"); }
                    let id = {
                        let player = self.roster.player_mut(seat).unwrap();
                        candidates[player.rngs.rewards.choice_index(candidates.len())]
                    };
                    used.push(id);
                    self.roster.next_card_id += 1;
                    let mut card = Card::from_factory(id, false, self.roster.next_card_id)
                        .ok_or("Scroll Boxes card factory")?;
                    card.owner = seat as u8;
                    bundle.push(card);
                }
                let candidates: Vec<_> = uncommon.iter().copied().filter(|id| !used.contains(id)).collect();
                if candidates.is_empty() { return Err("Scroll Boxes uncommon pool exhausted"); }
                let id = {
                    let player = self.roster.player_mut(seat).unwrap();
                    candidates[player.rngs.rewards.choice_index(candidates.len())]
                };
                used.push(id);
                self.roster.next_card_id += 1;
                let mut card = Card::from_factory(id, false, self.roster.next_card_id)
                    .ok_or("Scroll Boxes card factory")?;
                card.owner = seat as u8;
                bundle.push(card);
            }
            bundles.push(bundle);
        }
        self.scroll_box_bundles[seat] = Some(bundles);
        Ok(())
    }

    pub fn select_scroll_box_bundle(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow { return Err("not at Scroll Boxes choice"); }
        let bundles = self.scroll_box_bundles[seat].as_ref().ok_or("no owner Scroll Boxes choice")?;
        if index >= bundles.len() { return Err("invalid Scroll Boxes bundle index"); }
        let mut next = self.clone();
        let bundle = next.scroll_box_bundles[seat].as_ref().unwrap()[index].clone();
        for card in bundle {
            super::cards::add(&mut next.roster, &mut next.world.run, seat, card)?;
        }
        next.scroll_box_bundles[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    fn begin_kaleidoscope(&mut self, seat: usize) -> Result<(), &'static str> {
        // Kaleidoscope.cs:33-50. Each of two CardRewards independently shuffles
        // the four non-owner character pools on RunState.Rng.Niche, takes three,
        // and creates one regular reward card from each selected pool.
        let owner_pool = self.roster.player(seat).unwrap().character;
        let mut rewards = Vec::with_capacity(2);
        for _ in 0..2 {
            let mut pools = vec![crate::character::Character::Defect, crate::character::Character::Ironclad, crate::character::Character::Necrobinder, crate::character::Character::Regent, crate::character::Character::Silent];
            pools.retain(|pool| *pool != owner_pool);
            self.world.run.rngs.niche.shuffle(&mut pools);
            let mut reward = Vec::with_capacity(3);
            for pool_name in pools.into_iter().take(3) {
                let pool = super::super::card_pools::cards(pool_name.into(), true)?;
                let mut card = self.make_other_regular_reward_cards(seat, &pool, 1, true)?;
                reward.push(card.pop().ok_or("Kaleidoscope card reward missing")?);
            }
            rewards.push(reward);
        }
        self.neow_cards[seat] = Some(rewards.remove(0));
        self.neow_card_queues[seat].extend(rewards);
        Ok(())
    }

    fn apply_large_capsule(&mut self, seat: usize) -> Result<(), &'static str> {
        // LargeCapsule.cs:24-36 performs two ordinary RelicFactory pulls and
        // obtains each relic before adding one character-basic Strike and Defend.
        // RelicFactory.RollRarity uses the owner's Rewards stream; each pull also
        // removes the chosen relic from the shared grab bag.
        for _ in 0..2 {
            let roll = self.roster.player_mut(seat).unwrap().rngs.rewards.next_float(1.0);
            let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
            let id = *self.world.player_bags[seat][rarity].first()
                .ok_or("Large Capsule personal relic bag empty")?;
            self.world.player_bags[seat][rarity].remove(0);
            for bag in &mut self.world.shared_bags { bag.retain(|other| *other != id); }
            self.roster.obtain_inventory_relic(&mut self.world.run, seat, id)?;
            self.record_relic_seen(seat, id);
        }

        let (strike, defend) = {
            let player = self.roster.player(seat).unwrap();
            let strike = player.deck.iter().find(|c|
                c.rarity == CardRarity::Basic && c.has_tag(CardTag::Strike))
                .map(|c| c.id).ok_or("Large Capsule basic Strike missing")?;
            let defend = player.deck.iter().find(|c|
                c.rarity == CardRarity::Basic && c.has_tag(CardTag::Defend))
                .map(|c| c.id).ok_or("Large Capsule basic Defend missing")?;
            (strike, defend)
        };
        self.add_owner_card_from_id(seat, strike)?;
        self.add_owner_card_from_id(seat, defend)
    }

    pub(super) fn add_owner_card_from_id(&mut self, seat: usize, id: CardId)
        -> Result<(), &'static str> {
        self.roster.next_card_id += 1;
        let mut card = Card::from_factory(id, false, self.roster.next_card_id)
            .ok_or("Neow card factory")?;
        card.owner = seat as u8;
        super::cards::add(&mut self.roster, &mut self.world.run, seat, card)
    }

    fn apply_arcane_scroll(&mut self, seat: usize) -> Result<(), &'static str> {
        // ArcaneScroll.cs:19-28: one uniform Rare from Owner.Character.CardPool,
        // NoUpgradeRoll, using the owner's reward-generation RNG.
        let character = self.roster.player(seat).unwrap().character;
        let candidates: Vec<_> = super::super::card_pools::cards(character.into(), true)?
            .into_iter()
            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|c| c.rarity == CardRarity::Rare))
            .collect();
        if candidates.is_empty() { return Ok(()); }
        let pick = {
            let player = self.roster.player_mut(seat).unwrap();
            candidates[player.rngs.rewards.choice_index(candidates.len())]
        };
        self.add_owner_card_from_id(seat, pick)
    }

    fn begin_hefty_tablet(&mut self, seat: usize) -> Result<(), &'static str> {
        // HeftyTablet.cs:27-44 creates three uniform Rare, non-upgraded options;
        // the choice is skippable and Injury is added after either pick or skip.
        let character = self.roster.player(seat).unwrap().character;
        let pool: Vec<_> = super::super::card_pools::cards(character.into(), true)?
            .into_iter()
            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|c| c.rarity == CardRarity::Rare))
            .collect();
        let mut cards = Vec::with_capacity(3);
        for _ in 0..3 {
            let candidates: Vec<_> = pool.iter().copied()
                .filter(|id| !cards.iter().any(|card: &Card| card.id == *id))
                .collect();
            if candidates.is_empty() { return Err("Hefty Tablet rare pool exhausted"); }
            let id = {
                let player = self.roster.player_mut(seat).unwrap();
                candidates[player.rngs.rewards.choice_index(candidates.len())]
            };
            self.roster.next_card_id += 1;
            let mut card = Card::from_factory(id, false, self.roster.next_card_id)
                .ok_or("Hefty Tablet card factory")?;
            card.owner = seat as u8;
            cards.push(card);
        }
        self.neow_cards[seat] = Some(cards);
        self.neow_card_sources[seat] = Some(RelicId::HeftyTablet);
        Ok(())
    }

    fn apply_neows_sacrifice(&mut self, seat: usize) -> Result<(), &'static str> {
        // NeowsSacrifice.cs:28-33 procures Ambergris, then adds Guilty.
        let procured = {
            let player = self.roster.player_mut(seat).unwrap();
            if player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                false
            } else if let Some(slot) = player.potions.iter_mut().find(|p| p.is_none()) {
                *slot = Some(PotionId::Ambergris);
                true
            } else {
                false
            }
        };
        if procured { self.mark_local_potion_seen(seat, PotionId::Ambergris); }
        self.add_owner_card_from_id(seat, CardId::Guilty)
    }

    fn apply_phial_holster(&mut self, seat: usize) -> Result<(), &'static str> {
        // PhialHolster.cs:29-34: +1 max potion count, then two out-of-combat
        // random potions from the shared CombatPotionGeneration stream.
        let character = self.roster.player(seat).unwrap().character;
        let mut pool = super::super::potion_pools::cards(character)
            .ok_or("Phial Holster owner potion pool")?;
        {
            let player = self.roster.player_mut(seat).unwrap();
            player.max_potion_slots += 1;
            player.potions.resize(player.max_potion_slots, None);
        }
        for _ in 0..2 {
            let roll = self.world.run.rngs.combat_potion_generation.next_float(1.0);
            let rarity = if roll <= 0.10 { PotionRarity::Rare }
                else if roll <= 0.35 { PotionRarity::Uncommon } else { PotionRarity::Common };
            let candidates: Vec<_> = pool.iter().copied()
                .filter(|id| POTION_INFO[id.index()].rarity == rarity)
                .collect();
            if candidates.is_empty() { return Err("Phial Holster potion rarity pool"); }
            let id = candidates[self.world.run.rngs.combat_potion_generation.choice_index(candidates.len())];
            // PotionFactory.CreateRandomPotions removes each selected model from
            // the option list before the next rarity roll, so duplicates cannot
            // be generated by this two-potion pickup.
            pool.retain(|other| *other != id);
            let procured = {
                let player = self.roster.player_mut(seat).unwrap();
                if player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                    false
                } else if let Some(slot) = player.potions.iter_mut().find(|p| p.is_none()) {
                    *slot = Some(id);
                    true
                } else {
                    false
                }
            };
            if procured { self.mark_local_potion_seen(seat, id); }
        }
        Ok(())
    }

    fn apply_leafy_poultice(&mut self, seat: usize) -> Result<(), &'static str> {
        // LeafyPoultice.cs:23-37 loses 12 max HP, then transforms the first
        // Basic Strike and first Basic Defend using PlayerRng.Transformations.
        let (strike, defend) = {
            let player = self.roster.player_mut(seat).unwrap();
            player.max_hp = (player.max_hp - 12).max(1);
            player.hp = player.hp.min(player.max_hp);
            let strike = player.deck.iter().find(|c|
                c.rarity == CardRarity::Basic && c.has_tag(CardTag::Strike)).map(|c| c.instance_id);
            let defend = player.deck.iter().find(|c|
                c.rarity == CardRarity::Basic && c.has_tag(CardTag::Defend)).map(|c| c.instance_id);
            (strike, defend)
        };
        for instance_id in [strike, defend].into_iter().flatten() {
            self.transform_owner_card(seat, instance_id)?;
        }
        Ok(())
    }

    fn transform_owner_card(&mut self, seat: usize, instance_id: u32) -> Result<(), &'static str> {
        let original = self.roster.player(seat).unwrap().deck.iter()
            .find(|c| c.instance_id == instance_id).cloned().ok_or("transform candidate missing")?;
        let candidates = super::super::card_pools::transformations(&original, true)?;
        if candidates.is_empty() { return Err("empty multiplayer transform pool"); }
        let id = {
            let player = self.roster.player_mut(seat).unwrap();
            candidates[player.rngs.transformations.choice_index(candidates.len())]
        };
        self.roster.next_card_id += 1;
        let mut replacement = Card::from_factory(id, false, self.roster.next_card_id)
            .ok_or("transform card factory")?;
        replacement.owner = seat as u8;
        let deck = &mut self.roster.player_mut(seat).unwrap().deck;
        let index = deck.iter().position(|c| c.instance_id == instance_id)
            .ok_or("transform candidate missing")?;
        self.roster.player_mut(seat).unwrap().remove_deck_card(index);
        super::cards::add(&mut self.roster, &mut self.world.run, seat, replacement)
    }

    fn begin_pomander(&mut self, seat: usize) -> Result<(), &'static str> {
        // Pomander.cs:17-23 selects exactly one upgradable deck card.
        let choices: Vec<_> = self.roster.player(seat).unwrap().deck.iter()
            .filter(|c| !c.upgraded && Card::from_factory(c.id, true, 0).is_some_and(|u| u.upgraded))
            .map(|c| c.instance_id).collect();
        match choices.as_slice() {
            [] => Ok(()),
            [only] => self.finish_pomander(seat, *only),
            _ => { self.pomander_choices[seat] = Some(choices); Ok(()) }
        }
    }

    pub fn select_pomander_card(&mut self, net_id: u64, seat: usize, instance_id: u32)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map) { return Err("not at inventory choice"); }
        let choices = self.pomander_choices[seat].as_ref().ok_or("no owner upgrade choice")?;
        if !choices.contains(&instance_id) { return Err("invalid owner upgrade selection"); }
        let mut next = self.clone();
        next.finish_pomander(seat, instance_id)?;
        next.pomander_choices[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    fn finish_pomander(&mut self, seat: usize, instance_id: u32) -> Result<(), &'static str> {
        self.world.run.next_card_id = self.world.run.next_card_id.max(self.roster.next_card_id);
        {
            let player = self.roster.player_mut(seat).unwrap();
            std::mem::swap(&mut self.world.run.deck, &mut player.deck);
        }
        let index = self.world.run.deck.iter().position(|c| c.instance_id == instance_id)
            .ok_or("upgrade candidate missing")?;
        if self.world.run.deck[index].upgraded
            || !Card::from_factory(self.world.run.deck[index].id, true, 0).is_some_and(|u| u.upgraded)
        {
            return Err("upgrade candidate no longer eligible");
        }
        self.world.run.upgrade_deck_card(index);
        {
            let player = self.roster.player_mut(seat).unwrap();
            std::mem::swap(&mut self.world.run.deck, &mut player.deck);
        }
        self.roster.next_card_id = self.roster.next_card_id.max(self.world.run.next_card_id);
        Ok(())
    }

    fn begin_precise_scissors(&mut self, seat: usize) -> Result<(), &'static str> {
        // PreciseScissors.cs:17-23 uses CardSelectCmd.FromDeckForRemoval;
        // IsRemovable excludes Eternal cards while they are in the Deck pile.
        let choices: Vec<_> = self.roster.player(seat).unwrap().deck.iter()
            .filter(|c| !c.has_kw(KW_ETERNAL)).map(|c| c.instance_id).collect();
        match choices.as_slice() {
            [] => Ok(()),
            [only] => self.finish_precise_scissors(seat, *only),
            _ => { self.precise_scissors_choices[seat] = Some(choices); Ok(()) }
        }
    }

    pub fn select_precise_scissors_card(&mut self, net_id: u64, seat: usize, instance_id: u32)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map) { return Err("not at inventory choice"); }
        let choices = self.precise_scissors_choices[seat].as_ref().ok_or("no owner removal choice")?;
        if !choices.contains(&instance_id) { return Err("invalid owner removal selection"); }
        let mut next = self.clone();
        next.finish_precise_scissors(seat, instance_id)?;
        next.precise_scissors_choices[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    fn finish_precise_scissors(&mut self, seat: usize, instance_id: u32) -> Result<(), &'static str> {
        let player = self.roster.player_mut(seat).unwrap();
        let index = player.deck.iter().position(|c| c.instance_id == instance_id)
            .ok_or("removal candidate missing")?;
        if player.deck[index].has_kw(KW_ETERNAL) { return Err("removal candidate no longer eligible"); }
        if player.deck[index].id == CardId::SpoilsMap {
            return Err("unported Spoils Map removal callback");
        }
        player.remove_deck_card(index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    fn party() -> MpRun {
        MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap()
    }

    fn force(mut run: MpRun, seat: usize, relic: RelicId) -> MpRun {
        run.neow[seat][0] = relic;
        let net = run.roster.player(seat).unwrap().net_id;
        run.choose_neow(net, seat, 0).unwrap();
        run
    }

    #[test]
    fn immediate_pickups_follow_original_owner_and_rng_streams() {
        let base = party();
        let other: Vec<_> = base.roster.player(0).unwrap().deck.iter()
            .map(|c| (c.id, c.instance_id, c.owner, c.upgraded)).collect();

        let arcane = force(base.clone(), 1, RelicId::ArcaneScroll);
        let owner = arcane.roster.player(1).unwrap();
        assert_eq!(owner.deck.len(), base.roster.player(1).unwrap().deck.len() + 1);
        assert_eq!(owner.deck.last().unwrap().rarity, CardRarity::Rare);
        assert_eq!(arcane.roster.player(0).unwrap().deck.iter()
            .map(|c| (c.id, c.instance_id, c.owner, c.upgraded)).collect::<Vec<_>>(), other);

        let dowsing = force(base.clone(), 1, RelicId::DowsingRod);
        assert_eq!(dowsing.roster.player(1).unwrap().deck.last().unwrap().id.name(), "DOWSING");

        let sacrifice = force(base.clone(), 1, RelicId::NeowsSacrifice);
        let owner = sacrifice.roster.player(1).unwrap();
        assert!(owner.potions.contains(&Some(PotionId::Ambergris)));
        assert_eq!(owner.deck.last().unwrap().id, CardId::Guilty);

        let holster = force(base, 1, RelicId::PhialHolster);
        let owner = holster.roster.player(1).unwrap();
        assert_eq!(owner.max_potion_slots, 4);
        assert_eq!(owner.potions.iter().filter(|p| p.is_some()).count(), 2);
        let potions: Vec<_> = owner.potions.iter().flatten().copied().collect();
        assert_ne!(potions[0], potions[1]);
    }

    #[test]
    fn leafy_poultice_transforms_owner_basics_and_loses_max_hp() {
        let base = party();
        let owner = base.roster.player(1).unwrap();
        let strike = owner.deck.iter().find(|c| c.rarity == CardRarity::Basic && c.has_tag(CardTag::Strike)).unwrap().instance_id;
        let defend = owner.deck.iter().find(|c| c.rarity == CardRarity::Basic && c.has_tag(CardTag::Defend)).unwrap().instance_id;
        let max_hp = owner.max_hp;
        let other: Vec<_> = base.roster.player(0).unwrap().deck.iter()
            .map(|c| (c.id, c.instance_id, c.owner, c.upgraded)).collect();

        let run = force(base, 1, RelicId::LeafyPoultice);
        let owner = run.roster.player(1).unwrap();
        assert_eq!(owner.max_hp, max_hp - 12);
        assert!(!owner.deck.iter().any(|c| c.instance_id == strike || c.instance_id == defend));
        assert_eq!(run.roster.player(0).unwrap().deck.iter()
            .map(|c| (c.id, c.instance_id, c.owner, c.upgraded)).collect::<Vec<_>>(), other);
    }

    #[test]
    fn pomander_and_precise_scissors_are_owner_modal_choices() {
        let mut pomander = force(party(), 1, RelicId::Pomander);
        let choice = pomander.pomander_choices[1].as_ref().unwrap()[0];
        pomander.select_pomander_card(42, 1, choice).unwrap();
        assert!(pomander.roster.player(1).unwrap().deck.iter().find(|c| c.instance_id == choice).unwrap().upgraded);

        let mut scissors = force(party(), 1, RelicId::PreciseScissors);
        let before = scissors.roster.player(1).unwrap().deck.len();
        let choice = scissors.precise_scissors_choices[1].as_ref().unwrap()[0];
        scissors.select_precise_scissors_card(42, 1, choice).unwrap();
        assert_eq!(scissors.roster.player(1).unwrap().deck.len(), before - 1);
        assert!(!scissors.roster.player(1).unwrap().deck.iter().any(|c| c.instance_id == choice));
    }

    #[test]
    fn hefty_tablet_skip_still_adds_injury() {
        let mut run = force(party(), 1, RelicId::HeftyTablet);
        assert_eq!(run.neow_cards[1].as_ref().unwrap().len(), 3);
        assert!(run.neow_cards[1].as_ref().unwrap().iter().all(|c| c.rarity == CardRarity::Rare));
        run.select_neow_card(42, 1, None).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, CardId::Injury);
    }

    #[test]
    fn large_capsule_pulls_two_owner_relics_then_adds_basic_strike_and_defend() {
        let base = party();
        let relics = base.roster.player(1).unwrap().relics.len();
        let deck = base.roster.player(1).unwrap().deck.len();
        let rewards = base.roster.player(1).unwrap().rngs.rewards.counter;
        let mut run = force(base, 1, RelicId::LargeCapsule);
        let owner = run.roster.player(1).unwrap();
        assert_eq!(owner.relics.len(), relics + 3); // Large Capsule + two pulls.
        assert_eq!(owner.deck.len(), deck + 2);
        assert_eq!(owner.deck[owner.deck.len() - 2].id, CardId::StrikeSilent);
        assert_eq!(owner.deck[owner.deck.len() - 1].id, CardId::DefendSilent);
        assert_eq!(owner.rngs.rewards.counter, rewards + 2);
        for relic in &owner.relics[relics + 1..] {
            assert!(run.world.shared_bags.iter().all(|bag| !bag.contains(&relic.id)));
        }
    }

    #[test]
    fn remaining_neow_reward_relics_keep_owner_modal_and_pool_rules() {
        let base = party();

        let massive = force(base.clone(), 1, RelicId::MassiveScroll);
        let cards = massive.neow_cards[1].as_ref().unwrap();
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|card| card.owner == 1 && CARD_MP[card.id.index()] == 1));

        let mut boxes = force(base.clone(), 1, RelicId::ScrollBoxes);
        let bundles = boxes.scroll_box_bundles[1].as_ref().unwrap();
        assert_eq!(bundles.len(), 2);
        assert!(bundles.iter().all(|bundle| bundle.len() == 3 && bundle.iter().all(|card| card.owner == 1)));
        let before = boxes.roster.player(1).unwrap().deck.len();
        boxes.select_scroll_box_bundle(42, 1, 0).unwrap();
        assert_eq!(boxes.roster.player(1).unwrap().deck.len(), before + 3);

        let coffer = force(base.clone(), 1, RelicId::LostCoffer);
        assert_eq!(coffer.neow_cards[1].as_ref().unwrap().len(), 3);
        assert!(coffer.neow_potion_rewards[1].is_some());

        let kaleidoscope = force(base, 1, RelicId::Kaleidoscope);
        let silent_pool = super::super::super::card_pools::cards(CardPool::Silent, true).unwrap();
        assert_eq!(kaleidoscope.neow_cards[1].as_ref().unwrap().len(), 3);
        assert_eq!(kaleidoscope.neow_card_queues[1].len(), 1);
        assert!(kaleidoscope.neow_cards[1].as_ref().unwrap().iter()
            .chain(kaleidoscope.neow_card_queues[1][0].iter())
            .all(|card| card.owner == 1 && !silent_pool.contains(&card.id)));
    }

    #[test]
    fn noncombat_default_odds_omit_upgrade_rolls() {
        let mut run=party();
        let pool=super::super::super::card_pools::cards(run.roster.player(0).unwrap().character.into(),true).unwrap();
        let before=run.roster.player(0).unwrap().rngs.rewards.counter;
        let cards=run.make_other_regular_reward_cards_with_upgrade_roll(0,&pool,5,true,false).unwrap();
        assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter,before+10);
        assert!(cards.iter().all(|card|!card.upgraded));
    }

    #[test]
    fn neows_bones_future_reward_queue_does_not_block_current_nested_pickup() {
        let mut run = party();
        run.relic_rewards[1] = Some(RelicId::ArcaneScroll);
        run.relic_reward_skippable[1] = false;
        run.relic_reward_queues[1].push((RelicId::DowsingRod, false));
        run.neows_bones_curse_pending[1] = true;

        run.select_relic_reward(42, 1, true).unwrap();

        assert!(run.roster.player(1).unwrap().relics.iter().any(|r| r.id == RelicId::ArcaneScroll));
        assert_eq!(run.relic_rewards[1], Some(RelicId::DowsingRod));
    }
}
