//! Borrowed native run-menu interfaces. No clones, RNG, localization or advancement.
//! Backing indices and display indices are distinct and remain outside learned rows.
use super::*;
use crate::identity::{ChoicePurpose, EffectId, RewardKind};

#[derive(Clone, Copy, Debug)]
pub enum NativeRewardContent<'a> {
    Potion(PotionId),
    Relic(RelicId),
    SpecialCard { owner: u8, card: &'a Card },
    RemoveCard { owner: u8 },
    Cards { cards: &'a [Card], closed: bool, reroll: bool, cards_picked: u32, rerolls_used: u32 },
    Bundles(&'a [ArrayVec<Card, 3>]),
}
#[derive(Clone, Copy, Debug)]
pub struct NativeReward<'a> {
    pub backing_index: usize,
    pub kind: RewardKind,
    pub content: NativeRewardContent<'a>,
    pub active: bool,
    pub consumed: bool,
    pub skippable: bool,
    pub skip_completes: bool,
}

#[derive(Clone, Copy)]
pub struct NativeRunChoice<'a> { run: &'a RunManager }
#[derive(Clone, Copy, Debug)]
pub struct NativeChoiceCard<'a> {
    pub card: &'a Card,
    pub selected_order: Option<usize>,
    pub offered: bool,
    pub enabled: bool,
}
impl<'a> NativeRunChoice<'a> {
    pub fn purpose(self) -> ChoicePurpose {
        if let Some(c) = &self.run.pending_run_choice { c.effect.native_purpose() }
        else { self.run.run_choice.as_ref().unwrap().kind.native_purpose() }
    }
    pub fn effect(self) -> Option<EffectId> {
        self.run.pending_run_choice.as_ref().map(|c| c.effect.native_effect_id())
    }
    pub fn enchantment(self) -> Option<(crate::identity::EnchantmentId, i32)> {
        match self.run.pending_run_choice.as_ref().map(|c| c.effect) {
            Some(RunChoiceEffect::Enchant { name, amount }) => Some((name, amount)), _ => None,
        }
    }
    pub fn bounds(self) -> (usize, usize, bool, bool) {
        if let Some(c) = &self.run.pending_run_choice {
            (c.min_choices, c.max_choices, c.allow_skip, c.single_select)
        } else {
            let cook = matches!(self.run.run_choice.as_ref().unwrap().kind, RunDeckChoiceKind::Cook { .. });
            if cook { (2, 2, false, false) } else { (1, 1, false, true) }
        }
    }
    pub fn len(self) -> usize {
        self.run.pending_run_choice.as_ref().map_or_else(
            || self.run.run_choice.as_ref().unwrap().candidates.len(), |c| c.options.len())
    }
    pub fn is_empty(self) -> bool { self.len() == 0 }
    pub fn selected_count(self) -> usize {
        self.run.pending_run_choice.as_ref().map_or_else(
            || self.run.run_choice.as_ref().unwrap().selected.len(), |c| c.selected.len())
    }
    pub fn can_confirm(self) -> bool {
        if let Some(c) = &self.run.pending_run_choice {
            if c.is_multi() { c.can_confirm() } else { c.allow_skip }
        } else {
            let c = self.run.run_choice.as_ref().unwrap();
            matches!(c.kind, RunDeckChoiceKind::Cook { .. }) && c.selected.len() == 2
        }
    }
    pub fn picks_generated_card(self) -> bool {
        self.run.pending_run_choice.as_ref().is_some_and(|c| c.single_select && !c.reward_cards.is_empty())
    }
    pub fn card(self, index: usize) -> Option<NativeChoiceCard<'a>> {
        if let Some(c) = &self.run.pending_run_choice {
            let option = c.options.get(index)?;
            let offered = !c.reward_cards.is_empty();
            let card = c.reward_cards.iter().chain(self.run.run_ctx().deck.iter())
                .find(|card| card.instance_id == option.instance_id && card.id == option.card_id)?;
            Some(NativeChoiceCard { card, selected_order: c.selected.iter().position(|&i| i == index),
                offered, enabled: c.can_toggle(index) })
        } else {
            let c = self.run.run_choice.as_ref()?;
            let deck_index = *c.candidates.get(index)?;
            let selected_order = c.selected.iter().position(|&i| i == deck_index);
            Some(NativeChoiceCard { card: self.run.run_ctx().deck.get(deck_index)?, selected_order,
                offered: false, enabled: selected_order.is_some()
                    || !matches!(c.kind, RunDeckChoiceKind::Cook { .. }) || c.selected.len() < 2 })
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeShopKind { Card, Relic, Potion, Removal }
#[derive(Clone, Copy, Debug)]
pub struct NativeShopOffer<'a> {
    pub kind: NativeShopKind,
    pub index: usize,
    pub card: Option<&'a Card>,
    pub relic: Option<RelicId>,
    pub potion: Option<PotionId>,
    pub price: i32,
    pub on_sale: bool,
    pub sold: bool,
    pub enabled: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct NativeRestOffer {
    pub index: usize,
    pub kind: super::super::rest::RestOptionKind,
    pub enabled: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct NativeBoss {
    pub id: crate::identity::PublicBossId,
    pub map_index: usize,
    pub complete: bool,
    pub current: bool,
}

impl crate::combat::RunCtx {
    /// Run the real in-place deck upgrade on an isolated value. This preserves
    /// instance enchantments, cost modifiers and self-mutating card progress.
    pub fn native_upgrade_preview(&self, card: &Card) -> Option<Card> {
        if card.upgraded { return None; }
        let mut preview = self.clone();
        preview.deck.clear();
        preview.deck.push(card.clone());
        preview.upgrade_deck_card(0);
        preview.deck.pop().filter(|c| c.upgraded)
    }
}

impl RunManager {
    pub fn native_character(&self) -> crate::run::mp::Character { self.native_character }

    /// New-run Neow is implemented; its old display marker is not a refusal.
    /// No names or legacy screen keys participate in the native decision path.
    pub fn native_fault(&self) -> Option<&'static str> {
        if !self.native_decision_started && self.options.all_cards_unlocked { None }
        else { self.unported }
    }

    pub fn native_choice(&self) -> Option<NativeRunChoice<'_>> {
        (self.phase != RunPhase::Combat && (self.pending_run_choice.is_some() || self.run_choice.is_some()))
            .then_some(NativeRunChoice { run: self })
    }
    pub fn native_reward_count(&self) -> usize { self.reward.items.len() }
    pub fn native_reward(&self, backing_index: usize) -> Option<NativeReward<'_>> {
        let item = self.reward.items.get(backing_index)?;
        let content = match &item.kind {
            RewardItemKind::Potion(id) => NativeRewardContent::Potion(*id),
            RewardItemKind::Relic(id) => NativeRewardContent::Relic(*id),
            RewardItemKind::SpecialCard { owner, card } => NativeRewardContent::SpecialCard { owner: *owner, card },
            RewardItemKind::CardRemoval { owner } => NativeRewardContent::RemoveCard { owner: *owner },
            RewardItemKind::Card { cards, closed, reroll, progress } => NativeRewardContent::Cards {
                cards, closed: *closed, reroll: reroll.is_some(), cards_picked: progress.cards_picked,
                rerolls_used: progress.rerolls_used },
            RewardItemKind::CardBundles { bundles } => NativeRewardContent::Bundles(bundles),
        };
        Some(NativeReward { backing_index, kind: item.kind.native_kind(), content,
            active: self.reward.active == Some(backing_index), consumed: item.consumed,
            skippable: item.skippable, skip_completes: item.skip_completes })
    }
    /// RewardsSet display order; stable among equal kinds. No temporary sorted vector.
    pub fn native_reward_indices(&self) -> impl Iterator<Item = usize> + '_ {
        [2, 3, 4, 5, 7].into_iter().flat_map(move |rank| self.reward.items.iter().enumerate()
            .filter_map(move |(index, item)| {
                let order = match item.kind {
                    RewardItemKind::Potion(_) => 2,
                    RewardItemKind::Relic(_) => 3,
                    RewardItemKind::SpecialCard { .. } => 4,
                    RewardItemKind::Card { .. } | RewardItemKind::CardBundles { .. } => 5,
                    RewardItemKind::CardRemoval { .. } => 7,
                };
                (!item.consumed && order == rank).then_some(index)
            }))
    }
    pub fn native_awaiting_next_act(&self) -> bool { self.awaiting_next_act }
    pub fn native_can_procure_potion(&self) -> bool { self.can_procure_potion() }
    pub fn native_potions_locked(&self) -> bool {
        matches!(self.native_event_id(), Some(crate::identity::EventId::TheFutureOfPotions
            | crate::identity::EventId::PunchOff | crate::identity::EventId::StoneOfAllTime
            | crate::identity::EventId::RanwidTheElder))
    }
    pub fn native_treasure_relic(&self) -> Option<RelicId> {
        (self.phase == RunPhase::Treasure).then_some(self.treasure_relic).flatten()
    }
    pub fn native_public_boss(&self) -> Option<crate::identity::PublicBossId> {
        self.visible_map().map(|_| self.acts[self.current_act].rooms.public_boss)
    }
    pub fn native_public_bosses(&self) -> [Option<NativeBoss>; 2] {
        let Some(map) = self.visible_map() else { return [None; 2]; };
        let rooms = &self.acts[self.current_act].rooms;
        let mut out = [None; 2]; let mut next = true;
        for (order, (encounter, index)) in [(Some(rooms.boss), Some(map.boss)),
            (rooms.second_boss, map.second_boss)].into_iter().enumerate() {
            if let (Some(encounter), Some(index)) = (encounter, index) {
                let complete = self.run_ctx().hp > 0 && self.native_map_visited(usize::from(index)).is_some()
                    && (self.current_point != index || self.phase != RunPhase::Combat);
                out[order] = Some(NativeBoss {
                    id: crate::identity::PublicBossId::from_name(encounter.name()).expect("public map boss"),
                    map_index: usize::from(index), complete, current: next && !complete });
                next &= complete;
            }
        }
        out
    }
    pub fn native_map_visited(&self, index: usize) -> Option<usize> {
        self.visited_points.iter().position(|&point| usize::from(point) == index)
    }
    pub fn native_map_reachable(&self, index: usize) -> bool {
        if self.phase != RunPhase::MapChoice { return false; }
        let Some(map) = self.visible_map() else { return false; };
        let Some(point) = map.points.get(index) else { return false; };
        // Dispatch resolves coordinates through ActMap::point, not a backing
        // array index. Retired/duplicate entries are not another legal move.
        if map.point(point.coord).is_none_or(|canonical| !std::ptr::eq(canonical,point)) { return false; }
        if self.visited_points.is_empty() { return index == usize::from(map.start); }
        if self.winged_boots_free_travel() {
            return point.coord.row == map.points[self.current_point as usize].coord.row + 1;
        }
        map.points[self.current_point as usize].children.iter().any(|&child| usize::from(child) == index)
    }
    pub fn native_rest_offers(&self) -> impl Iterator<Item = NativeRestOffer> + '_ {
        self.rest.options.iter().enumerate().map(|(index, option)| NativeRestOffer {
            index, kind: option.kind, enabled: option.enabled })
    }
    pub fn native_rest_heal_amount(&self) -> i32 {
        let run = self.run_ctx();
        self.native_rest_base_heal_amount()
            .min((run.max_hp - run.hp).max(0))
    }
    pub fn native_rest_base_heal_amount(&self) -> i32 {
        let run = self.run_ctx();
        rest::heal_amount(run.max_hp, run.has_owned_relic(0, RelicId::RegalPillow))
    }
    pub fn native_rest_action(&self, index: usize) -> Option<RunAction> {
        let option = self.rest.options.get(index)?;
        let mut action = RunAction::new(RunActionKind::RestOption);
        action.option_id = Some(option.kind.legacy_id());
        action.rest_option = Some(option.kind);
        Some(action)
    }
    /// Conversion to legacy action text occurs only when executing a checked binding.
    pub fn native_event_action(&self, index: usize) -> Option<RunAction> {
        let option = if let Some(offers) = self.native_neow_offers() { offers.get(index)?.0 }
            else { self.native_event_options()?.get(index)?.option_id };
        let mut action = RunAction::new(RunActionKind::EventChoice);
        action.option_id = Some(option.legacy_id());
        action.event_option_id = Some(option);
        Some(action)
    }
    pub fn native_event_offer(&self, option: crate::run::EventOptionIdentity) -> Option<crate::run::NativeEventOffer> {
        self.current_event.as_ref()?.native_offer(option, &self.run_ctx().potions)
    }
    pub fn native_event_parameters(&self, option: crate::run::EventOptionIdentity,
        description: Option<crate::run::EventDescription>) -> Result<Vec<super::super::event_preview::PublicEventParameter>, &'static str> {
        use super::super::event_preview::*;
        let run = self.run_ctx();
        public_event_parameters(option, PublicEventContext { hp: run.hp, max_hp: run.max_hp,
            gold: run.gold, deck: &run.deck, players: 1, description,
            dynamic: event_dynamic(self.current_event.as_ref())? })
    }
    pub fn native_event_offer_detail(&self, option: crate::run::EventOptionIdentity)
        -> (Option<CardId>, Option<CardType>) {
        self.current_event.as_ref().map_or((None, None),
            |event| event.native_offer_detail(option, &self.run_ctx().potions))
    }
    /// The card and escalating damage already displayed by SlipperyBridge.
    pub fn native_bridge_offer(&self) -> Option<(i32, &Card)> {
        let event=self.current_event.as_ref()?;
        if event.id!=crate::identity::EventId::SlipperyBridge {return None;}
        let card=self.run_ctx().deck.iter().find(|card|Some(card.instance_id)==event.bridge.current)?;
        Some((3+event.bridge.hold_ons,card))
    }
    pub fn native_wongo_featured(&self) -> Option<RelicId> {
        self.current_event.as_ref()?.public_wongo_featured()
    }
    pub fn native_shop_offers(&self) -> impl Iterator<Item = NativeShopOffer<'_>> {
        let gold = self.run_ctx().gold;
        let can_buy_potion=self.can_buy_shop_potion();
        let cards = self.shop.cards.iter().chain(self.shop.colorless_cards.iter()).enumerate()
            .map(move |(index, entry)| NativeShopOffer { kind: NativeShopKind::Card, index,
                card: entry.card.as_ref(), relic: None, potion: None, price: entry.price,
                on_sale: entry.on_sale, sold: entry.card.is_none(),
                enabled: entry.card.is_some() && gold >= entry.price });
        let relics = self.shop.relics.iter().enumerate().map(move |(index, entry)| NativeShopOffer {
            kind: NativeShopKind::Relic, index, card: None, relic: Some(entry.relic_id), potion: None,
            price: entry.price, on_sale: false, sold: entry.price >= shop::SOLD_OUT,
            enabled: entry.price < shop::SOLD_OUT && gold >= entry.price });
        let potions = self.shop.potions.iter().enumerate().map(move |(index, entry)| NativeShopOffer {
            kind: NativeShopKind::Potion, index, card: None, relic: None, potion: Some(entry.potion_id),
            price: entry.price, on_sale: false, sold: entry.price >= shop::SOLD_OUT,
            enabled: entry.price < shop::SOLD_OUT && gold >= entry.price && can_buy_potion });
        let removal = NativeShopOffer { kind: NativeShopKind::Removal, index: 0, card: None,
            relic: None, potion: None, price: self.shop.removal_cost, on_sale: false,
            sold: self.shop.removal_used, enabled: !self.shop.removal_used && gold >= self.shop.removal_cost
                && self.run_ctx().deck.iter().any(|card| !card.has_kw(KW_ETERNAL)) };
        cards.chain(relics).chain(potions).chain(std::iter::once(removal))
    }
}

#[cfg(test)]
mod potion_purchase_tests {
    use super::*;
    #[test]
    fn native_moves_use_the_same_coordinate_identity_as_dispatch() {
        std::thread::Builder::new().stack_size(64*1024*1024).spawn(|| {
            let mut game=RunManager::new(20400032,RunOptions{ascension:10,..Default::default()});
            game.phase=RunPhase::MapChoice;
            let map=game.acts[0].map.as_mut().unwrap();
            let current=map.start;
            let next=map.points[current as usize].children[0];
            let duplicate=map.points.len() as u8;
            map.points.push(map.points[next as usize].clone());
            map.points[current as usize].children.clear();
            map.points[current as usize].children.push(next);
            map.points[current as usize].children.push(duplicate);
            game.current_point=current;
            game.visited_points.push(current);
            assert!(game.native_map_reachable(next as usize));
            assert!(!game.native_map_reachable(duplicate as usize));
        }).unwrap().join().unwrap();
    }
    #[test]
    fn native_shop_potion_legality_matches_failed_procurement_and_empty_space() {
        std::thread::Builder::new().stack_size(64*1024*1024).spawn(|| {
        let mut game=RunManager::new(20400000,RunOptions{ascension:10,..Default::default()});
        game.phase=RunPhase::Shop;
        game.run_ctx_mut().gold=1000;
        game.shop.potions.push(shop::ShopPotionEntry{
            potion_id:PotionId::Dexteritypotion,rarity:PotionRarity::Common,price:50});
        let capacity=game.run_ctx().max_potion_slots;
        game.run_ctx_mut().potions.clear();
        for _ in 0..capacity {game.run_ctx_mut().potions.push(Some(PotionId::Dexteritypotion));}
        assert!(game.native_shop_offers().filter(|o|matches!(o.kind,NativeShopKind::Potion))
            .all(|o|!o.enabled));
        assert!(!game.available_actions().iter().any(|a|a.kind==RunActionKind::BuyPotion));
        game.run_ctx_mut().potions.fill(None);
        assert!(game.native_shop_offers().any(|o|matches!(o.kind,NativeShopKind::Potion)&&o.enabled));
        assert!(game.available_actions().iter().any(|a|a.kind==RunActionKind::BuyPotion));
        }).unwrap().join().unwrap();
    }
    #[test]
    fn native_event_actions_carry_the_same_typed_option_as_the_run_menu() {
        std::thread::Builder::new().stack_size(64*1024*1024).spawn(|| {
        let mut game=RunManager::new(42,RunOptions::default());
        let menu:Vec<_>=game.available_actions().into_iter().filter(|a|a.kind==RunActionKind::EventChoice).collect();
        assert!(!menu.is_empty());
        for (index,action) in menu.iter().enumerate() {
            let native=game.native_event_action(index).unwrap();
            assert!(action.event_option_id.is_some());
            assert_eq!((native.kind,native.event_option_id,native.option_id),
                (action.kind,action.event_option_id,action.option_id));
        }
        }).unwrap().join().unwrap();
    }
}
