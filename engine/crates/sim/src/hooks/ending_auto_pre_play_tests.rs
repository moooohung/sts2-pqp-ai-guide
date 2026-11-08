use super::*;
use crate::{combat::ChoiceScheduling, rng::Rng, relic::Relic, run::{RunManager, RunOptions}};

#[test]
fn lethal_mayhem_finishes_its_started_tier_including_history_course_dupe_allocation() {
    for scheduling in [ChoiceScheduling::Live, ChoiceScheduling::Sequential] {
        for seats in 2..=4 {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.choice_scheduling = scheduling;
            let mut c = Combat::new(run, 1);
            for _ in 1..seats { c.add_player(1000, 1000, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.combat_started = true;
            c.in_play_phase = true;
            c.creatures[c.enemies[0] as usize].hp = 1;
            c.players[0].draw.clear();
            c.players[0].hand.clear();
            c.players[0].turn_number = 2;
            let prior = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
            c.hist.finished_plays.push((0, 1, prior));
            let top = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
            c.players[0].draw.push(top);
            c.apply_power_to(0, PowerId::Mayhem, 1, Some(0));
            let mut relic = Relic::new(RelicId::HistoryCourse);
            relic.owner = 0;
            c.run.relics.push(relic);
            let allocated = c.cards.len();
            let dispatch = |c: &mut Combat| {
                if scheduling == ChoiceScheduling::Live {
                    c.fire_setup_hooks(0, &[Hook::AfterAutoPrePlayPhaseEntered],
                        crate::combat::TurnStage::AutoPrePlay { next_player: 0, before_play: true });
                } else {
                    c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(0), None);
                }
            };
            dispatch(&mut c);
            assert!(c.is_ending && !c.is_over);
            // HistoryCourse.cs:21-24 creates the dupe BEFORE AutoPlay rejects
            // ending combat. Thus allocation still occurs, but no second play.
            assert_eq!(c.cards.len(), allocated + 1);
            assert_eq!(c.hist.play_start_cards.as_slice(), &[top]);
            assert_eq!(c.fault, None);
            dispatch(&mut c); // a NEW tier/hook invocation must now stop
            assert_eq!(c.cards.len(), allocated + 1);
        }
    }
}

#[test]
fn resumed_lethal_autoplay_stops_later_history_course_before_victory_check() {
    for scheduling in [ChoiceScheduling::Live, ChoiceScheduling::Sequential] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.choice_scheduling = scheduling;
        let mut c = Combat::new(run, 1);
        c.add_player(1000, 1000, 3, 0, &[]);
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.combat_started = true;
        c.in_play_phase = true; // the enclosing phase has not checked victory yet
        c.players[0].hand.clear();
        let e = c.enemies[0];
        c.creatures[e as usize].hp = 3;
        for _ in 0..2 {
            let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
            c.players[0].discard.push(k);
        }
        let k = c.new_card(0, CardId::Headbutt, false).unwrap();
        // Headbutt's pending selection survives a lethal hit in an already-running
        // callback. Present it before a second child ends the combat.
        c.creatures[e as usize].hp = 12;
        c.auto_play_card(k, Some(e), false);
        assert!(c.pending_choice.is_some());
        let strike = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.autoplay_snapshots.push((0, crate::autoplay::AutoplayWork::Snapshot(vec![strike], false)));
        assert!(c.resolve_pending_choice_for_player(0, Some(0)));
        assert!(c.is_ending && !c.is_over);
        let mut relic = Relic::new(RelicId::HistoryCourse);
        relic.owner = 1;
        c.run.relics.push(relic);
        c.players[1].turn_number = 2;
        let previous = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
        c.hist.finished_plays.push((1, 1, previous));
        let allocated = c.cards.len();
        let started = c.hist.play_start_cards.len();
        if scheduling == ChoiceScheduling::Live {
            c.fire_setup_hooks(1, &[Hook::AfterAutoPrePlayPhaseEntered],
                crate::combat::TurnStage::AutoPrePlay { next_player: 1, before_play: true });
        } else {
            c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(1), None);
        }
        assert_eq!(c.cards.len(), allocated);
        assert_eq!(c.hist.play_start_cards.len(), started);
        assert_eq!(c.fault, None);
    }
}
