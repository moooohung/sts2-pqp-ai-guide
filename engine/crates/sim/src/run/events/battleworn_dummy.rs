use super::*;

pub(in crate::run) const ENCOUNTERS: [crate::identity::EncounterId; 3] = [
    crate::identity::EncounterId::BattlewornDummyEventV1Encounter,
    crate::identity::EncounterId::BattlewornDummyEventV2Encounter,
    crate::identity::EncounterId::BattlewornDummyEventV3Encounter,
];

pub(in crate::run) fn setting(option: EventOptionIdentity) -> Result<usize, &'static str> {
    match option.id {
        crate::identity::EventOptionId::BattlewornDummyInitialSetting1 => Ok(0),
        crate::identity::EventOptionId::BattlewornDummyInitialSetting2 => Ok(1),
        crate::identity::EventOptionId::BattlewornDummyInitialSetting3 => Ok(2),
        _ => Err("invalid Battleworn Dummy setting") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Side;

    fn manager(option: &str) -> RunManager {
        let mut run = RunManager::new(42, RunOptions::default());
        run.unported = None;
        run.phase = RunPhase::Event;
        run.current_event = super::super::super::events::initialize(crate::identity::EventId::BattlewornDummy,42,0,0,80,80);
        run.take_event_choice(option).unwrap();
        run
    }

    #[test]
    fn battleworn_dummy_times_out_after_three_enemy_turns_without_rewards() {
        let mut run = manager("setting_1");
        let gold = run.run_ctx().gold;
        let c = run.combat.as_mut().unwrap();
        assert_eq!(c.fault, None);
        let enemy = c.enemies[0];
        assert_eq!(c.cr(enemy).hp, 75);
        for _ in 0..2 {
            c.fire_after_turn_end(Side::Enemy);
            assert!(!c.cr(enemy).escaped);
        }
        c.fire_after_turn_end(Side::Enemy);
        assert!(c.cr(enemy).escaped);
        assert_eq!(c.cr(enemy).hp, 75);
        assert!(c.is_over && c.won);
        run.after_combat_action().unwrap();
        assert_eq!(run.phase, RunPhase::MapChoice);
        assert!(run.reward.items.is_empty());
        assert_eq!(run.run_ctx().gold, gold);
    }

    #[test]
    fn battleworn_dummy_victories_grant_only_the_selected_setting_reward() {
        for (option, hp, upgrades) in [("setting_1",75,0),("setting_2",150,2),("setting_3",300,0)] {
            let mut run = manager(option);
            let gold = run.run_ctx().gold;
            let c = run.combat.as_mut().unwrap();
            let enemy = c.enemies[0];
            assert_eq!(c.cr(enemy).hp,hp);
            c.force_kill_creature(enemy);
            c.check_combat_end();
            run.after_combat_action().unwrap();
            assert_eq!(run.unported,None);
            assert_eq!(run.run_ctx().gold,gold);
            assert_eq!(run.run_ctx().deck.iter().filter(|c|c.upgraded).count(),upgrades);
            match option {
                "setting_1" => assert!(matches!(run.reward.items[0].kind,RewardItemKind::Potion(_))),
                "setting_3" => assert!(matches!(run.reward.items[0].kind,RewardItemKind::Relic(_))),
                _ => assert_eq!(run.phase,RunPhase::MapChoice),
            }
        }
    }
}

impl RunManager {
    pub(super) fn choose_battleworn_dummy(&mut self, mut event: EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let tier = setting(option)?;
        event.ints[0] = tier as i32;
        self.prepare_event_combat(ENCOUNTERS[tier])?;
        self.start_prepared_event_combat(ENCOUNTERS[tier])?;
        self.current_event = Some(event);
        Ok(())
    }

    pub(super) fn resume_battleworn_dummy(&mut self) -> Result<(), &'static str> {
        let mut event = self.current_event.take().ok_or("Dummy event missing")?;
        self.reward = RewardState::default();
        let timed_out = !self.combat.as_ref().unwrap().escaped_enemies.is_empty();
        if !timed_out {
            let kind = match event.ints[0] {
                0 => self.roll_event_potion().map(RewardItemKind::Potion),
                1 => { self.upgrade_random_event_cards(&mut event.rng, 2); None }
                2 => Some(RewardItemKind::Relic(self.pull_player_relic(None, false))),
                _ => return Err("invalid Dummy reward"),
            };
            if let Some(kind) = kind {
                self.reward.items.push(RewardItem {kind, consumed:false, skippable:true,
                    skip_completes:true, add_injury_after:false, neows_bones_final:false});
            }
            self.append_combat_end_rewards();
        }
        self.phase = RunPhase::CardReward;
        self.advance_rewards();
        Ok(())
    }
}
