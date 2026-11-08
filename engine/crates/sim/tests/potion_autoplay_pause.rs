use sts2_sim::{combat::Combat, relic::Relic, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PotionId, PowerId, RelicId}};

#[test]
fn distilled_chaos_owner_awaits_autoplays_before_potion_hook_but_teammate_does_not() {
    for seats in [1, 4] {
    for owner in 0..seats {
        for self_target in [true, false] {
            if seats == 1 && !self_target { continue; }
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            let mut relic = Relic::new(RelicId::ReptileTrinket);
            relic.owner = owner;
            c.run.relics.push(relic);
            let target = if self_target { owner } else { (owner + 1) % 4 };
            c.apply_power_to(target, PowerId::Stratagem, 1, Some(target));
            for _ in 0..4 {
                let k = c.new_card(target, CardId::StrikeIronclad, false).unwrap();
                c.players[target].discard.push(k);
            }
            if owner == 0 { c.run.potions = vec![Some(PotionId::Distilledchaos), None, None]; }
            else { c.players[owner].potions = Some(vec![Some(PotionId::Distilledchaos), None, None]); }
            let enemy = c.enemies[0];
            let hp_before = c.cr(enemy).hp;
            assert!(c.use_potion_for_player(owner, 0, Some(target as usize)));
            assert!(c.hist.played_combat.is_empty());
            assert_eq!(c.pending_choice.as_ref().unwrap().player, target);
            assert_eq!(c.fault, None);
            assert_eq!(c.power_amount(owner, PowerId::ReptileTrinket), if self_target { 0 } else { 3 });
            assert!(c.resolve_pending_choice_for_player(target, Some(0)));
            assert_eq!(c.hist.played_combat.len(), 3);
            // StrikeIronclad deals 6. The owner's +3 Strength must arrive
            // after all three Strikes, not turn this into 3 * 9 damage.
            assert_eq!(hp_before - c.cr(enemy).hp, 3 * 6);
            assert_eq!(c.power_amount(owner, PowerId::ReptileTrinket), 3);
            assert_eq!(c.fault, None);
        }
    }
    }
}

#[test]
fn self_choice_potions_await_owner_selection_before_after_potion_relics() {
    for seats in 1..=4 {
        for owner in 0..seats {
            for id in [PotionId::Gamblersbrew, PotionId::Attackpotion, PotionId::Skillpotion,
                PotionId::Powerpotion, PotionId::Colorlesspotion, PotionId::Ashwater] {
                let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
                run.deck.clear();
                let mut c = Combat::new(run, 1);
                for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
                c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
                c.start_combat();
                c.players[owner].character = Some(sts2_sim::character::Character::Ironclad);
                let mut relic = Relic::new(RelicId::ReptileTrinket);
                relic.owner = owner;
                c.run.relics.push(relic);
                for _ in 0..2 {
                    let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                    c.players[owner].hand.push(k);
                    let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    c.players[owner].draw.push(k);
                }
                if owner == 0 { c.run.potions = vec![Some(id), None, None]; }
                else { c.players[owner].potions = Some(vec![Some(id), None, None]); }
                assert!(c.use_potion_for_player(owner, 0, None), "{id:?}");
                assert_eq!(c.fault, None, "{id:?}");
                assert_eq!(c.power_amount(owner, PowerId::ReptileTrinket), 0, "{id:?}");
                assert!(c.pending_choice_for_player(owner).is_some(), "{id:?}");
                assert!(c.resolve_pending_choice_for_player(owner, Some(0)), "{id:?}");
                if c.pending_choice_for_player(owner).is_some() {
                    assert!(c.resolve_pending_choice_for_player(owner, None), "{id:?}");
                }
                assert!(c.pending_choice_for_player(owner).is_none(), "{id:?}");
                assert_eq!(c.power_amount(owner, PowerId::ReptileTrinket), 3, "{id:?}");
                assert_eq!(c.fault, None, "{id:?}");
            }
        }
    }
}
