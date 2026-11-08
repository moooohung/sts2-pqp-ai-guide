//! Owner-bound event transport; event rules are shared with the solo reducer.
use crate::character::CardPool;
use crate::identity::{EventId, EventOptionId};
include!(concat!(env!("OUT_DIR"), "/gen_mp_event_identity.rs"));
#[path = "tinker_time.rs"]
mod tinker_time;
#[path = "trial.rs"]
mod trial;
#[path = "stone_of_all_time.rs"]
mod stone_of_all_time;
#[path = "zen_weaver.rs"]
mod zen_weaver;
#[path = "relic_trader.rs"]
mod relic_trader;
use super::{MpPhase, MpRun};
use crate::{card::Card, relic::Relic, rng::Rng, run::{events::{EventEffect, EventOption, EventOptionIdentity, EventState}, manager::slippery_bridge},
    tables::{CardId, CardRarity, CardTag, CardType, MapPointType, PotionId, PotionRarity, RelicId, RelicRarity,
        POTION_INFO, RELIC_INFO, KW_ETERNAL}};

fn is_shared_event(id: EventId) -> bool {
    // C# EventModel.BeginEvent seeds owner-local events with the player slot,
    // while these models override IsShared and therefore share one event RNG.
    matches!(id, EventId::BattlewornDummy | EventId::DenseVegetation | EventId::FakeMerchant |
        EventId::JungleMazeAdventure | EventId::MorphicGrove | EventId::PunchOff |
        EventId::TheLanternKey | EventId::WarHistorianRepy)
}

#[derive(Clone, Debug)]
pub(super) enum EventCardAction {
    AddGenerated,
    TransformUpgrade,
    Upgrade,
    UpgradeAndDamage(i32),
    Remove,
    RemoveAndLoseGold(i32),
    Transform,
    TransformAndResumeConveyor,
    TransformAndHollow,
    TransformMaul,
    StorePaelsTooth,
    Enchant(crate::identity::EnchantmentId, i32),
    RemoveAndAdd(CardId),
}

#[derive(Clone, Debug)]
pub(super) struct EventCardModal {
    pub cards: Vec<Card>,
    pub min: usize,
    pub max: usize,
    pub skippable: bool,
    pub action: EventCardAction,
}

impl MpRun {
    /// CardCmd.TransformToRandom from Astrolabe/PandorasBox: Niche stream,
    /// upgrade replacements before insertion, then normal owner deck callbacks.
    fn darv_transform(&mut self,seat:usize,instance:u32,upgrade:bool)->Result<(),&'static str>{
        let original=self.roster.player(seat).unwrap().deck.iter().find(|c|c.instance_id==instance)
            .cloned().ok_or("Darv original missing")?;
        let pool=super::super::card_pools::transformations(&original,true)?;
        let id=pool[self.world.run.rngs.niche.choice_index(pool.len())];
        self.roster.next_card_id+=1;
        let mut card=Card::from_factory(id,false,self.roster.next_card_id).ok_or("Darv replacement factory")?;
        if upgrade{
            let mut ctx=self.rest_context(seat);
            super::super::shop::upgrade_template(&mut ctx,&mut card);self.carry_rest_context(seat,ctx);
        }
        card.owner=seat as u8;
        self.roster.player_mut(seat).unwrap().deck.retain(|c|c.instance_id!=instance);
        super::cards::add(&mut self.roster,&mut self.world.run,seat,card)
    }
    /// GlassEye/SeaGlass CardCreationOptions: canonical pool, uniform without
    /// replacement, no rarity/upgrade-roll draws, then late option modifiers.
    fn orobas_cards(&mut self,seat:usize,character:crate::character::CardPool,rarity:CardRarity,count:usize,is_reward:bool)
        ->Result<Vec<Card>,&'static str>{
        self.uniform_event_cards(seat,character,rarity,count,is_reward,true,false)
    }
    pub(super) fn uniform_event_cards(&mut self,seat:usize,character:crate::character::CardPool,rarity:CardRarity,count:usize,is_reward:bool,modify_pool:bool,roll_upgrade:bool)
        ->Result<Vec<Card>,&'static str>{
        let mut pool=super::super::card_pools::cards(character,true)?;
        if modify_pool&&is_reward&&self.roster.player(seat).unwrap().relics.iter().any(|r|r.id==RelicId::DingyRug&&r.enabled){
            for id in super::super::card_pools::cards(CardPool::Colorless,true)?{if !pool.contains(&id){pool.push(id);}}
        }
        let mut pool:Vec<_>=pool.into_iter().filter(|&id|
            crate::tables::card_info(id,false).is_some_and(|c|c.rarity==rarity)).collect();
        let mut cards=Vec::new();
        for _ in 0..count{
            if pool.is_empty(){break;}
            let pick=self.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(pool.len());
            let upgraded=if roll_upgrade {
                let roll=self.roster.player_mut(seat).unwrap().rngs.rewards.next_float(1.0);
                let odds=if rarity==CardRarity::Rare {0.0} else {
                    self.world.run.act_index as f64 * if self.world.run.ascension>=7 {0.125}else{0.25}
                };
                roll<=odds
            } else {false};
            self.roster.next_card_id+=1;
            let mut c=Card::from_factory(pool.remove(pick),upgraded,self.roster.next_card_id).ok_or("Orobas card factory")?;
            c.owner=seat as u8;cards.push(c);
        }
        let mut next_id=self.roster.next_card_id;
        let p=self.roster.player_mut(seat).unwrap();
        super::super::reward_modifiers::apply(&mut p.relics,&mut cards,seat as u8,Some(self.world.run.room_type),
            is_reward,&mut next_id,&mut self.world.run.rngs.niche,|card,next_id|{
                let enchants=card.enchantments.clone();*next_id+=1;
                let mut upgraded=Card::from_factory(card.id,true,*next_id).ok_or("Orobas upgrade factory")?;
                upgraded.owner=card.owner;
                for (name,amount) in enchants{
                    if !crate::card::enchant::apply_static_enchantment(&mut upgraded,name,amount){return Err("Orobas upgrade enchantment");}
                }
                *card=upgraded;Ok(())
            })?;
        self.roster.next_card_id=next_id;
        Ok(cards)
    }
    fn tezcatara_golden_map(&mut self,seat:usize,obtained:usize)->Result<(),&'static str>{
        // GoldenCompass.AfterObtained regenerates the ordinary map first;
        // GoldenPathActMap.cs then replaces it, removing row 3 in multiplayer.
        use super::super::map::{generate_act_map,ActMap,Coord};
        let act=self.world.run.act_index as usize;
        let generated=generate_act_map(self.world.act_defs[act].num_rooms-1,
            &mut self.world.map_rngs[act],self.world.run.ascension,act);
        let mut template=generated.points[generated.start as usize].clone();
        template.children.clear();template.parents.clear();template.can_be_modified=true;
        let kinds=[MapPointType::Monster,MapPointType::Unknown,
            MapPointType::RestSite,MapPointType::Monster,MapPointType::RestSite,
            MapPointType::Unknown,MapPointType::Treasure,MapPointType::Unknown,
            MapPointType::Treasure,MapPointType::Unknown,MapPointType::Shop,
            MapPointType::Elite,MapPointType::RestSite,MapPointType::Elite,MapPointType::RestSite];
        let mut points=Vec::new();
        for (row,kind) in std::iter::once((0,MapPointType::Ancient))
            .chain(std::iter::once((16,MapPointType::Boss)))
            .chain(kinds.into_iter().enumerate().map(|(i,k)|(i as i32+1,k))){
            let mut p=template.clone();p.coord=Coord::new(3,row);p.point_type=kind;points.push(p);
        }
        let chain:Vec<u8>=std::iter::once(0).chain(2..points.len() as u8).chain(std::iter::once(1)).collect();
        for pair in chain.windows(2){
            points[pair[0] as usize].children.push(pair[1]);points[pair[1] as usize].parents.push(pair[0]);
        }
        self.world.map=ActMap{map_length:16,points,start:0,boss:1,second_boss:None};
        let r=self.roster.player_mut(seat).unwrap().relics.get_mut(obtained)
            .ok_or("Golden Compass missing")?;
        r.flag1=true;r.counter2=act as i32;
        Ok(())
    }
    fn deck_candidates(&self, seat: usize, predicate: impl Fn(&Card) -> bool) -> Vec<Card> {
        self.roster.player(seat).unwrap().deck.iter().filter(|card| predicate(card)).cloned().collect()
    }

    pub(super) fn begin_event_card_modal(&mut self, seat: usize, cards: Vec<Card>, min: usize, max: usize,
        skippable: bool, action: EventCardAction) -> Result<(), &'static str> {
        if cards.len() < min { return Err("event card selector has too few candidates"); }
        self.event_card_modals[seat] = Some(EventCardModal { cards, min, max, skippable, action });
        Ok(())
    }

    fn transformable_event_cards(&self, seat: usize) -> Vec<Card> {
        self.roster.player(seat).unwrap().deck.iter()
            .filter(|card| super::super::card_pools::transformations(card, true)
                .is_ok_and(|pool| !pool.is_empty()))
            .cloned().collect()
    }

    pub(super) fn upgradable_event_cards(&self, seat: usize) -> Vec<Card> {
        let mut run = self.rest_context(seat);
        let indices = super::super::rest::upgradable_indices(&mut run);
        indices.into_iter().map(|index| self.roster.player(seat).unwrap().deck[index].clone()).collect()
    }

    pub(super) fn enchantable_event_cards(&self, seat: usize, name: &str, card_type: Option<CardType>) -> Vec<Card> {
        self.roster.player(seat).unwrap().deck.iter()
            .filter(|card| card_type.is_none_or(|ty| card.card_type == ty)
                && super::super::manager::RunManager::event_can_enchant(card, name))
            .cloned().collect()
    }

    pub(super) fn removable_event_cards(&self, seat: usize) -> Vec<Card> {
        let indices = super::super::manager::RunManager::removable_card_indices(
            &self.roster.player(seat).unwrap().deck);
        indices.into_iter().map(|index| self.roster.player(seat).unwrap().deck[index].clone()).collect()
    }

    fn basic_transform_event_cards(&self, seat: usize) -> Vec<Card> {
        self.roster.player(seat).unwrap().deck.iter().filter(|card|
            card.rarity == CardRarity::Basic && card.id != CardId::AscendersBane
                && card.card_type != CardType::Quest && !card.has_kw(KW_ETERNAL))
            .cloned().collect()
    }

    fn resume_mp_conveyor(&self, seat: usize, event: &mut EventState) {
        let player = self.roster.player(seat).unwrap();
        // EndlessConveyor.cs:104-147 uses owner-local gold, open potion slots,
        // HP, and event RNG. The room itself is not shared.
        super::super::manager::conveyor::resume_conveyor_state(
            event,
            player.gold,
            player.potions.iter().any(Option::is_none),
            player.hp != player.max_hp,
        );
    }

    fn upgrade_one_conveyor_card(&mut self, seat: usize, event: &mut EventState) {
        // EndlessConveyor.cs:200-208,265-274 uses NextItem over deck-order
        // upgradable cards rather than StableShuffle.
        let candidates = self.upgradable_event_cards(seat);
        if candidates.is_empty() { return; }
        let instance_id = candidates[event.rng.choice_index(candidates.len())].instance_id;
        let mut run = self.rest_context(seat);
        if let Some(index) = run.deck.iter().position(|card| card.instance_id == instance_id) {
            run.upgrade_deck_card(index);
        }
        self.carry_rest_context(seat, run);
    }

    fn transform_conveyor_card(
        &mut self,
        seat: usize,
        event: &mut EventState,
        instance_id: u32,
    ) -> Result<(), &'static str> {
        let selected = self.roster.player(seat).unwrap().deck.iter()
            .find(|card| card.instance_id == instance_id)
            .cloned().ok_or("conveyor transform card missing")?;
        let index = self.roster.player(seat).unwrap().deck.iter()
            .position(|card| card.instance_id == instance_id)
            .ok_or("conveyor transform card missing")?;
        let pool = super::super::card_pools::transformations(&selected, true)?;
        if pool.is_empty() { return Err("empty conveyor transform pool"); }
        let id = pool[event.rng.choice_index(pool.len())];
        self.world.run.next_card_id = self.world.run.next_card_id.max(self.roster.next_card_id);
        let iid = self.world.run.new_card_id();
        self.roster.next_card_id = self.roster.next_card_id.max(iid);
        let mut card = Card::from_factory(id, false, iid).ok_or("conveyor transform factory")?;
        card.owner = seat as u8;
        self.roster.player_mut(seat).unwrap().remove_deck_card(index);
        super::cards::add(&mut self.roster, &mut self.world.run, seat, card)
    }

    fn generate_conveyor_eel_card(&mut self, seat: usize) -> Result<Card, &'static str> {
        // EndlessConveyor.cs:188-193: Colorless default rarity odds, then a
        // direct CreateForReward with no upgrade roll.
        let roll = self.roster.player_mut(seat).unwrap().rngs.rewards.next_float(1.0) as f32;
        let (uncommon, rare) = if self.world.run.ascension >= 7 {
            (0.37f32, 0.0149f32)
        } else {
            (0.37f32, 0.03f32)
        };
        let mut rarity = if roll < rare { CardRarity::Rare }
            else if roll < uncommon + rare { CardRarity::Uncommon }
            else { CardRarity::Common };
        let initial = rarity;
        let pool = super::super::card_pools::cards(CardPool::Colorless, true)?;
        let candidates = loop {
            let candidates: Vec<_> = pool.iter().copied()
                .filter(|id| Card::from_factory(*id, false, 0)
                    .is_some_and(|card| card.rarity == rarity))
                .collect();
            if !candidates.is_empty() { break candidates; }
            rarity = match rarity {
                CardRarity::Common => CardRarity::Uncommon,
                CardRarity::Uncommon => CardRarity::Rare,
                CardRarity::Rare => CardRarity::Common,
                _ => return Err("unsupported conveyor card rarity"),
            };
            if rarity == initial { return Err("conveyor colorless pool exhausted"); }
        };
        let pick = self.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(candidates.len());
        self.roster.next_card_id += 1;
        self.world.run.next_card_id = self.world.run.next_card_id.max(self.roster.next_card_id);
        let mut card = Card::from_factory(candidates[pick], false, self.roster.next_card_id)
            .ok_or("conveyor colorless card factory")?;
        card.owner = seat as u8;
        Ok(card)
    }

    fn initialize_future_potions_event(
        &self,
        seat: usize,
        event: &mut EventState,
    ) -> Result<(), &'static str> {
        // TheFutureOfPotions.cs:34-64,101-120 lazily assigns a random type to
        // every held potion in inventory order, but exposes only the first 3.
        let held: Vec<_> = self.roster.player(seat).unwrap().potions.iter().enumerate()
            .filter_map(|(slot, potion)| potion.map(|id| (slot, id))).collect();
        if held.len() < 2 { return Err("Future of Potions needs two held potions"); }
        for (i, (slot, id)) in held.into_iter().enumerate() {
            let count = match POTION_INFO[id.index()].rarity {
                PotionRarity::Common | PotionRarity::Token => 2,
                _ => 3,
            };
            let card_type = event.rng.next_int_exclusive(0, count);
            if i < 3 {
                event.ints[i] = ((slot as i32) << 2) | card_type as i32;
                event.options.push(EventOption::new(
                    ["trade_0", "trade_1", "trade_2"][i],
                    "Trade",
                    "Discard potion for upgraded card reward",
                ));
            }
        }
        Ok(())
    }

    fn generate_future_potion_cards(
        &mut self,
        seat: usize,
        rarity: CardRarity,
        kind: CardType,
    ) -> Result<Vec<Card>, &'static str> {
        // TheFutureOfPotions.cs:123-138: owner pool, uniform same-rarity/type
        // options, no pool/rarity modification, then normal CardReward option
        // modifiers before every offered card is upgraded.
        let character = self.roster.player(seat).unwrap().character;
        let pool = super::super::card_pools::cards(character.into(), true)?;
        let mut available: Vec<_> = pool.into_iter().filter(|id|
            Card::from_factory(*id, false, 0)
                .is_some_and(|card| card.rarity == rarity && card.card_type == kind))
            .collect();
        let mut cards = Vec::new();
        for _ in 0..3 {
            if available.is_empty() { break; }
            let pick = self.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(available.len());
            let id = available.remove(pick);
            self.roster.next_card_id += 1;
            let mut card = Card::from_factory(id, false, self.roster.next_card_id)
                .ok_or("Future of Potions card factory")?;
            card.owner = seat as u8;
            cards.push(card);
        }
        if cards.is_empty() { return Err("Future of Potions empty card pool"); }
        let mut next_id = self.roster.next_card_id;
        super::rewards::apply_card_reward_modifiers(
            self.roster.player_mut(seat).unwrap(),
            &mut cards,
            &mut next_id,
            Some(self.world.run.room_type),
            &mut self.world.run.rngs.niche,
        )?;
        self.roster.next_card_id = next_id;
        self.world.run.next_card_id = self.world.run.next_card_id.max(next_id);
        let mut run = self.rest_context(seat);
        for card in &mut cards {
            let owner = card.owner;
            let enchantments = card.enchantments.clone();
            super::super::shop::upgrade_template(&mut run, card);
            card.owner = owner;
            for (name, amount) in enchantments {
                if !crate::card::enchant::apply_static_enchantment(card, name, amount) {
                    return Err("unsupported Future of Potions reward enchantment");
                }
            }
        }
        self.carry_rest_context(seat, run);
        Ok(cards)
    }

    fn trade_future_potion(
        &mut self,
        seat: usize,
        mut event: EventState,
        option_id: EventOptionIdentity,
    ) -> Result<(), &'static str> {
        let index = event.options.iter().position(|option| option.option_id == option_id)
            .ok_or("invalid Future of Potions option")?;
        let encoded = event.ints[index];
        let slot = (encoded >> 2) as usize;
        let potion = self.roster.player(seat).unwrap().potions.get(slot)
            .copied().flatten().ok_or("Future of Potions potion missing")?;
        let kind = [CardType::Attack, CardType::Skill, CardType::Power][(encoded & 3) as usize];
        let rarity = match POTION_INFO[potion.index()].rarity {
            PotionRarity::Common | PotionRarity::Token => CardRarity::Common,
            PotionRarity::Uncommon => CardRarity::Uncommon,
            _ => CardRarity::Rare,
        };
        self.roster.player_mut(seat).unwrap().potions[slot] = None;
        let cards = self.generate_future_potion_cards(seat, rarity, kind)?;
        event.options.clear();
        self.events[seat] = Some(event);
        self.event_card_reward_queues[seat]=vec![super::PlayerRewards{cards,..Default::default()}];
        Ok(())
    }

    fn choose_mp_conveyor(
        &mut self,
        seat: usize,
        mut event: EventState,
        option_id: EventOptionIdentity,
    ) -> Result<(), &'static str> {
        if option_id.is(EventOptionId::EndlessConveyorInitialObserveChef) {
            self.upgrade_one_conveyor_card(seat, &mut event);
            self.finish_event_seat(seat);
            return Ok(());
        }
        if option_id.is(EventOptionId::EndlessConveyorGrabSomethingOffTheBeltLeave) {
            self.finish_event_seat(seat);
            return Ok(());
        }
        if !option_id.is(EventOptionId::EndlessConveyorAllId) { return Err("invalid Endless Conveyor option"); }
        let dish = super::super::manager::conveyor::Dish::from_index(event.ints[1]);
        if dish != super::super::manager::conveyor::Dish::Fysh {
            let player = self.roster.player_mut(seat).unwrap();
            if player.gold < 40 { return Err("Endless Conveyor grab cannot be afforded"); }
            player.gold -= 40;
        }
        match dish {
            super::super::manager::conveyor::Dish::Caviar => {
                let player = self.roster.player_mut(seat).unwrap();
                player.max_hp += 4;
                player.hp += 4;
            }
            super::super::manager::conveyor::Dish::Clam => {
                let player = self.roster.player_mut(seat).unwrap();
                player.hp = (player.hp + 10).min(player.max_hp);
            }
            super::super::manager::conveyor::Dish::Fysh => {
                self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run,75);
            }
            super::super::manager::conveyor::Dish::Spicy => {
                self.upgrade_one_conveyor_card(seat, &mut event);
            }
            super::super::manager::conveyor::Dish::Salad => {
                self.add_event_card(seat, CardId::FeedingFrenzyCard)?;
            }
            super::super::manager::conveyor::Dish::Eel => {
                let card = self.generate_conveyor_eel_card(seat)?;
                super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?;
            }
            super::super::manager::conveyor::Dish::Jelly => {
                let cards = self.transformable_event_cards(seat);
                if cards.len() > 1 {
                    self.events[seat] = Some(event);
                    return self.begin_event_card_modal(
                        seat, cards, 1, 1, false, EventCardAction::TransformAndResumeConveyor);
                }
                if let Some(card) = cards.first() {
                    self.transform_conveyor_card(seat, &mut event, card.instance_id)?;
                }
            }
            super::super::manager::conveyor::Dish::Condiment => {
                let character = self.roster.player(seat).unwrap().character;
                let pool = super::super::potion_pools::cards(character)
                    .ok_or("Endless Conveyor potion pool")?;
                if pool.is_empty() { return Err("Endless Conveyor potion pool empty"); }
                let pick = self.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(pool.len());
                self.events[seat] = Some(event);
                return self.begin_event_potion_reward(seat, pool[pick]);
            }
        }
        self.resume_mp_conveyor(seat, &mut event);
        self.events[seat] = Some(event);
        Ok(())
    }

    pub(super) fn upgrade_random_event_cards(&mut self, seat: usize, event: &mut EventState, count: usize) {
        let mut run = self.rest_context(seat);
        let mut candidates: Vec<crate::card::KRef> = (0..run.deck.len())
            .filter(|index| run.is_deck_card_upgradable(*index))
            .map(|index| index as crate::card::KRef).collect();
        crate::flow::dotnet_sort_cards(&mut candidates, &run.deck);
        event.rng.shuffle(&mut candidates);
        for index in candidates.into_iter().take(count) {
            run.upgrade_deck_card(index as usize);
        }
        self.carry_rest_context(seat, run);
    }

    pub(super) fn pull_event_relic_from_front(&mut self, seat: usize) -> Result<RelicId, &'static str> {
        // RelicFactory.PullNextRelicFromFront(owner): the owner's Rewards RNG
        // rolls rarity, then the grab bag filters eligibility and escalates to
        // higher rarities, the MP fallback queue, and finally Circlet.
        let roll = self.roster.player_mut(seat).unwrap().rngs.rewards.next_float(1.0) as f32;
        let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
        Ok(self.world.pull_player_relic_from_front(&self.roster, seat, rarity))
    }

    fn begin_event_potion_reward(&mut self, seat: usize, potion: PotionId) -> Result<(), &'static str> {
        if self.event_potion_rewards[seat].is_some() {
            return Err("event potion reward already begun");
        }
        self.event_potion_rewards[seat] = Some([Some(potion), None, None]);
        Ok(())
    }

    pub fn event_potion_rewards(&self, net_id: u64, seat: usize)
        -> Result<&[Option<PotionId>; 3], &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event potion reward"); }
        self.event_potion_rewards[seat].as_ref().ok_or("event potion rewards missing")
    }

    pub fn take_event_potion_reward(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event potion reward"); }
        if index >= 3 { return Err("invalid event potion reward index"); }
        let potion = self.event_potion_rewards[seat].as_ref()
            .ok_or("event potion rewards missing")?[index]
            .ok_or("event potion reward already selected")?;
        let mut next = self.clone();
        let procured = {
            let player = next.roster.player_mut(seat).unwrap();
            if player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                false
            } else if let Some(slot) = player.potions.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(potion);
                true
            } else {
                return Ok(false);
            }
        };
        if procured { next.mark_local_potion_seen(seat, potion); }
        next.event_potion_rewards[seat].as_mut().unwrap()[index] = None;
        *self = next;
        Ok(true)
    }

    pub fn skip_event_potion_reward(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event potion reward"); }
        if !self.events[seat].as_ref().is_some_and(|event| event.id == EventId::PotionCourier) {
            return Err("not at Potion Courier");
        }
        let reward = self.event_potion_rewards[seat].as_mut()
            .and_then(|rewards| rewards.get_mut(index)).ok_or("invalid event potion reward index")?;
        if reward.take().is_none() { return Err("event potion reward already selected"); }
        Ok(())
    }

    pub fn finish_event_potion_rewards(&mut self, net_id: u64, seat: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event potion reward"); }
        let rewards = self.event_potion_rewards[seat].as_ref()
            .ok_or("event potion rewards missing")?;
        if rewards.iter().any(Option::is_some) { return Err("event potion rewards remain"); }
        let mut next = self.clone();
        next.event_potion_rewards[seat] = None;
        if next.events[seat].as_ref().is_some_and(|event| event.id == EventId::EndlessConveyor) {
            let mut event = next.events[seat].take().unwrap();
            next.resume_mp_conveyor(seat, &mut event);
            next.events[seat] = Some(event);
        } else {
            next.finish_event_seat(seat);
        }
        *self = next;
        Ok(())
    }

    pub fn event_relic_reward(&self, net_id: u64, seat: usize) -> Result<RelicId, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event relic reward"); }
        self.relic_rewards[seat].ok_or("event relic reward missing")
    }

    pub fn select_event_relic_reward(&mut self, net_id: u64, seat: usize, accept: bool)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event relic reward"); }
        let id = self.relic_rewards[seat].ok_or("event relic reward missing")?;
        if !self.relic_reward_skippable[seat] && !accept {
            return Err("event relic reward cannot be skipped");
        }
        let mut next = self.clone();
        next.relic_rewards[seat] = None;
        next.relic_reward_skippable[seat] = true;
        if accept {
            next.attach_event_relic(seat, id)?;
            next.record_relic_seen(seat, id);
        }
        next.finish_event_seat(seat);
        *self = next;
        Ok(())
    }

    pub fn event_card_modal(&self, net_id: u64, seat: usize)
        -> Result<(&[Card], usize, usize, bool), &'static str> {
        self.roster.actor(net_id, seat)?;
        if let Some(reward)=self.event_card_reward_queues[seat].first(){
            return Ok((&reward.cards,0,1,true));
        }
        if self.phase != MpPhase::Event { return Err("not at event"); }
        let modal = self.event_card_modals.get(seat).and_then(Option::as_ref)
            .ok_or("event card selector missing")?;
        Ok((&modal.cards, modal.min, modal.max, modal.skippable))
    }

    pub fn select_event_cards(&mut self, net_id: u64, seat: usize, indices: &[usize])
        -> Result<(), &'static str> {
        if self.phase==MpPhase::Event&&!self.event_card_reward_queues.get(seat).ok_or("invalid event owner")?.is_empty(){
            self.roster.actor(net_id,seat)?;
            if indices.len()>1{return Err("invalid card reward selection");}
            let mut next=self.clone();
            if let Some(&i)=indices.first(){next.take_card_reward(net_id,seat,i)?;}
            next.finish_event_card_reward(net_id,seat)?;*self=next;return Ok(());
        }
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event"); }
        let mut next = self.clone();
        let modal = next.event_card_modals[seat].take().ok_or("event card selector missing")?;
        if indices.is_empty() && !modal.skippable { return Err("event card selector cannot be skipped"); }
        if !indices.is_empty() && (indices.len() < modal.min || indices.len() > modal.max) {
            return Err("invalid event card selection count");
        }
        let mut unique = indices.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != indices.len() || unique.iter().any(|index| *index >= modal.cards.len()) {
            return Err("invalid event card selection");
        }
        let selected: Vec<Card> = indices.iter().map(|index| modal.cards[*index].clone()).collect();
        let mut event = next.events[seat].take().ok_or("event already finished")?;
        match modal.action {
            EventCardAction::AddGenerated => {
                for card in selected { super::cards::add(&mut next.roster, &mut next.world.run, seat, card)?; }
            }
            EventCardAction::TransformUpgrade=>{
                for card in selected{next.darv_transform(seat,card.instance_id,true)?;}
            }
            EventCardAction::Upgrade | EventCardAction::UpgradeAndDamage(_) => {
                let ids: Vec<_> = selected.iter().map(|card| card.instance_id).collect();
                let mut run = next.rest_context(seat);
                let indices: Vec<_> = ids.iter().filter_map(|id|
                    run.deck.iter().position(|card| card.instance_id == *id)).collect();
                for index in indices { run.upgrade_deck_card(index); }
                next.carry_rest_context(seat, run);
            }
            EventCardAction::Remove | EventCardAction::RemoveAndLoseGold(_) => {
                let ids: Vec<_> = selected.iter().map(|card| card.instance_id).collect();
                next.roster.player_mut(seat).unwrap().retain_deck_cards(|card| !ids.contains(&card.instance_id));
                if let EventCardAction::RemoveAndLoseGold(cost) = modal.action {
                    next.apply_event_effect(seat, EventEffect::LoseGold(cost), &mut event)?;
                }
            }
            EventCardAction::Transform | EventCardAction::TransformAndResumeConveyor | EventCardAction::TransformAndHollow => {
                for selected in selected {
                    next.transform_conveyor_card(seat, &mut event, selected.instance_id)?;
                }
            }
            EventCardAction::Enchant(name, amount) => {
                for selected in selected {
                    let card = next.roster.player_mut(seat).unwrap().deck.iter_mut()
                        .find(|card| card.instance_id == selected.instance_id)
                        .ok_or("event enchant card missing")?;
                    if !crate::card::enchant::apply_static_enchantment(card, name, amount) {
                        return Err("event enchantment capacity exceeded");
                    }
                }
            }
            EventCardAction::RemoveAndAdd(id) => {
                let ids: Vec<_> = selected.iter().map(|card| card.instance_id).collect();
                next.roster.player_mut(seat).unwrap().retain_deck_cards(|card| !ids.contains(&card.instance_id));
                next.add_event_card(seat, id)?;
            }
            EventCardAction::TransformMaul=>{
                for card in selected {
                    next.roster.next_card_id+=1;
                    let replacement=super::super::tanx::maul(&card,next.roster.next_card_id)?;
                    let index=next.roster.player(seat).unwrap().deck.iter().position(|c|c.instance_id==card.instance_id).unwrap();
                    next.roster.player_mut(seat).unwrap().retain_deck_cards(|c|c.instance_id!=card.instance_id);
                    let added=next.roster.player(seat).unwrap().deck.len();
                    super::cards::add(&mut next.roster,&mut next.world.run,seat,replacement)?;
                    let replacement=next.roster.player_mut(seat).unwrap().remove_deck_card(added);
                    next.roster.player_mut(seat).unwrap().deck.insert(index,replacement);
                }
            }
            EventCardAction::StorePaelsTooth=>{
                let mut ctx=next.rest_context(seat);
                let indices:Vec<_>=selected.iter().map(|card|
                    ctx.deck.iter().position(|c|c.instance_id==card.instance_id).unwrap()).collect();
                ctx.paels_tooth_store_selected(&indices);
                next.carry_rest_context(seat,ctx);
            }
        }
        let resume_conveyor = matches!(modal.action, EventCardAction::TransformAndResumeConveyor);
        if let EventCardAction::UpgradeAndDamage(amount) = modal.action {
            next.apply_event_effect(seat, EventEffect::LoseHp(amount), &mut event)?;
        }
        if event.id == EventId::StoneOfAllTime {
            event.rng.next_int_exclusive(0, 100);
        }
        if matches!(modal.action,EventCardAction::TransformAndHollow) {
            next.apply_event_effect(seat,EventEffect::LoseHp(9),&mut event)?;
        }
        if resume_conveyor {
            next.resume_mp_conveyor(seat, &mut event);
            next.events[seat] = Some(event);
        } else {
            next.events[seat] = Some(event);
            next.finish_event_seat(seat);
        }
        *self = next;
        Ok(())
    }

    pub(super) fn attach_event_relic(&mut self, seat: usize, id: RelicId) -> Result<usize, &'static str> {
        let index=self.roster.player(seat).ok_or("invalid relic owner")?.relics.len();
        if matches!(RELIC_INFO[id.index()].rarity,
            RelicRarity::Common | RelicRarity::Uncommon | RelicRarity::Rare)
            || id == RelicId::NeowsTalisman
        {
            self.roster.obtain_inventory_relic(&mut self.world.run, seat, id)?;
            return Ok(index);
        }
        // RelicCmd.Obtain -> Player.AddRelicInternal appends a new model even
        // when the ID is already owned. IsStackable controls bag removal only.
        let mut relic = Relic::new(id);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        Ok(index)
    }

    pub(super) fn event_is_allowed(&self, id: crate::identity::EventId) -> bool {
        let act = self.world.run.act_index;
        let floor = self.world.run.total_floor;
        let removable = |seat: usize| !super::super::manager::RunManager::removable_card_indices(
            &self.roster.player(seat).unwrap().deck).is_empty();
        let transformable_count = |seat: usize| super::super::manager::RunManager::removable_card_indices(
            &self.roster.player(seat).unwrap().deck).len();
        let can_enchant = |seat: usize, name: &str| self.roster.player(seat).unwrap().deck.iter()
            .any(|card| super::super::manager::RunManager::event_can_enchant(card, name));
        let tradable_count = |seat: usize| self.roster.player(seat).unwrap().relics.iter()
            .filter(|relic| Self::tradable_relic(relic)).count();
        let all = |pred: &dyn Fn(usize) -> bool| (0..self.roster.players().len()).all(pred);

        match id {
            // C# event IsAllowed overrides are RunState/party predicates. The
            // room is shared, so any owner that cannot satisfy a required
            // inventory/HP/gold precondition makes the event ineligible.
            EventId::BrainLeech | EventId::RoomFullOfCheese => act < 2,
            EventId::TheLegendsWereTrue => act == 0 && all(&|seat| {
                let p = self.roster.player(seat).unwrap(); !p.deck.is_empty() && p.hp >= 10
            }),
            EventId::TeaMaster => act < 2 && all(&|seat| self.roster.player(seat).unwrap().gold >= 150),
            EventId::CrystalSphere => act > 0 && all(&|seat| self.roster.player(seat).unwrap().gold >= 100),
            EventId::DollRoom => act == 1,
            EventId::EndlessConveyor => all(&|seat| self.roster.player(seat).unwrap().gold >= 120),
            EventId::JungleMazeAdventure => self.roster.players().len() == 1
                || all(&|seat| self.roster.player(seat).unwrap().hp > 18),
            EventId::LuminousChoir => all(&|seat| self.roster.player(seat).unwrap().gold >= 149
                && self.world.player_bags[seat].iter().any(|bag| !bag.is_empty())),
            EventId::MorphicGrove => all(&|seat| self.roster.player(seat).unwrap().gold >= 100
                && transformable_count(seat) >= 2),
            EventId::PotionCourier => act > 0,
            EventId::RanwidTheElder => act > 0 && all(&|seat| {
                let p = self.roster.player(seat).unwrap();
                p.gold >= 100 && p.potions.iter().any(Option::is_some) && tradable_count(seat) >= 1
            }),
            EventId::RelicTrader => act > 0 && all(&|seat| tradable_count(seat) >= 5),
            EventId::SlipperyBridge => floor > 6 && all(&removable),
            EventId::SpiralingWhirlpool => all(&|seat| can_enchant(seat, "Spiral")),
            EventId::StoneOfAllTime => act == 1
                && all(&|seat| self.roster.player(seat).unwrap().potions.iter().any(Option::is_some)),
            EventId::Symbiote => act > 0,
            EventId::FieldOfManSizedHoles => all(&|seat| can_enchant(seat, "PerfectFit")),
            EventId::TheFutureOfPotions => all(&|seat| self.roster.player(seat).unwrap().potions
                .iter().filter(|p| p.is_some()).count() >= 2),
            EventId::WaterloggedScriptorium => all(&|seat| self.roster.player(seat).unwrap().gold >= 55),
            EventId::WelcomeToWongos => act == 1 && all(&|seat| self.roster.player(seat).unwrap().gold >= 100),
            EventId::WhisperingHollow => all(&|seat| self.roster.player(seat).unwrap().gold >= 44),
            EventId::Amalgamator => all(&|seat| {
                let p = self.roster.player(seat).unwrap();
                let eligible = |card: &&Card| card.rarity == CardRarity::Basic
                    && card.id != CardId::AscendersBane && card.card_type != CardType::Quest
                    && !card.has_kw(KW_ETERNAL);
                p.deck.iter().filter(eligible).filter(|c| c.has_tag(CardTag::Strike)).count() >= 2
                    && p.deck.iter().filter(eligible).filter(|c| c.has_tag(CardTag::Defend)).count() >= 2
            }),
            EventId::ByrdonisNest => all(&|seat| {
                let p = self.roster.player(seat).unwrap();
                !p.relics.iter().any(|r| matches!(r.id, RelicId::Byrdpip | RelicId::PaelsLegion))
                    && !p.deck.iter().any(|c| c.id == CardId::ByrdonisEgg)
            }),
            EventId::GraveOfTheForgotten => all(&|seat| can_enchant(seat, "SoulsPower")),
            EventId::RoundTeaParty => all(&|seat| self.roster.player(seat).unwrap().hp >= 12),
            EventId::ColossalFlower => all(&|seat| self.roster.player(seat).unwrap().hp >= 19),
            EventId::DenseVegetation => self.roster.players().len() == 1
                || all(&|seat| self.roster.player(seat).unwrap().hp > 8),
            EventId::PunchOff => floor >= 6,
            EventId::TrashHeap => all(&|seat| self.roster.player(seat).unwrap().hp > 5),
            EventId::UnrestSite => all(&|seat| {
                let p = self.roster.player(seat).unwrap(); p.hp * 100 <= p.max_hp * 70
            }),
            EventId::WoodCarvings => all(&|seat| self.roster.player(seat).unwrap().deck.iter().any(|card|
                card.rarity == CardRarity::Basic && card.id != CardId::AscendersBane
                    && card.card_type != CardType::Quest && !card.has_kw(KW_ETERNAL))),
            EventId::ZenWeaver => all(&|seat| self.roster.player(seat).unwrap().gold >= 125),
            // FakeMerchant.IsAllowed explicitly rejects multiplayer parties.
            EventId::Neow | EventId::TheArchitect | EventId::FakeMerchant | EventId::WarHistorianRepy |
            EventId::DeprecatedEvent | EventId::DeprecatedAncientEvent => false,
            _ => true,
        }
    }

    pub(super) fn begin_generic_event(&mut self, id: crate::identity::EventId) -> Result<(), &'static str> {
        self.shared_event_votes.fill(None);
        if matches!(id, EventId::PunchOff | EventId::TheLanternKey) {
            // Combat-layout events create CombatStateForLayout before options
            // are chosen. Keep that exact enemy layout dormant until Fight.
            let encounter=if id == EventId::PunchOff {
                crate::identity::EncounterId::PunchOffEventEncounter
            } else {
                crate::identity::EncounterId::MysteriousKnightEventEncounter
            };
            self.combat = Some(self.roster.prepare_combat_id(
                self.world.run.clone(), self.world.run.rngs.seed,
                encounter, crate::tables::RoomType::Monster)?);
            // Layout enemy global IDs consume shared Niche RNG even when the
            // players choose the noncombat option.
            self.world.run.rngs = self.combat.as_ref().unwrap().run.rngs.clone();
        }
        for seat in 0..self.roster.players().len() {
            let player = self.roster.player(seat).unwrap();
            self.event_done[seat] = player.hp <= 0;
            if player.hp <= 0 {
                self.events[seat] = None;
                continue;
            }
            let has_pillow = player.relics.iter().any(|r|
                r.id == RelicId::RegalPillow && r.enabled);
            let rest_heal = player.max_hp * 3 / 10 + if has_pillow { 15 } else { 0 };
            let event_seed = self.world.run.rngs.seed.wrapping_add(
                if is_shared_event(id) { 0 } else { seat as u64 });
            let mut event = match super::super::events::initialize(
                id, event_seed, player.gold, rest_heal, player.hp, player.max_hp) {
                Some(event) => event,
                None if matches!(id,EventId::TheFutureOfPotions|EventId::Tanx|EventId::Pael|EventId::Tezcatara|EventId::Orobas|EventId::Darv) => EventState::new(
                    id,
                    Rng::new(event_seed.wrapping_add(crate::rng::deterministic_hash64(id.name()))),
                ),
                None => return Err(match id {
                    EventId::CrystalSphere => "unported multiplayer event model CRYSTAL_SPHERE",
                    EventId::DollRoom => "unported multiplayer event model DOLL_ROOM",
                    EventId::RanwidTheElder => "unported multiplayer event model RANWID_THE_ELDER",
                    EventId::RelicTrader => "unported multiplayer event model RELIC_TRADER",
                    EventId::StoneOfAllTime => "unported multiplayer event model STONE_OF_ALL_TIME",
                    EventId::WelcomeToWongos => "unported multiplayer event model WELCOME_TO_WONGOS",
                    // Preserve the failed model's identity even when map entry
                    // is rolled back; the previous Ancient is not this event.
                    _ => id.name(),
                }),
            };
            if id == EventId::WelcomeToWongos {
                self.initialize_wongo_event(seat, &mut event)?;
            } else if id == EventId::ColorfulPhilosophers {
                self.initialize_philosophers(seat, &mut event);
            } else if id == EventId::TheFutureOfPotions {
                self.initialize_future_potions_event(seat, &mut event)?;
            } else if id == EventId::RelicTrader {
                self.initialize_relic_trader_event(seat, &mut event)?;
            } else if id == EventId::StoneOfAllTime {
                self.initialize_stone_event(seat, &mut event);
            } else if id==EventId::Darv{
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                super::super::manager::RunManager::populate_darv_options(&mut event,p.character,
                    self.world.run.act_index as usize,true,&mut p.rngs.rewards)?;
            } else if id==EventId::Orobas{
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                super::super::manager::RunManager::populate_orobas_options(&mut event,p.character,&p.deck,&p.relics);
            } else if id==EventId::Tezcatara{
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                let soup=p.deck.iter().any(|c|c.rarity==CardRarity::Basic&&c.has_tag(CardTag::Strike));
                super::super::manager::RunManager::populate_tezcatara_options(&mut event,soup);
            } else if id==EventId::Nonupeipe{
                let allowed=self.enchantable_event_cards(seat,"Swift",None).len()>=4;
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;
                p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                super::super::manager::RunManager::populate_nonupeipe_options(&mut event,allowed);
            } else if id==EventId::Vakuu{
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;
                p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
            } else if id==EventId::Pael{
                let removable=self.removable_event_cards(seat).len();
                let p=self.roster.player_mut(seat).unwrap();
                let missing=p.max_hp-p.hp;
                p.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                let pet=p.relics.iter().any(|r|matches!(r.id,RelicId::Byrdpip|RelicId::PaelsLegion))
                    ||p.deck.iter().any(|c|c.id==CardId::ByrdonisEgg);
                super::super::manager::RunManager::populate_pael_options(&mut event,&p.deck,removable,pet);
            } else if id==EventId::Tanx {
                let player=self.roster.player_mut(seat).unwrap();
                let missing=player.max_hp-player.hp;
                player.hp+=if self.world.run.ascension>=2{missing*4/5}else{missing};
                super::super::tanx::options(&mut event,&player.deck);
            } else if id == EventId::EndlessConveyor {
                self.resume_mp_conveyor(seat, &mut event);
            }
            // C# locked EventOptions have null actions. Preserve that by
            // exposing only the choices the owner can actually take.
            if id == EventId::SelfHelpBook {
                let attack = self.enchantable_event_cards(seat, "Sharp", Some(CardType::Attack));
                let skill = self.enchantable_event_cards(seat, "Nimble", Some(CardType::Skill));
                let power = self.enchantable_event_cards(seat, "Swift", Some(CardType::Power));
                if attack.is_empty() && skill.is_empty() && power.is_empty() {
                    event.options.clear();
                    event.options.push(EventOption::new("skip_book", "Skip", ""));
                } else {
                    for option in &mut event.options {
                        option.enabled = match option.option_id.id {
                            EventOptionId::SelfHelpBookInitialReadTheBack => !attack.is_empty(),
                            EventOptionId::SelfHelpBookInitialReadPassage => !skill.is_empty(),
                            EventOptionId::SelfHelpBookInitialReadEntireBook => !power.is_empty(),
                            _ => option.enabled,
                        };
                    }
                }
            } else if id == EventId::GraveOfTheForgotten {
                let confront = !self.enchantable_event_cards(seat, "SoulsPower", None).is_empty();
                for option in &mut event.options {
                    if option.option_id.is(EventOptionId::GraveOfTheForgottenInitialConfront) { option.enabled = confront; }
                }
            } else if id == EventId::WoodCarvings {
                let snake = !self.enchantable_event_cards(seat, "Slither", None).is_empty();
                for option in &mut event.options {
                    if option.option_id.is(EventOptionId::WoodCarvingsInitialSnake) { option.enabled = snake; }
                }
            }
            self.events[seat] = Some(event);
        }
        Ok(())
    }

    pub fn native_event_page(&self, net_id: u64, seat: usize)
        -> Result<Option<super::super::EventPageIdentity>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.events.get(seat).and_then(Option::as_ref).map(|event| event.options.page))
    }

    pub fn native_event_options(&self, net_id: u64, seat: usize)
        -> Result<Option<&[super::super::NativeEventOption]>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.events.get(seat).and_then(Option::as_ref).map(|event| &*event.options))
    }

    pub fn event_options(&self, net_id: u64, seat: usize)
        -> Result<Vec<&'static str>, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event"); }
        let event = self.events.get(seat).and_then(Option::as_ref)
            .ok_or("event already finished")?;
        Ok(event.options.iter().filter(|option| option.enabled)
            .map(|option| option.option_id.legacy_id()).collect())
    }

    pub(super) fn finish_event_seat(&mut self, seat: usize) {
        self.events[seat] = None;
        self.event_done[seat] = true;
        if self.roster.players().iter().all(|p| p.hp <= 0) {
            self.phase = MpPhase::RunOver;
        } else if self.event_done.iter().all(|done| *done) {
            self.finish_noncombat();
        }
    }

    pub(super) fn add_event_card(&mut self, seat: usize, id: CardId) -> Result<(), &'static str> {
        self.roster.next_card_id += 1;
        let mut card = Card::from_factory(id, false, self.roster.next_card_id)
            .ok_or("event card factory")?;
        card.owner = seat as u8;
        super::cards::add(&mut self.roster, &mut self.world.run, seat, card)
    }

    /// Original NEventOptionButton.OnRelease rejects lethal multiplayer clicks.
    /// Keep the raw event options/command indices intact for DLL replay.
    pub fn native_event_click_blocked(&self,seat:usize,option:crate::identity::EventOptionId)->bool {
        use crate::identity::EventOptionId;
        if self.roster.players().len()<=1 {return false;}
        match option {
            EventOptionId::AbyssalBathsInitialImmerse|EventOptionId::AbyssalBathsAllLinger=>{
                let Some(event)=self.events[seat].as_ref() else {return false};
                self.roster.player(seat).unwrap().hp<=event.ints[0]-2
            },
            EventOptionId::SlipperyBridgeInitialHoldOn0|
            EventOptionId::SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2=>{
                let Some(event)=self.events[seat].as_ref() else {return false};
                self.roster.player(seat).unwrap().hp<=3+event.bridge.hold_ons
            },
            _=>false,
        }
    }

    pub(super) fn apply_event_effect(&mut self, seat: usize, effect: EventEffect,
        event: &mut EventState) -> Result<(), &'static str> {
        match effect {
            EventEffect::LoseHp(amount) => {
                let mut run = self.rest_context(seat);
                run.potions = self.roster.player(seat).unwrap().potions.clone();
                slippery_bridge::damage_event_player(&mut run, amount, self.roster.players().len() > 1)?;
                self.carry_rest_context(seat, run);
            }
            EventEffect::Heal(amount) => {
                let p = self.roster.player_mut(seat).unwrap();
                p.hp = (p.hp + amount.max(0)).min(p.max_hp);
            }
            EventEffect::GainMaxHp(amount) => {
                let p = self.roster.player_mut(seat).unwrap();
                let amount = amount.max(0);
                p.max_hp += amount;
                p.hp += amount;
            }
            EventEffect::LoseMaxHp(amount) => {
                let p = self.roster.player_mut(seat).unwrap();
                p.max_hp = (p.max_hp - amount.max(0)).max(1);
                p.hp = p.hp.min(p.max_hp);
            }
            EventEffect::GainGold(amount) => {self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run,amount);}
            EventEffect::LoseGold(amount) => {
                let p = self.roster.player_mut(seat).unwrap();
                p.gold = (p.gold - amount.max(0)).max(0);
            }
            EventEffect::AddCard(id) => self.add_event_card(seat, id)?,
            EventEffect::AddRelic(id) => {
                self.attach_event_relic(seat, id)?;
                if id == RelicId::BigMushroom {
                    let p = self.roster.player_mut(seat).unwrap();
                    p.max_hp += 20;
                    p.hp += 20;
                } else if id == RelicId::FragrantMushroom {
                    let mut run = self.rest_context(seat);
                    run.potions = self.roster.player(seat).unwrap().potions.clone();
                    slippery_bridge::damage_event_player(&mut run, 15, self.roster.players().len() > 1)?;
                    if run.hp > 0 {
                        let mut candidates: Vec<crate::card::KRef> = (0..run.deck.len())
                            .filter(|index| run.is_deck_card_upgradable(*index))
                            .map(|index| index as crate::card::KRef).collect();
                        crate::flow::dotnet_sort_cards(&mut candidates, &run.deck);
                        event.rng.shuffle(&mut candidates);
                        for index in candidates.into_iter().take(2) { run.upgrade_deck_card(index as usize); }
                    }
                    self.carry_rest_context(seat, run);
                }
            }
            EventEffect::RollTrashRelic => {
                let id = super::super::events::roll_trash_relic(event);
                self.attach_event_relic(seat, id)?;
            }
            EventEffect::UpgradeRandomDeckCards(count) => {
                let mut run = self.rest_context(seat);
                let mut candidates: Vec<crate::card::KRef> = (0..run.deck.len())
                    .filter(|index| run.is_deck_card_upgradable(*index))
                    .map(|index| index as crate::card::KRef).collect();
                crate::flow::dotnet_sort_cards(&mut candidates, &run.deck);
                let mut rng = self.world.run.rngs.niche.clone();
                rng.shuffle(&mut candidates);
                for index in candidates.into_iter().take(count) { run.upgrade_deck_card(index as usize); }
                self.world.run.rngs.niche = rng;
                self.carry_rest_context(seat, run);
            }
        }
        Ok(())
    }

    /// EventSynchronizer.IsShared: a shared page needs one vote per seat.
    pub fn event_is_shared(&self) -> bool {
        self.phase==MpPhase::Event && self.world.current_room.and_then(crate::identity::EventId::from_name).is_some_and(is_shared_event)
    }

    pub fn choose_event_option(&mut self, net_id: u64, seat: usize, option: usize)
        -> Result<(), &'static str> {
        if self.event_is_shared() {
            return self.vote_shared_event_option(net_id,seat,option);
        }
        self.choose_event_option_now(net_id,seat,option)
    }

    pub(super) fn choose_event_option_now(&mut self, net_id: u64, seat: usize, option: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event"); }
        let mut next = self.clone();
        let mut event = next.events[seat].take().ok_or("event already finished")?;
        let enabled: Vec<_> = event.options.iter().filter(|candidate| candidate.enabled)
            .map(|candidate| candidate.option_id).collect();
        let option_id = *enabled.get(option).ok_or("invalid event option")?;

        match event.id {
            EventId::WelcomeToWongos => {
                next.choose_wongo_option(seat, event, option_id)?;
                *self=next;
                return Ok(());
            }
            EventId::ColorfulPhilosophers => {
                next.offer_philosopher_rewards(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::TinkerTime => {
                next.choose_tinker_time(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::Trial => {
                next.choose_trial(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::BattlewornDummy => {
                next.start_battleworn_dummy(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::Reflections => {
                next.resolve_reflections(seat, &mut event, option_id)?;
                next.finish_event_seat(seat);
                *self = next;
                return Ok(());
            }
            EventId::DollRoom => {
                next.choose_doll_room(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::SpiritGrafter => {
                next.choose_spirit_grafter(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::TheLanternKey => {
                next.events[seat] = Some(event);
                next.choose_lantern_key(option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::FieldOfManSizedHoles | EventId::Amalgamator | EventId::InfestedAutomaton => {
                if next.event_card_modals[seat].is_some() {
                    return Err("event card selector already open");
                }
                let id = event.id;
                next.events[seat] = Some(event);
                match id {
                    EventId::FieldOfManSizedHoles => next.choose_holes(net_id, seat, option_id)?,
                    EventId::Amalgamator => next.choose_amalgamator(net_id, seat, option_id)?,
                    _ => next.choose_infested_automaton(seat, option_id)?,
                }
                *self = next;
                return Ok(());
            }
            EventId::CrystalSphere => {
                next.choose_crystal_sphere(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::PotionCourier => {
                next.choose_potion_courier(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::RoundTeaParty => {
                next.choose_round_tea_party(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::GraveOfTheForgotten if option_id.is(EventOptionId::GraveOfTheForgottenInitialConfront) => {
                next.confront_forgotten_grave(seat, event)?;
                *self = next;
                return Ok(());
            }
            EventId::RelicTrader => {
                next.choose_relic_trader_event(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::ZenWeaver if matches!(option_id.id,
                EventOptionId::ZenWeaverInitialEmotionalAwareness |
                EventOptionId::ZenWeaverInitialArachnidAcupuncture) => {
                next.choose_zen_weaver(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::StoneOfAllTime => {
                next.choose_stone_event(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::TheArchitect=>{
                next.choose_architect(seat,event);
                *self=next;
                return Ok(());
            }
            EventId::Darv=>{
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Darv option missing")?;
                if event.ints[0]==1{
                    if option_id.offer_index(EventOptionId::DarvPageNameRelicIdEntry)==Some(6){next.finish_event_seat(seat);}else{
                        let id=event.relics[index];next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                        event.relics.remove(index);event.options.remove(index);
                        if event.relics.is_empty(){next.finish_event_seat(seat);}else{next.events[seat]=Some(event);}
                    }
                    *self=next;return Ok(());
                }
                let id=event.relics[index];let obtained=next.attach_event_relic(seat,id)?;
                next.record_relic_seen(seat,id);
                match id{
                    RelicId::Astrolabe|RelicId::EmptyCage=>{
                        let (cards,count,action)=if id==RelicId::Astrolabe{
                            (next.transformable_event_cards(seat),3,EventCardAction::TransformUpgrade)
                        }else{(next.removable_event_cards(seat),2,EventCardAction::Remove)};
                        let count=count.min(cards.len());
                        if count>0{
                            let all=count==cards.len();next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,count,count,false,action)?;
                            if all{next.select_event_cards(net_id,seat,&(0..count).collect::<Vec<_>>())?;}
                            *self=next;return Ok(());
                        }
                    }
                    RelicId::PandorasBox=>{
                        next.apply_pandoras_box_batch(seat)?;
                    }
                    RelicId::CallingBell=>{
                        next.add_event_card(seat,CardId::CurseOfTheBell)?;
                        event.relics.clear();event.options.clear();event.ints[0]=1;
                        for (rarity,option) in ["bell_0","bell_1","bell_2"].into_iter().enumerate(){
                            let id=next.world.pull_player_relic_from_front(&next.roster,seat,rarity);
                            event.relics.push(id);event.options.push(EventOption::new(option,id.name(),"Gain a relic"));
                        }
                        event.options.push(EventOption::new("skip_bell","Proceed","Skip remaining rewards"));
                        next.events[seat]=Some(event);*self=next;return Ok(());
                    }
                    RelicId::DustyTome=>{
                        let id=*event.cards.first().ok_or("Darv configured card missing")?;
                        next.roster.player_mut(seat).unwrap().relics[obtained].ancient_card=Some(id);
                        next.roster.next_card_id+=1;
                        let mut card=Card::from_factory(id,false,next.roster.next_card_id).ok_or("Dusty Tome factory")?;
                        let mut ctx=next.rest_context(seat);super::super::shop::upgrade_template(&mut ctx,&mut card);
                        next.carry_rest_context(seat,ctx);card.owner=seat as u8;
                        super::cards::add(&mut next.roster,&mut next.world.run,seat,card)?;
                    }
                    _=>{}
                }
                next.finish_event_seat(seat);*self=next;return Ok(());
            }
            EventId::Orobas=>{
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Orobas option missing")?;
                let id=event.relics[index];let obtained=next.attach_event_relic(seat,id)?;
                next.record_relic_seen(seat,id);
                match id{
                    RelicId::ElectricShrymp=>{
                        let cards=next.enchantable_event_cards(seat,"Imbued",None);
                        if !cards.is_empty(){
                            let all=cards.len()==1;next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,1,1,false,EventCardAction::Enchant(crate::identity::EnchantmentId::Imbued,1))?;
                            if all{next.select_event_cards(net_id,seat,&[0])?;}
                            *self=next;return Ok(());
                        }
                    }
                    RelicId::GlassEye=>{
                        let character=next.roster.player(seat).unwrap().character;
                        let mut bundles=Vec::new();
                        for rarity in [CardRarity::Common,CardRarity::Common,CardRarity::Uncommon,CardRarity::Uncommon,CardRarity::Rare]{
                            bundles.push(next.orobas_cards(seat,character.into(),rarity,3,true)?);
                        }
                        next.events[seat]=Some(event);
                        next.event_card_reward_queues[seat]=bundles.into_iter().map(|cards|
                            super::PlayerRewards{cards,..Default::default()}).collect();
                        *self=next;return Ok(());
                    }
                    RelicId::SeaGlass=>{
                        let character=super::super::manager::orobas::CHARACTERS[event.ints[0] as usize];
                        let mut cards=Vec::new();
                        for rarity in [CardRarity::Common,CardRarity::Uncommon,CardRarity::Rare]{
                            cards.extend(next.orobas_cards(seat,character.into(),rarity,5,false)?);
                        }
                        next.events[seat]=Some(event);
                        next.begin_event_card_modal(seat,cards,0,15,true,EventCardAction::AddGenerated)?;
                        *self=next;return Ok(());
                    }
                    RelicId::AlchemicalCoffer=>{
                        let character=next.roster.player(seat).unwrap().character;
                        let mut pool=super::super::potion_pools::cards(character).ok_or("Coffer potion pool")?;
                        let p=next.roster.player_mut(seat).unwrap();
                        let slots=p.max_potion_slots;p.max_potion_slots+=4;p.potions.resize(p.max_potion_slots,None);
                        for i in 0..4{
                            let rarity=super::super::shop::roll_potion_rarity(&mut next.world.run.rngs.combat_potion_generation);
                            let options:Vec<_>=pool.iter().copied().filter(|id|POTION_INFO[id.index()].rarity==rarity).collect();
                            let potion=options[next.world.run.rngs.combat_potion_generation.choice_index(options.len())];
                            pool.retain(|id|*id!=potion);
                            if !p.relics.iter().any(|r|r.id==RelicId::Sozu){p.potions[slots+i]=Some(potion);}
                        }
                    }
                    RelicId::SandCastle=>next.apply_event_effect(seat,EventEffect::UpgradeRandomDeckCards(6),&mut event)?,
                    RelicId::TouchOfOrobas=>{
                        let p=next.roster.player_mut(seat).unwrap();
                        let index=p.relics.iter().position(|r|RELIC_INFO[r.id.index()].rarity==RelicRarity::Starter).ok_or("Orobas starter missing")?;
                        let mut relic=Relic::new(super::super::manager::orobas::upgraded_starter_relic(p.relics[index].id));
                        relic.owner=seat as u8;relic.floor_added_to_deck=next.world.run.total_floor;
                        p.relics[index]=relic;
                    }
                    RelicId::ArchaicTooth=>{
                        let p=next.roster.player(seat).unwrap();
                        let (index,target)=p.deck.iter().enumerate().find_map(|(i,c)|
                            super::super::manager::orobas::STARTER_CARDS.iter().find(|&&(from,_)|c.id.name()==from).map(|&(_,to)|(i,to)))
                            .ok_or("Archaic Tooth starter missing")?;
                        let old=next.roster.player_mut(seat).unwrap().deck.remove(index);
                        next.roster.next_card_id+=1;
                        let mut card=Card::from_factory(CardId::from_name(target).unwrap(),old.upgraded,next.roster.next_card_id).ok_or("Archaic Tooth factory")?;
                        card.owner=seat as u8;
                        for &(name,amount) in &old.enchantments{
                            if !crate::card::enchant::apply_static_enchantment(&mut card,name,amount){return Err("Archaic Tooth enchantment capacity");}
                        }
                        next.roster.player_mut(seat).unwrap().relics[obtained].saved_cards=Some(Box::new([old,card.clone()]));
                        super::cards::add(&mut next.roster,&mut next.world.run,seat,card)?;
                    }
                    _=>{}
                }
                next.finish_event_seat(seat);*self=next;return Ok(());
            }
            EventId::Tezcatara=>{
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Tezcatara option missing")?;
                if event.ints[0]==1{
                    if option_id.offer_index(EventOptionId::TezcataraPageNameRelicIdEntry)==Some(8){next.finish_event_seat(seat);}else{
                        let id=event.relics[index];
                        let obtained=next.attach_event_relic(seat,id)?;
                        next.roster.player_mut(seat).unwrap().relics[obtained].is_wax=true;
                        next.record_relic_seen(seat,id);
                        event.relics.remove(index);event.options.remove(index);
                        if event.relics.is_empty(){next.finish_event_seat(seat);}else{next.events[seat]=Some(event);}
                    }
                    *self=next;return Ok(());
                }
                let id=event.relics[index];
                let obtained=next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                match id{
                    RelicId::Storybook=>next.add_event_card(seat,CardId::BrightestFlame)?,
                    RelicId::NutritiousSoup=>{
                        for c in &mut next.roster.player_mut(seat).unwrap().deck{
                            if c.rarity==CardRarity::Basic&&c.has_tag(CardTag::Strike)
                                &&super::super::manager::RunManager::event_can_enchant(c,"TezcatarasEmber")
                                &&!crate::card::enchant::apply_static_enchantment(c,"TezcatarasEmber",1){
                                return Err("Tezcatara Ember capacity");
                            }
                        }
                    }
                    RelicId::YummyCookie|RelicId::BiiigHug=>{
                        let (cards,action)=if id==RelicId::YummyCookie{
                            (next.upgradable_event_cards(seat),EventCardAction::Upgrade)
                        }else{(next.removable_event_cards(seat),EventCardAction::Remove)};
                        let count=cards.len().min(4);
                        if count>0{
                            let all=count==cards.len();next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,count,count,false,action)?;
                            if all{next.select_event_cards(net_id,seat,&(0..count).collect::<Vec<_>>())?;}
                            *self=next;return Ok(());
                        }
                    }
                    RelicId::ToyBox=>{
                        event.ints[0]=1;event.relics.clear();event.options.clear();
                        for option in ["wax_0","wax_1","wax_2","wax_3","wax_4"]{
                            let id=next.pull_event_relic_from_front(seat)?;
                            event.relics.push(id);
                            event.options.push(super::super::events::EventOption::new(option,id.name(),"Gain a wax relic"));
                        }
                        event.options.push(super::super::events::EventOption::new("skip_wax","Proceed","Skip remaining rewards"));
                        next.events[seat]=Some(event);*self=next;return Ok(());
                    }
                    RelicId::GoldenCompass=>next.tezcatara_golden_map(seat,obtained)?,
                    _=>{}
                }
                next.finish_event_seat(seat);*self=next;return Ok(());
            }
            EventId::Nonupeipe=>{
                // Nonupeipe.cs OptionPool and the selected relic's AfterObtained;
                // BeautifulBracelet consumes shared Niche, FurCoat a private RNG.
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Nonupeipe option missing")?;
                let id=event.relics[index];
                let coords=if id==RelicId::FurCoat{
                    let relics:Vec<_>=next.roster.players().iter().flat_map(|p|p.relics.iter().cloned()).collect();
                    Some(next.world.map.fur_coat_coords(next.world.run.rngs.seed,seat,&relics))
                }else{None};
                let obtained=next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                match id{
                    RelicId::FurCoat=>{
                        let r=&mut next.roster.player_mut(seat).unwrap().relics[obtained];
                        r.counter=next.world.run.act_index;
                        r.set_fur_coat_coords(&coords.unwrap()).map_err(|_|"Fur Coat coordinate capacity")?;
                    }
                    RelicId::JewelryBox=>next.add_event_card(seat,CardId::Apotheosis)?,
                    RelicId::LoomingFruit=>next.apply_event_effect(seat,EventEffect::GainMaxHp(31),&mut event)?,
                    RelicId::SignetRing=>next.apply_event_effect(seat,EventEffect::GainGold(888),&mut event)?,
                    RelicId::BeautifulBracelet=>{
                        let mut ids:Vec<_>=next.enchantable_event_cards(seat,"Swift",None).iter().map(|c|c.instance_id).collect();
                        next.world.run.rngs.niche.shuffle(&mut ids);
                        for id in ids.into_iter().take(4){
                            let c=next.roster.player_mut(seat).unwrap().deck.iter_mut().find(|c|c.instance_id==id).unwrap();
                            if !crate::card::enchant::apply_static_enchantment(c,"Swift",2){return Err("Beautiful Bracelet enchantment capacity");}
                        }
                    }
                    _=>{}
                }
                next.finish_event_seat(seat);*self=next;return Ok(());
            }
            EventId::Vakuu=>{
                // Models/Relics/{BloodSoakedRose,SereTalon,DistinguishedCape,
                // PreservedFog}.cs AfterObtained. Niche is the shared run stream.
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Vakuu option missing")?;
                let id=event.relics[index];
                next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                match id{
                    RelicId::BloodSoakedRose=>next.add_event_card(seat,CardId::Enthralled)?,
                    RelicId::SereTalon=>{
                        let new_max=next.roster.player(seat).unwrap().max_hp-9;
                        let damage=next.roster.player(seat).unwrap().hp-new_max;
                        if damage>0{next.apply_event_effect(seat,EventEffect::LoseHp(damage),&mut event)?;}
                        let p=next.roster.player_mut(seat).unwrap();
                        p.max_hp=new_max.max(1);p.hp=p.hp.min(p.max_hp);
                        for _ in 0..3{next.add_event_card(seat,CardId::Wish)?;}
                    }
                    RelicId::DistinguishedCape=>{
                        let mut curses:Vec<_>=super::super::card_pools::cards(CardPool::Curse,true)?.into_iter()
                            .filter(|&id|crate::tables::card_info(id,false).is_some_and(|c|c.gen_modifiers)).collect();
                        curses.sort_by_key(|id|id.name());
                        for _ in 0..2{
                            let index=next.world.run.rngs.niche.choice_index(curses.len());
                            next.add_event_card(seat,curses.remove(index))?;
                        }
                        for _ in 0..3{next.add_event_card(seat,CardId::Apparition)?;}
                    }
                    RelicId::PreservedFog=>{
                        let cards=next.removable_event_cards(seat);let count=cards.len().min(3);
                        if count==0{next.add_event_card(seat,CardId::Folly)?;}else{
                            let all=cards.len()<=count;
                            next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,count,count,false,EventCardAction::RemoveAndAdd(CardId::Folly))?;
                            if all{next.select_event_cards(net_id,seat,&(0..count).collect::<Vec<_>>())?;}
                            *self=next;return Ok(());
                        }
                    }
                    _=>{}
                }
                next.finish_event_seat(seat);
                *self=next;return Ok(());
            }
            EventId::Pael=>{
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Pael option missing")?;
                let id=event.relics[index];
                next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                match id {
                    RelicId::PaelsHorn=>{for _ in 0..2{next.add_event_card(seat,CardId::Relax)?;}next.finish_event_seat(seat);}
                    RelicId::PaelsClaw=>{
                        let ids:Vec<_>=next.enchantable_event_cards(seat,"Goopy",None).iter().map(|c|c.instance_id).collect();
                        for c in &mut next.roster.player_mut(seat).unwrap().deck{
                            if ids.contains(&c.instance_id)&&!crate::card::enchant::apply_static_enchantment(c,"Goopy",1){
                                return Err("PaelsClaw enchantment capacity exceeded");
                            }
                        }
                        next.finish_event_seat(seat);
                    }
                    RelicId::PaelsGrowth|RelicId::PaelsTooth=>{
                        let (cards,action,count)=if id==RelicId::PaelsGrowth{
                            let cards=next.enchantable_event_cards(seat,"Clone",None);
                            let count=cards.len().min(1);(cards,EventCardAction::Enchant(crate::identity::EnchantmentId::Clone,4),count)
                        }else{
                            let upgradable=next.upgradable_event_cards(seat);
                            let cards:Vec<_>=next.removable_event_cards(seat).into_iter()
                                .filter(|c|upgradable.iter().any(|u|u.instance_id==c.instance_id)).collect();
                            let count=cards.len().min(5);(cards,EventCardAction::StorePaelsTooth,count)
                        };
                        if count==0{
                            if id==RelicId::PaelsTooth{next.roster.player_mut(seat).unwrap().relics.last_mut().unwrap().enabled=false;}
                            next.finish_event_seat(seat);
                        }else{
                            let all=cards.len()<=count;
                            next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,count,count,false,action)?;
                            if all{
                                let indices:Vec<_>=(0..count).collect();
                                next.select_event_cards(net_id,seat,&indices)?;
                            }
                        }
                    }
                    _=>next.finish_event_seat(seat),
                }
                *self=next;return Ok(());
            }
            EventId::Tanx=>{
                let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Tanx option missing")?;
                let id=event.relics[index];next.attach_event_relic(seat,id)?;next.record_relic_seen(seat,id);
                match id {
                    RelicId::TanxsWhistle=>{next.add_event_card(seat,CardId::Whistle)?;next.finish_event_seat(seat);}
                    RelicId::Claws=>{
                        let cards=next.transformable_event_cards(seat);let max=cards.len().min(6);
                        if max==0{next.finish_event_seat(seat);}else{
                            next.events[seat]=Some(event);next.begin_event_card_modal(seat,cards,0,max,true,EventCardAction::TransformMaul)?;
                        }
                    }
                    RelicId::TriBoomerang=>{
                        let cards=next.enchantable_event_cards(seat,"Instinct",None);let count=cards.len().min(3);
                        if count==0{next.finish_event_seat(seat);}else{
                            next.events[seat]=Some(event);
                            next.begin_event_card_modal(seat,cards,count,count,false,EventCardAction::Enchant(crate::identity::EnchantmentId::Instinct,1))?;
                        }
                    }
                    _=>next.finish_event_seat(seat),
                }
                *self=next;return Ok(());
            }
            EventId::WhisperingHollow => {
                if option_id.is(EventOptionId::WhisperingHollowInitialGold) {
                    next.events[seat]=Some(event);
                    next.offer_whispering_hollow_rewards(net_id,seat)?;
                } else {
                    let cards=next.transformable_event_cards(seat);
                    if cards.is_empty() {
                        next.apply_event_effect(seat,EventEffect::LoseHp(9),&mut event)?;
                        next.finish_event_seat(seat);
                    } else {
                        next.events[seat]=Some(event);
                        next.begin_event_card_modal(seat,cards,1,1,false,EventCardAction::TransformAndHollow)?;
                    }
                }
                *self=next;return Ok(());
            }
            EventId::TheFutureOfPotions => {
                next.trade_future_potion(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::EndlessConveyor => {
                next.choose_mp_conveyor(seat, event, option_id)?;
                *self = next;
                return Ok(());
            }
            EventId::JungleMazeAdventure => {
                if option_id.is(EventOptionId::JungleMazeAdventureInitialSoloQuest) {
                    let mut fx = [0, 1, 2];
                    event.rng.shuffle(&mut fx);
                    next.apply_event_effect(seat, EventEffect::LoseHp(18), &mut event)?;
                    if next.roster.player(seat).unwrap().hp > 0 {
                        next.apply_event_effect(seat, EventEffect::GainGold(event.ints[0]), &mut event)?;
                    }
                } else {
                    next.apply_event_effect(seat, EventEffect::GainGold(event.ints[1]), &mut event)?;
                }
                next.finish_event_seat(seat);
                *self = next;
                return Ok(());
            }
            EventId::SapphireSeed => {
                let (cards, action) = if option_id.is(EventOptionId::SapphireSeedInitialEat) {
                    next.apply_event_effect(seat, EventEffect::Heal(9), &mut event)?;
                    (next.upgradable_event_cards(seat), EventCardAction::Upgrade)
                } else {
                    (next.enchantable_event_cards(seat, "Sown", None), EventCardAction::Enchant(crate::identity::EnchantmentId::Sown, 1))
                };
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false, action)?;
                }
                *self = next;
                return Ok(());
            }
            EventId::TabletOfTruth => {
                if option_id.is(EventOptionId::TabletOfTruthInitialSmash)
                    || option_id.is(EventOptionId::TabletOfTruthDecipherGiveUp) {
                    if option_id.is(EventOptionId::TabletOfTruthInitialSmash) {
                        next.apply_event_effect(seat, EventEffect::Heal(20), &mut event)?;
                    }
                    next.finish_event_seat(seat);
                    *self = next;
                    return Ok(());
                }
                let cost = event.ints[1];
                let max_hp = next.roster.player(seat).unwrap().max_hp;
                if cost >= max_hp {
                    let p = next.roster.player_mut(seat).unwrap();
                    p.max_hp = 1;
                    p.hp = 0;
                    next.finish_event_seat(seat);
                    *self = next;
                    return Ok(());
                }
                next.apply_event_effect(seat, EventEffect::LoseMaxHp(cost), &mut event)?;
                // TabletOfTruth.LoseMaxHpAndUpgrade selects one entry from
                // the owner's unsorted deck. The fifth decipher upgrades
                // every remaining card without consuming event RNG.
                let mut owner_run = next.rest_context(seat);
                let candidates: Vec<_> = (0..owner_run.deck.len())
                    .filter(|&index| owner_run.is_deck_card_upgradable(index)).collect();
                if event.ints[0] == 4 {
                    for index in candidates { owner_run.upgrade_deck_card(index); }
                } else if !candidates.is_empty() {
                    let index = candidates[event.rng.choice_index(candidates.len())];
                    owner_run.upgrade_deck_card(index);
                }
                next.carry_rest_context(seat, owner_run);
                event.ints[0] += 1;
                if event.ints[0] == 5 {
                    next.finish_event_seat(seat);
                } else {
                    event.ints[1] = match event.ints[0] {
                        1 => 6, 2 => 12, 3 => 24,
                        4 => next.roster.player(seat).unwrap().max_hp - 1,
                        _ => unreachable!(),
                    };
                    event.options.clear();
                    event.options.set_page(crate::identity::EventPageId::TabletOfTruthDecipherDecipherCount,
                        super::super::EventParameter::Counter(event.ints[0]));
                    event.options.push(EventOption::new("decipher", "Decipher", ""));
                    event.options.push(EventOption::new("give_up", "Give Up", ""));
                    next.events[seat] = Some(event);
                }
                *self = next;
                return Ok(());
            }
            EventId::SelfHelpBook => {
                if option_id.is(EventOptionId::SelfHelpBookInitialNoOptions) {
                    next.finish_event_seat(seat);
                    *self = next;
                    return Ok(());
                }
                let (ty, enchantment) = match option_id.id {
                    EventOptionId::SelfHelpBookInitialReadTheBack => (CardType::Attack, "Sharp"),
                    EventOptionId::SelfHelpBookInitialReadPassage => (CardType::Skill, "Nimble"),
                    EventOptionId::SelfHelpBookInitialReadEntireBook => (CardType::Power, "Swift"),
                    _ => return Err("invalid book option"),
                };
                let cards = next.enchantable_event_cards(seat, enchantment, Some(ty));
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false,
                        EventCardAction::Enchant(crate::identity::EnchantmentId::from_name(enchantment).expect("book enchantment"), 2))?;
                }
                *self = next;
                return Ok(());
            }
            EventId::BrainLeech => {
                let character = next.roster.player(seat).unwrap().character;
                let rip = option_id.is(EventOptionId::BrainLeechInitialRip);
                let pool = if rip {
                    next.apply_event_effect(seat, EventEffect::LoseHp(5), &mut event)?;
                    if next.roster.player(seat).unwrap().hp <= 0 {
                        next.finish_event_seat(seat);
                        *self = next;
                        return Ok(());
                    }
                    super::super::card_pools::cards(CardPool::Colorless, true)?
                } else {
                    super::super::card_pools::cards(character.into(), true)?
                };
                let count = if rip { 3 } else { 5 };
                // BrainLeech uses ForNonCombatWithDefaultOdds: NoUpgradeRoll.
                let cards = next.make_other_regular_reward_cards_with_upgrade_roll(seat, &pool, count, true, false)?;
                next.events[seat] = Some(event);
                next.begin_event_card_modal(seat, cards, 1, 1, rip,
                    EventCardAction::AddGenerated)?;
                *self = next;
                return Ok(());
            }
            EventId::LuminousChoir => {
                if option_id.is(EventOptionId::LuminousChoirInitialReachIntoTheFlesh) {
                    let cards = next.removable_event_cards(seat);
                    let count = cards.len().min(2);
                    if count == 0 {
                        next.add_event_card(seat, CardId::SporeMind)?;
                        next.finish_event_seat(seat);
                    } else {
                        next.events[seat] = Some(event);
                        next.begin_event_card_modal(seat, cards, count, count, false,
                            EventCardAction::RemoveAndAdd(CardId::SporeMind))?;
                    }
                } else {
                    next.apply_event_effect(seat, EventEffect::LoseGold(event.ints[0]), &mut event)?;
                    let relic = next.pull_event_relic_from_front(seat)?;
                    next.attach_event_relic(seat, relic)?;
                    next.finish_event_seat(seat);
                }
                *self = next;
                return Ok(());
            }
            EventId::PunchOff => {
                match option_id.id {
                    EventOptionId::PunchOffInitialNab => {
                        next.add_event_card(seat, CardId::Injury)?;
                        let relic = next.pull_event_relic_from_front(seat)?;
                        next.relic_rewards[seat] = Some(relic);
                        next.relic_reward_skippable[seat] = true;
                        next.events[seat] = Some(event);
                    }
                    EventOptionId::PunchOffInitialICanTakeThem => {
                        event.options.clear();
                        event.options.set_page(crate::identity::EventPageId::PunchOffICanTakeThem,
                            super::super::events::native_identity::EventParameter::None);
                        event.options.push(EventOption::new(
                            "fight", "Fight", "Fight for relic + potion"));
                        next.events[seat] = Some(event);
                    }
                    EventOptionId::PunchOffICanTakeThemFight => {
                        let mut combat = next.combat.take().ok_or("Punch Off layout missing")?;
                        next.roster.refresh_prepared_combat_inventory(&mut combat);
                        combat.start_combat();
                        if let Some(fault) = combat.fault { return Err(fault); }
                        next.combat = Some(combat);
                        next.punch_off_owners = vec![seat];
                        next.events.fill(None);
                        next.event_done.fill(true);
                        next.world.current_room = Some(crate::identity::EncounterId::PunchOffEventEncounter.name());
                        next.world.run.room_type = crate::tables::RoomType::Monster;
                        next.phase = MpPhase::Combat;
                    }
                    _ => return Err("invalid Punch Off option"),
                }
                *self = next;
                return Ok(());
            }
            EventId::ThisOrThat if option_id.is(EventOptionId::ThisOrThatInitialOrnate) => {
                let relic = next.pull_event_relic_from_front(seat)?;
                next.attach_event_relic(seat, relic)?;
                next.add_event_card(seat, CardId::Clumsy)?;
                next.finish_event_seat(seat);
                *self = next;
                return Ok(());
            }
            EventId::AromaOfChaos => {
                let (cards, action) = if option_id.is(EventOptionId::AromaOfChaosInitialLetGo) {
                    (next.transformable_event_cards(seat), EventCardAction::Transform)
                } else {
                    (next.upgradable_event_cards(seat), EventCardAction::Upgrade)
                };
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false, action)?;
                }
                *self = next;
                return Ok(());
            }
            EventId::DoorsOfLightAndDark => {
                if option_id.is(EventOptionId::DoorsOfLightAndDarkInitialLight) {
                    next.upgrade_random_event_cards(seat, &mut event, 2);
                    next.finish_event_seat(seat);
                } else {
                    let cards = next.removable_event_cards(seat);
                    if cards.is_empty() {
                        next.finish_event_seat(seat);
                    } else {
                        next.events[seat] = Some(event);
                        next.begin_event_card_modal(seat, cards, 1, 1, false, EventCardAction::Remove)?;
                    }
                }
                *self = next;
                return Ok(());
            }
            EventId::DrowningBeacon if option_id.is(EventOptionId::DrowningBeaconInitialBottle) => {
                next.events[seat] = Some(event);
                next.begin_event_potion_reward(seat, PotionId::Glowwaterpotion)?;
                *self = next;
                return Ok(());
            }
            EventId::MorphicGrove if option_id.is(EventOptionId::MorphicGroveInitialGroup) => {
                next.roster.player_mut(seat).unwrap().gold = 0;
                let cards = next.transformable_event_cards(seat);
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    let count = cards.len().min(2);
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, count, count, false, EventCardAction::Transform)?;
                }
                *self = next;
                return Ok(());
            }
            EventId::SpiralingWhirlpool if option_id.is(EventOptionId::SpiralingWhirlpoolInitialObserve) => {
                let cards = next.enchantable_event_cards(seat, "Spiral", None);
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false,
                        EventCardAction::Enchant(crate::identity::EnchantmentId::Spiral, 1))?;
                }
                *self = next;
                return Ok(());
            }
            EventId::WaterloggedScriptorium if matches!(option_id.id,
                EventOptionId::WaterloggedScriptoriumInitialTentacleQuill |
                EventOptionId::WaterloggedScriptoriumInitialPricklySponge) => {
                let (cost, wanted) = if option_id.is(EventOptionId::WaterloggedScriptoriumInitialTentacleQuill) { (55, 1) } else { (99, 2) };
                next.apply_event_effect(seat, EventEffect::LoseGold(cost), &mut event)?;
                let cards = next.enchantable_event_cards(seat, "Steady", None);
                let count = wanted.min(cards.len());
                if count == 0 {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, count, count, false,
                        EventCardAction::Enchant(crate::identity::EnchantmentId::Steady, 1))?;
                }
                *self = next;
                return Ok(());
            }
            EventId::Wellspring if option_id.is(EventOptionId::WellspringInitialBathe) => {
                let cards = next.removable_event_cards(seat);
                if cards.is_empty() {
                    next.add_event_card(seat, CardId::Guilty)?;
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false,
                        EventCardAction::RemoveAndAdd(CardId::Guilty))?;
                }
                *self = next;
                return Ok(());
            }
            EventId::Wellspring if option_id.is(EventOptionId::WellspringInitialBottle) => {
                let character = next.roster.player(seat).unwrap().character;
                let pool = super::super::potion_pools::cards(character)
                    .ok_or("Wellspring potion pool")?;
                let index = next.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(pool.len());
                next.events[seat] = Some(event);
                next.begin_event_potion_reward(seat, pool[index])?;
                *self = next;
                return Ok(());
            }
            EventId::WoodCarvings => {
                let (cards, action) = match option_id.id {
                    EventOptionId::WoodCarvingsInitialSnake => (next.enchantable_event_cards(seat, "Slither", None),
                        EventCardAction::Enchant(crate::identity::EnchantmentId::Slither, 1)),
                    EventOptionId::WoodCarvingsInitialBird => (next.basic_transform_event_cards(seat),
                        EventCardAction::RemoveAndAdd(CardId::Peck)),
                    EventOptionId::WoodCarvingsInitialTorus => (next.basic_transform_event_cards(seat),
                        EventCardAction::RemoveAndAdd(CardId::ToricToughness)),
                    _ => return Err("invalid Wood Carvings option"),
                };
                if cards.is_empty() {
                    next.finish_event_seat(seat);
                } else {
                    next.events[seat] = Some(event);
                    next.begin_event_card_modal(seat, cards, 1, 1, false, action)?;
                }
                *self = next;
                return Ok(());
            }
            EventId::DenseVegetation if option_id.is(EventOptionId::DenseVegetationRestFight) => {
                // EnterCombatWithoutExitingEvent(..., shouldResumeAfterCombat:false)
                // is one shared combat transition for the party.
                next.events.fill(None);
                next.event_done.fill(true);
                next.apply_room_entry_hooks(crate::tables::RoomType::Monster)?;
                let encounter=crate::identity::EncounterId::DenseVegetationEvent;
                next.combat = Some(next.roster.enter_combat_id(next.world.run.clone(), 0,
                    encounter, crate::tables::RoomType::Monster)?);
                next.world.current_room = Some(encounter.name());
                next.world.run.room_type = crate::tables::RoomType::Monster;
                next.phase = MpPhase::Combat;
                *self = next;
                return Ok(());
            }
            _ => {}
        }

        let resolution = super::super::events::choose(&mut event, option_id)
            .ok_or("unported multiplayer event option")?;
        for effect in resolution.effects {
            if next.roster.player(seat).unwrap().hp <= 0 { break; }
            next.apply_event_effect(seat, effect, &mut event)?;
        }
        if next.roster.player(seat).unwrap().hp <= 0 || resolution.finished {
            next.finish_event_seat(seat);
        } else {
            event.refresh_public_page(next.roster.player(seat).unwrap().hp);
            event.options.replace(resolution.next_options);
            next.events[seat] = Some(event);
        }
        *self = next;
        Ok(())
    }

    pub(super) fn whispering_hollow_is_allowed(&self) -> bool {
        // WhisperingHollow.IsAllowed: every player must be able to pay the
        // maximum possible 35 + 9 gold roll before the room is selected.
        self.roster.players().iter().all(|player| player.gold >= 44)
    }

    pub(super) fn begin_whispering_hollow(&mut self) -> Result<(), &'static str> {
        if !self.whispering_hollow_is_allowed() {
            return Err("Whispering Hollow is ineligible");
        }
        for seat in 0..self.roster.players().len() {
            let player = self.roster.player(seat).unwrap();
            if player.hp <= 0 {
                self.event_done[seat] = true;
                self.events[seat] = None;
                continue;
            }
            let event = super::super::events::initialize(
                EventId::WhisperingHollow,
                self.world.run.rngs.seed.wrapping_add(seat as u64),
                player.gold,
                0,
                player.hp,
                player.max_hp,
            ).ok_or("Whispering Hollow event initialization")?;
            self.events[seat] = Some(event);
        }
        Ok(())
    }

    /// WhisperingHollow.Gold starts an owner-bound custom RewardsSet with two
    /// PotionRewards. Populate consumes that owner's Rewards RNG twice before
    /// the rewards screen is shown; SetEventFinished runs only after it closes.
    pub fn offer_whispering_hollow_rewards(&mut self, net_id: u64, seat: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("WHISPERING_HOLLOW") {
            return Err("not at Whispering Hollow");
        }
        if self.event_done[seat] || self.event_potion_rewards[seat].is_some() {
            return Err("Whispering Hollow reward already begun");
        }
        let mut next = self.clone();
        let cost = next.events[seat].as_ref().ok_or("Whispering Hollow event missing")?.ints[0];
        let player = next.roster.player_mut(seat).unwrap();
        if player.gold < cost { return Err("insufficient Whispering Hollow gold"); }
        player.gold -= cost;
        let potions = super::rewards::generate_potions(player, 2)?;
        let rewards: [PotionId; 2] = potions.try_into().map_err(|_| "Whispering Hollow potion count")?;
        next.event_potion_rewards[seat] = Some([Some(rewards[0]), Some(rewards[1]), None]);
        *self = next;
        Ok(())
    }

    pub fn whispering_hollow_rewards(&self, net_id: u64, seat: usize)
        -> Result<&[Option<PotionId>; 3], &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("WHISPERING_HOLLOW") {
            return Err("not at Whispering Hollow");
        }
        self.event_potion_rewards[seat].as_ref().ok_or("Whispering Hollow rewards missing")
    }

    pub fn take_whispering_hollow_reward(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("WHISPERING_HOLLOW") {
            return Err("not at Whispering Hollow");
        }
        if index >= 2 { return Err("invalid Whispering Hollow reward index"); }
        let potion = self.event_potion_rewards[seat].as_ref()
            .ok_or("Whispering Hollow rewards missing")?[index]
            .ok_or("Whispering Hollow reward already selected")?;
        let mut next = self.clone();
        let mut procured = false;
        {
            let player = next.roster.player_mut(seat).unwrap();
            if !player.relics.iter().any(|r| r.id == crate::tables::RelicId::Sozu && r.enabled) {
                let Some(slot) = player.potions.iter_mut().find(|slot| slot.is_none()) else {
                    return Ok(false);
                };
                *slot = Some(potion);
                procured = true;
            }
        }
        if procured { next.mark_local_potion_seen(seat, potion); }
        next.event_potion_rewards[seat].as_mut().unwrap()[index] = None;
        *self = next;
        Ok(true)
    }

    pub fn finish_whispering_hollow_option(&mut self, net_id: u64, seat: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("WHISPERING_HOLLOW") {
            return Err("not at Whispering Hollow");
        }
        let rewards = self.event_potion_rewards[seat].as_ref()
            .ok_or("Whispering Hollow rewards missing")?;
        if rewards.iter().any(Option::is_some) { return Err("Whispering Hollow rewards remain"); }
        let mut next = self.clone();
        next.event_potion_rewards[seat] = None;
        next.events[seat] = None;
        next.event_done[seat] = true;
        if next.event_done.iter().all(|done| *done) { next.finish_noncombat(); }
        *self = next;
        Ok(())
    }

    pub(super) fn bridge_is_allowed(&self) -> bool {
        // C# SlipperyBridge.IsAllowed:82-90: only after floor 6, and every
        // player must own at least one removable card.
        self.world.run.total_floor > 6
            && self.roster.players().iter().all(|p|
                !super::super::manager::RunManager::removable_card_indices(&p.deck).is_empty())
    }

    pub(super) fn begin_bridge(&mut self) -> Result<(), &'static str> {
        if !self.bridge_is_allowed() {
            return Err("Slippery Bridge is ineligible");
        }
        for seat in 0..self.roster.players().len() {
            if self.roster.player(seat).unwrap().hp <= 0 {
                self.event_done[seat] = true;
                self.events[seat] = None;
                continue;
            }
            let run = self.rest_context(seat);
            let mut event = EventState::new(EventId::SlipperyBridge,
                Rng::for_model(self.world.run.rngs.seed, seat as u64, "SLIPPERY_BRIDGE", 0));
            slippery_bridge::roll_bridge_card(&run, &mut event)?;
            self.events[seat] = Some(event);
        }
        Ok(())
    }

    pub fn choose_bridge_option(&mut self, net_id: u64, seat: usize, option: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("SLIPPERY_BRIDGE") {
            return Err("not at Slippery Bridge");
        }
        let mut next = self.clone();
        let mut event = next.events[seat].take().ok_or("event already finished")?;
        let option = event.options.get(option).ok_or("invalid event option")?.option_id;
        let mut run = next.rest_context(seat);
        run.potions = next.roster.player(seat).unwrap().potions.clone();
        let finished = slippery_bridge::choose_bridge(&mut run, &mut event, option, next.roster.players().len() > 1)?;
        next.carry_rest_context(seat, run);
        next.event_done[seat] = finished;
        if !finished { next.events[seat] = Some(event); }
        if next.roster.players().iter().all(|p| p.hp <= 0) { next.phase = MpPhase::RunOver; }
        else if next.event_done.iter().all(|done| *done) { next.finish_noncombat(); }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_potion_lock_tracks_each_owners_original_finish_callback() {
        for id in [crate::identity::EventId::TheFutureOfPotions,crate::identity::EventId::StoneOfAllTime] {
            let specs=[PlayerSpec{net_id:7,character:Character::Ironclad},
                PlayerSpec{net_id:42,character:Character::Silent}];
            let mut run=MpRun::new("73003",&specs,0).unwrap();
            run.phase=MpPhase::Event;
            run.world.current_room=Some(id.name());
            run.event_done.fill(false);
            for seat in 0..2 {
                run.roster.player_mut(seat).unwrap().potions[0]=Some(PotionId::Firepotion);
                run.roster.player_mut(seat).unwrap().potions[1]=Some(PotionId::Firepotion);
            }
            run.begin_generic_event(id).unwrap();
            for (seat,net) in [(0,7),(1,42)] {
                let before=format!("{:?}",run.roster.player(seat).unwrap());
                assert!(!run.can_discard_potion(net,seat,0));
                assert!(run.discard_potion(net,seat,0).is_err());
                assert_eq!(format!("{:?}",run.roster.player(seat).unwrap()),before);
            }
            if id==EventId::TheFutureOfPotions {
                run.choose_event_option(7,0,0).unwrap();
                let reward=run.native_event_card_reward(0).unwrap();
                assert_eq!((reward.cards.len(),reward.min,reward.max,reward.skippable),(3,0,1,true));
                assert!(reward.cards.iter().all(|card|card.upgraded));
                let before=run.roster.player(0).unwrap().deck.len();
                run.select_event_cards(7,0,&[0]).unwrap();
                assert_eq!(run.roster.player(0).unwrap().deck.len(),before+1);
                assert!(run.native_event_card_reward(0).is_none());
                // The traded slot was consumed; another held potion unlocks.
                run.roster.player_mut(0).unwrap().potions[0]=Some(PotionId::Firepotion);
            } else {run.finish_event_seat(0);}
            assert_eq!(run.phase,MpPhase::Event);
            assert!(run.can_discard_potion(7,0,0));
            assert!(!run.can_discard_potion(42,1,0));
            run.discard_potion(7,0,0).unwrap();
            if id==EventId::TheFutureOfPotions {
                run.choose_event_option(42,1,0).unwrap();
                run.select_event_cards(42,1,&[]).unwrap();
                run.roster.player_mut(1).unwrap().potions[0]=Some(PotionId::Firepotion);
            } else {run.finish_event_seat(1);}
            assert!(run.can_discard_potion(42,1,0));
            run.discard_potion(42,1,0).unwrap();
        }
    }
    #[test]
    fn glass_eye_normal_rewards_offer_wing_sacrifice_and_grant_every_second(){
        let mut run=event_party(EventId::Orobas);
        let wing=run.attach_event_relic(1,RelicId::PaelsWing).unwrap();
        let event=run.events[1].as_mut().unwrap();
        event.relics.clear();event.relics.push(RelicId::GlassEye);
        event.options.replace([EventOption::new("relic_1","Glass Eye","")]);
        run.choose_event_option(42,1,0).unwrap();
        let before=run.roster.player(1).unwrap().deck.len();
        assert!(run.sacrifice_card_reward(7,1,wing).is_err());
        for count in 1..=4{
            assert!(run.card_reward_sacrifices(42,1).unwrap().contains(&wing));
            let relics=run.roster.player(1).unwrap().relics.len();
            let rng=run.roster.player(1).unwrap().rngs.rewards.counter;
            run.sacrifice_card_reward(42,1,wing).unwrap();
            assert_eq!(run.roster.player(1).unwrap().relics[wing].counter,count);
            assert_eq!(run.roster.player(1).unwrap().relics.len(),relics+usize::from(count%2==0));
            assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter,rng+u64::from(count%2==0));
            assert_eq!(run.roster.player(1).unwrap().deck.len(),before);
        }
        assert_eq!(run.event_card_reward_queues[1].len(),1);
        run.select_event_cards(42,1,&[0]).unwrap();assert!(run.event_done[1]);
        assert_eq!(run.roster.player(1).unwrap().deck.len(),before+1);
    }
    #[test]
    fn darv_pandora_reuses_batch_id_order_with_egg_lens_and_bing_bong(){
        let mut run=event_party(EventId::Darv);
        for id in [RelicId::MoltenEgg,RelicId::ToxicEgg,RelicId::FrozenEgg,RelicId::FresnelLens,RelicId::BingBong]{
            run.attach_event_relic(1,id).unwrap();
        }
        let mut reference=run.clone();reference.phase=MpPhase::Map;
        reference.obtain_pandoras_box(42,1).unwrap();
        let event=run.events[1].as_mut().unwrap();
        event.relics.clear();event.relics.push(RelicId::PandorasBox);
        event.options.replace([EventOption::new("relic_1","Pandora","")]);
        run.choose_event_option(42,1,0).unwrap();
        assert_eq!(format!("{:?}",run.roster.player(1).unwrap().deck),
            format!("{:?}",reference.roster.player(1).unwrap().deck));
        assert_eq!(run.roster.next_card_id,reference.roster.next_card_id);
        assert_eq!(format!("{:?}",run.world.run.rngs.niche),format!("{:?}",reference.world.run.rngs.niche));
    }
    #[test]
    fn darv_owned_transform_remove_bell_and_configured_tome(){
        for id in [RelicId::Astrolabe,RelicId::EmptyCage,RelicId::PandorasBox,RelicId::CallingBell,RelicId::DustyTome]{
            let mut run=event_party(EventId::Darv);
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let before=run.roster.player(1).unwrap().deck.clone();
            let event=run.events[1].as_mut().unwrap();
            assert_eq!(event.options.len(),3);
            event.relics.clear();event.relics.push(id);event.cards.clear();event.cards.push(CardId::Suppress);
            event.options.replace([EventOption::new("relic_1","Relic","")]);
            run.choose_event_option(42,1,0).unwrap();
            match id{
                RelicId::Astrolabe|RelicId::EmptyCage=>{
                    let count=if id==RelicId::Astrolabe{3}else{2};
                    let (_,min,max,skip)=run.event_card_modal(42,1).unwrap();
                    assert_eq!((min,max,skip),(count,count,false));
                    run.select_event_cards(42,1,&(0..count).collect::<Vec<_>>()).unwrap();
                    let deck=&run.roster.player(1).unwrap().deck;
                    assert_eq!(deck.len(),before.len()-if id==RelicId::EmptyCage{2}else{0});
                    if id==RelicId::Astrolabe{assert!(deck.iter().rev().take(3).all(|c|c.upgraded));}
                }
                RelicId::PandorasBox=>{
                    assert!(run.roster.player(1).unwrap().deck.iter().all(|c|
                        !(c.rarity==CardRarity::Basic&&(c.has_tag(CardTag::Strike)||c.has_tag(CardTag::Defend)))));
                    assert_eq!(run.roster.player(1).unwrap().deck.len(),before.len());
                }
                RelicId::CallingBell=>{
                    assert_eq!(run.events[1].as_ref().unwrap().relics.len(),3);
                    assert!(run.roster.player(1).unwrap().deck.iter().any(|c|c.id==CardId::CurseOfTheBell));
                    choose_id(&mut run,1,"skip_bell");
                }
                RelicId::DustyTome=>{
                    let p=run.roster.player(1).unwrap();
                    assert_eq!(p.deck.last().unwrap().id,CardId::Suppress);
                    assert!(p.deck.last().unwrap().upgraded);
                    assert_eq!(p.relics.last().unwrap().ancient_card,Some(CardId::Suppress));
                }
                _=>unreachable!(),
            }
            assert!(run.event_done[1]);assert_eq!(other,format!("{:?}",run.roster.player(0).unwrap()));
        }
    }
    #[test]
    fn orobas_owned_pickups_keep_streams_and_await_complete_card_sets(){
        for id in [RelicId::ElectricShrymp,RelicId::GlassEye,RelicId::SeaGlass,RelicId::AlchemicalCoffer,
            RelicId::SandCastle,RelicId::TouchOfOrobas,RelicId::ArchaicTooth]{
            let mut run=event_party(EventId::Orobas);
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let before=run.roster.player(1).unwrap().clone();
            let event=run.events[1].as_mut().unwrap();
            assert_eq!(event.options.len(),3);
            event.ints[0]=0;event.relics.clear();event.relics.push(id);
            event.options.replace([EventOption::new("relic_1","Relic","")]);
            run.choose_event_option(42,1,0).unwrap();
            match id{
                RelicId::ElectricShrymp=>{
                    run.select_event_cards(42,1,&[0]).unwrap();
                    assert_eq!(run.roster.player(1).unwrap().deck.iter().filter(|c|c.enchantments.contains(&(crate::card::enchantment_name("Imbued").unwrap(),1))).count(),1);
                }
                RelicId::GlassEye=>{
                    for rarity in [CardRarity::Common,CardRarity::Common,CardRarity::Uncommon,CardRarity::Uncommon,CardRarity::Rare]{
                        let (cards,min,max,skip)=run.event_card_modal(42,1).unwrap();
                        assert_eq!((cards.len(),min,max,skip),(3,0,1,true));
                        assert!(cards.iter().all(|c|c.rarity==rarity&&c.owner==1));
                        run.select_event_cards(42,1,&[0]).unwrap();
                    }
                    assert_eq!(run.roster.player(1).unwrap().deck.len(),before.deck.len()+5);
                }
                RelicId::SeaGlass=>{
                    let (cards,min,max,_)=run.event_card_modal(42,1).unwrap();
                    assert_eq!((cards.len(),min,max),(15,0,15));
                    assert_eq!(cards.iter().filter(|c|c.rarity==CardRarity::Rare).count(),5);
                    run.select_event_cards(42,1,&[0,5,10]).unwrap();
                    assert_eq!(run.roster.player(1).unwrap().deck.len(),before.deck.len()+3);
                }
                RelicId::AlchemicalCoffer=>{
                    assert_eq!(run.roster.player(1).unwrap().max_potion_slots,before.max_potion_slots+4);
                    assert_eq!(run.roster.player(1).unwrap().potions.iter().flatten().count(),4);
                }
                RelicId::SandCastle=>assert_eq!(run.roster.player(1).unwrap().deck.iter().filter(|c|c.upgraded).count(),6),
                RelicId::TouchOfOrobas=>assert_eq!(run.roster.player(1).unwrap().relics[0].id,RelicId::RingOfTheDrake),
                RelicId::ArchaicTooth=>{
                    assert!(run.roster.player(1).unwrap().deck.iter().any(|c|c.id==CardId::Suppress));
                    assert!(run.roster.player(1).unwrap().relics.last().unwrap().saved_cards.is_some());
                }
                _=>unreachable!(),
            }
            assert!(run.event_done[1]);assert_eq!(other,format!("{:?}",run.roster.player(0).unwrap()));
        }
    }
    #[test]
    fn duplicate_ancient_initialization_only_mutates_obtained_model(){
        for (event_id,id) in [(crate::identity::EventId::Tezcatara,RelicId::GoldenCompass),(crate::identity::EventId::Nonupeipe,RelicId::FurCoat)]{
            let mut run=event_party(event_id);
            let index=run.attach_event_relic(1,id).unwrap();
            let old=&mut run.roster.player_mut(1).unwrap().relics[index];
            old.counter=42;old.counter2=123;old.counter3=456;old.flag1=false;
            let before=format!("{old:?}");
            let event=run.events[1].as_mut().unwrap();
            event.relics.clear();event.relics.push(id);
            event.options.replace([EventOption::new(if event_id==EventId::Tezcatara {"comfort_1"}else{"relic_1"},"Relic","")]);
            run.choose_event_option(42,1,0).unwrap();
            let p=run.roster.player(1).unwrap();
            assert_eq!(format!("{:?}",p.relics[index]),before);
            let new=p.relics.last().unwrap();
            if id==RelicId::GoldenCompass{
                assert!(new.flag1);assert_eq!(new.counter2,run.world.run.act_index);
            }else{
                assert_eq!(new.counter,run.world.run.act_index);
                assert_ne!((new.counter2,new.counter3),(0,0));
            }
        }
    }
    #[test]
    fn toy_box_exhausted_bags_claim_five_wax_circlets_even_when_already_owned(){
        for owned in [false,true]{
            let mut run=event_party(EventId::Tezcatara);
            if owned{run.attach_event_relic(1,RelicId::Circlet).unwrap();}
            for bag in &mut run.world.player_bags[1]{bag.clear();}
            run.world.player_fallbacks[1].clear();
            for bag in &mut run.world.shared_bags{bag.clear();}
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let event=run.events[1].as_mut().unwrap();
            event.relics.clear();event.relics.push(RelicId::ToyBox);
            event.options.replace([EventOption::new("comfort_1","Toy Box","")]);
            run.choose_event_option(42,1,0).unwrap();
            assert_eq!(run.events[1].as_ref().unwrap().relics.as_slice(),&[RelicId::Circlet;5]);
            for count in 1..=5{
                run.choose_event_option(42,1,0).unwrap();
                let circlets:Vec<_>=run.roster.player(1).unwrap().relics.iter().filter(|r|r.id==RelicId::Circlet).collect();
                assert_eq!(circlets.len(),count+usize::from(owned));
                assert_eq!(circlets.iter().filter(|r|r.is_wax).count(),count);
                assert!(circlets.iter().all(|r|r.owner==1&&r.floor_added_to_deck==run.world.run.total_floor));
                if owned{assert!(!circlets[0].is_wax);}
            }
            assert!(run.event_done[1]);assert_eq!(other,format!("{:?}",run.roster.player(0).unwrap()));
        }
    }
    #[test]
    fn relic_cmd_does_not_reject_a_second_nonstackable_model(){
        let mut run=event_party(EventId::Tezcatara);
        for _ in 0..2{run.attach_event_relic(1,RelicId::Anchor).unwrap();}
        assert_eq!(run.roster.player(1).unwrap().relics.iter().filter(|r|r.id==RelicId::Anchor).count(),2);
    }
    #[test]
    fn event_gold_sources_honor_owner_ectoplasm_and_dragon_fruit(){
        for blocked in [false,true]{
            let mut run=event_party(EventId::Nonupeipe);
            if blocked{
                let mut relic=Relic::new(RelicId::Ectoplasm);relic.owner=1;
                run.roster.player_mut(1).unwrap().relics.push(relic);
            }
            let mut fruit=Relic::new(RelicId::DragonFruit);fruit.owner=1;
            run.roster.player_mut(1).unwrap().relics.push(fruit);
            let before=run.roster.player(1).unwrap().clone();
            let event=run.events[1].as_mut().unwrap();
            event.relics.clear();event.relics.push(RelicId::SignetRing);
            event.options.replace([EventOption::new("relic_1","Ring","")]);
            run.choose_event_option(42,1,0).unwrap();
            let p=run.roster.player(1).unwrap();
            assert_eq!(p.gold,before.gold+if blocked{0}else{888});
            assert_eq!(p.max_hp,before.max_hp+if blocked{0}else{1});
            assert_eq!(run.roster.player(0).unwrap().gold,99);
        }
        let mut run=event_party(EventId::SunkenTreasury);
        let mut relic=Relic::new(RelicId::Ectoplasm);relic.owner=1;
        run.roster.player_mut(1).unwrap().relics.push(relic);
        let gold=run.roster.player(1).unwrap().gold;
        choose_id(&mut run,1,"first_chest");
        assert_eq!(run.roster.player(1).unwrap().gold,gold);
    }
    #[test]
    fn tezcatara_owned_pickups_and_multiplayer_golden_path(){
        for id in [RelicId::NutritiousSoup,RelicId::Storybook,RelicId::YummyCookie,
            RelicId::BiiigHug,RelicId::GoldenCompass,RelicId::ToyBox]{
            let mut run=event_party(EventId::Tezcatara);
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let before=run.roster.player(1).unwrap().deck.len();
            let event=run.events[1].as_mut().unwrap();
            assert_eq!(event.options.len(),3);
            event.relics.clear();event.relics.push(id);
            event.options.replace([super::super::super::events::EventOption::new("comfort_1","Gift","")]);
            run.choose_event_option(42,1,0).unwrap();
            if matches!(id,RelicId::YummyCookie|RelicId::BiiigHug){
                let (_,min,max,skip)=run.event_card_modal(42,1).unwrap();
                assert_eq!((min,max,skip),(4,4,false));
                run.select_event_cards(42,1,&[0,1,2,3]).unwrap();
            }
            let p=run.roster.player(1).unwrap();
            match id{
                RelicId::NutritiousSoup=>assert!(p.deck.iter().filter(|c|c.rarity==CardRarity::Basic&&c.has_tag(CardTag::Strike))
                    .all(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::TezcatarasEmber,1)))),
                RelicId::Storybook=>assert_eq!(p.deck.last().unwrap().id,CardId::BrightestFlame),
                RelicId::YummyCookie=>assert_eq!(p.deck.iter().filter(|c|c.upgraded).count(),4),
                RelicId::BiiigHug=>assert_eq!(p.deck.len(),before-4),
                RelicId::GoldenCompass=>{
                    assert_eq!(run.world.map.points.len(),17);
                    assert_eq!(run.world.map.points[run.world.map.boss as usize].coord.row,16);
                    assert_eq!(run.world.map.points.iter().filter(|p|p.point_type==MapPointType::Monster).count(),2);
                    let odds=run.unknown_odds;
                    let draws=run.world.run.rngs.unknown_map_point.counter;
                    run.world.encounters.events=vec![EventId::SunkenTreasury];
                    run.event_cursor=0;
                    run.enter_noncombat(MapPointType::Unknown,super::super::super::map::Coord::new(3,2)).unwrap();
                    assert_eq!(run.phase,MpPhase::Event);
                    assert_eq!(run.unknown_odds,odds);
                    assert_eq!(run.world.run.rngs.unknown_map_point.counter,draws+1);
                }
                RelicId::ToyBox=>{
                    assert_eq!(run.events[1].as_ref().unwrap().relics.len(),5);
                    assert_eq!(run.events[1].as_ref().unwrap().options.len(),6);
                    run.events[1].as_mut().unwrap().relics[0]=RelicId::Anchor;
                    run.choose_event_option(42,1,0).unwrap();
                    assert!(run.roster.player(1).unwrap().relics.iter().any(|r|r.id==RelicId::Anchor&&r.is_wax));
                    choose_id(&mut run,1,"skip_wax");
                }
                _=>unreachable!(),
            }
            if id!=RelicId::GoldenCompass{assert!(run.event_done[1]);}
            assert_eq!(other,format!("{:?}",run.roster.player(0).unwrap()));
        }
    }
    #[test]
    fn failed_event_initialization_reports_the_attempted_model(){
        let specs=[PlayerSpec{net_id:7,character:Character::Ironclad},
            PlayerSpec{net_id:42,character:Character::Silent}];
        let mut run=MpRun::new("73003",&specs,0).unwrap();
        run.world.current_room=Some("NONUPEIPE");
        assert_eq!(run.begin_generic_event(EventId::DeprecatedEvent),Err("DEPRECATED_EVENT"));
    }
    #[test]
    fn paels_tooth_returns_one_owned_upgraded_card_each_combat_until_empty(){
        for owner in 0..2{
            let mut run=event_party(EventId::Pael);
            let net=run.roster.player(owner).unwrap().net_id;
            let event=run.events[owner].as_mut().unwrap();
            event.relics.clear();event.relics.push(RelicId::PaelsTooth);
            event.options.replace([super::super::super::events::EventOption::new("relic_1","Tooth","")]);
            run.choose_event_option(net,owner,0).unwrap();
            run.select_event_cards(net,owner,&[0,1,2,3,4]).unwrap();
            assert_eq!(run.roster.player(owner).unwrap().paels_tooth_cards.len(),5);
            for remaining in (1..=5).rev(){
                let p=run.roster.player(owner).unwrap();
                let mut expected_rng=p.rngs.rewards.clone();
                let expected=p.paels_tooth_cards[expected_rng.choice_index(remaining)].instance_id;
                let deck_len=p.deck.len();
                let other_before=format!("{:?}",run.roster.player(1-owner).unwrap().rngs.rewards);
                run.combat=Some(run.roster.enter_combat(run.world.run.clone(),99,
                    "SLUDGE_SPINNER_WEAK",RoomType::Monster).unwrap());
                run.phase=MpPhase::Combat;
                run.debug_win_combat().unwrap();
                run.world.run=run.roster.leave_combat(run.combat.take().unwrap()).unwrap();
                let p=run.roster.player(owner).unwrap();
                assert_eq!(p.paels_tooth_cards.len(),remaining-1);
                assert_eq!(p.deck.len(),deck_len+1);
                let returned=p.deck.iter().find(|c|c.instance_id==expected).unwrap();
                assert!(returned.upgraded);assert_eq!(returned.owner,owner as u8);
                assert_eq!(format!("{:?}",p.rngs.rewards),format!("{expected_rng:?}"));
                assert_eq!(format!("{:?}",run.roster.player(1-owner).unwrap().rngs.rewards),other_before);
            }
        }
    }
    #[test]
    fn nonupeipe_owner_effects_and_private_fur_coat_marks(){
        for id in [RelicId::FurCoat,RelicId::BeautifulBracelet,RelicId::LoomingFruit,
            RelicId::SignetRing,RelicId::JewelryBox]{
            let mut run=event_party(EventId::Nonupeipe);
            let other=format!("{:?}",run.roster.player(0).unwrap());
            let before=run.roster.player(1).unwrap().clone();
            let niche=run.world.run.rngs.niche.counter;
            let event=run.events[1].as_mut().unwrap();
            assert_eq!(event.options.len(),3);
            event.relics.clear();event.relics.push(id);
            event.options.replace([super::super::super::events::EventOption::new("relic_1","Nonupeipe","")]);
            let expected=run.world.map.fur_coat_coords(run.world.run.rngs.seed,1,&[]);
            run.choose_event_option(42,1,0).unwrap();
            let p=run.roster.player(1).unwrap();
            assert!(run.event_done[1]);assert_eq!(other,format!("{:?}",run.roster.player(0).unwrap()));
            match id{
                RelicId::FurCoat=>{
                    let r=p.relics.iter().find(|r|r.id==id).unwrap();
                    assert_eq!(expected.len(),8);assert!(expected.iter().all(|&c|r.fur_coat_has_coord(c)));
                    assert_eq!(run.world.run.rngs.niche.counter,niche);
                }
                RelicId::BeautifulBracelet=>{
                    let eligible=before.deck.iter().filter(|c|super::super::super::manager::RunManager::event_can_enchant(c,"Swift")).count();
                    assert_eq!(run.world.run.rngs.niche.counter,niche+eligible.saturating_sub(1) as u64);
                    assert_eq!(p.deck.iter().filter(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::Swift,2))).count(),4);
                }
                RelicId::LoomingFruit=>assert_eq!((p.max_hp,p.hp),(before.max_hp+31,before.hp+31)),
                RelicId::SignetRing=>assert_eq!(p.gold,before.gold+888),
                RelicId::JewelryBox=>assert_eq!(p.deck.last().unwrap().id,CardId::Apotheosis),
                _=>unreachable!(),
            }
        }
    }
    #[test]
    fn vakuu_pickups_apply_owned_cards_max_hp_and_removal_continuations(){
        for id in [RelicId::SereTalon,RelicId::DistinguishedCape,RelicId::PreservedFog,RelicId::BloodSoakedRose]{
            let mut run=event_party(EventId::Vakuu);
            let before=run.roster.player(0).unwrap().clone();
            let owner=run.roster.player(1).unwrap().clone();
            let niche=run.world.run.rngs.niche.counter;
            let event=run.events[1].as_mut().unwrap();
            event.relics.clear();event.relics.push(id);
            event.options.replace([super::super::super::events::EventOption::new("relic_1","Vakuu","")]);
            run.choose_event_option(42,1,0).unwrap();
            if id==RelicId::PreservedFog{
                let (_,min,max,skip)=run.event_card_modal(42,1).unwrap();
                assert_eq!((min,max,skip),(3,3,false));
                run.select_event_cards(42,1,&[0,1,2]).unwrap();
            }
            let after=run.roster.player(1).unwrap();
            assert_eq!(format!("{:?}",run.roster.player(0).unwrap()),format!("{before:?}"));
            assert!(run.event_done[1]);
            match id{
                RelicId::SereTalon=>{
                    assert_eq!(after.max_hp,owner.max_hp-9);assert_eq!(after.hp,owner.hp-9);
                    assert_eq!(after.deck.iter().filter(|c|c.id==CardId::Wish).count(),3);
                }
                RelicId::DistinguishedCape=>{
                    assert_eq!(after.deck.len(),owner.deck.len()+5);
                    assert_eq!(after.deck.iter().filter(|c|c.id==CardId::Apparition).count(),3);
                    assert_eq!(run.world.run.rngs.niche.counter,niche+2);
                    assert_ne!(after.deck[owner.deck.len()].id,after.deck[owner.deck.len()+1].id);
                }
                RelicId::PreservedFog=>{
                    assert_eq!(after.deck.len(),owner.deck.len()-2);
                    assert_eq!(after.deck.last().unwrap().id,CardId::Folly);
                }
                RelicId::BloodSoakedRose=>assert_eq!(after.deck.last().unwrap().id,CardId::Enthralled),
                _=>unreachable!(),
            }
        }
    }
    #[test]
    fn pael_owner_pickups_enchant_all_goopy_store_five_and_add_two_relax(){
        for id in [RelicId::PaelsClaw,RelicId::PaelsTooth,RelicId::PaelsGrowth,RelicId::PaelsHorn]{
            let mut run=event_party(EventId::Pael);
            let before=run.roster.player(0).unwrap().clone();
            let owner=run.roster.player(1).unwrap().clone();
            let event=run.events[1].as_mut().unwrap();
            event.relics.clear();event.relics.push(id);
            event.options.replace([super::super::super::events::EventOption::new("relic_1","Pael","")]);
            run.choose_event_option(42,1,0).unwrap();
            if matches!(id,RelicId::PaelsTooth|RelicId::PaelsGrowth){
                let (cards,min,max,skip)=run.event_card_modal(42,1).unwrap();
                assert_eq!(min,if id==RelicId::PaelsTooth{5}else{1});
                assert_eq!(min,max);assert!(!skip);assert!(cards.len()>=min);
                run.select_event_cards(42,1,&(0..min).collect::<Vec<_>>()).unwrap();
            }
            let after=run.roster.player(1).unwrap();
            assert!(run.event_done[1]);
            assert_eq!(format!("{:?}",run.roster.player(0).unwrap()),format!("{before:?}"));
            match id{
                RelicId::PaelsHorn=>assert_eq!(after.deck.iter().filter(|c|c.id==CardId::Relax).count(),2),
                RelicId::PaelsClaw=>assert_eq!(after.deck.iter().filter(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::Goopy,1))).count(),
                    owner.deck.iter().filter(|c|super::super::super::manager::RunManager::event_can_enchant(c,"Goopy")).count()),
                RelicId::PaelsGrowth=>assert_eq!(after.deck.iter().filter(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::Clone,4))).count(),1),
                RelicId::PaelsTooth=>assert_eq!(after.deck.len(),owner.deck.len()-5),
                _=>unreachable!(),
            }
        }
    }
    #[test]
    fn hollow_hug_selects_transform_before_damage_and_keeps_gold() {
        let mut run=event_party(EventId::WhisperingHollow);
        let before=run.roster.player(1).unwrap().clone();
        run.choose_event_option(42,1,1).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp,before.hp);
        let (cards,min,max,skip)=run.event_card_modal(42,1).unwrap();
        assert_eq!((min,max,skip),(1,1,false));
        let original=cards[0].instance_id;
        run.select_event_cards(42,1,&[0]).unwrap();
        let after=run.roster.player(1).unwrap();
        assert_eq!((after.hp,after.gold,after.deck.len()),(before.hp-9,before.gold,before.deck.len()));
        assert!(!after.deck.iter().any(|c|c.instance_id==original));
        assert!(run.event_done[1]);
    }
    use super::*;
    use crate::{card::Card, run::mp::{Character, PlayerSpec}, tables::{CardId, PotionId, RoomType}};

    fn event_party(id: crate::identity::EventId) -> MpRun {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent }];
        let mut run = MpRun::new("73003", &specs, 0).unwrap();
        run.phase = MpPhase::Event;
        run.world.current_room=Some(id.name());
        run.event_done.fill(false);
        run.begin_generic_event(id).unwrap();
        run
    }

    fn choose_id(run: &mut MpRun, seat: usize, id: &str) {
        let net = run.roster.player(seat).unwrap().net_id;
        let enabled: Vec<_> = run.events[seat].as_ref().unwrap().options.iter()
            .filter(|option| option.enabled).map(|option| option.option_id).collect();
        let index = enabled.iter().position(|option| *option == id).unwrap();
        run.choose_event_option(net, seat, index).unwrap();
    }

    #[test]
    fn lethal_multiplayer_event_damage_preserves_owner_and_unused_revivers() {
        for seat in 0..2 {
            for protection in 0..3 {
                let mut run = event_party(EventId::AbyssalBaths);
                let other_hp = run.roster.player(1 - seat).unwrap().hp;
                let p = run.roster.player_mut(seat).unwrap();
                let max_hp = p.max_hp;
                p.hp = 1;
                if protection == 1 {
                    let mut tail = Relic::new(RelicId::LizardTail);
                    tail.owner = seat as u8;
                    p.relics.push(tail);
                } else if protection == 2 {
                    p.potions[0] = Some(PotionId::Fairyinabottle);
                }
                choose_id(&mut run, seat, "immerse");
                let p = run.roster.player(seat).unwrap();
                assert_eq!(p.hp, 1);
                assert_eq!(p.max_hp, max_hp + 2);
                assert_eq!(run.roster.player(1 - seat).unwrap().hp, other_hp);
                assert_eq!(run.phase, MpPhase::Event);
                assert!(run.events[seat].is_some());
                if protection == 1 {
                    let tail = p.relics.iter().find(|r| r.id == RelicId::LizardTail).unwrap();
                    assert!(tail.enabled);
                    assert!(!tail.flag1);
                } else if protection == 2 {
                    assert_eq!(p.potions[0], Some(PotionId::Fairyinabottle));
                }
                // C# caps the description counter at nine, but OnImmerse
                // keeps increasing Damage.BaseValue after every selection.
                for linger in 1..=12 {
                    choose_id(&mut run, seat, "linger");
                    let p = run.roster.player(seat).unwrap();
                    assert_eq!(p.hp, 1);
                    assert_eq!(p.max_hp, max_hp + 2 * (linger + 1));
                    assert_eq!(run.events[seat].as_ref().unwrap().ints[0], 4 + linger);
                    assert_eq!(run.events[seat].as_ref().unwrap().ints[1], linger.min(9));
                    assert_eq!(run.roster.player(1 - seat).unwrap().hp, other_hp);
                    if protection == 1 {
                        assert!(!p.relics.iter().find(|r| r.id == RelicId::LizardTail).unwrap().flag1);
                    } else if protection == 2 {
                        assert_eq!(p.potions[0], Some(PotionId::Fairyinabottle));
                    }
                }
                choose_id(&mut run, seat, "exit_baths");
                assert!(run.events[seat].is_none());
            }
            let mut run = event_party(EventId::AbyssalBaths);
            run.world.current_room = Some("SLIPPERY_BRIDGE");
            run.world.run.total_floor = 12;
            run.begin_bridge().unwrap();
            run.roster.player_mut(seat).unwrap().hp = 1;
            let rng = run.events[seat].as_ref().unwrap().rng.counter;
            let net_id = run.roster.player(seat).unwrap().net_id;
            run.choose_bridge_option(net_id, seat, 1).unwrap();
            assert_eq!(run.roster.player(seat).unwrap().hp, 1);
            assert_eq!(run.events[seat].as_ref().unwrap().bridge.hold_ons, 1);
            assert_eq!(run.events[seat].as_ref().unwrap().rng.counter, rng + 1);
            assert_eq!(run.phase, MpPhase::Event);
        }
    }

    #[test]
    fn baths_ui_blocks_lethal_clicks_but_preserves_raw_dll_options() {
        use crate::identity::EventOptionId;
        let mut run=event_party(EventId::AbyssalBaths);
        run.roster.player_mut(0).unwrap().hp=1;
        assert!(run.native_event_click_blocked(0,EventOptionId::AbyssalBathsInitialImmerse));
        run.roster.player_mut(0).unwrap().hp=2;
        assert!(!run.native_event_click_blocked(0,EventOptionId::AbyssalBathsInitialImmerse));
        choose_id(&mut run,0,"immerse");
        assert_eq!(run.roster.player(0).unwrap().hp,1);
        assert!(run.native_event_click_blocked(0,EventOptionId::AbyssalBathsAllLinger));
        assert!(!run.native_event_click_blocked(0,EventOptionId::AbyssalBathsAllExitBaths));
        assert_eq!(run.native_event_options(7,0).unwrap().unwrap().len(),2);
        choose_id(&mut run,0,"exit_baths");
        assert!(run.events[0].is_none());
    }

    #[test]
    fn bridge_ui_blocks_lethal_hold_on_and_leaves_overcome_available() {
        use crate::identity::EventOptionId;
        let mut run=event_party(EventId::AbyssalBaths);
        run.world.current_room=Some("SLIPPERY_BRIDGE");
        run.world.run.total_floor=12;
        run.begin_bridge().unwrap();
        run.roster.player_mut(0).unwrap().hp=3;
        assert!(run.native_event_click_blocked(0,EventOptionId::SlipperyBridgeInitialHoldOn0));
        run.roster.player_mut(0).unwrap().hp=4;
        assert!(!run.native_event_click_blocked(0,EventOptionId::SlipperyBridgeInitialHoldOn0));
        run.choose_bridge_option(7,0,1).unwrap();
        assert_eq!(run.roster.player(0).unwrap().hp,1);
        assert!(run.native_event_click_blocked(0,EventOptionId::SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2));
        run.choose_bridge_option(7,0,0).unwrap();
        assert!(run.events[0].is_none());
    }

    #[test]
    fn event_potion_reward_can_free_an_owner_slot_without_leaving_event() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent }];
        let mut run = MpRun::new("73007", &specs, 0).unwrap();
        run.world.current_room = Some("WHISPERING_HOLLOW");
        run.phase = MpPhase::Event;
        run.roster.player_mut(1).unwrap().potions.fill(Some(PotionId::Energypotion));
        run.event_potion_rewards[1] = Some([Some(PotionId::Firepotion), None, None]);

        assert!(!run.take_whispering_hollow_reward(42, 1, 0).unwrap());
        run.discard_potion(42, 1, 0).unwrap();
        assert!(run.take_whispering_hollow_reward(42, 1, 0).unwrap());
        assert!(run.roster.player(1).unwrap().potions.iter().any(|p| *p == Some(PotionId::Firepotion)));
        assert_eq!(run.phase, MpPhase::Event);
    }

    #[test]
    fn bridge_uses_owner_event_rng_and_shared_solo_rules_without_cross_owner_removal() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.world.run.total_floor = 12;
        run.world.current_room = Some("SLIPPERY_BRIDGE");
        run.phase = MpPhase::Event;
        for seat in 0..2 {
            run.roster.next_card_id += 1;
            let mut card = Card::from_factory(CardId::Headbutt, false, run.roster.next_card_id).unwrap();
            card.owner = seat as u8;
            run.roster.player_mut(seat).unwrap().deck.push(card);
        }
        run.begin_bridge().unwrap();
        let first = run.events[1].as_ref().unwrap().bridge.current.unwrap();
        let before = run.roster.player(0).unwrap().deck.clone();
        assert!(run.choose_bridge_option(7, 1, 0).is_err());
        let hp = run.roster.player(1).unwrap().hp;
        run.choose_bridge_option(42, 1, 1).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, hp - 3);
        let second = run.events[1].as_ref().unwrap().bridge.current.unwrap();
        assert_ne!(first, second);
        run.choose_bridge_option(42, 1, 0).unwrap();
        assert_eq!(run.phase, MpPhase::Event);
        assert!(!run.roster.player(1).unwrap().deck.iter().any(|c| c.instance_id == second));
        assert_eq!(run.roster.player(0).unwrap().deck.iter().map(|c| c.instance_id).collect::<Vec<_>>(),
            before.iter().map(|c| c.instance_id).collect::<Vec<_>>());
        run.choose_bridge_option(7, 0, 0).unwrap();
        assert_eq!(run.phase, MpPhase::Map);
    }

    #[test]
    fn owner_card_selection_events_preserve_source_order_and_owner_scope() {
        // SapphireSeed.cs:35-58, AromaOfChaos.cs:29-47, SelfHelpBook.cs:82-125.
        let mut run = event_party(EventId::SapphireSeed);
        run.roster.player_mut(0).unwrap().hp = 40;
        choose_id(&mut run, 0, "eat");
        assert_eq!(run.roster.player(0).unwrap().hp, 49);
        let (cards, min, max, skip) = run.event_card_modal(7, 0).unwrap();
        assert_eq!((min, max, skip), (1, 1, false));
        let selected = cards[0].instance_id;
        run.select_event_cards(7, 0, &[0]).unwrap();
        assert!(run.roster.player(0).unwrap().deck.iter()
            .find(|card| card.instance_id == selected).unwrap().upgraded);

        let mut run = event_party(EventId::AromaOfChaos);
        choose_id(&mut run, 1, "let_go");
        let selected = run.event_card_modal(42, 1).unwrap().0[0].instance_id;
        let other = run.roster.player(0).unwrap().deck.clone();
        run.select_event_cards(42, 1, &[0]).unwrap();
        assert!(!run.roster.player(1).unwrap().deck.iter().any(|card| card.instance_id == selected));
        assert_eq!(run.roster.player(0).unwrap().deck.iter().map(|c| c.instance_id).collect::<Vec<_>>(),
            other.iter().map(|c| c.instance_id).collect::<Vec<_>>());

        let mut run = event_party(EventId::SelfHelpBook);
        choose_id(&mut run, 0, "read_back");
        let selected = run.event_card_modal(7, 0).unwrap().0[0].instance_id;
        run.select_event_cards(7, 0, &[0]).unwrap();
        let card = run.roster.player(0).unwrap().deck.iter()
            .find(|card| card.instance_id == selected).unwrap();
        assert!(card.enchantments.iter().any(|(name, amount)| *name == crate::identity::EnchantmentId::Sharp && *amount == 2));
    }

    #[test]
    fn custom_event_rewards_and_fixed_followups_match_original_dll() {
        // DrowningBeacon.cs:32-49, Wellspring.cs:29-55, ThisOrThat.cs:35-49.
        let mut run = event_party(EventId::DrowningBeacon);
        choose_id(&mut run, 0, "bottle");
        assert_eq!(run.event_potion_rewards(7, 0).unwrap()[0], Some(PotionId::Glowwaterpotion));
        assert!(run.take_event_potion_reward(7, 0, 0).unwrap());
        run.finish_event_potion_rewards(7, 0).unwrap();
        assert!(run.roster.player(0).unwrap().potions.contains(&Some(PotionId::Glowwaterpotion)));

        let mut run = event_party(EventId::Wellspring);
        let removed = run.roster.player(1).unwrap().deck[0].instance_id;
        choose_id(&mut run, 1, "bathe");
        let index = run.event_card_modal(42, 1).unwrap().0.iter()
            .position(|card| card.instance_id == removed).unwrap();
        run.select_event_cards(42, 1, &[index]).unwrap();
        assert!(!run.roster.player(1).unwrap().deck.iter().any(|card| card.instance_id == removed));
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, CardId::Guilty);

        let mut run = event_party(EventId::ThisOrThat);
        let relics = run.roster.player(0).unwrap().relics.len();
        choose_id(&mut run, 0, "ornate");
        assert_eq!(run.roster.player(0).unwrap().relics.len(), relics + 1);
        assert_eq!(run.roster.player(0).unwrap().deck.last().unwrap().id, CardId::Clumsy);
    }

    #[test]
    fn multiplayer_only_event_continuations_cover_rng_enchants_and_shared_combat() {
        // DoorsOfLightAndDark.cs:25-42, WaterloggedScriptorium.cs,
        // WoodCarvings.cs:44-85, DenseVegetation.cs:88-103.
        let mut run = event_party(EventId::DoorsOfLightAndDark);
        choose_id(&mut run, 0, "light");
        assert_eq!(run.roster.player(0).unwrap().deck.iter().filter(|c| c.upgraded).count(), 2);

        let mut run = event_party(EventId::WaterloggedScriptorium);
        run.roster.player_mut(0).unwrap().gold = 200;
        choose_id(&mut run, 0, "tentacle_quill");
        let selected = run.event_card_modal(7, 0).unwrap().0[0].instance_id;
        run.select_event_cards(7, 0, &[0]).unwrap();
        assert_eq!(run.roster.player(0).unwrap().gold, 145);
        assert!(run.roster.player(0).unwrap().deck.iter()
            .find(|card| card.instance_id == selected).unwrap().enchantments.iter()
            .any(|(name, amount)| *name == crate::identity::EnchantmentId::Steady && *amount == 1));

        let mut run = event_party(EventId::WoodCarvings);
        choose_id(&mut run, 0, "bird");
        let index = run.event_card_modal(7, 0).unwrap().0.iter()
            .position(|card| card.rarity == CardRarity::Basic).unwrap();
        run.select_event_cards(7, 0, &[index]).unwrap();
        assert_eq!(run.roster.player(0).unwrap().deck.last().unwrap().id, CardId::Peck);

        let mut run = event_party(EventId::DenseVegetation);
        choose_id(&mut run, 0, "rest");
        choose_id(&mut run, 1, "rest");
        choose_id(&mut run, 0, "fight");
        choose_id(&mut run, 1, "fight");
        assert_eq!(run.phase, MpPhase::Combat);
        assert_eq!(run.world.current_room, Some("DENSE_VEGETATION_EVENT"));
    }

    #[test]
    fn tablet_uses_one_owner_rng_pick_in_deck_order_then_upgrades_all() {
        for seat in 0..2 {
            let mut run = event_party(EventId::TabletOfTruth);
            run.roster.player_mut(seat).unwrap().deck =
                [CardId::Headbutt, CardId::Bash, CardId::Headbutt].into_iter().enumerate()
                    .map(|(i, id)| {
                        let mut card = Card::from_factory(id, false, 1000 + i as u32).unwrap();
                        card.owner = seat as u8;
                        card
                    }).collect();
            let other = format!("{:?}", run.roster.player(1 - seat).unwrap().deck);
            let mut rng = run.events[seat].as_ref().unwrap().rng.clone();
            let selected = rng.choice_index(3);
            choose_id(&mut run, seat, "decipher");
            for (index, card) in run.roster.player(seat).unwrap().deck.iter().enumerate() {
                assert_eq!(card.upgraded, index == selected);
            }
            let event_rng = &run.events[seat].as_ref().unwrap().rng;
            assert_eq!(event_rng.counter, rng.counter);
            assert_eq!(event_rng.inner.s, rng.inner.s);
            assert_eq!(format!("{:?}", run.roster.player(1 - seat).unwrap().deck), other);

            let max_hp = run.roster.player(seat).unwrap().max_hp;
            let event = run.events[seat].as_mut().unwrap();
            event.ints[0] = 4;
            event.ints[1] = max_hp - 1;
            choose_id(&mut run, seat, "decipher");
            assert!(run.roster.player(seat).unwrap().deck.iter().all(|card| card.upgraded));
            assert_eq!(run.roster.player(seat).unwrap().max_hp, 1);
            assert_eq!(format!("{:?}", run.roster.player(1 - seat).unwrap().deck), other);
        }
    }

    #[test]
    fn elite_rewards_populate_one_relic_per_owner_and_retire_shared_bags() {
        let mut run = event_party(EventId::PunchOff);
        choose_id(&mut run, 0, "take_them");
        choose_id(&mut run, 1, "take_them");
        choose_id(&mut run, 0, "fight");
        choose_id(&mut run, 1, "fight");
        // Exercise ordinary elite rewards in an active combat fixture;
        // the separate Punch Off regression covers the event's extra set.
        run.punch_off_owners.clear();
        let combat = run.combat.as_mut().unwrap();
        combat.run.room_type = RoomType::Elite;
        for enemy in combat.alive_enemies() { combat.kill_creature(enemy); }
        combat.check_combat_end();
        run.phase = MpPhase::Rewards;
        let mut expected_bags = run.world.player_bags.clone();
        let mut expected_shared = run.world.shared_bags.clone();
        run.offer_rewards().unwrap();
        for (seat, reward) in run.rewards.as_ref().unwrap().iter().enumerate() {
            assert_eq!(reward.extra_relics.len(), 1);
            let relic = reward.extra_relics[0].unwrap();
            // RelicFactory pulls from this owner's bag and removes from the
            // shared bag only; the other owners retain their own copies.
            for bag in &mut expected_bags[seat] { bag.retain(|id| *id != relic); }
            for bag in &mut expected_shared { bag.retain(|id| *id != relic); }
        }
        assert_eq!(run.world.player_bags, expected_bags);
        assert_eq!(run.world.shared_bags, expected_shared);
    }

    #[test]
    fn fake_merchant_is_never_selected_in_multiplayer_and_punch_off_keeps_layout_rewards() {
        // FakeMerchant.cs:89-103 rejects Players.Count > 1.
        let run = event_party(EventId::ThisOrThat);
        assert!(!run.event_is_allowed(EventId::FakeMerchant));

        // PunchOff.cs:104-141 prepares the event combat layout before choices;
        // Nab retains one RelicReward; Fight reuses the layout.
        let mut run = event_party(EventId::PunchOff);
        assert_eq!(run.combat().unwrap().alive_enemies().len(), 2);
        assert_eq!(run.world.run.rngs.niche.counter,run.combat().unwrap().run.rngs.niche.counter);
        let cards = run.roster.player(0).unwrap().deck.len();
        choose_id(&mut run, 0, "nab");
        choose_id(&mut run, 1, "nab");
        assert_eq!(run.roster.player(0).unwrap().deck.len(), cards + 1);
        assert_eq!(run.roster.player(0).unwrap().deck.last().unwrap().id, CardId::Injury);
        assert!(run.event_relic_reward(7, 0).is_ok());
        run.select_event_relic_reward(7, 0, false).unwrap();
        for seat in 1..run.roster.players().len() {
            run.select_event_relic_reward(run.roster.player(seat).unwrap().net_id,seat,false).unwrap();
        }
        assert_eq!(run.phase,MpPhase::Map);
        assert!(run.combat().is_none());
        assert!(run.roster.players().iter().all(|p|p.deck.last().unwrap().id==CardId::Injury));

        let mut run = event_party(EventId::PunchOff);
        choose_id(&mut run, 0, "take_them");
        choose_id(&mut run, 1, "take_them");
        choose_id(&mut run, 0, "fight");
        choose_id(&mut run, 1, "fight");
        assert_eq!(run.phase, MpPhase::Combat);
        assert_eq!(run.punch_off_owners, vec![0,1]);
        let combat = run.combat.as_mut().unwrap();
        for enemy in combat.alive_enemies() { combat.kill_creature(enemy); }
        combat.check_combat_end();
        run.phase = MpPhase::Rewards;
        run.offer_rewards().unwrap();
        let reward = &run.rewards.as_ref().unwrap()[0];
        assert_eq!(reward.extra_relics.len(), 1);
        assert_eq!(reward.extra_potions.len(), 1);
    }
}
