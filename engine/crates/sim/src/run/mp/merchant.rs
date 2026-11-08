//! C# Entities/Merchant/{MerchantEntry,MerchantCardEntry,MerchantRelicEntry,
//! MerchantPotionEntry,MerchantCardRemovalEntry}; no independent solo manager.
use super::{MpRun, MpPhase};
use super::super::{shop, manager::RelicBags};
use crate::{card::Card, combat::RunCtx, rng::Rng, tables::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MerchantAction { BuyCard(usize), BuyRelic(usize), BuyPotion(usize), RemoveCard(Option<u32>) }

#[derive(Clone, Debug)]
pub struct MerchantOffer {
    pub action: MerchantAction, pub price: i32, pub enabled: bool,
    pub card: Option<Card>, pub relic: Option<RelicId>, pub potion: Option<PotionId>,
}

// Rng.cs:119-125 casts AFTER double interpolation with float32 endpoints.
fn varied(base: i32, sample: f64, low: f32, high: f32) -> i32 {
    let factor = (sample * (high - low) as f64 + low as f64) as f32;
    (base as f32 * factor).round_ties_even() as i32
}

fn last_price_draw(mut before: Rng, after: &Rng) -> f64 {
    // Both shared card constructors end in CalcCost (sale recalculates cost).
    // Replay ONLY the cloned stream to that exact call; no game RNG advances.
    while before.counter + 1 < after.counter { before.next_float(1.0); }
    before.next_float(1.0)
}

fn card_entry(ctx: &mut RunCtx, character: crate::character::Character, offset: f64, ty: CardType,
    type_name: &'static str, stocked: &[CardId], sale: bool, colorless: Option<CardRarity>)
    -> Result<shop::ShopCardEntry, &'static str> {
    let before = ctx.rngs.shops.clone();
    let mut entry = if let Some(rarity) = colorless {
        shop::make_colorless_card(ctx, true, rarity, stocked)?
    } else { shop::make_character_card(ctx, character.into(), true, offset, ty, type_name, stocked, sale)? };
    let card = entry.card.as_ref().unwrap();
    let mut base = match card.rarity { CardRarity::Rare => 150, CardRarity::Uncommon => 75, _ => 50 };
    if colorless.is_some() { base = (base as f32 * 1.15f32).round_ties_even() as i32; }
    entry.price = varied(base, last_price_draw(before, &ctx.rngs.shops), 0.95, 1.05);
    if sale { entry.price /= 2; }
    Ok(entry)
}

fn relic_entry(ctx: &mut RunCtx, bags: &mut RelicBags, rarity: RelicRarity, excluded: &[RelicId])
    -> shop::ShopRelicEntry {
    let id = shop::pull_shop_relic(bags, rarity, excluded);
    let rarity = RELIC_INFO[id.index()].rarity;
    let base = if id == RelicId::Circlet { 1 } else { match rarity {
        RelicRarity::Common => 175, RelicRarity::Uncommon => 225, RelicRarity::Rare => 275, _ => 200 } };
    let price = varied(base, ctx.rngs.shops.next_float(1.0), 0.85, 1.15);
    shop::ShopRelicEntry { relic_id: id, rarity, price }
}

fn potion_cost(ctx: &mut RunCtx, id: PotionId) -> i32 {
    let base = match POTION_INFO[id.index()].rarity {
        PotionRarity::Rare => 100, PotionRarity::Uncommon => 75, _ => 50 };
    varied(base, ctx.rngs.shops.next_float(1.0), 0.95, 1.05)
}

impl MpRun {
    fn merchant_ctx(&self, seat: usize) -> Result<RunCtx, &'static str> {
        let p = self.roster.player(seat).ok_or("invalid merchant owner")?;
        let mut ctx = self.world.run.clone();
        ctx.paels_tooth_cards=p.paels_tooth_cards.clone();
        ctx.max_energy=p.max_energy;ctx.base_orb_slots=p.base_orb_slots as i32;
        ctx.deck = p.deck.clone(); ctx.relics = p.relics.clone(); ctx.potions = p.potions.clone();
        ctx.gold = p.gold; ctx.hp = p.hp; ctx.max_hp = p.max_hp; ctx.max_potion_slots = p.max_potion_slots as i32;
        ctx.next_card_id = self.roster.next_card_id;
        ctx.rngs.rewards = p.rngs.rewards.clone(); ctx.rngs.shops = p.rngs.shops.clone();
        ctx.rngs.transformations = p.rngs.transformations.clone();
        for r in &mut ctx.relics { r.owner = 0; }
        for c in ctx.deck.iter_mut().chain(&mut ctx.paels_tooth_cards){c.owner=0;}
        Ok(ctx)
    }

    fn merchant_bags(&self, seat: usize) -> RelicBags {
        RelicBags { shared: self.world.shared_bags.clone(), player: vec![self.world.player_bags[seat].clone()] }
    }

    fn carry_merchant(&mut self, seat: usize, mut ctx: RunCtx, mut bags: RelicBags) {
        for r in &mut ctx.relics { r.owner = seat as u8; }
        for c in &mut ctx.deck { c.owner = seat as u8; }
        for c in &mut ctx.paels_tooth_cards{c.owner=seat as u8;}
        let p = self.roster.player_mut(seat).unwrap();
        p.paels_tooth_cards=ctx.paels_tooth_cards;
        p.max_energy=ctx.max_energy;p.base_orb_slots=ctx.base_orb_slots as usize;
        p.deck = ctx.deck; p.relics = ctx.relics; p.potions = ctx.potions; p.gold = ctx.gold;
        p.hp = ctx.hp; p.max_hp = ctx.max_hp; p.max_potion_slots = ctx.max_potion_slots as usize;
        p.rngs.rewards = ctx.rngs.rewards; p.rngs.shops = ctx.rngs.shops;
        p.rngs.transformations = ctx.rngs.transformations;
        self.roster.next_card_id = self.roster.next_card_id.max(ctx.next_card_id);
        self.world.run.rngs.niche = ctx.rngs.niche;
        self.world.run.ref_cache = ctx.ref_cache;
        self.world.player_bags[seat] = bags.player.remove(0); self.world.shared_bags = bags.shared;
    }

    pub(super) fn enter_merchants(&mut self) -> Result<(), &'static str> {
        self.shops.clear();
        for seat in 0..self.roster.players().len() {
            let character = self.roster.player(seat).unwrap().character;
            let offset = self.roster.player(seat).unwrap().card_rarity_offset as f64;
            let mut ctx = self.merchant_ctx(seat)?;
            let mut bags = self.merchant_bags(seat);
            let mut state = shop::ShopState::default();
            state.character = character; state.multiplayer = true;
            let sale = ctx.rngs.shops.next_int(0, 4) as usize;
            let mut stocked = Vec::new();
            for (i, (ty, name)) in [(CardType::Attack,"Attack"), (CardType::Attack,"Attack"),
                (CardType::Skill,"Skill"), (CardType::Skill,"Skill"), (CardType::Power,"Power")].into_iter().enumerate() {
                let entry = card_entry(&mut ctx, character, offset, ty, name, &stocked, i == sale, None)?;
                stocked.push(entry.card.as_ref().unwrap().id); state.cards.push(entry);
            }
            for rarity in [CardRarity::Uncommon, CardRarity::Rare] {
                let entry = card_entry(&mut ctx, character, offset, CardType::Skill, "Colorless", &stocked, false, Some(rarity))?;
                stocked.push(entry.card.as_ref().unwrap().id); state.colorless_cards.push(entry);
            }
            let rarities = [shop::roll_relic_rarity(&mut ctx.rngs.rewards),
                shop::roll_relic_rarity(&mut ctx.rngs.rewards), RelicRarity::Shop];
            let mut excluded: Vec<_> = ctx.relics.iter().map(|r| r.id).collect();
            for rarity in rarities {
                let entry = relic_entry(&mut ctx, &mut bags, rarity, &excluded);
                excluded.push(entry.relic_id); state.relics.push(entry);
            }
            let mut pool = super::super::potion_pools::cards(character).ok_or("merchant potion pool")?;
            let mut selected = Vec::new();
            for _ in 0..3 {
                let rarity = shop::roll_potion_rarity(&mut ctx.rngs.shops);
                let candidates: Vec<_> = pool.iter().enumerate().filter(|(_, id)| POTION_INFO[id.index()].rarity == rarity).map(|(i, _)| i).collect();
                selected.push(pool.remove(candidates[ctx.rngs.shops.choice_index(candidates.len())]));
            }
            for id in selected {
                let price = potion_cost(&mut ctx, id);
                state.potions.push(shop::ShopPotionEntry { potion_id: id, rarity: POTION_INFO[id.index()].rarity, price });
                // MerchantPotionEntry.FillSlot calls SaveManager.MarkPotionAsSeen
                // for every displayed potion. Progress.MarkPotionAsSeen happens
                // before LocalContext.IsMine, so every seat's stock updates the
                // shared persistent seen set even when no Player.DiscoveredPotions
                // entry is appended.
                self.progress_discovered_potions.insert(id);
            }
            for entry in state.cards.iter_mut().chain(state.colorless_cards.iter_mut()) {
                entry.card.as_mut().unwrap().owner = seat as u8;
            }
            self.carry_merchant(seat, ctx, bags);
            self.shops.push(state);
        }
        Ok(())
    }

    pub(super) fn merchant_price(&self, seat: usize, raw: i32) -> i32 {
        // MerchantEntry.cs:16-24: decimal modifiers, ONE truncation at the end.
        // MembershipCard.cs:19-28 intentionally checks LocalContext.IsMe.
        let p = self.roster.player(seat).unwrap();
        let member = p.net_id == self.local_net_id && p.relics.iter().any(|r| r.enabled && r.id == RelicId::MembershipCard);
        let courier = p.relics.iter().any(|r| r.enabled && r.id == RelicId::TheCourier);
        (raw as i64 * if courier { 4 } else { 5 } / if member { 10 } else { 5 }) as i32
    }

    pub fn merchant_offers(&self, net_id: u64, seat: usize) -> Result<Vec<MerchantOffer>, &'static str> {
        let p = self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Shop { return Err("not in merchant room"); }
        if self.merchant_pickups[seat].is_some() {return Ok(Vec::new());}
        let shop = self.shops.get(seat).ok_or("merchant inventory missing")?;
        let mut offers = Vec::new();
        for (index, e) in shop.cards.iter().chain(shop.colorless_cards.iter()).enumerate() {
            if let Some(card) = &e.card { let price = self.merchant_price(seat, e.price);
                offers.push(MerchantOffer { action: MerchantAction::BuyCard(index), price, enabled: p.gold >= price,
                    card: Some(card.clone()), relic: None, potion: None }); }
        }
        for (index, e) in shop.relics.iter().enumerate().filter(|(_, e)| e.price < shop::SOLD_OUT) {
            let price = self.merchant_price(seat, e.price);
            offers.push(MerchantOffer { action: MerchantAction::BuyRelic(index), price, enabled: p.gold >= price,
                card: None, relic: Some(e.relic_id), potion: None });
        }
        for (index, e) in shop.potions.iter().enumerate().filter(|(_, e)| e.price < shop::SOLD_OUT) {
            let price = self.merchant_price(seat, e.price);
            offers.push(MerchantOffer { action: MerchantAction::BuyPotion(index), price, enabled: p.gold >= price
                && p.potions.iter().any(Option::is_none) && !p.relics.iter().any(|r| r.enabled && r.id == RelicId::Sozu),
                card: None, relic: None, potion: Some(e.potion_id) });
        }
        if !shop.removal_used {
            let raw = if self.world.run.ascension >= 6 { 100 + 50 * p.extra_fields.card_shop_removals_used }
                else { 75 + 25 * p.extra_fields.card_shop_removals_used };
            let price = self.merchant_price(seat, raw);
            for index in super::super::rest::removable_indices(&self.merchant_ctx(seat)?) {
                offers.push(MerchantOffer { action: MerchantAction::RemoveCard(Some(p.deck[index].instance_id)), price,
                    enabled: p.gold >= price, card: Some(p.deck[index].clone()), relic: None, potion: None });
            }
        }
        Ok(offers)
    }

    pub(super) fn purchase_merchant(&mut self, net_id: u64, seat: usize, action: MerchantAction) -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Shop { return Err("not in merchant room"); }
        if action == MerchantAction::RemoveCard(None) { return Ok(false); } // OneOffSynchronizer.cs:76-90 cancellation.
        let offer = self.merchant_offers(net_id, seat)?.into_iter().find(|o| o.action == action).ok_or("merchant entry not stocked")?;
        if !offer.enabled { return Err("merchant purchase unavailable"); }
        let mut ctx = self.merchant_ctx(seat)?;
        let mut bags = self.merchant_bags(seat);
        let courier = ctx.has_owned_relic(0, RelicId::TheCourier);
        match action {
            MerchantAction::BuyCard(index) => {
                let card = offer.card.unwrap();
                super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?;
                ctx = self.merchant_ctx(seat)?;
                ctx.gold -= offer.price;
                let state = &mut self.shops[seat];
                if courier {
                    let stocked: Vec<_> = state.cards.iter().chain(state.colorless_cards.iter()).filter_map(|e| e.card.as_ref().map(|c| c.id)).collect();
                    let old = state.cards.iter().chain(state.colorless_cards.iter()).nth(index).unwrap();
                    let ty = old.card.as_ref().unwrap().card_type;
                    let colorless = (index >= state.cards.len()).then_some(old.card.as_ref().unwrap().rarity);
                    let entry = card_entry(&mut ctx, state.character, self.roster.player(seat).unwrap().card_rarity_offset as f64,
                        ty, old.card_type, &stocked, false, colorless)?;
                    if index < state.cards.len() { state.cards[index] = entry; } else { state.colorless_cards[index - state.cards.len()] = entry; }
                } else {
                    let e = state.cards.iter_mut().chain(state.colorless_cards.iter_mut()).nth(index).unwrap();
                    e.card = None; e.price = shop::SOLD_OUT;
                }
            }
            MerchantAction::BuyRelic(index) => {
                let id = offer.relic.unwrap();
                if matches!(id,RelicId::Orrery|RelicId::Cauldron|RelicId::DollysMirror
                    |RelicId::RoyalStamp|RelicId::PunchDagger|RelicId::Kifuda) {
                    self.roster.player_mut(seat).unwrap().gold-=offer.price;
                    self.attach_merchant_pickup(seat,id);
                    self.begin_merchant_pickup(seat,id,index,offer.price)?;
                    return Ok(true);
                }
                self.roster.player_mut(seat).unwrap().gold -= offer.price;
                ctx.gold -= offer.price;
                if matches!(id, RelicId::Whetstone | RelicId::NeowsTalisman | RelicId::PotionBelt) {
                    self.roster.obtain_inventory_relic(&mut self.world.run, seat, id)?;
                    ctx = self.merchant_ctx(seat)?;
                } else { shop::add_relic(&mut ctx, id)?; }
                ctx.potions.resize(ctx.max_potion_slots as usize, None);
                self.record_relic_seen(seat, id);
                if courier {
                    let rarity = shop::roll_relic_rarity(&mut ctx.rngs.rewards);
                    let mut excluded: Vec<_> = ctx.relics.iter().map(|r| r.id).collect();
                    excluded.extend(self.shops[seat].relics.iter().filter(|e| e.price < shop::SOLD_OUT).map(|e| e.relic_id));
                    self.shops[seat].relics[index] = relic_entry(&mut ctx, &mut bags, rarity, &excluded);
                } else { self.shops[seat].relics[index].price = shop::SOLD_OUT; }
            }
            MerchantAction::BuyPotion(index) => {
                if !shop::procure_potion(&mut ctx, offer.potion.unwrap()) { return Ok(false); }
                ctx.gold -= offer.price;
                if courier {
                    let rarity = shop::roll_potion_rarity(&mut ctx.rngs.shops);
                    let pool: Vec<_> = super::super::potion_pools::cards(self.roster.player(seat).unwrap().character)
                        .ok_or("merchant potion pool")?.into_iter().filter(|id| POTION_INFO[id.index()].rarity == rarity).collect();
                    let id = pool[ctx.rngs.shops.choice_index(pool.len())];
                    let price = potion_cost(&mut ctx, id);
                    self.shops[seat].potions[index] = shop::ShopPotionEntry { potion_id: id, rarity, price };
                } else { self.shops[seat].potions[index].price = shop::SOLD_OUT; }
            }
            MerchantAction::RemoveCard(Some(id)) => {
                // BingBong, BookOfFiveRings and LuckyFysh require the NEW pile
                // to be Deck. CardPileCmd.Remove leaves no pile, so none fires.
                ctx.gold -= offer.price;
                ctx.retain_run_deck_cards(|card| card.instance_id != id);
                self.shops[seat].removal_used = true;
                self.roster.player_mut(seat).unwrap().extra_fields.card_shop_removals_used += 1;
            }
            MerchantAction::RemoveCard(None) => unreachable!(),
        }
        // MerchantEntry.cs:75-84: refill THEN AfterItemPurchased/MawBank.
        if offer.price > 0 {
            for r in &mut ctx.relics { if r.id == RelicId::MawBank { r.flag1 = true; } }
        }
        let state = &mut self.shops[seat];
        for entry in state.cards.iter_mut().chain(state.colorless_cards.iter_mut()) {
            if let Some(c) = &mut entry.card { c.owner = seat as u8; }
        }
        self.carry_merchant(seat, ctx, bags);
        Ok(true)
    }

    pub(super) fn finish_merchant_relic_purchase(&mut self,seat:usize,index:usize,price:i32)
        ->Result<(),&'static str> {
        let mut ctx=self.merchant_ctx(seat)?;
        let mut bags=self.merchant_bags(seat);
        if ctx.has_owned_relic(0,RelicId::TheCourier) {
            let rarity=shop::roll_relic_rarity(&mut ctx.rngs.rewards);
            let mut excluded:Vec<_>=ctx.relics.iter().map(|r|r.id).collect();
            excluded.extend(self.shops[seat].relics.iter().filter(|e|e.price<shop::SOLD_OUT).map(|e|e.relic_id));
            self.shops[seat].relics[index]=relic_entry(&mut ctx,&mut bags,rarity,&excluded);
        } else {self.shops[seat].relics[index].price=shop::SOLD_OUT;}
        if price>0 {for r in &mut ctx.relics {if r.id==RelicId::MawBank {r.flag1=true;}}}
        self.carry_merchant(seat,ctx,bags);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{relic::Relic, run::mp::{Character, PlayerSpec}};

    fn run(seats: usize) -> MpRun {
        let specs: Vec<_> = (0..seats).map(|s| PlayerSpec { net_id: 100 + s as u64, character: Character::Ironclad }).collect();
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        for seat in 0..seats { run.roster.player_mut(seat).unwrap().gold = 2000; }
        run.enter_merchants().unwrap();
        run.phase = MpPhase::Shop;
        run
    }

    #[test]
    fn merchant_pickups_retain_owner_choices_and_finish_before_restock() {
        use super::super::MerchantPickupChoice;
        for id in [RelicId::Orrery,RelicId::Cauldron,RelicId::DollysMirror,
            RelicId::Kifuda,RelicId::RoyalStamp,RelicId::PunchDagger] {
            let mut run=run(3);
            run.shops[1].relics[0].relic_id=id;
            run.shops[1].relics[0].price=100;
            let before=run.roster.player(1).unwrap().deck.len();
            let other=format!("{:?}",run.roster.player(0).unwrap());
            run.merchant_purchase(101,1,MerchantAction::BuyRelic(0)).unwrap();
            assert!(run.merchant_offers(101,1).unwrap().is_empty());
            assert!(run.native_shop_offers(1).unwrap().all(|offer| !offer.enabled));
            let before_queries = format!("{run:?}");
            for selected in [&[][..], &[0][..], &[0, 0][..], &[0, 1, 2][..], &[usize::MAX][..]] {
                let accepted = run.native_can_select_merchant_pickup(101, 1, selected);
                assert_eq!(accepted, run.clone().select_merchant_pickup(101, 1, selected).is_ok(), "{id:?}/{selected:?}");
            }
            for seat in 0..3 {
                assert_eq!(run.native_can_leave_shop(100 + seat as u64, seat),
                    run.clone().leave_shop(100 + seat as u64, seat).is_ok());
            }
            assert_eq!(format!("{run:?}"), before_queries);
            assert!(run.merchant_pickup(100,1).is_err());
            assert!(run.leave_shop(100,0).is_err());
            assert_eq!(run.shops[1].relics[0].price,100);
            for _ in 0..5 {
                let Some(choice)=run.merchant_pickup(101,1).unwrap().cloned() else {break;};
                let selected=match choice {
                    MerchantPickupChoice::Deck{min,..}=>if min==0 {vec![]}else{vec![0]},
                    _=>vec![],
                };
                run.select_merchant_pickup(101,1,&selected).unwrap();
            }
            assert!(run.merchant_pickup(101,1).unwrap().is_none());
            assert_eq!(run.shops[1].relics[0].price,shop::SOLD_OUT);
            assert_eq!(run.roster.player(1).unwrap().deck.len(),
                before+usize::from(id==RelicId::DollysMirror));
            assert_eq!(format!("{:?}",run.roster.player(0).unwrap()),other);
        }
    }

    #[test]
    fn royal_stamp_filters_powers_before_shuffling_the_csharp_candidate_list() {
        use super::super::MerchantPickupChoice;
        let mut run=run(3);
        let id=super::super::super::card_pools::cards(crate::character::CardPool::Ironclad,true).unwrap().into_iter()
            .find(|&id|crate::tables::card_info(id,false).is_some_and(|c|c.card_type==CardType::Power)).unwrap();
        let mut power=Card::from_factory(id,false,900).unwrap();power.owner=1;
        run.roster.player_mut(1).unwrap().deck.push(power);
        let mut expected:Vec<_>=run.roster.player(1).unwrap().deck.iter()
            .filter(|c|matches!(c.card_type,CardType::Attack|CardType::Skill)
                &&!c.is_unplayable()&&c.enchantments.is_empty()).map(|c|c.instance_id).collect();
        let mut niche=run.world.run.rngs.niche.clone();niche.shuffle(&mut expected);
        run.shops[1].relics[0].relic_id=RelicId::RoyalStamp;run.shops[1].relics[0].price=100;
        run.merchant_purchase(101,1,MerchantAction::BuyRelic(0)).unwrap();
        let Some(MerchantPickupChoice::Deck{cards,..})=run.merchant_pickup(101,1).unwrap() else{panic!("Royal Stamp selector");};
        assert_eq!(cards.iter().map(|c|c.instance_id).collect::<Vec<_>>(),expected);
        assert!(cards.iter().all(|c|c.card_type!=CardType::Power));
        assert_eq!(run.world.run.rngs.niche.counter,niche.counter);
    }

    #[test]
    fn orrery_populates_fresnel_lens_options_before_offering_them() {
        use super::super::MerchantPickupChoice;
        let mut block_options=0;
        for seed in 1..=12 {
            let mut run=run(3);
            run.roster.player_mut(1).unwrap().rngs.rewards=Rng::new(seed);
            let mut lens=Relic::new(RelicId::FresnelLens);lens.owner=1;
            run.roster.player_mut(1).unwrap().relics.push(lens);
            run.shops[1].relics[0].relic_id=RelicId::Orrery;run.shops[1].relics[0].price=100;
            run.merchant_purchase(101,1,MerchantAction::BuyRelic(0)).unwrap();
            let Some(MerchantPickupChoice::Cards(bundles))=run.merchant_pickup(101,1).unwrap() else{panic!("Orrery rewards");};
            assert_eq!(bundles.len(),5);
            for card in bundles.iter().flatten() {
                if card_info(card.id,card.upgraded).unwrap().gains_block && !card.is_unplayable() {
                    block_options+=1;
                    assert_eq!(card.enchantments.as_slice(),&[(crate::identity::EnchantmentId::Nimble,2)]);
                } else {assert!(card.enchantments.is_empty());}
                assert_eq!(card.owner,1);
            }
        }
        assert!(block_options>0);
    }

    #[test]
    fn removal_does_not_fire_deck_entry_relics() {
        let mut run=run(3);
        for id in [RelicId::BingBong,RelicId::BookOfFiveRings,RelicId::LuckyFysh] {
            let mut r=Relic::new(id);r.owner=1;r.counter=4;
            run.roster.player_mut(1).unwrap().relics.push(r);
        }
        let offer=run.merchant_offers(101,1).unwrap().into_iter()
            .find(|o|matches!(o.action,MerchantAction::RemoveCard(Some(_)))).unwrap();
        let p=run.roster.player(1).unwrap();
        let (gold,hp,len)=(p.gold,p.hp,p.deck.len());
        run.merchant_purchase(101,1,offer.action).unwrap();
        let p=run.roster.player(1).unwrap();
        assert_eq!((p.gold,p.hp,p.deck.len()),(gold-offer.price,hp,len-1));
        assert_eq!(p.relics.iter().find(|r|r.id==RelicId::BookOfFiveRings).unwrap().counter,4);
    }

    #[test]
    fn purchases_and_removal_are_atomic_and_owned_for_every_seat() {
        for seats in 2..=4 { for owner in 0..seats {
            let mut run = run(seats);
            let initial: Vec<_> = run.roster.players().iter().map(|p| format!("{p:?}")).collect();
            let card_offer = run.merchant_offers(100 + owner as u64, owner).unwrap().into_iter()
                .find(|o| matches!(o.action, MerchantAction::BuyCard(0))).unwrap();
            let before = format!("{run:?}");
            assert!(run.merchant_purchase(999, owner, card_offer.action).is_err());
            assert!(!run.merchant_purchase(100 + owner as u64, owner, MerchantAction::RemoveCard(None)).unwrap());
            assert_eq!(format!("{run:?}"), before);
            let count = run.roster.player(owner).unwrap().deck.len();
            run.merchant_purchase(100 + owner as u64, owner, card_offer.action).unwrap();
            let p = run.roster.player(owner).unwrap();
            assert_eq!(p.gold, 2000 - card_offer.price);
            assert_eq!(p.deck.len(), count + 1);
            assert_eq!(p.deck.last().unwrap().instance_id, card_offer.card.unwrap().instance_id);
            assert_eq!(p.deck.last().unwrap().owner as usize, owner);
            for seat in 0..seats { if seat != owner { assert_eq!(format!("{:?}", run.roster.player(seat).unwrap()), initial[seat]); } }
            let rng = run.roster.player(owner).unwrap().rngs.clone();
            let potion = run.merchant_offers(100 + owner as u64, owner).unwrap().into_iter().find(|o| matches!(o.action, MerchantAction::BuyPotion(0))).unwrap();
            run.merchant_purchase(100 + owner as u64, owner, potion.action).unwrap();
            assert_eq!(run.roster.player(owner).unwrap().potions[0], potion.potion);
            assert_eq!(run.roster.player(owner).unwrap().rngs, rng, "no Courier means no refill draws");
            let removal = run.merchant_offers(100 + owner as u64, owner).unwrap().into_iter().find(|o| matches!(o.action, MerchantAction::RemoveCard(_))).unwrap();
            run.merchant_purchase(100 + owner as u64, owner, removal.action).unwrap();
            let p = run.roster.player(owner).unwrap();
            assert_eq!(p.extra_fields.card_shop_removals_used, 1);
            assert_eq!(p.deck.len(), count);
            assert!(run.merchant_offers(100 + owner as u64, owner).unwrap().iter().all(|o| !matches!(o.action, MerchantAction::RemoveCard(_))));
        }}
    }

    #[test]
    fn courier_refills_only_owner_and_discount_is_truncated_once() {
        for owner in 0..4 {
            let mut run = run(4);
            let mut courier = Relic::new(RelicId::TheCourier); courier.owner = owner as u8;
            let mut member = Relic::new(RelicId::MembershipCard); member.owner = owner as u8;
            run.roster.player_mut(owner).unwrap().relics.extend([courier, member]);
            assert_eq!(run.merchant_price(owner, 77), if owner == 0 { 30 } else { 61 });
            let rngs: Vec<_> = run.roster.players().iter().map(|p| p.rngs.clone()).collect();
            run.merchant_purchase(100 + owner as u64, owner, MerchantAction::BuyCard(0)).unwrap();
            for seat in 0..4 {
                let p = run.roster.player(seat).unwrap();
                if seat == owner { assert!(p.rngs.shops.counter > rngs[seat].shops.counter); }
                else { assert_eq!(p.rngs, rngs[seat]); }
            }
            assert!(run.shops[owner].cards[0].card.is_some());
            assert!(!run.shops[owner].cards[0].on_sale);
        }
    }
}
