use super::{MpPhase, MpRun};
use crate::{
    card::Card,
    relic::Relic,
    tables::{CardType, RelicId, KW_ETERNAL},
};

impl MpRun {
    /// C# NewLeaf.AfterObtained -> CardSelectCmd.FromDeckForTransformation.
    /// The selector offers exactly the owner's non-Quest, transformable deck
    /// cards; one candidate auto-selects, while larger sets await that owner.
    pub(super) fn begin_new_leaf(&mut self, seat: usize) -> Result<(), &'static str> {
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let player = self.roster.player(seat).ok_or("invalid pickup owner")?;
        let choices: Vec<u32> = player.deck.iter()
            .filter(|card| card.card_type != CardType::Quest && !card.has_kw(KW_ETERNAL))
            .map(|card| card.instance_id)
            .collect();

        let mut relic = Relic::new(RelicId::NewLeaf);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(relic);
        self.record_relic_seen(seat, RelicId::NewLeaf);

        match choices.as_slice() {
            [] => Ok(()),
            [only] => self.finish_new_leaf(seat, *only),
            _ => {
                self.new_leaf_choices[seat] = Some(choices);
                Ok(())
            }
        }
    }

    pub fn select_new_leaf_card(&mut self, net_id: u64, seat: usize, instance_id: u32)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map) {
            return Err("not at inventory choice");
        }
        let choices = self.new_leaf_choices[seat].as_ref().ok_or("no owner transform choice")?;
        if !choices.contains(&instance_id) { return Err("invalid owner transform selection"); }
        let mut next = self.clone();
        next.finish_new_leaf(seat, instance_id)?;
        next.new_leaf_choices[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    fn finish_new_leaf(&mut self, seat: usize, instance_id: u32) -> Result<(), &'static str> {
        // NewLeaf.cs:22-27 passes RunState.Rng.Niche to CardCmd.TransformToRandom.
        // CardCmd.Transform materializes the replacement before removing the
        // original, then runs normal owner-local deck-add hooks.
        let original = self.roster.player(seat).unwrap().deck.iter()
            .find(|card| card.instance_id == instance_id)
            .cloned().ok_or("transform candidate missing")?;
        if original.card_type == CardType::Quest || original.has_kw(KW_ETERNAL) {
            return Err("transform candidate no longer eligible");
        }
        let candidates = super::super::card_pools::transformations(&original, true)?;
        if candidates.is_empty() { return Err("empty multiplayer transform pool"); }
        let id = candidates[self.world.run.rngs.niche.choice_index(candidates.len())];
        self.roster.next_card_id += 1;
        let mut replacement = Card::from_factory(id, false, self.roster.next_card_id)
            .ok_or("transform card factory")?;
        replacement.owner = seat as u8;

        let deck = &mut self.roster.player_mut(seat).unwrap().deck;
        let index = deck.iter().position(|card| card.instance_id == instance_id)
            .ok_or("transform candidate missing")?;
        self.roster.player_mut(seat).unwrap().remove_deck_card(index);
        super::cards::add(&mut self.roster, &mut self.world.run, seat, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    #[test]
    fn new_leaf_choice_and_transform_are_owner_local() {
        let specs = [
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder },
        ];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        let other_deck: Vec<_> = run.roster.player(0).unwrap().deck.iter()
            .map(|card| (card.id, card.instance_id, card.owner))
            .collect();
        run.begin_new_leaf(1).unwrap();
        let choices = run.new_leaf_choices[1].clone().unwrap();
        let selected = choices[0];
        let before_len = run.roster.player(1).unwrap().deck.len();

        run.select_new_leaf_card(42, 1, selected).unwrap();

        let owner = run.roster.player(1).unwrap();
        assert_eq!(owner.deck.len(), before_len);
        assert!(owner.deck.iter().all(|card| card.instance_id != selected));
        assert!(owner.deck.iter().all(|card| card.owner == 1));
        assert!(owner.relics.iter().any(|relic| relic.id == RelicId::NewLeaf));
        assert_eq!(run.roster.player(0).unwrap().deck.iter()
            .map(|card| (card.id, card.instance_id, card.owner))
            .collect::<Vec<_>>(), other_deck);
        assert!(run.new_leaf_choices[1].is_none());
    }
}
