//! r7 combat card generation: `cards/factory.py` `create_character_cards` /
//! `create_cards_from_ids` and `core/combat.py` `_add_generated_card_to_combat`.
use crate::character::Character;

use crate::card::{CId, Card, KRef};
use crate::combat::{Combat, PileKind, PilePos, MAX_HAND_SIZE};
use crate::hooks::Ev;
use crate::tables::*;
use arrayvec::ArrayVec;
use smallvec::SmallVec;
#[cfg(test)]
#[path = "metamorphosis_tests.rs"]
mod metamorphosis_tests;

#[cfg(test)]
mod bing_bong_tests {
    use super::*;
    use crate::{relic::Relic,run::{RunManager,RunOptions}};
    #[test]
    fn guilty_retained_reference_records_fifth_combat_mutation_before_detachment_for_each_owner() {
        for owner in 0..2 {
            let run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
            let mut c=Combat::new(run,1);
            c.add_player(80,80,3,0,&[]);c.add_player(80,80,3,0,&[]);
            c.run.deck.clear();c.run.relics.clear();
            let mut guilty=Card::from_factory(CardId::Guilty,false,42).unwrap();
            guilty.owner=owner;
            let mut relic=Relic::new(RelicId::BingBong);relic.owner=owner;
            let mut retained=crate::relic::RetainedCardSet::default();retained.insert(guilty.clone());
            relic.bing_bong_cards_to_skip=Some(retained);
            c.run.relics.push(relic);
            c.player_deck_mut(owner).push(guilty);
            for combat in 1..=5 {
                c.fire_card_after_combat_end();
                if combat<5 {assert_eq!(c.player_deck(owner)[0].var(VarKey::Combats),Some(5-combat));}
            }
            assert!(c.player_deck(owner).is_empty());
            let retained=c.run.relics[0].bing_bong_cards_to_skip.as_ref().unwrap().entries[0].as_ref().unwrap();
            assert_eq!(retained.var(VarKey::Combats),Some(0)); // C# CombatsSeen == 5.
        }
    }

    #[test]
    fn clones_once_runs_other_hooks_and_retains_the_last_state_after_removal() {
        let mut run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
        run.deck.clear();run.relics.clear();run.gold=0;run.total_floor=5;
        for id in [RelicId::LuckyFysh,RelicId::BingBong,RelicId::BookOfFiveRings] {
            run.relics.push(Relic::new(id));
        }
        assert!(run.relics[1].bing_bong_cards_to_skip.is_none());
        let card=Card::from_factory(CardId::Bash,false,run.new_card_id()).unwrap();
        run.deck.push(card);run.after_run_deck_entry(0);
        assert_eq!(run.deck.len(),2);
        assert_eq!(run.gold,30);
        assert_eq!(run.relics[2].counter,2);
        assert_ne!(run.deck[0].instance_id,run.deck[1].instance_id);
        run.deck[1].upgraded=true;
        let removed=run.remove_run_deck_card(1);
        let stored=run.relics[1].bing_bong_cards_to_skip.as_ref().unwrap().entries[0].as_ref().unwrap();
        assert_eq!(stored.instance_id,removed.instance_id);
        assert!(stored.upgraded);
        assert_eq!(stored.floor_added_to_deck,Some(5));
        run.deck.push(removed);run.after_run_deck_entry(1);
        assert_eq!(run.deck.len(),2);
        assert!(run.relics[1].bing_bong_cards_to_skip.as_ref().unwrap().entries.iter().all(Option::is_none));
        run.relics[1].owner=1;
        run.after_run_deck_entry(0);
        assert_eq!(run.deck.len(),2);
    }

    #[test]
    fn retained_hash_set_reuses_removed_slots_in_lifo_order_without_aliasing() {
        let mut set=crate::relic::RetainedCardSet::default();
        for id in 1..=3 {set.insert(Card::from_factory(CardId::Bash,false,id).unwrap());}
        assert!(set.remove(2));assert!(set.remove(1));
        set.insert(Card::from_factory(CardId::Bash,false,4).unwrap());
        set.insert(Card::from_factory(CardId::Bash,false,4).unwrap());
        assert_eq!(set.entries.iter().flatten().map(|c|c.instance_id).collect::<Vec<_>>(),vec![4,3]);
    }
}

impl crate::combat::RunCtx {
    /// PlayerCmd.cs:107-132; BowlerHat.cs:25-31 / Ectoplasm.cs:18-24.
    pub(crate) fn gain_run_gold(&mut self, amount: i32) -> i32 {
        self.gain_run_gold_for(crate::combat::PLAYER, amount)
    }

    fn gain_run_gold_for(&mut self, owner: CId, amount: i32) -> i32 {
        let mut amount = amount.max(0) as f64;
        for relic in self.owned_relics(owner).filter(|r| r.enabled) {
            match relic.id {
                RelicId::BowlerHat => amount *= 1.25,
                RelicId::Ectoplasm => amount = 0.0,
                _ => {}
            }
        }
        let gold = amount as i32;
        self.gold += gold;
        // PlayerCmd.GainGold returns before AfterGoldGained when modified
        // amount is zero. DragonFruit.cs:22 applies to its owner only.
        if amount>0.0{
            let count=self.owned_relics(owner).filter(|r|r.enabled&&r.id==RelicId::DragonFruit).count() as i32;
            self.max_hp+=count;self.hp=(self.hp+count).min(self.max_hp);
        }
        gold
    }

    /// AfterCardChangedPiles for an already inserted deck card.
    /// LuckyFysh.cs:24-31; DarkstonePeriapt.AfterCardChangedPiles.
    pub(crate) fn after_run_deck_entry(&mut self, index: usize) {
        self.after_run_deck_entry_for(crate::combat::PLAYER, index);
    }

    pub(crate) fn after_run_deck_entry_for(&mut self, owner: CId, index: usize) {
        self.after_run_deck_entry_with_origin(owner,index,false);
    }

    fn after_run_deck_entry_with_origin(&mut self, owner:CId,index:usize,cloned_by:bool) {
        let is_curse = self.deck[index].card_type == CardType::Curse;
        let is_owners_card = self.deck[index].owner == owner;
        let relics: Vec<_> = self.relics.iter().enumerate()
            .filter(|(_,r)|r.owner==owner&&r.enabled).map(|(i,r)|(i,r.id)).collect();
        for (relic_index,relic) in relics {
            if relic == RelicId::LuckyFysh {
                self.gain_run_gold_for(owner, 15);
            } else if relic == RelicId::DarkstonePeriapt && is_curse {
                self.max_hp += 6;
                self.hp = (self.hp + 6).min(self.max_hp);
            } else if relic == RelicId::BookOfFiveRings && is_owners_card && self.hp > 0 {
                // BookOfFiveRings.cs:77-91: living Owner's fifth Deck entry
                // increments the persistent CardsAdded counter and heals 20.
                // Frozen relics/common.py:539-563 uses the same threshold/heal.
                let heal = {
                    let relic = self.owned_relics_mut(owner)
                        .find(|r| r.id == RelicId::BookOfFiveRings && r.enabled).unwrap();
                    relic.counter += 1;
                    relic.counter % 5 == 0
                };
                if heal { self.hp = (self.hp + 20).min(self.max_hp); }
            } else if relic==RelicId::BingBong && is_owners_card && !cloned_by {
                let instance=self.deck[index].instance_id;
                if self.relics[relic_index].bing_bong_cards_to_skip
                    .get_or_insert_with(Default::default).remove(instance) {continue;}
                let mut clone=self.deck[index].clone();
                clone.instance_id=self.new_card_id();
                clone.floor_added_to_deck=Some(self.total_floor);
                self.relics[relic_index].bing_bong_cards_to_skip.as_mut().unwrap().insert(clone.clone());
                self.deck.push(clone);
                self.after_run_deck_entry_with_origin(owner,self.deck.len()-1,true);
            }
        }
    }

    pub(crate) fn remove_run_deck_card(&mut self,index:usize)->Card {
        crate::relic::remove_deck_object(&mut self.deck,&mut self.relics,index)
    }

    pub(crate) fn retain_run_deck_cards(&mut self,predicate:impl FnMut(&Card)->bool) {
        crate::relic::retain_deck_objects(&mut self.deck,&mut self.relics,predicate);
    }

    /// `is_card_upgradable(card)` (r7 cards/base.py): builds the upgraded template, so a
    /// non-upgraded card consumes an instance id.
    pub fn is_deck_card_upgradable(&mut self, i: usize) -> bool {
        let c = &self.deck[i];
        if c.upgraded {
            return false;
        }
        let id = c.id;
        self.new_card_id();
        Card::from_factory(id, true, 0).is_some_and(|u| u.upgraded)
    }

    /// `PlayerState.upgrade_card_instance(deck card)` (r7 run/run_state.py).
    pub fn upgrade_deck_card(&mut self, i: usize) {
        let card = &self.deck[i];
        if card.upgraded {
            return;
        }
        let (id, was_upgraded) = (card.id, card.upgraded);
        if self.ref_cache_miss(id, was_upgraded) {
            self.new_card_id();
        }
        let card = &self.deck[i];
        let reference = Card::from_factory(id, was_upgraded, 0);
        let mut dmg_bonus = 0;
        let mut block_bonus = 0;
        if let Some(r) = &reference {
            if SELF_MUTATING_DAMAGE.contains(&id) && let (Some(d), Some(rd)) = (card.damage, r.damage) {
                dmg_bonus = d - rd;
            }
            if SELF_MUTATING_BLOCK.contains(&id) && let Some(rb) = r.var(VarKey::Block).or(r.block) {
                block_bonus = card.var(VarKey::Block).unwrap_or(card.block.unwrap_or(0)) - rb;
            }
        }
        let iid = self.new_card_id();
        let Some(up) = Card::from_factory(id, true, iid) else { return };
        if !up.upgraded {
            return;
        }
        let c = &mut self.deck[i];
        if id == CardId::MadScience {
            c.upgraded = true;
            c.keywords |= crate::tables::KW_INNATE;
            return;
        }
        c.cost = up.cost;
        c.card_type = up.card_type;
        c.target = up.target;
        c.rarity = up.rarity;
        c.damage = up.damage;
        c.block = up.block;
        c.upgraded = true;
        let base=reference.as_ref().expect("upgrade canonical card");
        c.keywords=(c.keywords & !(base.keywords & !up.keywords)) | (up.keywords & !base.keywords);
        c.tags = up.tags;
        c.gen_combat = up.gen_combat;
        c.gen_modifiers = up.gen_modifiers;
        c.turn_end_in_hand = up.turn_end_in_hand;
        c.vars = up.vars;
        c.apply_canonical_energy_upgrade(base.original_cost,up.original_cost);
        c.x_cost = up.x_cost;
        c.star_cost = up.star_cost;
        if dmg_bonus != 0 {
            c.damage = Some(c.damage.unwrap_or(0) + dmg_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
                v.1 += dmg_bonus;
            }
        }
        if block_bonus != 0 {
            c.block = Some(c.block.unwrap_or(0) + block_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Block) {
                v.1 += block_bonus;
            }
        }
    }

    /// FishingRod: `player.upgrade_card_instance(niche.choice(upgradable_deck_cards()))`.
    pub fn upgrade_random_deck_card(&mut self) {
        let mut candidates: Vec<usize> = Vec::new();
        for i in 0..self.deck.len() {
            if self.is_deck_card_upgradable(i) {
                candidates.push(i);
            }
        }
        if !candidates.is_empty() {
            let pick = candidates[self.rngs.niche.choice_index(candidates.len())];
            self.upgrade_deck_card(pick);
        }
    }
}

/// `eligible_character_cards(character, card_type, rarity, generation_context="combat")`.
pub fn character_pool(pool: crate::character::CardPool, multiplayer: bool, card_type: Option<CardType>, rarity: Option<CardRarity>) -> Option<&'static [CardId]> {
    CARD_POOLS.iter().find(|p| p.0 == pool && p.1 == multiplayer && p.2 == card_type && p.3 == rarity).map(|p| p.4)
}

impl Combat {
    /// CardFactory.GetDistinctForCombat over the all-unlocked owner's character pool.
    /// Audited basic-Strike transform options have exactly the same eligible set/order:
    /// common/uncommon/rare, combat-generatable, player-count filtered, with no original
    /// to remove (the basic Strike is already outside that set).
    pub fn generate_unlocked_owner_cards(&mut self, owner: CId, kind: Option<CardType>, count: usize) -> Vec<KRef> {
        self.generate_unlocked_owner_cards_filtered(owner, kind, 0, count)
    }

    pub(crate) fn generate_unlocked_zero_cost_owner_cards(&mut self, owner: CId, count: usize) -> Vec<KRef> {
        self.generate_unlocked_owner_cards_matching(owner,None,0,count,true,false)
    }

    pub(crate) fn generate_unlocked_ethereal_cards(&mut self, owner: CId, count: usize) -> Vec<KRef> {
        self.generate_unlocked_owner_cards_filtered(owner, None, KW_ETHEREAL, count)
    }

    fn generate_unlocked_owner_cards_filtered(&mut self, owner: CId, kind: Option<CardType>, keywords: u16, count: usize) -> Vec<KRef> {
        self.generate_unlocked_owner_cards_matching(owner,kind,keywords,count,false,true)
    }

    fn generate_unlocked_owner_cards_matching(&mut self, owner: CId, kind: Option<CardType>, keywords: u16, count: usize, zero_cost:bool, distinct:bool) -> Vec<KRef> {
        if self.run.all_cards_unlocked != Some(true) {
            self.unported("card generation requires explicit all-unlocked run configuration");
            return Vec::new();
        }
        let original = match self.players[owner].character {
            Some(Character::Ironclad) => "STRIKE_IRONCLAD", Some(Character::Silent) => "STRIKE_SILENT",
            Some(Character::Defect) => "STRIKE_DEFECT", Some(Character::Regent) => "STRIKE_REGENT",
            Some(Character::Necrobinder) => "STRIKE_NECROBINDER",
            _ => { self.unported("card generation owner character"); return Vec::new(); }
        };
        self.generate_audited_pool(owner, original, kind, keywords, count,zero_cost,distinct)
    }

    /// CardFactory.GetDefaultTransformationOptions(Token) uses ColorlessCardPool;
    /// Soul itself is outside that pool, so the audited transform list is exactly
    /// the full combat-eligible colorless pool, including multiplayer-only cards.
    pub fn generate_unlocked_colorless_cards(&mut self, owner: CId, count: usize) -> Vec<KRef> {
        if self.run.all_cards_unlocked != Some(true) {
            self.unported("card generation requires explicit all-unlocked run configuration");
            return Vec::new();
        }
        self.generate_audited_pool(owner, "SOUL", None, 0, count,false,true)
    }

    fn generate_audited_pool(&mut self, owner: CId, original: &str, kind: Option<CardType>, keywords: u16, count: usize, zero_cost:bool, distinct:bool) -> Vec<KRef> {
        let Some((_, _, names)) = CSHARP_TRANSFORMS.iter().find(|(mp, name, _)|
            *mp == (self.players.len() > 1) && *name == original) else {
            self.unported("audited owner combat generation pool"); return Vec::new();
        };
        let mut ids = Vec::new();
        for &name in *names {
            let Some(id) = CardId::ALL.into_iter().find(|id|
                id.name().strip_suffix("_CARD").unwrap_or(id.name()) == name) else {
                self.unported("owner generation candidate identity"); return Vec::new();
            };
            if kind.is_none_or(|kind| card_info(id, false).is_some_and(|info| info.card_type == kind))
                && (!zero_cost || card_info(id,false).is_some_and(|info|info.cost==0&&!info.x_cost))
                && (keywords == 0 || card_info(id, false).is_some_and(|info| info.keywords & keywords == keywords)) {
                ids.push(id);
            }
        }
        self.generate_pool_cards(owner, &ids, count, distinct).into_iter().collect()
    }
    /// Explicit C# Player.Character binding; never infer character from a cross-color deck.
    pub fn set_character_pool(&mut self, owner: CId, character: crate::character::Character) -> Result<(), &'static str> {
        self.players[owner].character_pool = Some(character.into());
        Ok(())
    }

    pub fn generate_owner_cards(&mut self, owner: CId, kind: CardType, count: usize, distinct: bool) -> SmallVec<[KRef; 10]> {
        self.generate_character_cards(owner, Some(kind), None, count, distinct)
    }

    /// Instantiate a factory card owned by `owner` in the arena (C# `CombatState.CreateCard(owner)`,
    /// r7 `create_card`), outside every pile.
    pub fn new_card(&mut self, owner: CId, id: CardId, upgraded: bool) -> Option<KRef> {
        let iid = self.run.new_card_id();
        let Some(mut card) = Card::from_factory(id, upgraded, iid) else {
            self.unported("card factory");
            return None;
        };
        card.owner = owner;
        if crate::card::has_unwired_card_hook(id) {
            self.unported("card hook");
        }
        let k = self.cards.len() as KRef;
        self.card_hook_mask |= CARD_HOOKS[id.index()];
        self.cards.push(card);
        Some(k)
    }

    /// `CardFactory.GetDistinctForCombat(owner, owner.Character.CardPool...)`; returns the
    /// new arena cards in the owner's character-pool order.
    pub fn generate_character_cards(&mut self, owner: CId, card_type: Option<CardType>, rarity: Option<CardRarity>,
                                    count: usize, distinct: bool) -> SmallVec<[KRef; 10]> {
        let Some(name) = self.players[owner].character_pool else {
            self.unported("owner character pool");
            return SmallVec::new();
        };
        let Some(pool) = character_pool(name, self.players.len() > 1, card_type, rarity)
            .or_else(|| character_pool(name, false, card_type, rarity)) else {
            self.unported("owner character pool");
            return SmallVec::new();
        };
        self.generate_pool_cards(owner, pool, count, distinct)
    }

    /// r7 `create_cards_from_ids(ids, combat_card_generation_rng, count, distinct=...)`; the
    /// cards belong to `owner`.
    pub fn generate_pool_cards(&mut self, owner: CId, pool: &[CardId], count: usize,
                               distinct: bool) -> SmallVec<[KRef; 10]> {
        // fire: cards/factory.py create_cards_from_ids
        let mut out = SmallVec::new();
        if pool.is_empty() || count == 0 {
            return out;
        }
        let mut chosen: SmallVec<[CardId; 10]> = SmallVec::new();
        if distinct {
            let mut ids: ArrayVec<CardId, 128> = pool.iter().copied().collect();
            self.run.rngs.combat_card_generation.shuffle(&mut ids);
            chosen.extend(ids.into_iter().take(count));
        } else {
            for _ in 0..count {
                let i = self.run.rngs.combat_card_generation.choice_index(pool.len());
                chosen.push(pool[i]);
            }
        }
        for id in chosen {
            if let Some(k) = self.new_card(owner, id, false) {
                out.push(k);
            }
        }
        out
    }

    /// `_add_generated_card_to_combat(card, owner, pile, added_by_player, random/bottom)`: into
    /// the piles of the card's owner.
    pub fn add_generated_card(&mut self, k: KRef, pile: PileKind, pos: PilePos, added_by_player: bool) {
        let creator = added_by_player.then_some(self.cards[k as usize].owner);
        self.add_generated_card_by(k, pile, pos, creator);
    }

    /// CardPileCmd.AddGeneratedCardsToCombat carries a creator distinct from card.Owner.
    pub fn add_generated_card_by(&mut self, k: KRef, pile: PileKind, pos: PilePos, creator: Option<CId>) {
        let old_pile = self.card_pile(k);
        let added_by_player = creator.is_some();
        if self.is_over_or_ending() {
            return;
        }
        let owner = self.cards[k as usize].owner;
        self.hist.generated_combat += 1;
        self.hist.generated_by_player_combat += i32::from(added_by_player);
        if let Some(creator)=creator {self.hist.generated_by_creator[creator as usize]+=1;}
        match pile {
            PileKind::Hand if self.players[owner].hand.len() < MAX_HAND_SIZE => self.players[owner].hand.push(k),
            PileKind::Hand => self.players[owner].discard.push(k),
            PileKind::Draw => match pos {
                PilePos::Random => {
                    let i = self.run.rngs.shuffle.next_int(0, self.players[owner].draw.len() as i64) as usize;
                    self.players[owner].draw.insert(i, k);
                    self.players[owner].draw_known = Default::default();
                }
                PilePos::Bottom => self.put_on_draw(k, false, true),
                _ => self.put_on_draw(k, true, true),
            },
            PileKind::Discard => match pos {
                PilePos::Random => {
                    let i = self.run.rngs.shuffle.next_int(0, self.players[owner].discard.len() as i64) as usize;
                    self.players[owner].discard.insert(i, k);
                }
                _ => self.players[owner].discard.push(k),
            },
            _ => {
                self.unported("generated card pile");
                return;
            }
        }
        // _apply_card_after_card_entered_combat: power + relic listeners, then (non-clones) card
        // listeners, which the card-hook guard already rejects.
        self.fire(Hook::AfterCardEnteredCombat, Ev::Card(k), None);
        if !self.cards[k as usize].cv.is_clone && self.cards[k as usize].has_tag(CardTag::Shiv)
            && self.power_amount(owner, PowerId::PhantomBlades) > 0
        {
            // fire: core/combat.py:1340-1341
            self.cards[k as usize].keywords |= KW_RETAIN;
        }
        // fire: core/combat.py:1329-1345
        self.fire_card_after_card_entered_combat(k);
        if old_pile != self.card_pile(k) {
            self.fire_after_card_changed_piles(k, old_pile, None);
        }
        self.fire_after_card_generated_by(k, creator);
    }

    /// `fire_after_card_generated_for_combat`: powers, relics, then card listeners.
    pub fn fire_after_card_generated(&mut self, k: KRef, by_player: bool) {
        self.fire_after_card_generated_by(k, by_player.then_some(self.cards[k as usize].owner));
    }

    fn fire_after_card_generated_by(&mut self, k: KRef, creator: Option<CId>) {
        #[cfg(test)]
        self.hist.card_lifecycle.push(crate::piles::Lifecycle::Generated(k, creator));
        let by_player = creator.is_some();
        self.fire(Hook::AfterCardGeneratedForCombat, Ev::Generated { card: k, by_player, creator }, None);
        // Monster-model listeners (Aeonglass.AfterCardGeneratedForCombat).
        crate::content::monsters_act3::aeonglass_after_card_generated(self, k);
        self.fire_card_after_card_generated(k, by_player);
    }

    /// `CombatState.upgrade_card`: re-read the upgraded factory card into this one. Like r7,
    /// building the upgraded template consumes a card instance id.
    pub fn upgrade_card(&mut self, k: KRef) {
        let card = &self.cards[k as usize];
        if card.upgraded || (self.is_over_or_ending() && !self.is_over) {
            return;
        }
        let id = card.id;
        // capture_self_mutating_card_progress (r7 cards/base.py:73) reads the reference card
        // through an lru_cache; only a cache miss builds it (consuming an instance id).
        let was_upgraded = card.upgraded;
        if self.run.ref_cache_miss(id, was_upgraded) {
            self.run.new_card_id();
        }
        let card = &self.cards[k as usize];
        let reference = Card::from_factory(id, was_upgraded, 0);
        let mut dmg_bonus = 0;
        let mut block_bonus = 0;
        if let Some(r) = &reference {
            if SELF_MUTATING_DAMAGE.contains(&id) && let (Some(d), Some(rd)) = (card.damage, r.damage) {
                dmg_bonus = d - rd;
            }
            if SELF_MUTATING_BLOCK.contains(&id) {
                let rb = r.var(VarKey::Block).or(r.block);
                if let Some(rb) = rb {
                    block_bonus = card.var(VarKey::Block).unwrap_or(card.block.unwrap_or(0)) - rb;
                }
            }
        }
        let iid = self.run.new_card_id();
        let Some(up) = Card::from_factory(id, true, iid) else {
            return;
        };
        if !up.upgraded {
            return;
        }
        // CardModel.UpgradeInternal runs OnUpgrade in place: apply only the canonical
        // keyword additions/removals, preserving combat-applied keywords (e.g. Snap).
        let base_keywords = reference.as_ref().map_or(0, |base| base.keywords);
        let added_keywords = up.keywords & !base_keywords;
        let removed_keywords = base_keywords & !up.keywords;
        let c = &mut self.cards[k as usize];
        // C# MadScience.OnUpgrade only adds Innate. TinkerTimeType, TinkerTimeRider
        // and the instance's dynamic vars are saved event state, not canonical fields.
        if id == CardId::MadScience {
            #[cfg(feature = "selector-proof")]
            if c.block.is_none() && up.block.is_some() && up.block==c.var(VarKey::Block) {
                self.mad_science_upgrade_proof.push((c.instance_id,c.block,up.block));
            }
            c.upgraded = true;
            c.keywords |= KW_INNATE;
            return;
        }
        c.card_type = up.card_type;
        c.target = up.target;
        c.rarity = up.rarity;
        c.damage = up.damage;
        c.block = up.block;
        c.upgraded = true;
        c.keywords = (c.keywords & !removed_keywords) | added_keywords;
        c.tags = up.tags;
        c.gen_combat = up.gen_combat;
        c.gen_modifiers = up.gen_modifiers;
        c.turn_end_in_hand = up.turn_end_in_hand;
        c.vars = up.vars;
        c.x_cost = up.x_cost;
        c.star_cost = up.star_cost;
        c.apply_canonical_energy_upgrade(reference.as_ref().expect("upgrade canonical card").original_cost,up.original_cost);
        // restore_self_mutating_card_progress: increase_base_damage / increase_base_block.
        if dmg_bonus != 0 {
            c.damage = Some(c.damage.unwrap_or(0) + dmg_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
                v.1 += dmg_bonus;
            }
        }
        if block_bonus != 0 {
            c.block = Some(c.block.unwrap_or(0) + block_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Block) {
                v.1 += block_bonus;
            }
        }
    }

    /// DampenPower's temporary downgrade. The card keeps its identity/combat vars/enchantments
    /// while its static upgraded fields are replaced by the base factory values.
    pub(crate) fn downgrade_card_for_dampen(&mut self, k: KRef) -> bool {
        if !self.cards[k as usize].upgraded {
            return false;
        }
        let id = self.cards[k as usize].id;
        let was_upgraded = true;
        if self.run.ref_cache_miss(id, was_upgraded) {
            self.run.new_card_id();
        }
        let reference = Card::from_factory(id, was_upgraded, 0);
        let mut dmg_bonus = 0;
        let mut block_bonus = 0;
        if let Some(r) = &reference {
            let card = &self.cards[k as usize];
            if SELF_MUTATING_DAMAGE.contains(&id)
                && let (Some(d), Some(rd)) = (card.damage, r.damage)
            {
                dmg_bonus = d - rd;
            }
            if SELF_MUTATING_BLOCK.contains(&id)
                && let Some(rb) = r.var(VarKey::Block).or(r.block)
            {
                block_bonus = card.var(VarKey::Block).unwrap_or(card.block.unwrap_or(0)) - rb;
            }
        }

        // r7 `create_card(id, upgraded=False)` allocates an instance even though Dampen copies
        // only its static values onto the existing combat card.
        self.run.new_card_id();
        let Some(base) = Card::from_factory(id, false, 0) else { return false };
        let c = &mut self.cards[k as usize];
        c.card_type = base.card_type;
        c.target = base.target;
        c.rarity = base.rarity;
        c.damage = base.damage;
        c.block = base.block;
        c.upgraded = false;
        c.keywords = base.keywords;
        c.tags = base.tags;
        c.gen_combat = base.gen_combat;
        c.gen_modifiers = base.gen_modifiers;
        c.turn_end_in_hand = base.turn_end_in_hand;
        c.vars = base.vars;
        c.x_cost = base.x_cost;
        c.star_cost = base.star_cost;
        c.set_base_energy_cost(base.original_cost);
        if dmg_bonus != 0 {
            c.damage = Some(c.damage.unwrap_or(0) + dmg_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
                v.1 += dmg_bonus;
            }
        }
        if block_bonus != 0 {
            c.block = Some(c.block.unwrap_or(0) + block_bonus);
            if let Some(v) = c.vars.iter_mut().find(|(key, _)| *key == VarKey::Block) {
                v.1 += block_bonus;
            }
        }
        c.cv.dampened = true;
        true
    }

    /// C# CardModel.SetToFreeThisTurn: energy expires at turn end OR play;
    /// the star override lasts through the turn (CardModel.cs:1017-1021).
    pub fn set_temporary_free_this_turn(&mut self, k: KRef) {
        let c = &mut self.cards[k as usize];
        c.set_energy_cost(0, crate::card::CostExpiry::TurnOrPlayed, false);
        c.cv.turn_star_cost_override = Some(0);
    }
}
