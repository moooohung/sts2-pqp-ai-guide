//! Borrowed public state for the native encoder; no independently cloned solo managers.
use super::*;
use crate::{card::Card, identity::{ChoicePurpose, EventId}, run::{NativeShopKind, NativeShopOffer},
    tables::{KW_ETERNAL, RelicId}};

/// Original owner-local selectors, identified by their native driver entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeModalKind { Cheese, PrecariousShears, ScrollBox, Pomander, PreciseScissors,
    NewLeaf, PaelsGrowth, Smith, Mend, NeowCardReward, RestCardReward, MerchantRemoval }

#[derive(Clone, Copy)]
pub struct NativeMpEventChoice<'a> {
    pub cards: &'a [Card], pub min: usize, pub max: usize, pub skippable: bool,
    pub purpose: ChoicePurpose,
}
impl MpRun {
    pub fn native_neow_option_id(&self, seat: usize, index: usize) -> Option<&'static str> {
        self.neow.get(seat)?.get(index)?;
        Some(super::super::EventOptionIdentity::neow(index as u16).legacy_id())
    }
    pub fn native_public_boss(&self) -> crate::identity::PublicBossId { self.world.encounters.public_boss }
    pub fn native_public_bosses(&self) -> [Option<crate::run::NativeBoss>; 2] {
        let rooms = &self.world.encounters; let map = &self.world.map;
        let mut out = [None; 2]; let mut next = true;
        for (order, (encounter, index)) in [(Some(rooms.boss), Some(map.boss)),
            (rooms.second_boss, map.second_boss)].into_iter().enumerate() {
            if let (Some(encounter), Some(index)) = (encounter, index) {
                let coord = map.points[usize::from(index)].coord;
                let visited = self.world.visited.iter().any(|&(act, point)|
                    act == self.world.run.act_index && point == coord);
                let complete = visited && self.roster.players().iter().any(|p| p.hp > 0)
                    && (self.world.run.coord != Some((coord.col, coord.row)) || self.phase != MpPhase::Combat);
                out[order] = Some(crate::run::NativeBoss {
                    id: crate::identity::PublicBossId::from_name(encounter.name()).expect("public map boss"),
                    map_index: usize::from(index), complete, current: next && !complete });
                next &= complete;
            }
        }
        out
    }
    pub fn native_rest_heal_amount(&self, seat: usize) -> Option<i32> {
        let player = self.roster.player(seat)?;
        Some(self.native_rest_base_heal_amount(seat)?
            .min((player.max_hp - player.hp).max(0)))
    }
    pub fn native_rest_base_heal_amount(&self, seat: usize) -> Option<i32> {
        let player = self.roster.player(seat)?;
        Some(super::super::rest::heal_amount(player.max_hp,
            player.relics.iter().any(|r| r.enabled && r.id == RelicId::RegalPillow)))
    }
    pub fn native_event_id(&self, seat: usize) -> Option<EventId> {
        self.events.get(seat).and_then(Option::as_ref).map(|event| event.id).or(self.native_event)
    }
    pub fn native_event_finished(&self, seat: usize) -> bool { self.event_done.get(seat).copied().unwrap_or(true) }
    pub fn native_sphere_reward(&self, seat: usize, offer: usize) -> Option<crate::run::NativeRewardContent<'_>> {
        use super::crystal_sphere::SphereLoot;
        use crate::run::{EventParameter, NativeRewardContent as R};
        if !matches!(self.events.get(seat)?.as_ref()?.options.page.parameter, EventParameter::SphereReward { .. }) {
            return None;
        }
        Some(match self.sphere_loot.get(seat)?.get(offer)?.as_ref()? {
            SphereLoot::Potion(p) => R::Potion(*p),
            SphereLoot::Relic(r) => R::Relic(*r),
            SphereLoot::Cards { cards, reroll, .. } => R::Cards {
                cards, closed: false, reroll: *reroll, cards_picked: 0, rerolls_used: 0 },
        })
    }
    pub fn native_event_parameters(&self, seat: usize, option: super::super::EventOptionIdentity,
        description: Option<super::super::EventDescription>) -> Result<Vec<super::super::event_preview::PublicEventParameter>, &'static str> {
        use super::super::event_preview::*;
        let player = self.roster.player(seat).ok_or("event owner missing")?;
        public_event_parameters(option, PublicEventContext { hp: player.hp, max_hp: player.max_hp,
            gold: player.gold, deck: &player.deck, players: self.roster.players().len(), description,
            dynamic: event_dynamic(self.events.get(seat).and_then(Option::as_ref))? })
    }
    pub fn native_event_offer(&self, seat: usize, option: super::super::EventOptionIdentity)
        -> Option<super::super::NativeEventOffer> {
        self.events.get(seat)?.as_ref()?.native_offer(option, &self.roster.player(seat)?.potions)
    }
    pub fn native_event_offer_detail(&self, seat: usize, option: super::super::EventOptionIdentity)
        -> (Option<crate::tables::CardId>, Option<crate::tables::CardType>) {
        self.events.get(seat).and_then(Option::as_ref).zip(self.roster.player(seat))
            .map_or((None, None), |(event, player)| event.native_offer_detail(option, &player.potions))
    }
    pub fn native_bridge_offer(&self, seat:usize) -> Option<(i32, &Card)> {
        let event=self.events.get(seat)?.as_ref()?;
        if event.id!=EventId::SlipperyBridge {return None;}
        let card=self.roster.player(seat)?.deck.iter().find(|card|Some(card.instance_id)==event.bridge.current)?;
        Some((3+event.bridge.hold_ons,card))
    }
    pub fn native_wongo_featured(&self, seat:usize) -> Option<RelicId> {
        self.events.get(seat)?.as_ref()?.public_wongo_featured()
    }
    pub fn native_event_card_choice(&self, seat: usize) -> Option<NativeMpEventChoice<'_>> {
        let choice = self.event_card_modals.get(seat)?.as_ref()?;
        Some(NativeMpEventChoice { cards: &choice.cards, min: choice.min, max: choice.max,
            skippable: choice.skippable, purpose: choice.action.native_purpose() })
    }
    pub fn native_event_card_reward(&self,seat:usize)->Option<NativeMpEventChoice<'_>> {
        let reward=self.event_card_reward_queues.get(seat)?.first()?;
        Some(NativeMpEventChoice{cards:&reward.cards,min:0,max:1,skippable:true,
            purpose:super::events::EventCardAction::AddGenerated.native_purpose()})
    }
    pub fn native_event_potions(&self, seat: usize) -> Option<&[Option<crate::tables::PotionId>; 3]> {
        self.event_potion_rewards.get(seat).and_then(Option::as_ref)
    }
    pub fn native_relic_skippable(&self, seat: usize) -> bool {
        self.relic_reward_skippable.get(seat).copied().unwrap_or(false)
    }
    pub fn native_neow_card_source(&self, seat: usize) -> Option<RelicId> {
        self.neow_card_sources.get(seat).copied().flatten()
    }
    pub fn native_map_blocked(&self) -> bool {
        (0..self.roster.players().len()).any(|seat| self.owner_pickup_pending(seat))
    }
    pub fn native_treasure_opened(&self, seat: usize) -> bool {
        self.treasure_opened.get(seat).copied().unwrap_or(false)
    }
    pub fn native_treasure_votes(&self) -> &[Option<Option<usize>>] { &self.treasure_votes }
    /// Read-only counterpart of leave_shop's authenticated, shared-room barrier.
    pub fn native_can_leave_shop(&self, net_id: u64, seat: usize) -> bool {
        self.roster.actor(net_id, seat).is_ok() && self.phase == MpPhase::Shop
            && self.merchant_pickups.iter().all(Option::is_none)
    }
    /// The retained selector's accepted index sets, without executing its
    /// acquisition hooks, consuming RNG, or cloning/mutating the run.
    pub fn native_can_select_merchant_pickup(&self, net_id: u64, seat: usize, indices: &[usize]) -> bool {
        let Ok(Some(choice)) = self.merchant_pickup(net_id, seat) else { return false; };
        match choice {
            MerchantPickupChoice::Deck { cards, min, max, .. } =>
                indices.len() >= (*min).min(cards.len()) && indices.len() <= *max
                    && indices.iter().enumerate().all(|(at, &index)|
                        index < cards.len() && !indices[..at].contains(&index)),
            MerchantPickupChoice::Cards(bundles) => bundles.first().is_some_and(|cards|
                indices.is_empty() || indices.len() == 1 && indices[0] < cards.len()),
            MerchantPickupChoice::Potions(potions) => !potions.is_empty() && (indices.is_empty()
                || indices == [0] && self.roster.player(seat).is_some_and(|player|
                    player.potions.iter().any(Option::is_none)
                        || player.relics.iter().any(|r| r.enabled && r.id == RelicId::Sozu))),
        }
    }
    pub fn native_shop_offers(&self, seat: usize) -> Result<impl Iterator<Item = NativeShopOffer<'_>>, &'static str> {
        let player = self.roster.player(seat).ok_or("invalid merchant owner")?;
        let state = self.shops.get(seat).ok_or("merchant inventory missing")?;
        // Same owner-local modal barrier as merchant_offers; stock stays
        // visible while its acquisition selector is pending, but not legal.
        let available = self.merchant_pickups[seat].is_none();
        let cards = state.cards.iter().chain(state.colorless_cards.iter()).enumerate().map(move |(index, entry)| {
            let sold = entry.card.is_none(); let price = self.merchant_price(seat, entry.price);
            NativeShopOffer { kind: NativeShopKind::Card, index, card: entry.card.as_ref(), relic: None, potion: None,
                price, on_sale: entry.on_sale, sold, enabled: available && !sold && player.gold >= price }
        });
        let relics = state.relics.iter().enumerate().map(move |(index, entry)| {
            let sold = entry.price >= super::super::shop::SOLD_OUT; let price = self.merchant_price(seat, entry.price);
            NativeShopOffer { kind: NativeShopKind::Relic, index, card: None, relic: Some(entry.relic_id), potion: None,
                price, on_sale: false, sold, enabled: available && !sold && player.gold >= price }
        });
        let potions = state.potions.iter().enumerate().map(move |(index, entry)| {
            let sold = entry.price >= super::super::shop::SOLD_OUT; let price = self.merchant_price(seat, entry.price);
            NativeShopOffer { kind: NativeShopKind::Potion, index, card: None, relic: None, potion: Some(entry.potion_id),
                price, on_sale: false, sold, enabled: available && !sold && player.gold >= price && player.potions.iter().any(Option::is_none)
                    && !player.relics.iter().any(|r| r.enabled && r.id == RelicId::Sozu) }
        });
        let raw = if self.world.run.ascension >= 6 { 100 + 50 * player.extra_fields.card_shop_removals_used }
            else { 75 + 25 * player.extra_fields.card_shop_removals_used };
        let price = self.merchant_price(seat, raw);
        // C# removal menu is curses first, preserving deck order within each group.
        let removals = [true, false].into_iter().flat_map(move |curse| player.deck.iter().enumerate()
            .filter(move |(_, card)| (card.card_type == crate::tables::CardType::Curse) == curse && !card.has_kw(KW_ETERNAL))
            .map(move |(index, card)| NativeShopOffer { kind: NativeShopKind::Removal, index, card: Some(card),
                relic: None, potion: None, price, on_sale: false, sold: state.removal_used,
                enabled: available && !state.removal_used && player.gold >= price }));
        Ok(cards.chain(relics).chain(potions).chain(removals))
    }
}
