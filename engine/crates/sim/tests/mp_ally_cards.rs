use sts2_sim::{
    combat::Combat,
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

#[test]
fn battle_trance_finishes_suspended_draw_before_applying_no_draw() {
    let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
    run.deck.clear();run.relics.clear();
    let mut c=Combat::new(run,1);
    c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
    c.start_combat();c.players[0].draw.clear();c.players[0].hand.clear();
    for _ in 0..5 {
        let k=c.new_card(0,CardId::DefendIronclad,false).unwrap();c.players[0].discard.push(k);
    }
    c.apply_power_to(0,PowerId::Stratagem,1,Some(0));
    let card=c.new_card(0,CardId::BattleTrance,false).unwrap();c.players[0].hand.push(card);
    assert!(c.play_card(0,0,None));
    assert_eq!(c.fault,None);
    assert!(!c.has_power(0,PowerId::NoDraw));
    assert!(c.resolve_pending_choice_for_player(0,Some(0)));
    assert_eq!(c.players[0].hand.len(),4);
    assert_eq!(c.power_amount(0,PowerId::NoDraw),1);
    assert_eq!(c.fault,None);
}

#[test]
fn glow_awaits_draw_before_adding_next_turn_draw_and_uses_upgraded_stars() {
    for upgraded in [false,true] {
        let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
        run.deck.clear();run.relics.clear();
        let mut c=Combat::new(run,1);
        c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
        c.start_combat();c.players[0].draw.clear();c.players[0].hand.clear();
        for _ in 0..3 {
            let k=c.new_card(0,CardId::DefendIronclad,false).unwrap();c.players[0].discard.push(k);
        }
        c.apply_power_to(0,PowerId::Stratagem,1,Some(0));
        let card=c.new_card(0,CardId::Glow,upgraded).unwrap();c.players[0].hand.push(card);
        assert!(c.play_card(0,0,None));
        assert_eq!(c.players[0].stars,if upgraded{2}else{1});
        assert!(!c.has_power(0,PowerId::DrawCardsNextTurn));
        assert_eq!(c.fault,None);
        assert!(c.resolve_pending_choice_for_player(0,Some(0)));
        assert_eq!(c.players[0].hand.len(),2);
        assert_eq!(c.power_amount(0,PowerId::DrawCardsNextTurn),1);
        assert_eq!(c.fault,None);
    }
}

#[test]
fn constellation_awaits_targets_shuffle_choice_before_energy_and_block() {
    let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
    run.deck.clear();run.relics.clear();
    let mut c=Combat::new(run,1);
    c.add_player(50,50,3,0,&[]);
    c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
    c.start_combat();
    c.players[1].draw.clear();c.players[1].hand.clear();
    for _ in 0..3 {
        let k=c.new_card(1,CardId::DefendIronclad,false).unwrap();
        c.players[1].discard.push(k);
    }
    c.apply_power_to(1,PowerId::Stratagem,1,Some(1));
    let card=c.new_card(0,CardId::Constellation,false).unwrap();
    c.players[0].hand.clear();c.players[0].hand.push(card);
    c.gain_stars(0,2);
    let energy=c.players[1].energy;
    assert!(c.play_card(0,0,Some(1)));
    assert_eq!(c.fault,None);
    assert!(c.pending_choice.is_some());
    assert_eq!(c.players[1].energy,energy);
    assert_eq!(c.cr(1).block,0);
    assert!(c.resolve_pending_choice_for_player(1,Some(0)));
    assert!(c.pending_choice.is_none());
    // Stratagem puts its selected card in hand; Constellation then draws one.
    assert_eq!(c.players[1].hand.len(),2);
    assert_eq!(c.players[1].energy,energy+1);
    assert_eq!(c.cr(1).block,9);
    assert_eq!(c.fault,None);
}

#[test]
fn midnight_uses_all_seats_exhaust_history_and_deals_exact_damage_for_each_owner() {
    use sts2_sim::combat::{PileKind, PilePos};
    for owner in 0..4 { for upgraded in [false, true] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let enemy = c.enemies[0]; c.cr_mut(enemy).hp = 200; c.cr_mut(enemy).max_hp = 200;
        // C# Midnight.AfterCardEnteredCombat counts prior CardExhausted entries
        // from every actor; AfterCardExhausted then reduces this copy further.
        for actor in 0..4 {
            for _ in 0..2 {
                let k = c.new_card(actor, CardId::DefendIronclad, false).unwrap();
                c.players[actor].exhaust.push(k);
                c.fire_after_card_exhausted(k);
            }
        }
        let midnight = c.new_card(owner, CardId::Midnight, upgraded).unwrap();
        c.add_generated_card(midnight, PileKind::Hand, PilePos::Bottom, true);
        assert_eq!(c.modified_card_cost(owner, midnight), 4);
        for actor in [owner, (owner + 1) % 4] {
            let k = c.new_card(actor, CardId::DefendIronclad, false).unwrap();
            c.players[actor].exhaust.push(k);
            c.fire_after_card_exhausted(k);
        }
        assert_eq!(c.modified_card_cost(owner, midnight), 2);
        c.players[owner].energy = 3;
        assert!(c.play_card(owner, 0, Some(0)));
        assert_eq!(c.cr(enemy).hp, 200 - if upgraded { 72 } else { 60 });
        assert_eq!(c.players[owner].energy, 1);
        for seat in 0..4 { assert_eq!(c.cr(seat).hp, if seat == 0 { c.run.hp } else { 50 }); }
        assert_eq!(c.fault, None);
    }}
}

#[test]
fn ally_action_slots_bind_player_seats_for_all_fifteen_cards_and_all_actors() {
    use sts2_sim::obs::{decode_combat_action_for_player, R7_MAX_HAND_SIZE};
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.deck.clear();
    let mut c = Combat::new(run, 1);
    for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let cards = [
        CardId::BelieveInYou, CardId::Blaze, CardId::Concoct, CardId::Constellation,
        CardId::CoordinateCard, CardId::DemonicShield, CardId::Fade,
        CardId::Ignition, CardId::ImitationLearning, CardId::InterceptCard,
        CardId::Largesse, CardId::Lift, CardId::Mimic, CardId::Soulbound, CardId::Tutor,
    ];
    for owner in 0..4 {
        for id in cards {
            c.players[owner].hand.clear();
            let k = c.new_card(owner, id, false).unwrap();
            c.players[owner].hand.push(k);
            for target in 0..4u8 {
                let slot = (1 + R7_MAX_HAND_SIZE + usize::from(target)) as u16;
                let action = decode_combat_action_for_player(slot, &c, owner, &c.enemies, false).unwrap();
                assert_eq!((action.actor, action.hand_index, action.target), (owner, 0, Some(target)),
                    "{id:?} owner {owner} target {target}");
            }
        }
    }
    c.kill_creature(3);
    let dead_target = (1 + R7_MAX_HAND_SIZE + 3) as u16;
    assert!(decode_combat_action_for_player(dead_target, &c, 0, &c.enemies, false).is_err());
    assert_eq!(c.fault, None);
}

#[test]
fn silent_multiplayer_group_resolves_for_every_owner_and_upgrade() {
    // C# BladeSymphony/Concoct/Fade/Flanking/Sneaky.OnPlay and OnUpgrade:
    // all living players get Shivs; targeted buffs belong to the chosen ally;
    // Flanking belongs to the enemy; Sneaky belongs only to its caster.
    for owner in 0..4 {
        for upgraded in [false, true] {
            for id in [
                CardId::BladeSymphony, CardId::Concoct, CardId::Fade,
                CardId::Flanking, CardId::SneakyCard,
            ] {
                let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
                run.deck.clear();
                let mut c = Combat::new(run, 1);
                for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
                c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
                c.start_combat();
                let ally = (owner + 1) % 4;
                let target = match id {
                    CardId::Concoct | CardId::Fade => Some(usize::from(ally)),
                    CardId::Flanking => Some(0),
                    _ => None,
                };
                let k = c.new_card(owner, id, upgraded).unwrap();
                c.players[owner].hand.push(k);
                assert!(c.play_card(owner, 0, target), "{id:?} owner {owner}, upgraded {upgraded}");
                match id {
                    CardId::BladeSymphony => {
                        for p in 0..4 {
                            assert_eq!(c.players[p].hand.iter()
                                .filter(|&&q| c.cards[q as usize].id == CardId::Shiv).count(), 2);
                        }
                    }
                    CardId::Concoct | CardId::Fade | CardId::SneakyCard => {
                        let (power, recipient, amount) = match id {
                            CardId::Concoct => (PowerId::Concoct, ally, if upgraded { 4 } else { 3 }),
                            CardId::Fade => (PowerId::Fade, ally, if upgraded { 9 } else { 6 }),
                            _ => (PowerId::Sneaky, owner, if upgraded { 2 } else { 1 }),
                        };
                        for p in 0..4 {
                            assert_eq!(c.power_amount(p, power), if p == recipient { amount } else { 0 });
                        }
                    }
                    CardId::Flanking => {
                        assert_eq!(c.power_amount(c.enemies[0], PowerId::Flanking), 2);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(c.fault, None);
            }
        }
    }
}

#[test]
fn thorns_uses_each_damage_card_source_not_the_enclosing_active_card() {
    use sts2_sim::combat::Props;
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let enemy = c.enemies[0];
        c.apply_power_to(enemy, PowerId::Thorns, 2, Some(enemy));
        let omni = c.new_card(owner, CardId::Omnislice, false).unwrap();
        let strike = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let hp = c.cr(owner).hp;
        c.active_card = Some(omni);
        // A power/relic reaction during Omnislice has no card source.
        c.deal_damage(Some(owner), enemy, 1, Props::UNPOWERED);
        assert_eq!(c.cr(owner).hp, hp);
        c.apply_damage_with_card_source(enemy, 1, Props::UNPOWERED, Some(owner), Some(strike));
        assert_eq!(c.cr(owner).hp, hp);
        // Conversely, Omnislice splash reacts even with another active context.
        c.active_card = Some(strike);
        c.apply_damage_with_card_source(enemy, 1, Props::UNPOWERED, Some(owner), Some(omni));
        assert_eq!(c.cr(owner).hp, hp - 2);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn gold_axe_counts_finished_plays_from_every_seat() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        for actor in 0..4 {
            let k = c.new_card(actor, CardId::DefendIronclad, false).unwrap();
            c.hist.played_combat.push((actor, k));
        }
        let enemy = c.enemies[0];
        let hp = c.cr(enemy).hp;
        let k = c.new_card(owner, CardId::GoldAxe, false).unwrap();
        c.players[owner].hand.push(k);
        assert!(c.play_card(owner, 0, Some(0)));
        assert_eq!(c.cr(enemy).hp, hp - 4);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn omnislice_triggers_thorns_on_its_powered_hit_and_unpowered_splash_in_every_seat() {
    // C# ThornsPower.BeforeDamageReceived explicitly accepts cardSource Omnislice.
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        c.start_combat();
        assert!(c.enemies.len() >= 2);
        let first = c.enemies[0];
        let second = c.enemies[1];
        c.apply_power_to(first, PowerId::Thorns, 2, Some(first));
        c.apply_power_to(second, PowerId::Thorns, 3, Some(second));
        let hp: Vec<_> = (0..4).map(|p| c.cr(p).hp).collect();
        let enemy_hp: Vec<_> = c.enemies.iter().map(|&e| (e, c.cr(e).hp)).collect();
        let k = c.new_card(owner, CardId::Omnislice, false).unwrap();
        c.players[owner].hand.push(k);
        assert!(c.play_card(owner, 0, Some(0)));
        for (enemy, before) in enemy_hp { assert_eq!(c.cr(enemy).hp, before - 8); }
        for p in 0..4 {
            assert_eq!(c.cr(p).hp, hp[p as usize] - if p == owner { 5 } else { 0 });
        }
        assert_eq!(c.fault, None);
    }
}
