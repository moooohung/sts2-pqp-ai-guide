use super::{MpPhase, MpRun};
use crate::{card::{Card, KRef}, relic::Relic, tables::{CardRarity, CardTag, RelicId, KW_ETERNAL}};

impl MpRun {
    /// PandorasBox.AfterObtained transforms the owner's removable basic
    /// Strikes/Defends. Its result screen is a preview, NOT an awaited choice.
    /// Return the resulting cards so the owning client can display that screen.
    pub fn obtain_pandoras_box(&mut self, net_id: u64, seat: usize) -> Result<Vec<Card>, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Map { return Err("pickup requires map phase"); }
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let mut next = self.clone();
        let result = next.transform_pandoras_box(seat)?;
        *self = next;
        Ok(result)
    }

    fn transform_pandoras_box(&mut self, seat: usize) -> Result<Vec<Card>, &'static str> {
        let mut relic = Relic::new(RelicId::PandorasBox);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        self.record_relic_seen(seat, RelicId::PandorasBox);
        self.apply_pandoras_box_batch(seat)
    }

    pub(super) fn apply_pandoras_box_batch(&mut self,seat:usize)->Result<Vec<Card>,&'static str>{
        let originals: Vec<_> = self.roster.player(seat).unwrap().deck.iter().filter(|c|
            c.rarity == CardRarity::Basic && (c.has_tag(CardTag::Strike) || c.has_tag(CardTag::Defend))
                && !c.has_kw(KW_ETERNAL)).cloned().collect();
        // CardCmd.Transform materializes the lazy transformations FIRST:
        // all shared Niche draws and replacement creation precede add hooks.
        let mut transformations = Vec::new();
        for original in originals {
            let candidates = super::super::card_pools::transformations(&original, true)?;
            if candidates.is_empty() { return Err("empty multiplayer transform pool"); }
            let id = candidates[self.world.run.rngs.niche.choice_index(candidates.len())];
            self.roster.next_card_id += 1;
            let mut card = Card::from_factory(id, false, self.roster.next_card_id)
                .ok_or("transform card factory")?;
            card.owner = seat as u8;
            transformations.push((original.instance_id, card, 0usize));
        }
        // C# captures each original's pile index immediately before removing
        // it, then List.Sort(PileIndexSort) orders the additions. Equal-index
        // tie order must use .NET's introsort, including lists longer than 16.
        for (original, _, position) in &mut transformations {
            let deck = &mut self.roster.player_mut(seat).unwrap().deck;
            *position = deck.iter().position(|c| c.instance_id == *original)
                .ok_or("transform original missing")?;
            self.roster.player_mut(seat).unwrap().remove_deck_card(*position);
        }
        let mut order: Vec<KRef> = (0..transformations.len()).map(KRef::try_from)
            .collect::<Result<_, _>>().map_err(|_| "transform index capacity exceeded")?;
        crate::flow::dotnet_sort_indices(&mut order, |a, b|
            transformations[a as usize].2 < transformations[b as usize].2);
        let mut preview = Vec::new();
        for index in order {
            let card = transformations[index as usize].1.clone();
            // CardPileAddResult.cardAdded is the (modified) replacement itself;
            // an AfterCardChangedPiles clone (BingBong) is appended after it.
            let added = self.roster.player(seat).unwrap().deck.len();
            super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?;
            preview.push(self.roster.player(seat).unwrap().deck[added].clone());
        }
        Ok(preview)
    }
}
