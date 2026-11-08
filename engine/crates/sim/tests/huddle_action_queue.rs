use sts2_sim::{
    combat::{ChoiceScheduling, Combat}, relic::Relic, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, PotionId, PowerId, RelicId},
};

fn combat(seats: u8) -> Combat {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.choice_scheduling = ChoiceScheduling::Live;
    run.deck.clear();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(100, 100, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c
}

fn discard(c: &mut Combat, player: u8) {
    for _ in 0..4 {
        let k = c.new_card(player, CardId::StrikeIronclad, false).unwrap();
        c.players[player].discard.push(k);
    }
}

fn answer_teammates(c: &mut Combat, owner: u8, seats: u8) {
    assert_eq!(c.fault, None);
    for player in (0..seats).rev() {
        if player == owner { continue; }
        assert!(c.pending_choice_for_player(player).is_some());
        assert!(c.resolve_pending_choice_for_player(player, Some(0)));
        assert_eq!(c.fault, None);
    }
    assert!(c.pending_choice.is_none());
}

#[test]
fn distilled_chaos_and_standalone_batches_dispatch_huddle_choices_only_after_action_completion() {
    for seats in 2..=4 {
        for owner in 0..seats {
            for potion in [false, true] {
                let mut c = combat(seats);
                for player in 0..seats {
                    discard(&mut c, player);
                    if player != owner {
                        c.apply_power_to(player, PowerId::Stratagem, 1, Some(player));
                    }
                }
                for id in [CardId::HuddleUp, CardId::DefendIronclad, CardId::DefendIronclad] {
                    let k = c.new_card(owner, id, false).unwrap();
                    c.players[owner].draw.push(k);
                }
                if potion {
                    let mut relic = Relic::new(RelicId::ReptileTrinket);
                    relic.owner = owner;
                    c.run.relics.push(relic);
                    if owner == 0 { c.run.potions = vec![Some(PotionId::Distilledchaos), None, None]; }
                    else { c.players[owner].potions = Some(vec![Some(PotionId::Distilledchaos), None, None]); }
                }
                c.on_choice_presented = Some(|c| {
                    // Huddle's first queued selector cannot interrupt the remaining
                    // two autoplays; each Defend supplies 5 block.
                    assert_eq!(c.hist.played_combat.len(), 3);
                    assert!(c.pending_play.is_empty());
                    assert!(c.players.0.iter().enumerate().any(|(seat, p)|
                        p.play.is_empty() && c.cr(seat as u8).block == 10));
                    if let Some(relic) = c.run.relics.iter().find(|r| r.id == RelicId::ReptileTrinket) {
                        assert_eq!(c.power_amount(relic.owner, PowerId::ReptileTrinket), 3);
                    }
                });
                if potion { assert!(c.use_potion_for_player(owner, 0, Some(owner as usize))); }
                else { c.auto_play_from_draw(owner, 3, false); }
                answer_teammates(&mut c, owner, seats);
                assert_eq!(c.hist.played_combat.len(), 3);
            }
        }
    }
}

#[test]
fn end_turn_mayhem_huddle_advances_to_teammate_choices_without_another_action() {
    for seats in 2..=4 {
        for owner in 0..seats {
            let mut c = combat(seats);
            for player in 0..seats {
                for _ in 0..5 {
                    let k = c.new_card(player, CardId::DefendIronclad, false).unwrap();
                    c.players[player].draw.push(k);
                }
                if player != owner {
                    discard(&mut c, player);
                    c.apply_power_to(player, PowerId::Stratagem, 1, Some(player));
                }
            }
            let k = c.new_card(owner, CardId::HuddleUp, false).unwrap();
            c.players[owner].draw.push(k);
            c.apply_power_to(owner, PowerId::Mayhem, 1, Some(owner));
            c.end_player_turn();
            assert_eq!(c.hist.played_combat.len(), 1);
            answer_teammates(&mut c, owner, seats);
        }
    }
}
