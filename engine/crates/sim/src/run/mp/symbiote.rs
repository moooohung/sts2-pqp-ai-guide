//! Symbiote.Approach / KillWithFire: recipient-owned deck selectors and event RNG.
use crate::identity::EventId;
use super::{MpPhase, MpRun};
use crate::{card::Card, run::{events, manager::RunManager}};

impl MpRun {
    pub(super) fn begin_symbiote(&mut self) -> Result<(), &'static str> {
        if self.world.run.act_index <= 0 { return Err("Symbiote is ineligible"); }
        for seat in 0..self.roster.players().len() {
            let p = self.roster.player(seat).unwrap();
            self.event_done[seat] = p.hp <= 0;
            self.events[seat] = if p.hp <= 0 { None } else {
                Some(events::initialize(EventId::Symbiote,
                    self.world.run.rngs.seed.wrapping_add(seat as u64),
                    p.gold, 0, p.hp, p.max_hp).ok_or("Symbiote initializer missing")?)
            };
        }
        Ok(())
    }

    fn symbiote_candidates(&self, seat: usize, option: i32) -> Vec<u32> {
        let deck = &self.roster.player(seat).unwrap().deck;
        if option == 0 {
            deck.iter().filter(|c| RunManager::event_can_enchant(c, "Corrupted"))
                .map(|c| c.instance_id).collect()
        } else {
            // The solo transformation filter has the same eligibility as removal;
            // removal prioritizes curses, so restore Deck.Cards order for this UI.
            let mut indices = RunManager::removable_card_indices(deck);
            indices.sort_unstable();
            indices.into_iter().map(|i| deck[i].instance_id).collect()
        }
    }

    pub fn choose_symbiote_option(&mut self, net_id: u64, seat: usize, option: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("SYMBIOTE") {
            return Err("not at Symbiote");
        }
        let event = self.events[seat].as_ref().ok_or("event already finished")?;
        if event.flags != 0 { return Err("event selector already open"); }
        if option >= event.options.len() { return Err("invalid event option"); }
        let candidates = self.symbiote_candidates(seat, option as i32);
        if option == 0 && candidates.is_empty() { return Err("Symbiote Approach is locked"); }
        if candidates.is_empty() {
            self.finish_symbiote(seat);
        } else {
            let event = self.events[seat].as_mut().unwrap();
            event.ints[0] = option as i32;
            event.flags = 1;
        }
        Ok(())
    }

    pub fn symbiote_choices(&self, net_id: u64, seat: usize) -> Result<Vec<u32>, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("SYMBIOTE") {
            return Err("not at Symbiote");
        }
        let event = self.events[seat].as_ref().ok_or("event already finished")?;
        if event.flags != 1 { return Err("event selector missing"); }
        Ok(self.symbiote_candidates(seat, event.ints[0]))
    }

    pub fn select_symbiote_card(&mut self, net_id: u64, seat: usize, instance: u32)
        -> Result<(), &'static str> {
        if !self.symbiote_choices(net_id, seat)?.contains(&instance) { return Err("invalid event card"); }
        let mut next = self.clone();
        let mut event = next.events[seat].take().unwrap();
        let index = next.roster.player(seat).unwrap().deck.iter()
            .position(|c| c.instance_id == instance).unwrap();
        if event.ints[0] == 0 {
            let card = &mut next.roster.player_mut(seat).unwrap().deck[index];
            if !crate::card::enchant::apply_static_enchantment(card, "Corrupted", 1) {
                return Err("event enchantment capacity exceeded");
            }
        } else {
            let original = next.roster.player(seat).unwrap().deck[index].clone();
            let pool = super::super::card_pools::transformations(&original, true)?;
            if pool.is_empty() { return Err("empty transform pool"); }
            let id = pool[event.rng.choice_index(pool.len())];
            next.world.run.next_card_id = next.world.run.next_card_id.max(next.roster.next_card_id);
            let iid = next.world.run.new_card_id();
            let mut card = Card::from_factory(id, false, iid).ok_or("transform card factory")?;
            card.owner = seat as u8;
            next.roster.player_mut(seat).unwrap().remove_deck_card(index);
            super::cards::add(&mut next.roster, &mut next.world.run, seat, card)?;
        }
        next.finish_symbiote(seat);
        *self = next;
        Ok(())
    }

    fn finish_symbiote(&mut self, seat: usize) {
        self.events[seat] = None;
        self.event_done[seat] = true;
        if self.event_done.iter().all(|done| *done) { self.finish_noncombat(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    fn event() -> MpRun {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.world.run.act_index = 1;
        run.world.run.total_floor = 20;
        run.world.current_room = Some("SYMBIOTE");
        run.phase = MpPhase::Event;
        run.begin_symbiote().unwrap();
        run
    }

    #[test]
    fn approach_and_transform_are_actor_bound_and_finish_only_the_recipient() {
        let mut run = event();
        assert!(run.choose_symbiote_option(7, 1, 0).is_err());
        run.choose_symbiote_option(42, 1, 0).unwrap();
        let choices = run.symbiote_choices(42, 1).unwrap();
        let foreign = run.roster.player(0).unwrap().deck[0].instance_id;
        assert!(run.select_symbiote_card(42, 1, foreign).is_err());
        run.select_symbiote_card(42, 1, choices[0]).unwrap();
        assert_eq!(run.phase, MpPhase::Event);
        let card = run.roster.player(1).unwrap().deck.iter().find(|c| c.instance_id == choices[0]).unwrap();
        assert!(card.enchantments.contains(&(crate::identity::EnchantmentId::Corrupted, 1)));
        let owner_deck: Vec<_> = run.roster.player(1).unwrap().deck.iter().map(|c| c.instance_id).collect();
        run.choose_symbiote_option(7, 0, 1).unwrap();
        let choices = run.symbiote_choices(7, 0).unwrap();
        let original = run.roster.player(0).unwrap().deck.iter().find(|c| c.instance_id == choices[0]).unwrap();
        let pool = super::super::super::card_pools::transformations(original, true).unwrap();
        let mut event_rng = crate::rng::Rng::for_model(run.world.run.rngs.seed, 0, "SYMBIOTE", 0);
        let expected = pool[event_rng.choice_index(pool.len())];
        let before = run.roster.player(0).unwrap().rngs.transformations.counter;
        let count = run.roster.player(0).unwrap().deck.len();
        run.select_symbiote_card(7, 0, choices[0]).unwrap();
        let p = run.roster.player(0).unwrap();
        assert_eq!(p.deck.len(), count);
        assert!(!p.deck.iter().any(|c| c.instance_id == choices[0]));
        assert_eq!(p.deck.last().unwrap().floor_added_to_deck, Some(20));
        assert_eq!(p.deck.last().unwrap().id, expected);
        assert_eq!(p.rngs.transformations.counter, before);
        assert_eq!(run.roster.player(1).unwrap().deck.iter().map(|c| c.instance_id).collect::<Vec<_>>(), owner_deck);
        assert_eq!(run.phase, MpPhase::Map);
    }

    #[test]
    fn locked_approach_cannot_open_a_selector_and_empty_transform_finishes() {
        let mut run = event();
        run.roster.player_mut(1).unwrap().deck.clear();
        assert_eq!(run.choose_symbiote_option(42, 1, 0), Err("Symbiote Approach is locked"));
        assert!(run.symbiote_choices(42, 1).is_err());
        run.choose_symbiote_option(42, 1, 1).unwrap();
        assert!(run.choose_symbiote_option(42, 1, 1).is_err());
        assert_eq!(run.phase, MpPhase::Event);
    }
}
