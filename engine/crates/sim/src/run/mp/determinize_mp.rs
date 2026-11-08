//! Multiplayer part of search determinization (see `crate::determinize`), from the
//! `viewer` seat's knowledge. Re-sampled from `seed` alone:
//! - streams: the combat's (`Combat::determinize_hidden`), the shared `RunRngSet`,
//!   every act's map stream, each seat's `PlayerRngSet`, each seat's Neow event
//!   stream, the map-vote sampler (`MapSelectionSynchronizer`), the shared-event
//!   vote sampler and each seat's current event stream;
//! - orders: the shared and per-seat relic bags and each seat's relic fallback queue
//!   (`RelicGrabBag._mpFallbackDequeue`; first-match pulls make its order matter);
//! - rooms not entered yet: the current act's remaining events and normal and elite
//!   encounters, and every later act's (bosses included).
//!
//! Kept: everything shown (piles' contents, intents, rewards and offers on screen,
//! votes cast, maps), the current act's boss, ancients, and content generated for a
//! screen still to come that a search line never opens (it ends at the viewer's first
//! decision after the combat): queued Neow, relic and event rewards, crystal-sphere
//! loot, treasure relics. Earlier acts' unused rooms are never read again.
use super::MpRun;
use crate::determinize::{hidden_order, run_rngs, stream};

impl MpRun {
    /// Refused, with nothing changed, while a seat other than `viewer` resolves a card choice
    /// (`Combat::determinize_hidden`).
    pub fn determinize_hidden(&mut self, seed: u64, viewer: usize) -> Result<(), &'static str> {
        if let Some(combat) = &mut self.combat {
            combat.determinize_hidden(seed, viewer)?;
        }
        run_rngs(&mut self.world.run.rngs, seed);
        for (act, rng) in self.world.map_rngs.iter_mut().enumerate() {
            *rng = stream(seed.wrapping_add(act as u64), "map");
        }
        let world = &mut self.world;
        for (index, bag) in world.shared_bags.iter_mut().chain(world.player_bags.iter_mut().flatten())
            .chain(world.player_fallbacks.iter_mut()).enumerate() {
            hidden_order(bag, &mut stream(seed.wrapping_add(index as u64), "relic_bag"));
        }
        let act = world.run.act_index.max(0) as usize;
        let mut rooms = stream(seed, "rooms");
        world.encounters.resample_unentered(world.act_defs[act], &mut rooms, true,
            Some((self.event_cursor, self.normal_cursor, self.elite_cursor)));
        world.encounters_by_act[act] = world.encounters.clone();
        for later in act + 1..world.encounters_by_act.len() {
            world.encounters_by_act[later].resample_unentered(world.act_defs[later], &mut rooms, true, None);
        }
        for seat in 0..self.roster.players().len() {
            let rngs = &mut self.roster.player_mut(seat).unwrap().rngs;
            let seat_seed = seed.wrapping_add(seat as u64);
            rngs.seed = seat_seed;
            rngs.rewards = stream(seat_seed, "rewards");
            rngs.shops = stream(seat_seed, "shops");
            rngs.transformations = stream(seat_seed, "transformations");
        }
        for (seat, rng) in self.neow_rngs.iter_mut().enumerate() {
            *rng = stream(seed.wrapping_add(seat as u64), "neow");
        }
        self.voting.rng = stream(seed, "map_point_selection");
        self.shared_event_selection_rng = stream(seed, "shared_event_selection");
        for (seat, event) in self.events.iter_mut().enumerate() {
            if let Some(event) = event {
                event.rng = stream(seed.wrapping_add(seat as u64), "event");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::run::mp::{Character, MpRun, PlayerSpec};
    use crate::rng::Rng;
    use crate::tables::RelicId;

    fn party() -> MpRun {
        let specs: Vec<PlayerSpec> = [Character::Ironclad, Character::Silent].iter().enumerate()
            .map(|(i, &character)| PlayerSpec { net_id: 1 + i as u64, character }).collect();
        MpRun::from_numeric_seed(20261108, &specs, 10).unwrap()
    }
    fn sample(run: &MpRun) -> String {
        let mut run = run.clone();
        run.determinize_hidden(5, 0).unwrap();
        format!("{run:?}")
    }

    /// Every hidden field, changed on its own, leaves the determinized state unchanged.
    #[test]
    fn each_hidden_field_alone_never_changes_the_sample() {
        let base = party();
        let expected = sample(&base);
        let edits: Vec<(&str, fn(&mut MpRun))> = vec![
            ("voting.rng", |r| r.voting.rng = Rng::new(1)),
            ("neow_rngs", |r| r.neow_rngs[1] = Rng::new(2)),
            ("player_fallbacks", |r| { r.world.player_fallbacks[0] = vec![RelicId::Anchor, RelicId::Vajra];
                    r.world.player_fallbacks[1] = vec![RelicId::Vajra, RelicId::Anchor]; }),
            ("shared_event_selection_rng", |r| r.shared_event_selection_rng = Rng::new(3)),
            ("run rngs", |r| { r.world.run.rngs.up_front.next_unsigned_long_raw(); r.world.run.rngs.monster_ai.next_unsigned_long_raw(); }),
            ("map_rngs", |r| { r.world.map_rngs[2].next_unsigned_long_raw(); }),
            ("player rngs", |r| { r.roster.player_mut(1).unwrap().rngs.rewards.next_unsigned_long_raw(); }),
            ("relic bag order", |r| r.world.player_bags[1][0].reverse()),
            ("shared bag order", |r| r.world.shared_bags[1].reverse()),
            ("unentered normal encounters", |r| r.world.encounters.normal[1..].reverse()),
            ("unentered elites", |r| r.world.encounters.elite.reverse()),
            ("unentered events", |r| r.world.encounters.events[2..].reverse()),
            ("later act rooms", |r| { r.world.encounters_by_act[2].normal.reverse(); r.world.encounters_by_act[1].elite.reverse(); }),
            ("later act boss", |r| { let other = r.world.act_defs[1].bosses.iter().copied()
                    .find(|&b| b != r.world.encounters_by_act[1].boss).unwrap(); r.world.encounters_by_act[1].boss = other; }),
        ];
        // The fallback edit needs the same multiset on both sides of the comparison.
        let mut fallback_base = base.clone();
        fallback_base.world.player_fallbacks[0] = vec![RelicId::Vajra, RelicId::Anchor];
        fallback_base.world.player_fallbacks[1] = vec![RelicId::Anchor, RelicId::Vajra];
        for (name, edit) in edits {
            let (reference, expected) = if name == "player_fallbacks" { let e = sample(&fallback_base); (fallback_base.clone(), e) }
                else { (base.clone(), expected.clone()) };
            let mut edited = reference.clone();
            edit(&mut edited);
            assert_ne!(format!("{edited:?}"), format!("{reference:?}"), "{name}: the edit must change the state");
            assert_eq!(sample(&edited), expected, "{name} leaks into the sample");
        }
        // Visible state is kept: the act's boss and the cursors' history.
        let mut run = base.clone();
        run.determinize_hidden(9, 1).unwrap();
        assert_eq!(run.world.encounters.boss, base.world.encounters.boss);
        assert_eq!(run.world.encounters.normal[..run.normal_cursor], base.world.encounters.normal[..base.normal_cursor]);
    }
}
