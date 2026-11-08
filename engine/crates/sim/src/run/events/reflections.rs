use super::*;

pub(in crate::run) fn touch(run: &mut RunCtx, rng: &mut Rng) -> Result<(), &'static str> {
    let mut upgraded: Vec<_> = run.deck.iter().enumerate()
        .filter_map(|(i, c)| c.upgraded.then_some(i)).collect();
    for _ in 0..2 {
        if upgraded.is_empty() { break; }
        let index = upgraded.remove(rng.choice_index(upgraded.len()));
        RunManager::downgrade_event_card(run, index)?;
    }
    let mut candidates: Vec<_> = (0..run.deck.len())
        .filter(|&i| run.is_deck_card_upgradable(i)).collect();
    for _ in 0..4 {
        if candidates.is_empty() { break; }
        let index = candidates.remove(rng.choice_index(candidates.len()));
        run.upgrade_deck_card(index);
    }
    Ok(())
}

impl RunManager {
    pub(super) fn choose_reflections(&mut self, mut event: EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        match option.id {
            crate::identity::EventOptionId::ReflectionsInitialTouchAMirror => touch(self.run_ctx_mut(), &mut event.rng)?,
            crate::identity::EventOptionId::ReflectionsInitialShatter => {
                let originals = self.run_ctx().deck.clone();
                for card in originals {
                    let id = self.run_ctx_mut().new_card_id();
                    self.add_card_instance_to_deck(card.clone_with_id(id))?;
                }
                self.add_card_to_deck(CardId::BadLuck)?;
            }
            _ => return Err("invalid Reflections option"),
        }
        self.unported = None;
        self.phase = RunPhase::MapChoice;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflections_duplicates_mutable_deck_then_adds_bad_luck() {
        let mut run = RunManager::new(42, RunOptions::default());
        run.run_ctx_mut().deck.truncate(2);
        run.run_ctx_mut().upgrade_deck_card(0);
        let original = run.run_ctx().deck.clone();
        run.current_event = super::super::super::events::initialize(
            crate::identity::EventId::Reflections, 42, 0, 0, 80, 80);
        run.phase = RunPhase::Event;
        run.take_event_choice("shatter").unwrap();
        let deck = &run.run_ctx().deck;
        assert_eq!(deck.len(), 5);
        for i in 0..2 {
            assert_eq!(deck[i + 2].id, original[i].id);
            assert_eq!(deck[i + 2].upgraded, original[i].upgraded);
            assert_ne!(deck[i + 2].instance_id, original[i].instance_id);
        }
        assert_eq!(deck[4].id, CardId::BadLuck);
        assert_eq!(run.phase, RunPhase::MapChoice);
    }

    #[test]
    fn reflections_touch_reupgrades_downgraded_cards_and_handles_empty_deck() {
        let mut run = RunManager::new(43, RunOptions::default());
        run.run_ctx_mut().deck.truncate(2);
        for i in 0..2 { run.run_ctx_mut().upgrade_deck_card(i); }
        let mut rng = Rng::new(7);
        touch(run.run_ctx_mut(), &mut rng).unwrap();
        assert!(run.run_ctx().deck.iter().all(|c| c.upgraded));
        assert_eq!(rng.counter, 4);
        run.run_ctx_mut().deck.clear();
        touch(run.run_ctx_mut(), &mut rng).unwrap();
        assert_eq!(rng.counter, 4);
    }
}
