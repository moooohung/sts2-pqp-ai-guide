use super::MpRoster;
use crate::{card::Card, tables::CardId};

impl MpRoster {
    /// AscensionManager.ApplyEffectsTo: Tight Belt (4), Ascender's Bane (5).
    /// Call once after creation; this is not Neow's separate heal operation.
    pub fn apply_starting_ascension(&mut self, level: i32) -> Result<(), &'static str> {
        if !(0..=10).contains(&level) { return Err("unsupported ascension"); }
        if self.ascension_applied { return Err("multiplayer ascension already applied"); }
        for seat in 0..self.players().len() {
            let mut bane = None;
            if level >= 5 {
                self.next_card_id += 1;
                let mut card = Card::from_factory(CardId::AscendersBane, false, self.next_card_id)
                    .ok_or("ascenders bane factory")?;
                card.owner = seat as u8;
                card.floor_added_to_deck = Some(1); // AscensionManager.ApplyEffectsTo
                bane = Some(card);
            }
            let p = self.player_mut(seat).unwrap();
            if level >= 4 {
                p.max_potion_slots -= 1;
                p.potions.truncate(p.max_potion_slots);
            }
            if let Some(card) = bane { p.deck.push(card); }
        }
        self.ascension_applied = true;
        Ok(())
    }
}
