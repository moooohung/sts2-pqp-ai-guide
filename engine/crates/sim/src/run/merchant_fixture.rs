//! Stages a mid-run merchant visit from facts read out of the original game, so a test can compare
//! the native encoder's observation with the advisor's live projection of the very same state.
//! Nothing here is part of a run: it only overwrites the fields a visit exposes to the observation.
use super::*;
use super::shop::{SOLD_OUT, ShopCardEntry, ShopPotionEntry, ShopRelicEntry, ShopState};
use crate::card::Card;
use crate::relic::Relic;

/// A card slot; `id: None` is a slot whose card was bought (its sale flag survives the purchase).
#[derive(Clone, Debug)]
pub struct MerchantFixtureCard { pub id: Option<CardId>, pub upgraded: bool, pub price: i32, pub on_sale: bool }
/// A relic or potion slot. A bought slot keeps the identity it held, as the simulator's own does.
#[derive(Clone, Copy, Debug)]
pub struct MerchantFixtureOffer<T> { pub id: T, pub price: i32, pub sold: bool }
#[derive(Clone, Debug)]
pub struct MerchantFixture {
    /// The act the shop is in (0-based). Its map, and so the second boss, depends on it: only the last act of an
    /// Ascension 10+ run has one.
    pub act_index: usize,
    /// Map coordinates `(col, row)` of every room entered this act, the shop last.
    pub visited: Vec<(i32, i32)>,
    pub total_floor: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub max_potion_slots: i32,
    pub potions: Vec<Option<PotionId>>,
    pub relics: Vec<RelicId>,
    pub deck: Vec<(CardId, bool)>,
    pub cards: Vec<MerchantFixtureCard>,
    pub colorless: Vec<MerchantFixtureCard>,
    pub relic_offers: Vec<MerchantFixtureOffer<RelicId>>,
    pub potion_offers: Vec<MerchantFixtureOffer<PotionId>>,
    pub removal_cost: i32,
    pub removal_used: bool,
    /// The act's boss encounters as the game holds them. A run rebuilt from the seed draws its own, which the
    /// game's save need not agree with, so the staged visit takes the game's.
    pub boss: crate::identity::EncounterId,
    pub second_boss: Option<crate::identity::EncounterId>,
}

impl RunManager {
    pub fn stage_merchant_fixture(&mut self, fixture: &MerchantFixture) -> Result<(), String> {
        // The act comes first: the visited rooms are coordinates of its map and the bosses are its own. A run is
        // rebuilt in act 0, so a later act is entered the way play enters it (its map drawn from its own stream).
        // The deck goes in before that, because a held Spoils Map changes the map the act generates.
        let character = self.character;
        let ctx = self.run_ctx_mut();
        ctx.deck.clear();
        for &(id, upgraded) in &fixture.deck {
            let instance = ctx.new_card_id();
            ctx.deck.push(Card::from_factory(id, upgraded, instance).ok_or("deck card has no factory")?);
        }
        if fixture.act_index != self.current_act {
            self.set_act_internal(fixture.act_index).map_err(|e| format!("act {}: {e}", fixture.act_index))?;
        }
        let map = self.visible_map().ok_or("run has no map")?;
        let indices = fixture.visited.iter().map(|&(col, row)| map.points.iter()
            .position(|point| point.coord == Coord::new(col, row)).and_then(|index| u8::try_from(index).ok())
            .ok_or_else(|| format!("map has no point ({col}, {row})"))).collect::<Result<Vec<_>, _>>()?;
        let has_second_boss = map.second_boss.is_some();
        let &(col, row) = fixture.visited.last().ok_or("no visited room")?;
        if has_second_boss != fixture.second_boss.is_some() {
            return Err("the staged map and the game's act disagree on whether there is a second boss".into());
        }
        let rooms = &mut self.acts[self.current_act].rooms;
        rooms.boss = fixture.boss;
        rooms.public_boss = crate::identity::PublicBossId::from_name(fixture.boss.name()).ok_or("the game's boss has no public identity")?;
        rooms.second_boss = fixture.second_boss;
        self.visited_points = indices.into_iter().collect();
        let ctx = self.run_ctx_mut();
        ctx.coord = Some((col, row));
        ctx.room_type = RoomType::Shop;
        ctx.total_floor = fixture.total_floor;
        ctx.hp = fixture.hp;
        ctx.max_hp = fixture.max_hp;
        ctx.gold = fixture.gold;
        ctx.max_potion_slots = fixture.max_potion_slots;
        ctx.potions = fixture.potions.clone();
        ctx.relics = fixture.relics.iter().map(|&id| Relic::new(id)).collect();
        let mut card_entry = |card: &MerchantFixtureCard, card_type: &'static str| -> Result<ShopCardEntry, String> {
            let Some(id) = card.id else {
                return Ok(ShopCardEntry { card: None, card_type, on_sale: card.on_sale, price: SOLD_OUT });
            };
            let instance = ctx.new_card_id();
            Ok(ShopCardEntry { card: Some(Card::from_factory(id, card.upgraded, instance).ok_or("shop card has no factory")?),
                card_type, on_sale: card.on_sale, price: card.price })
        };
        let mut shop = ShopState { character, ..ShopState::default() };
        // The slot order is the buy index: `cards` then `colorless`, sold slots included.
        for card in &fixture.cards { shop.cards.try_push(card_entry(card, "Class")?).map_err(|_| "too many class cards")?; }
        for card in &fixture.colorless { shop.colorless_cards.try_push(card_entry(card, "Colorless")?).map_err(|_| "too many colorless cards")?; }
        for offer in &fixture.relic_offers {
            shop.relics.try_push(ShopRelicEntry { relic_id: offer.id, rarity: RELIC_INFO[offer.id.index()].rarity,
                price: if offer.sold { SOLD_OUT } else { offer.price } }).map_err(|_| "too many relics")?;
        }
        for offer in &fixture.potion_offers {
            shop.potions.try_push(ShopPotionEntry { potion_id: offer.id, rarity: POTION_INFO[offer.id.index()].rarity,
                price: if offer.sold { SOLD_OUT } else { offer.price } }).map_err(|_| "too many potions")?;
        }
        shop.removal_cost = fixture.removal_cost;
        shop.removal_used = fixture.removal_used;
        self.shop = shop;
        self.run_choice = None;
        self.unported = None;
        self.phase = RunPhase::Shop;
        Ok(())
    }
}
