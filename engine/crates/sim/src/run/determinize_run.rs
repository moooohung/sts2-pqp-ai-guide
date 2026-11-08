//! Run-level part of search determinization (see `crate::determinize`). A search
//! leaf can lie past the combat's end (its rewards screen), so the streams and
//! bags that generate post-combat content are re-sampled too: the relic bags'
//! remaining contents are kept, their hidden draw order is not. Rooms not entered
//! yet (the current act's remaining events, normal and elite encounters; every later
//! act's, bosses included) are re-drawn; the current act's boss and the ancients stay.
use super::RunManager;
use crate::determinize::{hidden_order, run_rngs, stream};

impl RunManager {
    /// Direct combat access for search tests that edit hidden state.
    pub fn combat_mut(&mut self) -> Option<&mut crate::combat::Combat> {
        self.combat.as_mut()
    }

    /// Solo: the viewer is seat 0.
    pub fn determinize_hidden(&mut self, seed: u64) -> Result<(), &'static str> {
        if let Some(combat) = &mut self.combat {
            combat.determinize_hidden(seed, 0)?;
        }
        if let Some(run) = &mut self.run {
            run_rngs(&mut run.rngs, seed);
        }
        self.manager_rng = stream(seed, "manager");
        for (act, rng) in self.map_rngs.iter_mut().enumerate() {
            *rng = stream(seed.wrapping_add(act as u64), "map");
        }
        let bags = &mut self.relic_bags;
        for (index, bag) in bags.shared.iter_mut().chain(bags.player.iter_mut().flatten()).enumerate() {
            hidden_order(bag, &mut stream(seed.wrapping_add(index as u64), "relic_bag"));
        }
        for (index, reward) in std::iter::once(&mut self.reward).chain(self.reward_parents.iter_mut().map(|(r, _)| r)).enumerate() {
            if let Some(rng) = &mut reward.sphere_rng {
                *rng = stream(seed.wrapping_add(index as u64), "crystal_sphere");
            }
        }
        let mut rooms = stream(seed, "rooms");
        let current = self.current_act;
        for (index, act) in self.acts.iter_mut().enumerate().skip(current) {
            let entered = (index == current).then_some((act.event_cursor, act.normal_cursor, act.elite_cursor));
            act.rooms.resample_unentered(act.def, &mut rooms, false, entered);
        }
        if let Some(event) = &mut self.current_event {
            event.rng = stream(seed, "event");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{RunManager, RunOptions};
    use crate::rng::Rng;

    fn sample(run: &RunManager) -> String {
        let mut run = run.clone();
        run.determinize_hidden(5).unwrap();
        format!("{run:?}")
    }

    /// Every hidden run-level field, changed on its own, leaves the determinized state unchanged.
    #[test]
    fn each_hidden_field_alone_never_changes_the_sample() {
        let base = RunManager::new_for_character(20261108, RunOptions { ascension: 10, saved_wongo_points: Some(0),
            ..RunOptions::default() }, crate::run::mp::Character::Ironclad);
        let expected = sample(&base);
        let edits: Vec<(&str, fn(&mut RunManager))> = vec![
            ("manager_rng", |r| r.manager_rng = Rng::new(1)),
            ("map_rngs", |r| { r.map_rngs[1].next_unsigned_long_raw(); }),
            ("run rngs", |r| { let rngs = &mut r.run.as_mut().unwrap().rngs; rngs.rewards.next_unsigned_long_raw();
                    rngs.up_front.next_unsigned_long_raw(); rngs.shops.next_unsigned_long_raw(); }),
            ("relic bag order", |r| r.relic_bags.shared[0].reverse()),
            ("player relic bag order", |r| r.relic_bags.player[0][1].reverse()),
            ("unentered normal encounters", |r| { let act = r.current_act; r.acts[act].rooms.normal[1..].reverse(); }),
            ("unentered events", |r| { let act = &mut r.acts[r.current_act]; let from = act.event_cursor;
                    act.rooms.events[from..].reverse(); }),
            ("later act rooms", |r| { r.acts[2].rooms.elite.reverse(); r.acts[1].rooms.events.reverse(); }),
            ("later act boss", |r| { let other = r.acts[1].def.bosses.iter().copied()
                    .find(|&b| b != r.acts[1].rooms.boss).unwrap(); r.acts[1].rooms.boss = other; }),
        ];
        for (name, edit) in edits {
            let mut edited = base.clone();
            edit(&mut edited);
            assert_ne!(format!("{edited:?}"), format!("{base:?}"), "{name}: the edit must change the state");
            assert_eq!(sample(&edited), expected, "{name} leaks into the sample");
        }
        let mut run = base.clone();
        run.determinize_hidden(9).unwrap();
        assert_eq!(run.acts[0].rooms.boss, base.acts[0].rooms.boss, "the act's boss is on the map");
    }
}
