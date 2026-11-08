//! Persistent Pael's Tooth card storage.

use arrayvec::ArrayVec;

use crate::combat::{Combat, RunCtx, PLAYER};
use crate::tables::{RelicId, card_info};

impl RunCtx {
    /// Resolver for Pael's Tooth's obtain-time deck choice.
    ///
    /// The run layer owns eligibility and supplies the selected deck indices. This function
    /// performs r7's first-five truncation, stable reference-entry sort, clone-with-new-id,
    /// and identity removal.
    pub fn paels_tooth_store_selected(&mut self, selected: &[usize]) -> usize {
        // fire: relics/shop_event.py:2685-2693
        let mut picked: ArrayVec<usize, 5> = ArrayVec::new();
        for &i in selected.iter().take(5) {
            if i < self.deck.len() && !picked.contains(&i) {
                picked.push(i);
            }
        }
        picked.sort_by_key(|&i| {
            let c = &self.deck[i];
            card_info(c.id, c.upgraded).map_or(c.id.name(), |info| info.entry)
        });

        let mut stored = Vec::with_capacity(picked.len());
        for &i in &picked {
            let iid = self.new_card_id();
            stored.push(self.deck[i].clone_with_id(iid));
        }

        let mut remove: ArrayVec<usize, 5> = picked;
        remove.sort_unstable_by(|a, b| b.cmp(a));
        for i in remove {
            self.remove_run_deck_card(i);
        }
        let count = stored.len();
        self.paels_tooth_cards.retain(|card| card.owner != PLAYER);
        self.paels_tooth_cards.extend(stored);
        if let Some(relic) = self.owned_relics_mut(PLAYER).find(|r| r.id == RelicId::PaelsTooth) {
            relic.counter = count as i32;
            relic.flag1 = false;
        }
        count
    }

    /// Return one stored card after combat, selected by the rewards RNG, then upgrade it.
    ///
    /// Relic.flag1 is reserved for the parity loader's opaque legacy snapshot marker. The
    /// frozen v2 fixture retained only "<CardInstance>" values; that path can preserve the
    /// observable rewards-RNG/count transition but cannot reconstruct a card identity.
    pub(crate) fn paels_tooth_return_one(&mut self, relic_index: usize) {
        // fire: relics/shop_event.py:2710-2721
        let indices: Vec<_> = self.paels_tooth_cards.iter().enumerate()
            .filter(|(_, card)| card.owner == PLAYER).map(|(i, _)| i).collect();
        if !indices.is_empty() {
            let i = indices[self.rngs.rewards.choice_index(indices.len())];
            let card = self.paels_tooth_cards.remove(i);
            self.deck.push(card);
            let deck_index = self.deck.len() - 1;
            self.upgrade_deck_card(deck_index);
            // PaelsTooth.cs:104-109 upgrades before CardPileCmd.Add(Deck).
            self.after_run_deck_entry(deck_index);
            self.relics[relic_index].counter = indices.len() as i32 - 1;
            return;
        }

        // Frozen corpus compatibility only: identities were discarded by its serializer.
        if self.relics[relic_index].flag1 && self.relics[relic_index].counter > 0 {
            let n = self.relics[relic_index].counter as usize;
            let _ = self.rngs.rewards.choice_index(n);
            self.relics[relic_index].counter -= 1;
            if self.relics[relic_index].counter == 0 {
                self.relics[relic_index].flag1 = false;
            }
        }
    }
}

impl Combat {
    pub(crate) fn relic_paels_tooth_return_one(&mut self, r: usize) {
        let owner = self.run.relics[r].owner;
        if owner == PLAYER {
            self.run.paels_tooth_return_one(r);
            return;
        }
        // PaelsTooth.cs:93-111: saved cards are partitioned by original Owner,
        // select with PlayerRng.Rewards, upgrade before entering that Owner's deck.
        let indices: Vec<_> = self.run.paels_tooth_cards.iter().enumerate()
            .filter(|(_, card)| card.owner == owner).map(|(i, _)| i).collect();
        if indices.is_empty() { return; }
        let needs_gold = self.run.owned_relics(owner).any(|r| r.enabled && r.id == RelicId::LuckyFysh);
        if needs_gold && self.players[owner].gold.is_none() {
            self.unported("relic owner initial gold not supplied");
            return;
        }
        let Some(rng) = self.players[owner].rewards_rng.as_mut() else {
            self.unported("PaelsTooth owner rewards RNG not supplied");
            return;
        };
        let i = indices[rng.choice_index(indices.len())];
        let card = self.run.paels_tooth_cards.remove(i);
        self.relic_owner_deck_mut(owner).push(card);
        let deck_index = self.relic_owner_deck(owner).len() - 1;
        self.relic_owner_upgrade_deck_card(owner, deck_index);
        // Reuse run-deck callbacks with the acting owner's persistent resources.
        let local = (self.run.hp, self.run.max_hp, self.run.gold);
        self.run.hp = self.cr(owner).hp;
        self.run.max_hp = self.cr(owner).max_hp;
        self.run.gold = self.players[owner].gold.unwrap_or(0);
        self.with_relic_owner_deck(owner, |run| run.after_run_deck_entry_for(owner, deck_index));
        let (hp, max_hp, gold) = (self.run.hp, self.run.max_hp, self.run.gold);
        (self.run.hp, self.run.max_hp, self.run.gold) = local;
        self.cr_mut(owner).hp = hp;
        self.cr_mut(owner).max_hp = max_hp;
        if self.players[owner].gold.is_some() { self.players[owner].gold = Some(gold); }
        self.run.relics[r].counter = indices.len() as i32 - 1;
    }
}
