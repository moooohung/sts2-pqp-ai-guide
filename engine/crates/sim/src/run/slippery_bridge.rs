use super::*;

pub(in crate::run) fn roll_bridge_card(run: &crate::combat::RunCtx, event: &mut EventState) -> Result<(), &'static str> {
        // C# SlipperyBridge.GetNewRandomCard: first prefer non-Basic cards;
        // subsequently exclude the previous TYPE and every skipped INSTANCE.
        let mut removable = RunManager::removable_card_indices(&run.deck);
        removable.sort_unstable(); // C# Deck.Cards.Where retains deck order.
        let previous_type = event.bridge.current.and_then(|id| run.deck.iter()
            .find(|card| card.instance_id == id).map(|card| card.id));
        if let Some(previous) = event.bridge.current {
            event.bridge.skipped.push(previous);
        }
        let candidates: Vec<usize> = removable.iter().copied().filter(|&index| {
            let card = &run.deck[index];
            if let Some(previous) = previous_type {
                card.id != previous && !event.bridge.skipped.contains(&card.instance_id)
            } else {
                card.rarity != CardRarity::Basic
            }
        }).collect();
        let candidates = if candidates.is_empty() { &removable } else { &candidates };
        if candidates.is_empty() { return Err("Slippery Bridge requires a removable card"); }
        let index = candidates[event.rng.choice_index(candidates.len())];
        event.bridge.current = Some(run.deck[index].instance_id);
        event.options.clear();
        if event.bridge.hold_ons > 0 {
            event.options.set_page(crate::identity::EventPageId::SlipperyBridgeHoldOnHoldOnSuffix,
                super::super::events::native_identity::EventParameter::Bridge(
                    (event.bridge.hold_ons - 1 < 7).then_some((event.bridge.hold_ons - 1) as u8)));
        }
        event.options.push(EventOption::new("overcome", "Overcome", "Lose a random card"));
        let damage = 3 + event.bridge.hold_ons;
        event.options.push(EventOption::new("hold_on", "Hold On", "")
            .with_dynamic_description(EventDescription::BridgeHoldOn(damage)));
        Ok(())
}

pub(in crate::run) fn choose_bridge(run: &mut crate::combat::RunCtx, event: &mut EventState, option: EventOptionIdentity, multiplayer: bool)
    -> Result<bool, &'static str> {
    if option.is(crate::identity::EventOptionId::SlipperyBridgeInitialOvercome) {
        let id = event.bridge.current.ok_or("Slippery Bridge missing offered card")?;
        run.retain_run_deck_cards(|card| card.instance_id != id);
        return Ok(true);
    }
    if !matches!(option.id,
        crate::identity::EventOptionId::SlipperyBridgeInitialHoldOn0 |
        crate::identity::EventOptionId::SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2)
    { return Err("invalid Slippery Bridge option"); }
    damage_event_player(run, 3 + event.bridge.hold_ons, multiplayer)?;
    if run.hp <= 0 { return Ok(true); }
    event.bridge.hold_ons += 1;
    roll_bridge_card(run, event)?;
    Ok(false)
}

/// CreatureCmd.Damage outside combat: shared with the solo event reducer.
pub(in crate::run) fn damage_event_player(run: &mut crate::combat::RunCtx, amount: i32, multiplayer: bool) -> Result<(), &'static str> {
    if run.hp<=0 {return Ok(());}
    let reduction = i32::from(run.owned_relics(crate::combat::PLAYER)
        .any(|r| r.id == RelicId::TungstenRod && r.enabled && !r.is_melted));
    let damage = (amount - reduction).max(0);
    // CreatureCmd.KillWithoutCheckingWinCondition restores one HP outside
    // multiplayer combat, before BeforeDeath/ShouldDie can consume revivers.
    if multiplayer {
        if damage >= run.hp {
            run.hp = 1;
            return Ok(());
        }
    }
    run.hp = (run.hp - damage).max(0);
    if run.hp==0 && run.max_hp>0 {
        // ShouldDie runs before ShouldDieLate, even while Stone locks potion UI.
        if let Some(slot)=run.potions.iter().position(|p|*p==Some(PotionId::Fairyinabottle)) {
            run.potions[slot]=None;
            run.hp=((i64::from(run.max_hp)*3/10).max(1) as i32).min(run.max_hp);
        } else if let Some(tail)=run.relics.iter_mut().find(|r|r.owner==crate::combat::PLAYER
            && r.id==RelicId::LizardTail && r.enabled && !r.is_melted && !r.flag1) {
            tail.flag1=true;
            run.hp=(run.max_hp/2).max(1).min(run.max_hp);
        }
    }
    Ok(())
}

impl RunManager {
    pub(super) fn roll_bridge_card(&self, event: &mut EventState) -> Result<(), &'static str> {
        roll_bridge_card(self.run_ctx(), event)
    }

    pub(super) fn choose_bridge(&mut self, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        self.current_event = Some(event.clone());
        let multiplayer = self.is_multiplayer();
        let finished = match choose_bridge(self.run_ctx_mut(), &mut event, option, multiplayer) {
            Ok(finished) => finished,
            Err(reason) => { self.unported = Some(reason); return Err(reason); }
        };
        if self.run_ctx().hp <= 0 || finished {
            self.current_event = None;
            self.phase = if self.run_ctx().hp <= 0 { RunPhase::RunOver } else { RunPhase::MapChoice };
            return Ok(());
        }
        self.current_event = Some(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> RunManager {
        let mut mgr = RunManager::new(4100103, RunOptions::default());
        mgr.phase = RunPhase::Event;
        mgr.add_card_to_deck(CardId::from_name("HEADBUTT").unwrap()).unwrap();
        let mut event = EventState::new(crate::identity::EventId::SlipperyBridge,
            Rng::new(mgr.run_ctx().rngs.seed.wrapping_add(deterministic_hash64("SLIPPERY_BRIDGE"))));
        mgr.roll_bridge_card(&mut event).unwrap();
        mgr.current_event = Some(event);
        mgr
    }

    #[test]
    fn bridge_initial_nonbasic_then_excludes_type_and_removes_exact_instance() {
        let mut mgr = manager();
        let event = mgr.current_event.as_ref().unwrap();
        let first = event.bridge.current.unwrap();
        let (damage,shown)=mgr.native_bridge_offer().unwrap();
        assert_eq!(damage,3);
        assert_eq!(shown.id,CardId::Headbutt);
        assert_eq!(shown.instance_id,first);
        assert_eq!(mgr.run_ctx().deck.iter().find(|card| card.instance_id == first)
            .unwrap().id, CardId::from_name("HEADBUTT").unwrap());
        let hp = mgr.run_ctx().hp;
        let rng = event.rng.counter;
        mgr.take_event_choice("hold_on").unwrap();
        assert_eq!(mgr.run_ctx().hp, hp - 3);
        let event = mgr.current_event.as_ref().unwrap();
        let second = event.bridge.current.unwrap();
        let (damage,shown)=mgr.native_bridge_offer().unwrap();
        assert_eq!(damage,4);
        assert_eq!(shown.instance_id,second);
        assert_ne!(first, second);
        assert_eq!(event.bridge.skipped, vec![first]);
        assert_eq!(event.rng.counter, rng + 1);
        assert_eq!(event.options[1].dynamic_description.unwrap().render(), "Take 4 damage, reroll card");
        assert_eq!(event.options.page.id, crate::identity::EventPageId::SlipperyBridgeHoldOnHoldOnSuffix);
        assert_eq!(event.options.page.parameter, crate::run::EventParameter::Bridge(Some(0)));
        assert_eq!(event.options[1].option_id.parameter, crate::run::EventParameter::Bridge(Some(1)));
        let len = mgr.run_ctx().deck.len();
        mgr.take_event_choice("overcome").unwrap();
        assert_eq!(mgr.phase, RunPhase::MapChoice);
        assert_eq!(mgr.run_ctx().deck.len(), len - 1);
        assert!(mgr.run_ctx().deck.iter().any(|card| card.instance_id == first));
        assert!(!mgr.run_ctx().deck.iter().any(|card| card.instance_id == second));
    }

    #[test]
    fn bridge_exhausted_candidates_fall_back_to_all_removable_instances() {
        let mut mgr = manager();
        let id = mgr.current_event.as_ref().unwrap().bridge.current.unwrap();
        mgr.run_ctx_mut().deck.retain(|card| card.instance_id == id);
        mgr.take_event_choice("hold_on").unwrap();
        assert_eq!(mgr.current_event.as_ref().unwrap().bridge.current, Some(id));
        mgr.take_event_choice("hold_on").unwrap();
        assert_eq!(mgr.current_event.as_ref().unwrap().bridge.hold_ons, 2);
    }

    #[test]
    fn bridge_fatal_damage_does_not_roll_another_card() {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 3;
        mgr.take_event_choice("hold_on").unwrap();
        assert_eq!(mgr.phase, RunPhase::RunOver);
        assert!(mgr.current_event.is_none());
    }
}
