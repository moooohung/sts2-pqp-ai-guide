use super::MpPlayer;
use crate::{card::Card, tables::{CardRarity, PotionId, PotionRarity, RelicId, RoomType, POTION_INFO}};

#[derive(Clone, Debug, Default)]
pub struct PlayerRewards {
    /// One entry per GoldReward in RewardsSet order. Empty means no
    /// GoldReward was constructed; `[0]` is a real zero-gold offer.
    pub gold: Vec<i32>,
    pub potion: Option<PotionId>,
    pub cards: Vec<Card>,
    pub extra_cards: Vec<Option<Vec<Card>>>,
    pub extra_relics: Vec<Option<RelicId>>,
    pub extra_potions: Vec<Option<PotionId>>,
    pub special_cards: Vec<Option<Card>>,
    pub card_removals: Vec<bool>,
    pub gold_taken: bool,
    pub potion_taken: bool,
    pub card_taken: bool,
    pub completed: bool,
}

impl PlayerRewards {
    /// RewardsSet sorts Gold(1), Potion(2), Relic(3), SpecialCard(4), Card(5).
    /// Collected rewards remain in that list, so use lengths, not live counts.
    pub fn special_card_reward_index(&self, index: usize) -> Option<usize> {
        if self.completed { return None; }
        self.special_cards.get(index)?.as_ref()?;
        Some(self.gold.len()+usize::from(self.potion.is_some())
            +self.extra_potions.len()+self.extra_relics.len()+index)
    }
}

impl super::MpRun{
    pub(super) fn active_card_reward(&self,seat:usize)->Result<&PlayerRewards,&'static str>{
        match self.phase{
            super::MpPhase::Event=>self.event_card_reward_queues.get(seat).and_then(|q|q.first()),
            super::MpPhase::Rewards=>self.rewards.as_ref().and_then(|r|r.get(seat)),
            _=>None,
        }.ok_or("card reward not offered")
    }
    pub(super) fn active_card_reward_mut(&mut self,seat:usize)->Result<&mut PlayerRewards,&'static str>{
        match self.phase{
            super::MpPhase::Event=>self.event_card_reward_queues.get_mut(seat).and_then(|q|q.first_mut()),
            super::MpPhase::Rewards=>self.rewards.as_mut().and_then(|r|r.get_mut(seat)),
            _=>None,
        }.ok_or("card reward not offered")
    }
    pub fn card_reward_sacrifices(&self,net:u64,seat:usize)->Result<Vec<usize>,&'static str>{
        let p=self.roster.actor(net,seat)?;let reward=self.active_card_reward(seat)?;
        if reward.completed||reward.card_taken||reward.cards.is_empty(){return Ok(Vec::new());}
        Ok(p.relics.iter().enumerate().filter_map(|(i,r)|
            (r.id==RelicId::PaelsWing&&r.enabled).then_some(i)).collect())
    }
    /// PaelsWing.cs:73-94: complete this card reward; every second invocation
    /// obtains a populated relic immediately, not a skippable relic reward.
    pub fn sacrifice_card_reward(&mut self,net:u64,seat:usize,relic:usize)->Result<(),&'static str>{
        if !self.card_reward_sacrifices(net,seat)?.contains(&relic){return Err("card reward sacrifice unavailable");}
        let mut next=self.clone();
        next.active_card_reward_mut(seat)?.card_taken=true;
        let r=&mut next.roster.player_mut(seat).unwrap().relics[relic];r.counter+=1;
        if r.counter%2==0{
            let id=next.pull_event_relic_from_front(seat)?;
            next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
        }
        if next.phase==super::MpPhase::Event{next.finish_event_card_reward(net,seat)?;}
        *self=next;Ok(())
    }
    pub fn finish_event_card_reward(&mut self,net:u64,seat:usize)->Result<(),&'static str>{
        self.roster.actor(net,seat)?;
        if self.phase!=super::MpPhase::Event||self.event_card_reward_queues[seat].is_empty(){return Err("event card reward missing");}
        self.event_card_reward_queues[seat].remove(0);
        if self.event_card_reward_queues[seat].is_empty(){self.finish_event_seat(seat);}
        Ok(())
    }
}

// CardRarityOdds.Roll / RollWithoutChangingFutureOdds: every sum and
// accumulation is a C# float operation, including uncommon + rareThreshold.
pub(super) fn roll_card_rarity(offset: &mut f32, roll: f32, ascension: i32,
    room_type: RoomType) -> CardRarity {
    let (rare, uncommon) = match room_type {
        RoomType::Monster => (if ascension >= 7 { 0.0149f32 } else { 0.03f32 }, 0.37f32),
        RoomType::Elite => (if ascension >= 7 { 0.05f32 } else { 0.1f32 }, 0.4f32),
        RoomType::Boss => (1.0f32, 0.0f32),
        _ => unreachable!("card reward requires a combat room type"),
    };
    // CardRarityOdds.Roll ignores accumulated pity for BossEncounter, but
    // still consumes its roll and resets pity after the guaranteed rare.
    let rare_threshold = rare + if room_type == RoomType::Boss { 0.0 } else { *offset };
    let rarity = if roll < rare_threshold { CardRarity::Rare }
        else if roll < uncommon + rare_threshold { CardRarity::Uncommon }
        else { CardRarity::Common };
    *offset = if rarity == CardRarity::Rare { -0.05 } else {
        (*offset + if ascension >= 7 { 0.005 } else { 0.01 }).min(0.40)
    };
    rarity
}

/// Repeated PotionReward.Populate calls: each reward independently calls
/// CreateRandomPotionOutOfCombat, so every roll starts from the owner's full
/// unlocked potion pool and duplicate models across separate rewards are valid.
pub(super) fn generate_potions(player: &mut MpPlayer, count: usize)
    -> Result<Vec<PotionId>, &'static str> {
    let available = super::super::potion_pools::cards(player.character)
        .ok_or("reward potion owner pool")?;
    let mut potions = Vec::with_capacity(count);
    for _ in 0..count {
        let roll = player.rngs.rewards.next_float(1.0);
        let rarity = if roll <= 0.10 { PotionRarity::Rare } else if roll <= 0.35 {
            PotionRarity::Uncommon
        } else { PotionRarity::Common };
        let candidates: Vec<_> = available.iter().enumerate()
            .filter_map(|(index, id)| (POTION_INFO[id.index()].rarity == rarity).then_some(index))
            .collect();
        if candidates.is_empty() { return Err("reward potion rarity pool"); }
        let candidate = candidates[player.rngs.rewards.choice_index(candidates.len())];
        potions.push(available[candidate]);
    }
    Ok(potions)
}

/// Same late reward-option pipeline as solo, with the recipient's inventory.
pub(super) fn apply_card_reward_modifiers(player: &mut MpPlayer, cards: &mut [Card],
    next_id: &mut u32, room:Option<RoomType>, niche:&mut crate::rng::Rng) -> Result<(), &'static str> {
    apply_card_creation_modifiers(player, cards, next_id, room, niche, true)
}

pub(super) fn apply_card_creation_modifiers(player: &mut MpPlayer, cards: &mut [Card],
    next_id: &mut u32, room:Option<RoomType>, niche:&mut crate::rng::Rng,
    is_reward: bool) -> Result<(), &'static str> {
    let owner=cards.first().map(|c|c.owner).or_else(||player.relics.first().map(|r|r.owner))
        .ok_or("reward owner missing")?;
    super::super::reward_modifiers::apply(&mut player.relics,cards,owner,room,is_reward,next_id,niche,|card,next_id| {
        let owner=card.owner;
        let enchantments=card.enchantments.clone();
        *next_id+=1;
        let mut upgraded=Card::from_factory(card.id,true,*next_id).ok_or("egg reward upgrade factory")?;
        upgraded.owner=owner;
        for (name,amount) in enchantments {
            if !crate::card::enchant::apply_static_enchantment(&mut upgraded,name,amount) {
                return Err("unsupported reward upgrade enchantment");
            }
        }
        *card=upgraded;Ok(())
    })
}

/// RewardsSet.WithRewardsFromRoom rolls potion availability before Populate;
/// then gold, potion model, and interleaved card rarity/model/upgrade rolls.
pub(super) fn generate(player: &mut MpPlayer, owner: u8, ascension: i32, act_index: i32,
    next_id: &mut u32, gold_proportion: f64, room_type: RoomType, niche:&mut crate::rng::Rng) -> Result<PlayerRewards, &'static str> {
    // EncounterModel.Min/MaxGoldReward and PotionRewardOdds.Roll. Only
    // ordinary encounters scale gold by the surviving encounter proportion.
    let (min_gold, max_gold) = match room_type {
        RoomType::Monster => (10.0, 20.0),
        RoomType::Elite => (35.0, 45.0),
        RoomType::Boss => (100.0, 100.0),
        _ => return Err("unsupported multiplayer reward room type"),
    };
    let potion_bonus = if room_type == RoomType::Elite { 0.25f32 * 0.5f32 } else { 0.0 };
    let drop = (player.rngs.rewards.next_float(1.0) as f32) < player.potion_reward_odds + potion_bonus;
    player.potion_reward_odds += if drop { -0.1 } else { 0.1 };
    let scale = if ascension >= 3 { 0.75 } else { 1.0 };
    let gold_proportion = if room_type == RoomType::Monster { gold_proportion as f32 } else { 1.0 };
    let low = ((min_gold * scale) as i32 as f32 * gold_proportion).round_ties_even() as i64;
    let high = ((max_gold * scale) as i32 as f32 * gold_proportion).round_ties_even() as i64;
    let mut gold = if gold_proportion > 0.0 {
        vec![player.rngs.rewards.next_int(low, high) as i32]
    } else { Vec::new() };
    let potion = if drop { generate_potions(player, 1)?.into_iter().next() } else { None };
    let cards = generate_card_reward(player, owner, ascension, act_index, next_id, 3, room_type,niche)?;
    // RewardsSet.GenerateWithoutOffering -> Hook.ModifyRewards;
    // AmethystAubergine.TryModifyRewards appends an already-populated fixed
    // GoldReward(15), so it consumes no Rewards RNG. Multiple enabled copies
    // each contribute their own reward (AmethystAubergine.cs:25-44).
    gold.extend(player.relics.iter()
        .filter(|r| r.id == crate::tables::RelicId::AmethystAubergine && r.enabled)
        .map(|_| 15));
    Ok(PlayerRewards { gold, potion, cards, card_removals: Vec::new(), extra_cards: Vec::new(), extra_relics: Vec::new(), extra_potions: Vec::new(),
        special_cards: Vec::new(),
        gold_taken: false, potion_taken: false, card_taken: false, completed: false })
}

/// CardCreationOptions.ForRoom for combat rewards. DreamCatcher explicitly
/// uses Monster odds. Unlike an Other-source reward, these update pity.
pub(super) fn generate_card_reward(player: &mut MpPlayer, owner: u8, ascension: i32,
    act_index: i32, next_id: &mut u32, count: usize, room_type: RoomType,niche:&mut crate::rng::Rng) -> Result<Vec<Card>, &'static str> {
    if !matches!(room_type, RoomType::Monster | RoomType::Elite | RoomType::Boss) {
        return Err("unsupported multiplayer card reward room type");
    }
    let pool = super::super::card_pools::cards(player.character.into(), true)?;
    let mut cards: Vec<Card> = Vec::with_capacity(count);
    for _ in 0..count {
        let roll = player.rngs.rewards.next_float(1.0) as f32;
        let rarity = roll_card_rarity(&mut player.card_rarity_offset, roll, ascension, room_type);
        let candidates: Vec<_> = pool.iter().copied().filter(|id| !cards.iter().any(|c| c.id == *id))
            .filter(|id| Card::from_factory(*id, false, 0).is_some_and(|c| c.rarity == rarity)).collect();
        let id = candidates[player.rngs.rewards.choice_index(candidates.len())];
        let upgrade_roll = player.rngs.rewards.next_float(1.0);
        // CardFactory.RollForUpgrade always consumes the roll. Rare reward
        // cards keep base chance 0; non-rares add act-index scaling (A7+ halves it).
        let scaling = if ascension >= 7 { 0.125 } else { 0.25 };
        let upgrade_odds = if rarity == CardRarity::Rare { 0.0 } else { act_index as f64 * scaling };
        let upgraded = upgrade_roll <= upgrade_odds;
        *next_id += 1;
        let mut card = Card::from_factory(id, upgraded, *next_id).ok_or("reward card factory")?;
        card.owner = owner;
        cards.push(card);
    }
    apply_card_reward_modifiers(player, &mut cards, next_id,Some(room_type),niche)?;
    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, MpRoster, PlayerSpec};

    #[test]
    fn frozen_egg_clones_and_upgrades_power_options_only_for_its_owner() {
        let mut roster=MpRoster::new("73003",&[
            PlayerSpec{net_id:100,character:Character::Ironclad},
            PlayerSpec{net_id:101,character:Character::Ironclad},
        ]).unwrap();
        let p=roster.player_mut(1).unwrap();
        let mut relic=crate::relic::Relic::new(RelicId::FrozenEgg);relic.owner=1;
        p.relics.push(relic);
        let mut power=super::super::super::card_pools::cards(crate::character::CardPool::Ironclad,true).unwrap().into_iter()
            .filter_map(|id|Card::from_factory(id,false,200)).find(|c|c.card_type==crate::tables::CardType::Power).unwrap();
        power.owner=1;
        let mut attack=Card::from_factory(crate::tables::CardId::StrikeIronclad,false,201).unwrap();
        attack.owner=1;
        let mut cards=[power,attack];
        let mut next_id=300;
        let counter=p.rngs.rewards.counter;
        apply_card_reward_modifiers(p,&mut cards,&mut next_id,Some(RoomType::RestSite),&mut crate::rng::Rng::new(1)).unwrap();
        assert!(cards[0].upgraded);
        assert_eq!((cards[0].owner,cards[0].instance_id),(1,301));
        assert!(!cards[1].upgraded);
        assert_eq!(cards[1].instance_id,201);
        assert_eq!(p.rngs.rewards.counter,counter);
        assert_eq!(roster.player(0).unwrap().relics.len(),1);
    }

    #[test]
    fn rarity_pity_and_thresholds_use_csharp_float_arithmetic() {
        let mut offset = -0.05f32;
        for _ in 0..12 { assert_eq!(roll_card_rarity(&mut offset, 0.99, 0, RoomType::Monster), CardRarity::Common); }
        // Original-DLL action 331 records 0.06999999, not f64's 0.07.
        assert_eq!(offset, 0.06999999f32);
        // At equality C# uses '<': this is not a rare card. Double
        // accumulation would yield a 0.1 threshold and classify it Rare.
        assert_eq!(roll_card_rarity(&mut offset, 0.099999994f32, 0, RoomType::Monster), CardRarity::Uncommon);
    }

    #[test]
    fn no_gold_reward_is_distinct_from_a_positive_proportion_rounding_to_zero() {
        let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
            PlayerSpec { net_id: 2, character: Character::Necrobinder }];
        let roster = MpRoster::new("73001", &specs).unwrap();
        let mut absent = roster.player(0).unwrap().clone();
        let mut zero = absent.clone();
        let mut absent_id = roster.next_card_id;
        let mut zero_id = absent_id;
        let absent_reward = generate(&mut absent, 0, 0, 0, &mut absent_id, 0.0, RoomType::Monster,&mut crate::rng::Rng::new(1)).unwrap();
        let zero_reward = generate(&mut zero, 0, 0, 0, &mut zero_id, 0.001, RoomType::Monster,&mut crate::rng::Rng::new(1)).unwrap();
        assert!(absent_reward.gold.is_empty());
        assert_eq!(zero_reward.gold, vec![0]);
        // GoldReward.Populate still calls NextInt(0, 1) for a zero amount.
        assert_eq!(zero.rngs.rewards.counter, absent.rngs.rewards.counter + 1);
    }

    #[test]
    fn reward_upgrade_roll_uses_act_index_and_scarcity_scaling() {
        let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
            PlayerSpec { net_id: 2, character: Character::Necrobinder }];
        for ascension in [0, 10] {
            let mut saw_act_upgrade = false;
            for n in 0..512 {
                let seed = (73_001 + n).to_string();
                let roster = MpRoster::new(&seed, &specs).unwrap();
                let mut act0 = roster.player(0).unwrap().clone();
                let mut act1 = act0.clone();
                let mut id0 = roster.next_card_id;
                let mut id1 = id0;
                let reward0 = generate(&mut act0, 0, ascension, 0, &mut id0, 0.0, RoomType::Monster,&mut crate::rng::Rng::new(1)).unwrap();
                let reward1 = generate(&mut act1, 0, ascension, 1, &mut id1, 0.0, RoomType::Monster,&mut crate::rng::Rng::new(1)).unwrap();
                assert_eq!(reward0.cards.iter().map(|c| c.id).collect::<Vec<_>>(),
                    reward1.cards.iter().map(|c| c.id).collect::<Vec<_>>());
                if reward0.cards.iter().zip(&reward1.cards)
                    .any(|(a, b)| !a.upgraded && b.upgraded) {
                    saw_act_upgrade = true;
                    break;
                }
            }
            assert!(saw_act_upgrade, "act-index upgrade scaling was never observable at A{ascension}");
        }
    }

    #[test]
    fn elite_and_boss_rewards_follow_original_room_odds_and_gold() {
        // CardRarityOdds.cs: the A7 elite rare boundary is 0.05, exclusive.
        let mut offset = 0.0;
        assert_eq!(roll_card_rarity(&mut offset, 0.049, 10, RoomType::Elite), CardRarity::Rare);
        offset = 0.0;
        assert_eq!(roll_card_rarity(&mut offset, 0.05, 10, RoomType::Elite), CardRarity::Uncommon);
        assert_eq!(roll_card_rarity(&mut offset, 0.999, 10, RoomType::Boss), CardRarity::Rare);
        assert_eq!(offset, -0.05);

        let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
            PlayerSpec { net_id: 2, character: Character::Necrobinder }];
        for ascension in [0, 10] {
            for seed in 1..=64 {
                let roster = MpRoster::new(&seed.to_string(), &specs).unwrap();
                let mut elite = roster.player(0).unwrap().clone();
                let mut rng = elite.rngs.rewards.clone();
                let drop = (rng.next_float(1.0) as f32) < 0.4f32 + 0.25f32 * 0.5f32;
                let mut next_id = roster.next_card_id;
                let reward = generate(&mut elite, 0, ascension, 0, &mut next_id, 0.0, RoomType::Elite,&mut crate::rng::Rng::new(1)).unwrap();
                assert_eq!(reward.potion.is_some(), drop);
                let range = if ascension >= 3 { 26..=33 } else { 35..=45 };
                assert_eq!(reward.gold.len(), 1);
                assert!(range.contains(&reward.gold[0]));

                let mut boss = roster.player(0).unwrap().clone();
                let reward = generate(&mut boss, 0, ascension, 0, &mut next_id, 0.0, RoomType::Boss,&mut crate::rng::Rng::new(1)).unwrap();
                assert_eq!(reward.gold, vec![if ascension >= 3 { 75 } else { 100 }]);
                assert!(reward.cards.iter().all(|card| card.rarity == CardRarity::Rare));
                assert_eq!(boss.card_rarity_offset, -0.05);
            }
        }
    }

    #[test]
    fn silken_tress_glams_first_owner_card_reward_then_disables() {
        let specs = [
            PlayerSpec { net_id: 1, character: Character::Ironclad },
            PlayerSpec { net_id: 2, character: Character::Necrobinder },
        ];
        let roster = MpRoster::new("73003", &specs).unwrap();
        let mut owner = roster.player(0).unwrap().clone();
        owner.relics.push(crate::relic::Relic::new(crate::tables::RelicId::SilkenTress));
        let mut next_id = roster.next_card_id;
        let before_id = next_id;

        let reward = generate(&mut owner, 0, 0, 0, &mut next_id, 1.0, RoomType::Monster,&mut crate::rng::Rng::new(1)).unwrap();

        assert!(reward.cards.iter().any(|card|
            card.enchantments.iter().any(|(name, amount)| *name == crate::identity::EnchantmentId::Glam && *amount == 1)));
        assert!(next_id > before_id + reward.cards.len() as u32);
        assert!(!owner.relics.iter().find(|relic| relic.id == crate::tables::RelicId::SilkenTress)
            .unwrap().enabled);
    }
}
