//! C# BurrowedPower.AfterBlockBroken, CardCmd.DiscardAndDraw and nullable damage targets.
use crate::character::Character;
use super::*;
use crate::combat::{RunCtx, RunRngs, REF_CACHE_WORDS};
use crate::rng::Rng;

#[path = "discovery_b_tests.rs"]
mod discovery_b_tests;

#[test]
fn supermassive_counts_its_owners_creations_not_every_players_or_card_ownership() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    for (owner,creator) in [(0,Some(1)),(1,Some(0)),(1,Some(0)),(1,None)] {
        let k=c.new_card(owner,CardId::Shiv,false).unwrap();
        c.add_generated_card_by(k,crate::combat::PileKind::Discard,crate::combat::PilePos::Bottom,creator);
    }
    assert_eq!(c.hist.generated_by_creator[0],2);
    assert_eq!(c.hist.generated_by_creator[1],1);
    c.players[1].hand.clear();c.players[1].energy=3;
    let target=c.enemies[0];c.cr_mut(target).hp=200;c.cr_mut(target).max_hp=200;c.cr_mut(target).block=0;
    let k=c.new_card(1,CardId::Supermassive,false).unwrap();c.players[1].hand.push(k);
    assert!(c.play_card(1,0,Some(0)));
    assert_eq!(c.cr(target).hp,192);
}

#[test]
fn tracking_stacks_percentage_points_against_weak_targets() {
    for stacks in [1,2] {
        let mut c=combat();c.run.relics.clear();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
        c.players[0].hand.clear();
        let target=c.enemies[0];
        c.cr_mut(target).hp=200;c.cr_mut(target).max_hp=200;c.cr_mut(target).block=0;
        c.apply_power_to(target,PowerId::Weak,1,Some(0));
        for _ in 0..stacks {
            let k=c.new_card(0,CardId::Tracking,false).unwrap();
            c.players[0].hand.push(k);c.players[0].energy=3;
            assert!(c.play_card(0,0,None));
        }
        assert_eq!(c.power_amount(0,PowerId::Tracking),50*stacks);
        let strike=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
        c.players[0].hand.push(strike);c.players[0].energy=3;
        assert!(c.play_card(0,0,Some(0)));
        assert_eq!(c.cr(target).hp,200-(6+3*stacks));
    }
}

#[test]
fn pinpoint_discount_counts_only_its_owners_skill_plays() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    let pin=c.new_card(0,CardId::Pinpoint,false).unwrap();
    c.players[0].hand.push(pin);
    for owner in [1,0] {
        c.players[owner].hand.clear();c.players[owner].energy=3;
        if owner==0 {c.players[0].hand.push(pin);}
        let skill=c.new_card(owner,CardId::DefendSilent,false).unwrap();
        c.players[owner].hand.insert(0,skill);
        assert!(c.play_card(owner,0,None));
        if owner==1 {
            c.players[0].energy=2;
            assert!(!c.play_card(0,0,Some(0)));
        } else {
            assert!(c.play_card(0,0,Some(0)));
            assert_eq!(c.players[0].energy,0);
        }
    }
}

#[test]
fn expertise_immediate_draw_retains_results_before_hand_flush() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();c.players[0].draw.clear();c.players[0].discard.clear();
    let expertise=c.new_card(0,CardId::Expertise,false).unwrap();
    let existing=c.new_card(0,CardId::DefendSilent,false).unwrap();
    let drawn=c.new_card(0,CardId::DefendSilent,false).unwrap();
    let curse=c.new_card(0,CardId::AscendersBane,false).unwrap();
    c.players[0].hand.push(expertise);c.players[0].hand.push(existing);
    c.players[0].draw.push(drawn);c.players[0].draw.push(curse);
    assert!(c.play_card(0,0,None));
    assert!(c.cards[drawn as usize].single_turn_retain);
    assert!(c.cards[curse as usize].single_turn_retain);
    assert!(!c.cards[existing as usize].single_turn_retain);
    c.end_player_turn();
    assert!(!c.players[0].discard.contains(&drawn));
    assert!(c.players[0].exhaust.contains(&curse));
}

#[test]
fn hyperbeam_focus_penalty_is_temporary_and_stacks() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.apply_power_to(0,PowerId::HyperbeamFocusDown,3,Some(0));
    c.apply_power_to(0,PowerId::HyperbeamFocusDown,2,Some(0));
    assert_eq!(c.power_amount(0,PowerId::Focus),-5);
    assert_eq!(c.power_amount(0,PowerId::HyperbeamFocusDown),5);
    c.end_player_turn();
    assert_eq!(c.power_amount(0,PowerId::Focus),0);
    assert!(!c.has_power(0,PowerId::HyperbeamFocusDown));
}

#[test]
fn expertise_retains_only_its_fixed_draw_results_across_stratagem() {
    for (upgraded,count) in [(false,2),(true,3)] {
        let mut c=combat();
        c.run.relics.clear();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
        c.players[0].hand.clear();c.players[0].draw.clear();c.players[0].discard.clear();
        let expertise=c.new_card(0,CardId::Expertise,upgraded).unwrap();
        let existing=c.new_card(0,CardId::DefendSilent,false).unwrap();
        let first=c.new_card(0,CardId::DefendSilent,false).unwrap();
        c.players[0].hand.push(expertise);c.players[0].hand.push(existing);
        c.players[0].draw.push(first);
        for _ in 0..6 {
            let k=c.new_card(0,CardId::DefendSilent,false).unwrap();
            c.players[0].discard.push(k);
        }
        c.apply_power_to(0,PowerId::Stratagem,1,Some(0));c.players[0].energy=3;
        assert!(c.play_card(0,0,None));
        assert!(c.fault.is_none(),"{:?}",c.fault);
        assert!(c.pending_choice.is_some());
        assert!(!c.cards[first as usize].single_turn_retain,"await the entire draw");
        let selected=c.choice_options[0];
        assert!(c.resolve_pending_choice(Some(0)));
        if c.pending_choice.is_some() {assert!(c.resolve_pending_choice(None));}
        assert!(c.fault.is_none(),"{:?}",c.fault);
        assert!(c.pending_choice.is_none());
        assert_eq!(c.players[0].hand.len(),count+2); // existing + Stratagem + Draw
        assert_eq!(c.players[0].hand.iter().filter(|&&k|c.cards[k as usize].single_turn_retain).count(),count);
        assert!(c.cards[first as usize].single_turn_retain);
        assert!(!c.cards[existing as usize].single_turn_retain);
        assert!(!c.cards[selected as usize].single_turn_retain,"Stratagem Add is not Draw's result");
    }
}

#[test]
fn aoe_resolves_all_damage_before_shrink_applier_death_removes_modifier() {
    let mut c=combat();
    c.run.relics.clear();
    c.setup_encounter("SLIMES_NORMAL",&mut Rng::new(1));
    c.start_combat();
    let targets=c.enemies.clone();
    for e in targets.iter().copied() {c.cr_mut(e).hp=100;c.cr_mut(e).block=0;}
    let applier=targets[0];c.cr_mut(applier).hp=1;
    c.apply_power_to(0,PowerId::Shrink,-1,Some(applier));
    c.apply_power_to(0,PowerId::Strength,3,Some(0));
    let k=c.new_card(0,CardId::Thunderclap,false).unwrap();
    c.players[0].hand.clear();c.players[0].hand.push(k);c.players[0].energy=3;
    assert!(c.play_card(0,0,None));
    assert!(c.cr(applier).is_dead());
    assert!(c.find_power(0,PowerId::Shrink).is_none());
    // Thunderclap base 4 + Strength 3, multiplied by Shrink's 0.7 => floor 4.
    for e in targets.into_iter().skip(1) {assert_eq!(c.cr(e).hp,96);}
}

#[test]
fn corrupted_killing_attack_still_pays_its_self_damage() {
    let mut c=combat();
    c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    c.players[0].hand.clear();
    c.cr_mut(0).hp=20;
    let e=c.enemies[0];c.cr_mut(e).hp=1;
    let k=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
    assert!(crate::card::enchant::apply_static_enchantment(&mut c.cards[k as usize],"Corrupted",1));
    c.players[0].hand.push(k);c.players[0].energy=3;
    assert!(c.play_card(0,0,Some(0)));
    assert!(c.is_over&&c.won);
    assert_eq!(c.cr(0).hp,18);
}

#[test]
fn radiate_entry_stars_are_filtered_by_card_owner_in_two_players() {
    // Original Radiate.cs:26 sums positive entries with Actor == card.Owner.
    for donor in [0,1] {
        let mut c=Combat::new(combat().run,1);
        c.add_player(80,80,3,0,&[]);
        let mut relic=crate::relic::Relic::new(RelicId::DivineRight);
        relic.owner=donor;
        c.run.relics.push(relic);
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        assert_eq!(c.hist.stars_gained_this_turn[donor as usize],3);
        assert_eq!(c.hist.stars_gained_this_turn[(1-donor) as usize],0);
        let e=c.enemies[0];c.cr_mut(e).hp=100;
        let k=c.new_card(0,CardId::Radiate,false).unwrap();
        // AutoPlay has no resource requirement; isolate history attribution
        // even when player 0 has no stars because player 1 owns Divine Right.
        assert!(c.auto_play_card(k,None,false));
        assert_eq!(c.cr(e).hp,if donor==0 {91}else{100});
    }
}

#[test]
fn reboot_awaits_stratagem_shuffle_before_its_draw() {
    let mut c=combat();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    c.players[0].hand.clear();c.players[0].draw.clear();c.players[0].discard.clear();
    let reboot=c.new_card(0,CardId::Reboot,false).unwrap();
    c.players[0].hand.push(reboot);
    for _ in 0..6 {
        let k=c.new_card(0,CardId::DefendDefect,false).unwrap();
        c.players[0].discard.push(k);
    }
    c.apply_power_to(0,PowerId::Stratagem,1,Some(0));
    assert!(c.play_card(0,0,None));
    assert!(c.fault.is_none(),"{:?}",c.fault);
    assert!(c.players[0].hand.is_empty(),"Draw must await shuffle's selection");
    assert!(c.pending_choice.is_some());
    let selected=c.choice_options[0];
    assert!(c.resolve_pending_choice(Some(0)));
    if c.pending_choice.is_some() {assert!(c.resolve_pending_choice(None));}
    assert!(c.fault.is_none(),"{:?}",c.fault);
    assert!(c.pending_choice.is_none());
    // StratagemPower.cs:24-26 puts the selected card in Hand, then
    // Reboot.cs:30 draws four additional cards.
    assert_eq!(c.players[0].hand.len(),5);
    assert!(c.players[0].hand.contains(&selected));
    assert!(c.players[0].exhaust.contains(&reboot));
}

#[test]
fn unceasing_top_awaits_stratagem_after_last_defend() {
    let mut c=combat();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    c.players[0].hand.clear();c.players[0].draw.clear();c.players[0].discard.clear();
    c.run.relics.push(crate::relic::Relic::new(RelicId::UnceasingTop));
    let defend=c.new_card(0,CardId::DefendSilent,false).unwrap();
    c.players[0].hand.push(defend);
    for _ in 0..3 {
        let k=c.new_card(0,CardId::StrikeSilent,false).unwrap();
        c.players[0].discard.push(k);
    }
    c.apply_power_to(0,PowerId::Stratagem,1,Some(0));
    assert!(c.play_card(0,0,None));
    assert!(c.fault.is_none(),"{:?}",c.fault);
    assert!(c.pending_choice.is_some());
    let selected=c.choice_options[0];
    assert!(c.resolve_pending_choice(Some(0)));
    if c.pending_choice.is_some(){assert!(c.resolve_pending_choice(None));}
    assert!(c.fault.is_none(),"{:?}",c.fault);
    assert!(c.pending_choice.is_none());
    assert_eq!(c.players[0].hand.len(),2,"Stratagem card plus the awaited Top draw");
    assert!(c.players[0].hand.contains(&selected));
}

#[test]
fn next_enemy_move_rolls_after_doom_removes_dead_enemies() {
    let mut c=combat();
    c.setup_encounter("SLIMES_NORMAL",&mut Rng::new(1));
    c.start_combat();
    let survivor=*c.enemies.iter().find(|&&e|c.cr(e).monster_id=="LEAF_SLIME_S").unwrap();
    for e in c.enemies.clone() {
        if e!=survivor {
            c.cr_mut(e).hp=1;
            c.apply_power_to(e,PowerId::Doom,1,Some(0));
        }
    }
    c.cr_mut(0).hp=999;
    let before=c.run.rngs.monster_ai.counter;
    c.end_player_turn();
    assert_eq!(c.enemies.as_slice(),&[survivor]);
    // LeafSlimeS.RAND consumes one draw even with CannotRepeat leaving one branch.
    assert_eq!(c.run.rngs.monster_ai.counter,before+1);
}

#[test]
fn corrupted_multiplies_base_before_strength_and_vulnerable() {
    let mut c=combat();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    let target=c.enemies[0];
    c.cr_mut(target).hp=100;
    let k=c.new_card(0,CardId::SetupStrikeCard,false).unwrap();
    assert!(crate::card::enchant::apply_static_enchantment(&mut c.cards[k as usize],"Corrupted",1));
    c.apply_power_to(0,PowerId::Strength,3,Some(0));
    c.apply_power_to(target,PowerId::Vulnerable,1,Some(0));
    c.active_card=Some(k);
    c.deal_card_damage(k,target);
    // C# Hook.ModifyDamage: floor((7 * 1.5 + 3) * 1.5) = 20,
    // not floor((7 + 3) * 1.5 * 1.5) = 22.
    assert_eq!(c.cr(target).hp,80);
}

#[test]
fn radiate_counts_divine_right_combat_entry_stars_on_first_turn() {
    let mut c=combat();
    c.run.relics.push(crate::relic::Relic::new(RelicId::DivineRight));
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    assert_eq!(c.hist.stars_gained_this_turn[0],3);
    c.players[0].hand.clear();
    let k=c.new_card(0,CardId::Radiate,false).unwrap();
    c.players[0].hand.push(k);
    let e=c.enemies[0];c.cr_mut(e).hp=100;
    assert!(c.play_card(0,0,None));
    assert_eq!(c.cr(e).hp,91);
    c.end_player_turn();
    assert_eq!(c.hist.stars_gained_this_turn[0],0);
}

#[test]
fn regen_heals_before_lethal_end_turn_lightning_and_removes_last_charge() {
    for amount in [1,4] {
        let mut c=combat();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.cr_mut(0).hp=30;
        c.apply_power_to(0,PowerId::Regen,amount,Some(0));
        c.players[0].orbs.open_base_slots(3);
        c.channel_orb(0,crate::orbs::OrbKind::Lightning);
        let e=c.enemies[0];
        c.cr_mut(e).hp=1;
        c.end_player_turn();
        assert_eq!(c.cr(0).hp,30+amount);
        assert!(c.is_over);
        if amount==1 {assert!(c.find_power(0,PowerId::Regen).is_none());}
    }
}

#[test]
fn upgraded_tesla_coil_triggers_each_lightning_passive_twice_on_explicit_target() {
    for upgraded in [false,true] {
        let mut c=combat();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.players[0].hand.clear();
        c.players[0].orbs.open_base_slots(3);
        c.channel_orb(0,crate::orbs::OrbKind::Lightning);
        let k=c.new_card(0,CardId::TeslaCoil,upgraded).unwrap();
        let e=c.enemies[0];
        c.cr_mut(e).hp=100;
        c.players[0].hand.push(k);
        c.players[0].energy=3;
        assert!(c.play_card(0,0,Some(0)));
        assert_eq!(c.cr(e).hp,if upgraded {90}else{94});
    }
}

#[test]
fn zero_value_tesla_passives_keep_the_explicit_enemy_target() {
    let mut c = combat();
    c.run.relics.clear();
    c.setup_encounter("INKLETS_NORMAL", &mut Rng::new(7));
    c.start_combat();
    c.players[0].hand.clear();
    c.players[0].orbs.open_base_slots(3);
    c.channel_orb(0, crate::orbs::OrbKind::Lightning);
    c.apply_power_to(0, PowerId::Focus, -100, Some(0));
    let target = c.enemies[1];
    c.cr_mut(target).hp = 100;
    c.cr_mut(target).max_hp = 100;
    let card = c.new_card(0, CardId::TeslaCoil, true).unwrap();
    c.players[0].hand.push(card);
    c.players[0].energy = 3;
    let mut plain = c.clone();
    let recorder = crate::between::Recorder::default();
    recorder.record_combat(&mut c);
    let capture = recorder.begin(3);
    assert!(c.play_card(0, 0, Some(1)));
    drop(capture);
    assert!(plain.play_card(0, 0, Some(1)));
    assert_eq!(c.digest_json(), plain.digest_json());
    assert_eq!(format!("{:?}", c.run.rngs), format!("{:?}", plain.run.rngs));
    let events = recorder.orb_activations();
    let passives: Vec<_> = events.iter().filter(|event| event.phase == "passive").collect();
    assert_eq!(passives.len(), 2);
    assert_eq!(passives[0].targets, [target]);
    assert_eq!(passives[1].targets, [target]);
}

#[test]
fn burst_removes_spent_instance_and_unspent_charges_at_owner_turn_end() {
    let mut c=combat();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    let skill=c.new_card(0,CardId::DefendSilent,false).unwrap();
    c.apply_power_to(0,PowerId::Burst,1,Some(0));
    let (count,modifiers)=c.modify_card_play_count(1,skill);
    assert_eq!(count,2);
    c.fire_after_modifying_card_play_count(skill,modifiers);
    assert!(c.find_power(0,PowerId::Burst).is_none());
    c.apply_power_to(0,PowerId::Burst,2,Some(0));
    c.fire_after_turn_end(Side::Player);
    assert!(c.find_power(0,PowerId::Burst).is_none());
}

#[test]
fn havoc_exhausts_unplayable_card_and_notifies_joss_without_playing_it() {
    for force_exhaust in [false,true] {
        let mut c=combat();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.players[0].draw.clear();
        c.run.relics.push(crate::relic::Relic::new(RelicId::JossPaper));
        let curse=c.new_card(0,CardId::Doubt,false).unwrap();
        c.players[0].draw.push(curse);
        c.auto_play_from_draw(0,1,force_exhaust);
        assert_eq!(c.players[0].exhaust.contains(&curse),force_exhaust);
        assert_eq!(c.players[0].discard.contains(&curse),!force_exhaust);
        assert_eq!(c.run.relics[0].counter,i32::from(force_exhaust));
    }
}

#[test]
fn reboot_shuffle_includes_discard_before_drawing() {
    let mut c=combat();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    c.start_combat();
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.players[0].discard.clear();
    let reboot=c.new_card(0,CardId::Reboot,false).unwrap();
    let hand=c.new_card(0,CardId::DefendDefect,false).unwrap();
    let draw=c.new_card(0,CardId::StrikeDefect,false).unwrap();
    let discard=c.new_card(0,CardId::Turbo,false).unwrap();
    c.players[0].hand.push(reboot);
    c.players[0].hand.push(hand);
    c.players[0].draw.push(draw);
    c.players[0].discard.push(discard);
    c.players[0].energy=3;
    assert!(c.play_card(0,0,None));
    // Reboot draws four from the combined three-card pile; it exhausts itself.
    for k in [hand,draw,discard] {assert!(c.players[0].hand.contains(&k));}
    assert!(c.players[0].discard.is_empty());
    assert!(c.players[0].exhaust.contains(&reboot));
}

#[test]
fn jackpot_uses_owner_canonical_zero_cost_pool() {
    for character in [crate::character::Character::Defect,crate::character::Character::Regent] {
        let mut c=combat();
        c.run.all_cards_unlocked=Some(true);
        c.set_character_pool(0,character).unwrap();
        c.players[0].character=Some(character);
        for _ in 0..20 {
            let generated=c.generate_unlocked_zero_cost_owner_cards(0,3);
            assert_eq!(generated.len(),3);
            for k in generated {
                let card=&c.cards[k as usize];
                let info=crate::tables::card_info(card.id,false).unwrap();
                assert_eq!(info.cost,0);
                assert!(!info.x_cost);
                // Original IroncladCardPool includes Anger, but neither
                // DefectCardPool nor RegentCardPool does.
                assert_ne!(card.id,CardId::Anger,"{character}");
            }
        }
    }
}

#[test]
fn monster_aoe_finishes_its_issued_targets_after_thorns_kills_the_attacker() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    let enemy=c.enemies[0];c.cr_mut(enemy).hp=3;
    c.apply_power_to(enemy,PowerId::Strength,3,Some(enemy));
    c.apply_power_to(enemy,PowerId::Weak,1,Some(0));
    c.apply_power_to(2,PowerId::Thorns,3,Some(2));
    for owner in 0..4 {c.cr_mut(owner).hp=50;c.cr_mut(owner).block=0;}
    c.cr_mut(2).block=9;
    c.monster_attack_players(enemy,7,1);
    assert!(c.cr(enemy).is_dead());
    assert_eq!((0..4).map(|p|c.cr(p).hp).collect::<Vec<_>>(),[43,43,50,43]);
}

#[test]
fn particle_wall_free_star_cost_expires_after_the_first_play() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();c.players[0].stars=4;
    let k=c.new_card(0,CardId::ParticleWall,false).unwrap();
    c.players[0].hand.push(k);c.set_temporary_free_this_turn(k);
    assert!(c.play_card(0,0,None));
    assert_eq!(c.players[0].stars,4);
    assert!(c.play_card(0,0,None));
    assert_eq!(c.players[0].stars,2);
}

#[test]
fn manifest_authority_samples_the_audited_multiplayer_colorless_pool() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.run.all_cards_unlocked=Some(true);c.players[1].hand.clear();c.players[1].energy=3;
    let (_,_,pool)=crate::tables::CSHARP_TRANSFORMS.iter().find(|(mp,id,_)|*mp&&*id=="SOUL").unwrap();
    let mut rng=c.run.rngs.combat_card_generation.clone();
    let mut candidates=pool.to_vec();rng.shuffle(&mut candidates);
    let expected=candidates[0];
    let k=c.new_card(1,CardId::ManifestAuthority,false).unwrap();c.players[1].hand.push(k);
    assert!(c.play_card(1,0,None));
    let generated=c.cards[*c.players[1].hand.last().unwrap() as usize].id.name();
    assert_eq!(generated.strip_suffix("_CARD").unwrap_or(generated),expected);
}

#[test]
fn splash_excludes_the_owners_pool_for_every_character() {
    for character in [crate::character::Character::Ironclad,crate::character::Character::Silent,crate::character::Character::Regent,crate::character::Character::Necrobinder,crate::character::Character::Defect] {
        let own=crate::cardgen::character_pool(character.into(),false,Some(crate::tables::CardType::Attack),None).unwrap();
        let mut c=combat();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.players[0].character=Some(character);
        for _ in 0..20 {
            c.players[0].hand.clear();
            let k=c.new_card(0,CardId::Splash,false).unwrap();
            c.players[0].hand.push(k);
            c.players[0].energy=3;
            assert!(c.play_card(0,0,None));
            assert!(c.fault.is_none(),"{:?}",c.fault);
            assert_eq!(c.choice_options.len(),3);
            for &option in &c.choice_options {
                assert!(!own.contains(&c.cards[option as usize].id),"{character}");
            }
            assert!(c.resolve_pending_choice(None));
        }
    }
}

#[test]
fn tesla_coil_finishes_attack_callbacks_before_lightning_passives() {
    let run=crate::run::RunManager::new(7,crate::run::RunOptions::default()).run_ctx().clone();
    let mut c=Combat::new(run,1);c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();
    let target=c.enemies[0];c.cr_mut(target).hp=100;c.cr_mut(target).max_hp=100;c.cr_mut(target).block=0;
    c.apply_power_to(target,PowerId::Skittish,7,Some(target));
    c.add_orb_slots(0,1);
    c.channel_orb(0,crate::orbs::OrbKind::Lightning);
    assert_eq!(c.players[0].orbs.orbs.len(),1);
    assert_eq!(c.orb_passive_value(0,c.players[0].orbs.orbs[0]),3);
    let k=c.new_card(0,CardId::TeslaCoil,false).unwrap();c.players[0].hand.push(k);
    assert!(c.play_card(0,0,Some(0)));
    assert_eq!(c.cr(target).block,4);
}

#[test]
fn molten_fist_exhaust_does_not_start_joss_listeners_after_a_killing_hit() {
    for killing in [false,true] {
        let mut c=combat();
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
        c.start_combat();
        c.run.relics.push(crate::relic::Relic::new(RelicId::JossPaper));
        c.run.relics[0].counter=1;
        let k=c.new_card(0,CardId::MoltenFist,false).unwrap();
        let target=c.enemies[0];
        c.cr_mut(target).hp=if killing {1}else{100};
        c.active_card=Some(k);
        c.deal_card_damage(k,target);
        c.active_card=None;
        c.fire_power_relic_after_card_exhausted(k,false);
        assert_eq!(c.run.relics[0].counter,if killing {1}else{2});
    }
}

#[test]
fn selected_power_potion_card_records_generation_history() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.run.all_cards_unlocked=Some(true);c.players[0].character=Some(Character::Regent);
    let before=c.hist.generated_by_player_combat;
    c.potion_effect_for_player(0,crate::tables::PotionId::Powerpotion,Some(0));
    let selected=c.choice_options[0];
    assert!(c.resolve_pending_choice(Some(0)));
    assert!(c.players[0].hand.contains(&selected));
    assert_eq!(c.hist.generated_by_player_combat,before+1);
}

#[test]
fn starting_exemption_allows_exhaust_listener_snapshot_before_combat_started() {
    let mut c=combat();
    c.run.relics.push(crate::relic::Relic::new(RelicId::JossPaper));
    let k=c.new_card(0,CardId::MoltenFist,false).unwrap();
    c.is_ending=true;
    c.fire_power_relic_after_card_exhausted(k,false);
    assert_eq!(c.run.relics[0].counter,1);
    c.combat_started=true;
    c.fire_power_relic_after_card_exhausted(k,false);
    assert_eq!(c.run.relics[0].counter,1);
}

#[test]
fn echo_form_does_not_start_a_repeat_after_lethal_damage() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();
    c.run.relics.push(crate::relic::Relic::new(RelicId::PenNib));
    c.run.relics[0].counter=7;
    c.apply_power_to(0,PowerId::EchoForm,1,Some(0));
    let target=c.enemies[0];c.cr_mut(target).hp=1;c.cr_mut(target).block=0;
    let k=c.new_card(0,CardId::Refract,false).unwrap();c.players[0].hand.push(k);c.players[0].energy=3;
    assert!(c.play_card(0,0,Some(0)));
    assert_eq!(c.run.relics[0].counter,8);
}

#[test]
fn ornamental_fan_preserves_attack_count_after_combat_until_next_owner_turn() {
    // Original DLL OrnamentalFan.AfterCombatEnd only sets IsActivating=false.
    // A57/A68 expose the retained count on the killing-card boundary.
    for count in [1,2,3,4] {
        let mut c=combat();
        c.run.relics.push(crate::relic::Relic::new(RelicId::OrnamentalFan));
        c.run.relics[0].counter=count;
        c.run.relics[0].flag1=true;
        c.fire_relics(Hook::AfterCombatEnd,Ev::None);
        assert_eq!(c.run.relics[0].counter,count);
        assert!(!c.run.relics[0].flag1);
        c.fire_relics(Hook::BeforeSideTurnStart,Ev::Side(Side::Player));
        assert_eq!(c.run.relics[0].counter,0);
    }
}

#[test]
fn dredge_returns_discard_cards_in_selected_order() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();c.players[0].discard.clear();
    let mut cards=Vec::new();
    for id in [CardId::Reave,CardId::DefendNecrobinder,CardId::Bodyguard,CardId::StrikeNecrobinder] {
        let k=c.new_card(0,id,false).unwrap();cards.push(k);c.players[0].discard.push(k);
    }
    let k=c.new_card(0,CardId::Dredge,false).unwrap();c.players[0].hand.push(k);c.players[0].energy=3;
    assert!(c.play_card(0,0,None));
    for index in [0,2,1] {assert!(c.resolve_pending_choice(Some(index)));}
    assert!(c.pending_choice.is_none());
    assert_eq!(c.players[0].hand.as_slice(),[cards[0],cards[2],cards[1]]);
}

#[test]
fn tezcataras_ember_adds_three_before_strength_and_vulnerable_damage_rounding() {
    // Original-DLL seed 20261008000031, action 163: Tunneler has 79 HP
    // after Bash; the Ember Strike deals floor((6 + 5 + 3) * 1.5) = 21.
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let target=c.enemies[0];
    c.cr_mut(target).hp=79;
    c.cr_mut(target).max_hp=92;
    c.apply_power_to(0,PowerId::Strength,5,Some(0));
    c.apply_power_to(target,PowerId::Vulnerable,2,Some(0));
    let card=c.new_card(0,CardId::StrikeIronclad,false).unwrap();
    assert!(crate::card::enchant::apply_static_enchantment(
        &mut c.cards[card as usize],"TezcatarasEmber",1));
    assert_eq!(crate::card::enchant::damage_additive(&c.cards[card as usize],Props::MOVE),3);
    assert_eq!(crate::card::enchant::damage_additive(
        &c.cards[card as usize],Props::MOVE.or(Props::UNPOWERED)),0);
    assert_eq!(crate::card::enchant::damage_additive(&c.cards[card as usize],Props::NONE),0);
    c.active_card=Some(card);
    c.deal_card_damage(card,target);
    c.active_card=None;
    assert_eq!(c.cr(target).hp,58);
}

#[test]
fn generated_free_card_energy_and_stars_expire_after_play_or_turn_end() {
    // DLL seed 20261008000062: InfernalBlade generated Unrelenting; after
    // NeowsFury retrieves it, its second play costs two energy, not zero.
    let mut c = combat();
    let card = c.new_card(0, CardId::Unrelenting, false).unwrap();
    c.set_temporary_free_this_turn(card);
    assert_eq!(c.cards[card as usize].cost,0);
    assert_eq!(c.cards[card as usize].cv.turn_star_cost_override,Some(0));
    c.cards[card as usize].after_card_played_cost_cleanup();
    assert_eq!(c.cards[card as usize].cost,2);
    // TemporaryCardCost.ThisTurn sets ClearsWhenCardIsPlayed as well.
    assert_eq!(c.cards[card as usize].cv.turn_star_cost_override,None);
    c.cards[card as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[card as usize].cv.turn_star_cost_override,None);
    c.set_temporary_free_this_turn(card);
    c.cards[card as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[card as usize].cost,2);
}

#[test]
fn toy_box_melting_does_not_cancel_a_snapshotted_combat_end_listener() {
    let mut c=combat();c.run.relics.clear();
    let mut toy=crate::relic::Relic::new(RelicId::ToyBox);toy.counter=2;
    let mut puzzle=crate::relic::Relic::new(RelicId::CentennialPuzzle);
    puzzle.is_wax=true;puzzle.flag1=true;
    c.run.relics.extend([toy,puzzle]);
    c.fire_relics(Hook::AfterCombatEnd,Ev::None);
    assert!(c.run.relics[1].is_melted);
    assert!(!c.run.relics[1].flag1);
}

#[test]
fn letter_opener_keeps_its_skill_counter_between_combats_until_next_combat_start() {
    let mut c=combat();
    let mut relic=crate::relic::Relic::new(RelicId::LetterOpener);
    relic.counter=5;relic.flag1=true;
    c.run.relics.push(relic);
    let index=c.run.relics.len()-1;
    c.fire_relics(Hook::AfterCombatEnd,Ev::None);
    assert_eq!(c.run.relics[index].counter,5);
    assert!(!c.run.relics[index].flag1);
    c.fire_relics(Hook::BeforeCombatStart,Ev::None);
    assert_eq!(c.run.relics[index].counter,0);
}

#[test]
fn ringing_publishes_csharp_amount_for_existing_and_new_owner_cards_only() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let own = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let ally = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    let afflicted = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].hand.extend([own, afflicted]);
    c.players[1].hand.push(ally);
    c.cards[afflicted as usize].cv.affliction = Some(CardAffliction::Tainted);
    c.cards[afflicted as usize].cv.affliction_amount = 3;
    c.apply_power_to(0, PowerId::Ringing, 1, Some(0));
    assert_eq!(c.cards[own as usize].cv.affliction, Some(CardAffliction::Ringing));
    assert_eq!(c.cards[own as usize].observation_affliction_amount(), 1);
    assert_eq!(c.cards[ally as usize].cv.affliction, None);
    assert_eq!(c.cards[afflicted as usize].cv.affliction, Some(CardAffliction::Tainted));
    assert_eq!(c.cards[afflicted as usize].observation_affliction_amount(), 3);
    let generated = c.new_card(0, CardId::Shiv, false).unwrap();
    c.add_generated_card(generated, PileKind::Hand, crate::combat::PilePos::Bottom, true);
    assert_eq!(c.cards[generated as usize].cv.affliction, Some(CardAffliction::Ringing));
    assert_eq!(c.cards[generated as usize].observation_affliction_amount(), 1);
}

#[test]
fn relic_owner_plumbing_isolated_and_unconverted_relics_still_refuse() {
    let mut c = combat();
    c.players[1].gold = Some(17);
    c.players[1].potions = Some(vec![None]);
    c.run.gold = 99;
    assert_eq!(c.relic_owner_gold(0), Some(99));
    assert_eq!(c.relic_owner_gold(1), Some(17));
    c.relic_owner_potions(1).unwrap()[0] = Some(PotionId::Bloodpotion);
    assert!(c.run.potions.is_empty());
    let card = crate::card::Card::from_factory(CardId::StrikeIronclad, false, 99).unwrap();
    c.relic_owner_deck_mut(1).push(card);
    assert_eq!(c.relic_owner_deck(1).len(), 1);
    assert!(c.relic_owner_deck(0).is_empty());
    let mut relic = crate::relic::Relic::new(RelicId::PureWater);
    relic.owner = 1;
    c.run.relics.push(relic);
    c.relic_event(0, Hook::BeforeCombatStart, Ev::None);
    assert_eq!(c.fault, Some("relic owner hooks not ported"));
    assert_eq!(c.players[0].energy, 0);
    assert_eq!(c.players[1].energy, 0);
}

#[test]
fn first_relic_batch_targets_only_its_owner_in_four_seats() {
    let ids = [RelicId::Lantern, RelicId::HappyFlower, RelicId::VenerableTeaSet,
        RelicId::Candelabra, RelicId::Chandelier, RelicId::SparklingRouge,
        RelicId::Pendulum, RelicId::ArtOfWar, RelicId::OrnamentalFan, RelicId::LetterOpener];
    for owner in 0..4 {
        for id in ids {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            for seat in 0..4 { c.players[seat].turn_number = 3; }
            c.players[owner].turn_number = 1;
            let mut relic = crate::relic::Relic::new(id);
            relic.owner = owner;
            c.run.relics.push(relic);
            match id {
                RelicId::Lantern => c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player)),
                RelicId::HappyFlower => {
                    c.run.relics[0].counter = 2;
                    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                }
                RelicId::VenerableTeaSet | RelicId::ArtOfWar => {
                    c.run.relics[0].flag1 = id == RelicId::VenerableTeaSet;
                    c.players[owner].turn_number = 2;
                    for seat in 0..4 { c.fire_after_energy_reset(seat); }
                }
                RelicId::Candelabra | RelicId::Chandelier => {
                    c.players[owner].turn_number = if id == RelicId::Candelabra { 2 } else { 3 };
                    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                }
                RelicId::SparklingRouge => {
                    c.players[owner].turn_number = 3;
                    for seat in 0..4 { c.fire_relics(Hook::AfterBlockCleared, Ev::Creature(seat)); }
                    assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
                    assert_eq!(c.power_amount(owner, PowerId::Dexterity), 1);
                }
                RelicId::Pendulum => {
                    c.run.relics[0].counter = 2;
                    for seat in 0..4 { c.fire(Hook::BeforeHandDraw, Ev::Creature(seat), Some(seat)); }
                    for seat in 0..4 {
                        assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 6 } else { 5 });
                    }
                }
                RelicId::OrnamentalFan | RelicId::LetterOpener => {
                    let card = if id == RelicId::OrnamentalFan { CardId::StrikeIronclad } else { CardId::DefendIronclad };
                    let enemy = c.enemies[0];
                    let hp = c.cr(enemy).hp;
                    for seat in 0..4 {
                        let k = c.new_card(seat, card, false).unwrap();
                        for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
                    }
                    assert_eq!(c.run.relics[0].counter, 3, "{id:?} counts only its owner's cards");
                    if id == RelicId::OrnamentalFan { assert_eq!(c.cr(owner).block, 4); }
                    else { assert_eq!(c.cr(enemy).hp, hp - 5); }
                }
                _ => unreachable!(),
            }
            let energy = match id {
                RelicId::Lantern | RelicId::HappyFlower | RelicId::ArtOfWar => 1,
                RelicId::VenerableTeaSet | RelicId::Candelabra => 2,
                RelicId::Chandelier => 3, _ => 0,
            };
            for seat in 0..4 {
                assert_eq!(c.players[seat].energy, if seat == owner { energy } else { 0 }, "{id:?}");
                if seat != owner {
                    assert_eq!(c.cr(seat).block, 0, "{id:?}");
                    assert_eq!(c.power_amount(seat, PowerId::Strength), 0, "{id:?}");
                }
            }
            if id == RelicId::ArtOfWar {
                for seat in 0..4 {
                    if seat != owner {
                        let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
                    }
                }
                assert!(!c.run.relics[0].flag2, "teammate attacks do not count");
                let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                assert!(c.run.relics[0].flag2);
                c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
                c.fire_after_energy_reset(owner);
                assert_eq!(c.players[owner].energy, 1, "own attack suppresses next bonus");
            }
            c.players[owner].hooks_active = false;
            let counter = c.run.relics[0].counter;
            let energy = c.players[owner].energy;
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            c.fire_after_energy_reset(owner);
            assert_eq!(c.run.relics[0].counter, counter);
            assert_eq!(c.players[owner].energy, energy, "inactive owner must not run");
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}

#[test]
fn relic_modifier_and_lifecycle_plumbing_filters_owner_and_keeps_admission_closed() {
    for owner in 0..4 {
        let mut c = combat();
        let mut relic = crate::relic::Relic::new(RelicId::Pendulum);
        relic.owner = owner;
        c.run.relics.push(relic);
        for seat in 0..4 {
            assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 6 } else { 5 });
            assert_eq!(c.max_energy(seat), 3);
            assert!(c.should_draw(seat, false));
            assert!(c.should_flush(seat));
            assert!(c.should_reset_energy(seat));
        }
        assert!(c.fault.is_none());
        if owner != 0 {
            // Both shuffle relics are now ported; retain the admission
            // regression with an actually unsupported owner hook.
            c.run.relics[0].id = RelicId::PureWater;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            assert_eq!(c.fault, Some("relic owner hooks not ported"));
            for seat in 0..4 { assert_eq!(c.cr(seat).block, 0); }
        }
    }
}

#[test]
fn relic_persistent_deck_upgrade_preserves_other_owners_and_shared_id_sequence() {
    for owner in 0..4 {
        let mut c = combat();
        for seat in 0..4 {
            let mut card = crate::card::Card::from_factory(CardId::StrikeIronclad, false, seat as u32).unwrap();
            card.owner = seat;
            c.relic_owner_deck_mut(seat).push(card);
        }
        let mut solo_run = c.run.clone();
        assert!(solo_run.is_deck_card_upgradable(0));
        solo_run.upgrade_deck_card(0);
        assert!(c.relic_owner_deck_card_upgradable(owner, 0));
        c.relic_owner_upgrade_deck_card(owner, 0);
        for seat in 0..4 {
            assert_eq!(c.relic_owner_deck(seat)[0].upgraded, seat == owner);
            assert_eq!(c.relic_owner_deck(seat)[0].owner, seat);
        }
        assert_eq!(c.run.next_card_id, solo_run.next_card_id);
        assert_eq!(c.run.ref_cache, solo_run.ref_cache);
    }
}

#[test]
fn second_relic_batch_matches_csharp_owner_guards_for_every_seat() {
    let ids = [
        RelicId::Akabeko, RelicId::MiniatureCannon, RelicId::IceCream,
        RelicId::LastingCandy, RelicId::CloakClasp, RelicId::Nunchaku,
        RelicId::Pocketwatch, RelicId::GamePiece, RelicId::HornCleat,
        RelicId::MercuryHourglass,
    ];
    for owner in 0..4 {
        for id in ids {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            let other = (owner + 1) % 4;
            c.round = 99; // Owner's TurnNumber, deliberately not the shared round.
            c.players[owner].turn_number = 1;
            let mut relic = crate::relic::Relic::new(id);
            relic.owner = owner;
            c.run.relics.push(relic);
            match id {
                RelicId::Akabeko => {
                    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
                    assert_eq!(c.power_amount(owner, PowerId::Vigor), 0);
                    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                    assert_eq!(c.power_amount(owner, PowerId::Vigor), 8);
                    c.players[owner].turn_number = 2;
                    c.round = 1;
                    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                    assert_eq!(c.power_amount(owner, PowerId::Vigor), 8);
                }
                RelicId::MiniatureCannon => {
                    let own = c.new_card(owner, CardId::StrikeIronclad, true).unwrap();
                    let ally = c.new_card(other, CardId::StrikeIronclad, true).unwrap();
                    let skill = c.new_card(owner, CardId::DefendIronclad, true).unwrap();
                    let basic = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                    let props = Props::MOVE;
                    let enemy = c.enemies[0];
                    for (dealer, card, expected) in [
                        (Some(owner), ally, 3), (Some(other), own, 3),
                        (Some(other), ally, 0),
                        (Some(owner), basic, 0), (Some(owner), skill, 3),
                    ] {
                        c.cr_mut(enemy).hp = 100;
                        c.cr_mut(enemy).block = 0;
                        c.active_card = Some(card);
                        c.deal_damage(dealer, enemy, 10, props);
                        assert_eq!(c.cr(enemy).hp, 100 - 10 - expected);
                    }
                    // deal_damage(None, ...) is the raw-loss API and deliberately
                    // bypasses damage calculation; retain the separate null-dealer
                    // modifier contract in addition to the real attack-path tests.
                    assert_eq!(c.relic_damage_additive(0, owner, None, enemy, props, Some(own)), 3);
                    c.cr_mut(enemy).hp = 100;
                    c.active_card = Some(own);
                    c.deal_damage(Some(owner), enemy, 10, Props::UNPOWERED);
                    assert_eq!(c.cr(enemy).hp, 90);
                    c.cr_mut(enemy).hp = 100;
                    c.active_card = None;
                    c.deal_damage(Some(owner), enemy, 10, props);
                    assert_eq!(c.cr(enemy).hp, 90);
                }
                RelicId::IceCream => {
                    for seat in 0..4 { assert!(c.should_reset_energy(seat)); }
                    c.players[owner].turn_number = 2;
                    c.round = 1;
                    for seat in 0..4 {
                        assert_eq!(c.should_reset_energy(seat), seat != owner);
                    }
                }
                RelicId::LastingCandy => {
                    c.run.relics[0].counter = 7;
                    c.fire_relics(Hook::AfterCombatEnd, Ev::None);
                    assert_eq!(c.run.relics[0].counter, 7, "only BeforeCombatRewardOffered increments it");
                }
                RelicId::CloakClasp => {
                    let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    let ally = c.new_card(other, CardId::DefendIronclad, false).unwrap();
                    c.players[owner].hand.push(own);
                    c.players[other].hand.push(ally);
                    c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Enemy));
                    assert_eq!(c.cr(owner).block, 0);
                    c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
                    assert_eq!(c.cr(owner).block, 1);
                }
                RelicId::Nunchaku => {
                    c.run.relics[0].counter = 9;
                    let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                    let ally = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
                    let skill = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    for k in [ally, skill] { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
                    assert_eq!(c.run.relics[0].counter, 9);
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    assert_eq!(c.run.relics[0].counter, 10);
                    assert_eq!(c.players[owner].energy, 1);
                    c.is_over = true;
                    c.run.relics[0].counter = 19;
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    assert_eq!(c.run.relics[0].counter, 20);
                    assert_eq!(c.players[owner].energy, 1);
                }
                RelicId::Pocketwatch => {
                    let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                    let ally = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
                    for _ in 0..4 {
                        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(ally));
                        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    }
                    assert_eq!(c.run.relics[0].counter, 4);
                    c.players[owner].turn_number = 2;
                    c.round = 1;
                    c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
                    assert_eq!(c.modify_hand_draw(5, owner), 5);
                    for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own)); }
                    c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
                    for seat in 0..4 {
                        assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 8 } else { 5 });
                    }
                    c.players[owner].turn_number = 1;
                    assert_eq!(c.modify_hand_draw(5, owner), 5);
                    c.is_over = true;
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    assert_eq!(c.run.relics[0].counter, 0);
                    c.fire_relics(Hook::AfterCombatEnd, Ev::None);
                    assert_eq!(c.run.relics[0].counter2, 0);
                }
                RelicId::GamePiece => {
                    let own = c.new_card(owner, CardId::Inflame, false).unwrap();
                    let ally = c.new_card(other, CardId::Inflame, false).unwrap();
                    let draw = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    c.players[owner].draw.push(draw);
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(ally));
                    assert!(c.players[owner].hand.is_empty());
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    assert_eq!(c.players[owner].hand.as_slice(), &[draw]);
                    c.is_over = true;
                    let second = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    c.players[owner].draw.push(second);
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
                    assert_eq!(c.players[owner].hand.as_slice(), &[draw]);
                }
                RelicId::HornCleat => {
                    c.players[owner].turn_number = 2;
                    for seat in 0..4 { c.fire_relics(Hook::AfterBlockCleared, Ev::Creature(seat)); }
                    assert_eq!(c.cr(owner).block, 14);
                }
                RelicId::MercuryHourglass => {
                    let enemy = c.enemies[0];
                    c.cr_mut(enemy).block = 0;
                    let hp = c.cr(enemy).hp;
                    for seat in 0..4 { c.fire(Hook::AfterPlayerTurnStart, Ev::Creature(seat), Some(seat)); }
                    assert_eq!(c.cr(enemy).hp, hp - 3);
                }
                _ => unreachable!(),
            }
            for seat in 0..4 {
                if seat != owner {
                    assert_eq!(c.cr(seat).block, 0, "{id:?} owner {owner}");
                    assert_eq!(c.players[seat].energy, 0, "{id:?} owner {owner}");
                    assert_eq!(c.power_amount(seat, PowerId::Vigor), 0);
                }
            }
            assert!(c.fault.is_none(), "{id:?} owner {owner}: {:?}", c.fault);
        }
    }
}

#[test]
fn relic_census_attribution_keeps_the_first_owner_refusal() {
    for modifiers in [false, true] {
        let mut c = combat();
        // PureWater remains unadmitted; a later admitted Sozu must not erase
        // the first refusal, for either lifecycle or modifier dispatch.
        let ids = [RelicId::PureWater, RelicId::Sozu];
        for id in ids {
            let mut relic = crate::relic::Relic::new(id);
            relic.owner = 1;
            c.run.relics.push(relic);
        }
        if modifiers {
            c.max_energy(1);
        } else {
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        }
        assert_eq!(c.fault, Some("relic owner hooks not ported"));
        assert_eq!(c.fault_relic, Some(ids[0]));
        assert_eq!(c.cr(0).block, 0);
        assert_eq!(c.power_amount(0, PowerId::Strength), 0);
    }
}

#[test]
fn relic_lifecycle_admission_filters_without_touching_seat_zero() {
    for id in [
        RelicId::TheAbacus,
    ] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.round = 1;
        let mut relic = crate::relic::Relic::new(id);
        relic.owner = 1;
        c.run.relics.push(relic);
        match id {
            RelicId::TheAbacus => c.fire_relic_after_shuffle(1),
            _ => unreachable!(),
        }
        if id == RelicId::TheAbacus {
            assert!(c.fault.is_none());
            assert_eq!(c.cr(1).block, 6);
        } else {
            assert_eq!(c.fault, Some("relic owner hooks not ported"), "{id:?}");
        }
        assert_eq!(c.players[0].energy, 0);
        assert_eq!(c.cr(0).block, 0);
        assert_eq!(c.power_amount(0, PowerId::Dexterity), 0);
        assert!(c.players[0].hand.is_empty());
        assert!(c.players[0].draw.is_empty());
    }
    let mut c = combat();
    // Added-player constructors now supply three empty slots. Explicitly omit
    // that input to keep testing the missing-inventory integrity refusal.
    c.players[1].potions = None;
    let mut buckle = crate::relic::Relic::new(RelicId::BeltBuckle);
    buckle.owner = 1;
    buckle.flag1 = true;
    c.run.relics.push(buckle);
    c.fire_relic_after_potion_procured(1);
    assert_eq!(c.fault, Some("relic owner potion inventory not supplied"));
    assert_eq!(c.power_amount(0, PowerId::Dexterity), 0);
}

#[test]
fn inline_relic_interactions_and_player_predicates_do_not_leak_from_seat_zero() {
    for id in [RelicId::TheBoot] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        let enemy = c.enemies[0];
        c.run.relics.push(crate::relic::Relic::new(id));
        let power = if id == RelicId::PaperPhrog { PowerId::Vulnerable } else { PowerId::Intangible };
        c.apply_power_to(enemy, power, 2, None);
        c.cr_mut(enemy).block = 0;
        let hp = c.cr(enemy).hp;
        c.deal_damage(Some(1), enemy, 4, Props::MOVE);
        assert_eq!(c.cr(enemy).hp, hp - if id == RelicId::PaperPhrog { 6 } else { 1 });
        assert!(c.fault.is_none());
        c.run.relics[0].owner = 1;
        let before = c.cr(enemy).hp;
        c.deal_damage(Some(1), enemy, 4, Props::MOVE);
        assert!(c.fault.is_none());
        assert_eq!(before - c.cr(enemy).hp, 5);
    }
    for id in [RelicId::Sozu, RelicId::Fiddle, RelicId::RunicPyramid, RelicId::RingingTriangle] {
        let mut c = combat();
        c.round = 1;
        c.run.relics.push(crate::relic::Relic::new(id));
        assert_eq!(c.max_energy(0), if id == RelicId::Sozu { 4 } else { 3 });
        assert_eq!(c.should_draw(0, false), id != RelicId::Fiddle);
        assert_eq!(c.should_flush(0), !matches!(id, RelicId::RunicPyramid | RelicId::RingingTriangle));
        for seat in 1..4 {
            assert_eq!(c.max_energy(seat), 3);
            assert!(c.should_draw(seat, false));
            assert!(c.should_flush(seat));
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn letter_opener_preserves_first_turn_autoplay_count_and_resets_at_combat_start() {
    for owner in 0..4 {
        let mut c = combat();
        let mut relic = crate::relic::Relic::new(RelicId::LetterOpener);
        relic.owner = owner;
        relic.counter = 7;
        c.run.relics.push(relic);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        c.players[owner].turn_number = 1;
        c.run.relics[0].counter = 2;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.run.relics[0].counter, 2);
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.run.relics[0].counter, 0);
    }
}

#[test]
fn census_relic_group_one_is_owner_scoped_in_normal_dispatch() {
    for owner in 0..4 {
        for id in [RelicId::Anchor, RelicId::Kusarigama, RelicId::Permafrost, RelicId::StoneCracker, RelicId::Gorget] {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            let mut relic = crate::relic::Relic::new(id);
            relic.owner = owner;
            c.run.relics.push(relic);
            if id == RelicId::StoneCracker {
                for seat in 0..4 {
                    for card in [CardId::StrikeIronclad, CardId::DefendIronclad, CardId::Inflame] {
                        let k = c.new_card(seat, card, false).unwrap();
                        c.players[seat].draw.push(k);
                    }
                }
            }
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            match id {
                RelicId::Anchor => assert_eq!(c.cr(owner).block, 10),
                RelicId::Gorget => assert_eq!(c.power_amount(owner, PowerId::Plating), 4),
                RelicId::StoneCracker => {
                    for seat in 0..4 {
                        let upgraded = c.players[seat].draw.iter().filter(|&&k| c.cards[k as usize].upgraded).count();
                        assert_eq!(upgraded, if seat == owner { 2 } else { 0 });
                    }
                }
                RelicId::Permafrost | RelicId::Kusarigama => {
                    let card = if id == RelicId::Permafrost { CardId::Inflame } else { CardId::StrikeIronclad };
                    let enemy = c.enemies[0];
                    c.cr_mut(enemy).block = 0;
                    let hp = c.cr(enemy).hp;
                    for seat in 0..4 {
                        if seat != owner {
                            let k = c.new_card(seat, card, false).unwrap();
                            for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
                        }
                    }
                    assert_eq!(c.cr(owner).block, 0);
                    assert_eq!(c.cr(enemy).hp, hp);
                    let k = c.new_card(owner, card, false).unwrap();
                    for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
                    if id == RelicId::Permafrost { assert_eq!(c.cr(owner).block, 7); }
                    else {
                        assert_eq!(c.cr(enemy).hp, hp - 6);
                        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Enemy));
                        assert_eq!(c.run.relics[0].counter, 3);
                        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
                        assert_eq!(c.run.relics[0].counter, 0);
                    }
                    c.is_over = true;
                    let counter = c.run.relics[0].counter;
                    c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
                    assert_eq!(c.run.relics[0].counter, counter);
                }
                _ => unreachable!(),
            }
            for seat in 0..4 {
                if seat != owner {
                    assert_eq!(c.cr(seat).block, 0, "{id:?}");
                    assert_eq!(c.power_amount(seat, PowerId::Plating), 0);
                }
            }
            assert!(c.fault.is_none(), "{id:?} owner {owner}: {:?}", c.fault);
        }
    }
}

pub(super) fn combat() -> Combat {
    let rng = Rng::new(1);
    let run = RunCtx {
        choice_scheduling: crate::combat::ChoiceScheduling::Sequential,
        all_cards_unlocked: None,
        rngs: RunRngs {
            seed: 1, up_front: rng.clone(), shuffle: rng.clone(),
            unknown_map_point: rng.clone(), combat_card_generation: rng.clone(),
            combat_potion_generation: rng.clone(), combat_card_selection: rng.clone(),
            combat_energy_costs: rng.clone(), combat_targets: rng.clone(),
            monster_ai: rng.clone(), niche: rng.clone(), combat_orbs: rng.clone(),
            treasure_room: rng.clone(), rewards: rng.clone(), shops: rng.clone(),
            transformations: rng,
        },
        ascension: 0, act_index: 0, total_floor: 1, coord: None,
        room_type: RoomType::Monster, gold: 0, extra_gold_reward: 0,
        max_hp: 50, hp: 50, max_energy: 3,
        max_potion_slots: 0, base_orb_slots: 0, deck: vec![],
        paels_tooth_cards: vec![], relics: vec![], potions: vec![],
        next_card_id: 0, ref_cache: [0; REF_CACHE_WORDS],
    };
    let mut c = Combat::new(run, 1);
    for _ in 0..3 {
        c.add_player(50, 50, 3, 0, &[]);
    }
    c
}

#[test]
fn scrape_discards_only_its_actual_draw_results_and_ignores_star_cost() {
    let mut c=combat();c.run.relics.clear();
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));c.start_combat();
    c.players[0].hand.clear();c.players[0].draw.clear();c.players[0].discard.clear();
    let scrape=c.new_card(0,CardId::Scrape,false).unwrap();
    let existing=c.new_card(0,CardId::DefendDefect,false).unwrap();
    let drawn=c.new_card(0,CardId::DefendDefect,false).unwrap();
    let star=c.new_card(0,CardId::ParticleWall,false).unwrap();
    let curse=c.new_card(0,CardId::AscendersBane,false).unwrap();
    c.players[0].hand.extend([scrape,existing]);c.players[0].draw.extend([drawn,star,curse]);
    c.players[0].energy=3;
    assert!(c.play_card(0,0,Some(0)));
    assert!(c.players[0].hand.contains(&existing));
    assert!(c.players[0].hand.contains(&star));
    assert!(c.players[0].discard.contains(&drawn));
    assert!(c.players[0].discard.contains(&curse));
}

#[test]
fn dead_player_powers_are_removed_and_ostys_global_veto_blocks_reapplication() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    let osty = c.summon_osty(0, 5).unwrap();
    c.apply_power_to(0, PowerId::Weak, 2, Some(e));
    c.apply_power_to(0, PowerId::Frail, 2, Some(e));
    c.kill_creature(0);
    // Creature.RemoveAllPowersAfterDeath defaults to removal; DieForYou is exempt.
    assert!(c.cr(0).power_list().is_empty());
    assert!(c.cr(osty).is_dead());
    assert!(c.has_power(osty, PowerId::DieForYou));
    // Axebot.HammerUppercut applies debuffs after its attack. A retained dead
    // Osty's ShouldAllowHitting veto must still reject every dead player's target.
    assert!(c.apply_power_to(0, PowerId::Weak, 2, Some(e)).is_none());
    assert!(c.apply_power_to(0, PowerId::Frail, 2, Some(e)).is_none());
    assert!(c.cr(0).power_list().is_empty());
    assert!(c.apply_power_to(1, PowerId::Weak, 2, Some(e)).is_some());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn upgrading_ember_cards_preserves_the_enchanted_base_cost() {
    let mut c=combat();
    let k=c.new_card(0,CardId::StrikeNecrobinder,false).unwrap();
    assert!(crate::card::enchant::apply_static_enchantment(&mut c.cards[k as usize],"TezcatarasEmber",1));
    c.upgrade_card(k);
    assert_eq!(c.cards[k as usize].cost,0);
    assert!(c.cards[k as usize].has_kw(crate::tables::KW_ETERNAL));
    let mut run=crate::run::RunManager::new(7,crate::run::RunOptions::default()).run_ctx().clone();
    let mut card=crate::card::Card::from_factory(CardId::StrikeNecrobinder,false,1).unwrap();
    assert!(crate::card::enchant::apply_static_enchantment(&mut card,"TezcatarasEmber",1));
    run.deck=vec![card];run.upgrade_deck_card(0);
    assert_eq!(run.deck[0].cost,0);
    assert!(run.deck[0].has_kw(crate::tables::KW_ETERNAL));
}

#[test]
fn dead_player_without_die_for_you_can_receive_powers_per_can_receive_powers() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.kill_creature(0);
    // Creature.CanReceivePowers has no blanket IsDead guard.
    assert!(c.apply_power_to(0, PowerId::Weak, 2, Some(c.enemies[0])).is_some());
    assert_eq!(c.power_amount(0, PowerId::Weak), 2);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn entropy_then_tools_awaits_transform_before_opening_discard_selector() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for id in [CardId::StrikeIronclad, CardId::DefendIronclad] {
        let k = c.new_card(1, id, false).unwrap();
        c.players[1].hand.push(k);
    }
    c.apply_power_to(1, PowerId::Entropy, 1, Some(1));
    c.apply_power_to(1, PowerId::ToolsOfTheTrade, 1, Some(1));
    c.run.all_cards_unlocked = Some(true);
    c.fire_after_player_turn_start(1);
    assert!(c.fault.is_none());
    assert_eq!(c.pending_choice.as_ref().unwrap().resolver.src,
        crate::choice::ChoiceSrc::Card(CardId::Entropy));
    let old = c.choice_options[0];
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.pending_choice.as_ref().unwrap().resolver.src,
        crate::choice::ChoiceSrc::Card(CardId::ToolsOfTheTrade));
    assert!(!c.choice_options.contains(&old));
    assert_eq!(c.choice_options.as_slice(), c.players[1].hand.as_slice());
    assert!(c.resolve_pending_choice(Some(0)));
    assert!(c.pending_choice.is_none());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn second_choice_request_cannot_overwrite_the_first() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for id in [CardId::StrikeIronclad, CardId::DefendIronclad] {
        let k = c.new_card(1, id, false).unwrap();
        c.players[1].hand.push(k);
    }
    c.apply_power_to(1, PowerId::Entropy, 1, Some(1));
    c.fire_after_player_turn_start(1);
    let original = c.choice_options.clone();
    c.request_discard(1, 1);
    assert_eq!(c.choice_options, original);
    assert_eq!(c.pending_choice.as_ref().unwrap().resolver.src,
        crate::choice::ChoiceSrc::Card(CardId::Entropy));
    assert_eq!(c.fault, Some("choice continuation would overwrite an unresolved choice"));
}

#[test]
fn run_unlock_setting_survives_multiple_combat_instances() {
    use crate::run::{RunManager, RunOptions};
    for unlocked in [false, true] {
        let manager = RunManager::new(7, RunOptions {
            all_cards_unlocked: unlocked, ..RunOptions::default()
        });
        if !unlocked {
            assert_eq!(manager.unported, Some("partial card unlock configuration is not ported"));
        }
        let first = Combat::new(manager.run_ctx().clone(), 1);
        assert_eq!(first.run.all_cards_unlocked, Some(unlocked));
        let second = Combat::new(first.run, 2);
        assert_eq!(second.run.all_cards_unlocked, Some(unlocked));
    }
}

#[test]
fn entropy_amount_two_preserves_selection_order_and_exact_shared_rng_picks() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.run.all_cards_unlocked = Some(true);
    let mut originals = Vec::new();
    for _ in 0..3 {
        let k = c.new_card(1, CardId::StrikeIronclad, true).unwrap();
        c.players[1].hand.push(k);
        originals.push(k);
    }
    let (_, _, pool) = CSHARP_TRANSFORMS.iter().find(|(mp, id, _)|
        *mp && *id == card_info(CardId::StrikeIronclad, false).unwrap().entry).unwrap();
    let mut expected_rng = c.run.rngs.combat_card_selection.clone();
    let expected = [pool[expected_rng.choice_index(pool.len())],
        pool[expected_rng.choice_index(pool.len())]];
    c.apply_power_to(1, PowerId::Entropy, 2, Some(1));
    c.fire_after_player_turn_start(1);
    assert!(c.pending_choice.is_some(), "3 options > Amount 2 requires real choice");
    assert!(c.resolve_pending_choice(Some(2)));
    assert!(c.resolve_pending_choice(Some(0)));
    assert!(c.resolve_pending_choice(None));
    for (slot, name) in [(2, expected[0]), (0, expected[1])] {
        let k = c.players[1].hand[slot];
        assert_eq!(card_info(c.cards[k as usize].id, false).unwrap().entry, name);
        assert!(pool.contains(&name));
        assert!(!c.cards[k as usize].upgraded);
        assert_eq!(c.cards[k as usize].owner, 1);
    }
    assert_eq!(c.players[1].hand[1], originals[1]);
    assert_eq!(c.run.rngs.combat_card_selection.counter, expected_rng.counter);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn hand_of_greed_and_debt_modify_only_the_card_owners_gold() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.run.gold = 99;
    c.players[1].gold = Some(7);
    let e = c.enemies[0];
    c.cr_mut(e).hp = 1;
    let greed = c.new_card(1, CardId::HandOfGreed, false).unwrap();
    c.play_card_effect(greed, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.players[1].gold, Some(27));
    let debt = c.new_card(1, CardId::Debt, false).unwrap();
    c.card_turn_end_in_hand(debt, 1);
    assert_eq!(c.players[1].gold, Some(17));
    assert_eq!(c.run.gold, 99);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn turn_start_relics_wait_for_selection_before_hp_effects() {
    for (later, hp_after) in [(RelicId::RoyalPoison, 36), (RelicId::BloodVial, 42)] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.cr_mut(0).hp = 40;
        for id in [CardId::StrikeIronclad, CardId::DefendIronclad] {
            let k = c.new_card(0, id, false).unwrap();
            c.players[0].hand.push(k);
        }
        c.run.relics.push(crate::relic::Relic::new(RelicId::ToastyMittens));
        c.run.relics.push(crate::relic::Relic::new(later));
        c.fire_after_player_turn_start(0);
        assert!(c.pending_choice.is_some());
        assert_eq!(c.cr(0).hp, 40, "C# awaits Mittens before Royal Poison / late Blood Vial");
        assert!(c.resolve_pending_choice(Some(0)));
        assert_eq!(c.cr(0).hp, hp_after);
        assert!(c.pending_choice.is_none());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn scheduling_mode_is_run_persistent_and_training_defaults_live() {
    use crate::run::{RunManager, RunOptions};
    use crate::combat::ChoiceScheduling;
    assert_eq!(RunOptions::default().choice_scheduling, ChoiceScheduling::Live);
    for mode in [ChoiceScheduling::Sequential, ChoiceScheduling::Live] {
        let manager = RunManager::new(7, RunOptions { choice_scheduling: mode, ..RunOptions::default() });
        let first = Combat::new(manager.run_ctx().clone(), 1);
        assert_eq!(first.run.choice_scheduling, mode);
        let second = Combat::new(first.run, 2);
        assert_eq!(second.run.choice_scheduling, mode);
    }
}

#[test]
fn sequential_selector_pauses_before_later_seat_setup_and_lantern() {
    use crate::run::{RunManager, RunOptions};
    let manager = RunManager::new(7, RunOptions {
        choice_scheduling: crate::combat::ChoiceScheduling::Sequential, ..RunOptions::default()
    });
    let mut c = Combat::new(manager.run_ctx().clone(), 1);
    c.run.relics.clear();
    let deck: Vec<_> = (0..6).map(|i|
        crate::card::Card::from_factory(CardId::StrikeIronclad, false, i).unwrap()).collect();
    c.add_player(80, 80, 3, 0, &deck);
    c.run.relics.push(crate::relic::Relic::new(RelicId::ToastyMittens));
    c.run.relics.push(crate::relic::Relic::new(RelicId::Lantern));
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.players[0].energy, 3);
    assert_eq!(c.players[1].energy, 0);
    assert!(c.players[1].hand.is_empty());
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[0].energy, 4);
    assert_eq!(c.players[1].energy, 3);
    assert_eq!(c.players[1].hand.len(), 5);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn live_setup_presents_choice_after_all_seats_side_start_and_lantern() {
    for seats in 1..=4 {
        use crate::run::{RunManager, RunOptions};
        let manager = RunManager::new(7, RunOptions::default());
        let mut c = Combat::new(manager.run_ctx().clone(), 1);
        c.run.relics.clear();
        let deck: Vec<_> = (0..6).map(|i|
            crate::card::Card::from_factory(CardId::StrikeIronclad, false, i).unwrap()).collect();
        for _ in 1..seats { c.add_player(80, 80, 3, 0, &deck); }
        c.run.relics.push(crate::relic::Relic::new(RelicId::ToastyMittens));
        c.run.relics.push(crate::relic::Relic::new(RelicId::Lantern));
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.start_combat();
        assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
        assert_eq!(c.players[0].energy, 4, "Lantern side-start ran before FromHand");
        for owner in 1..seats as u8 {
            assert_eq!(c.players[owner].energy, 3);
            assert_eq!(c.players[owner].hand.len(), 5);
        }
        assert!(c.resolve_pending_choice(Some(0)));
        assert!(c.pending_choice.is_none());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn two_mittens_setup_choices_are_independent_in_live_and_sequential_still_awaits() {
    use crate::combat::ChoiceScheduling;
    use crate::run::{RunManager, RunOptions};
    for mode in [ChoiceScheduling::Live, ChoiceScheduling::Sequential] {
        let manager = RunManager::new(7, RunOptions { choice_scheduling: mode, ..RunOptions::default() });
        let mut c = Combat::new(manager.run_ctx().clone(), 1);
        c.run.relics.clear();
        let deck: Vec<_> = (100..106).map(|i|
            crate::card::Card::from_factory(CardId::StrikeIronclad, false, i).unwrap()).collect();
        c.add_player(80, 80, 3, 0, &deck);
        for owner in 0..2 {
            let mut relic = crate::relic::Relic::new(RelicId::ToastyMittens);
            relic.owner = owner;
            c.run.relics.push(relic);
        }
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.start_combat();
        assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
        if mode == ChoiceScheduling::Live {
            assert!(c.fault.is_none(), "{:?}", c.fault);
            assert!(c.pending_choice_for_player(0).is_some());
            assert!(c.pending_choice_for_player(1).is_some());
            assert!(c.players[0].exhaust.is_empty());
            assert!(c.players[1].exhaust.is_empty());
            let seat0 = c.choice_options_for_player(0).unwrap().to_vec();
            let seat1 = c.choice_options_for_player(1).unwrap().to_vec();
            assert!(seat0.iter().all(|&k| c.cards[k as usize].owner == 0));
            assert!(seat1.iter().all(|&k| c.cards[k as usize].owner == 1));
            assert!(c.resolve_pending_choice_for_player(1, Some(1)));
            assert!(c.pending_choice_for_player(1).is_none());
            assert_eq!(c.choice_options_for_player(0).unwrap(), seat0);
            assert_eq!(c.players[1].exhaust.as_slice(), &[seat1[1]]);
            assert!(c.resolve_pending_choice_for_player(0, Some(0)));
            assert_eq!(c.players[0].exhaust.as_slice(), &[seat0[0]]);
            assert!(c.pending_choice.is_none());
        } else {
            assert!(c.fault.is_none(), "{:?}", c.fault);
            assert!(c.resolve_pending_choice(Some(0)));
            assert_eq!(c.pending_choice.as_ref().unwrap().player, 1);
            assert!(c.resolve_pending_choice(Some(0)));
            assert!(c.pending_choice.is_none());
            assert_eq!(c.players[0].exhaust.len(), 1);
            assert_eq!(c.players[1].exhaust.len(), 1);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn live_from_hand_options_are_built_at_assignment_not_signal_time() {
    use crate::run::{RunManager, RunOptions};
    let manager = RunManager::new(7, RunOptions::default());
    let mut run = manager.run_ctx().clone();
    run.deck.truncate(6);
    let mut c = Combat::new(run, 1);
    c.run.relics.clear();
    let mut imbued = crate::card::Card::from_factory(CardId::Tutor, false, 100).unwrap();
    imbued.enchantments.push((crate::identity::EnchantmentId::Imbued, 1));
    let mut deck = vec![imbued];
    for i in 101..107 {
        deck.push(crate::card::Card::from_factory(CardId::StrikeIronclad, false, i).unwrap());
    }
    c.add_player(80, 80, 3, 0, &deck);
    c.run.relics.push(crate::relic::Relic::new(RelicId::ToastyMittens));
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    assert_eq!(c.players[0].hand.len(), 6, "later seat Tutor changes chooser's hand between signal and assignment");
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.choice_options, c.players[0].hand.to_vec());
    assert!(c.resolve_pending_choice(Some(0)));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn automatic_foregone_conclusion_then_mittens_keeps_both_live_choosers_open() {
    use crate::run::{RunManager, RunOptions};
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let deck = run.deck.clone();
    let mut c = Combat::new(run, 1);
    c.run.relics.clear();
    c.add_player(80, 80, 3, 0, &deck);
    for owner in 0..2 {
        let mut relic = crate::relic::Relic::new(RelicId::ToastyMittens);
        relic.owner = owner;
        c.run.relics.push(relic);
    }
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.apply_power_to(0, PowerId::ForegoneConclusion, 20, Some(0));
    c.start_combat();
    assert!(c.fault.is_none(), "{:?}", c.fault);
    for owner in 0..2 {
        assert!(matches!(c.pending_choice_for_player(owner).unwrap().resolver.src,
            crate::choice::ChoiceSrc::Relic(RelicId::ToastyMittens)));
    }
    assert!(c.resolve_pending_choice_for_player(1, Some(0)));
    assert!(c.pending_choice_for_player(0).is_some());
    assert!(c.resolve_pending_choice_for_player(0, Some(0)));
    assert!(c.pending_choice.is_none());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn burning_blood_victory_heals_each_owning_seat_and_caps_at_max_hp() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for owner in 0..c.players.len() as CId {
        c.cr_mut(owner).hp = if owner == 0 { 40 } else { 48 };
        let mut relic = crate::relic::Relic::new(RelicId::BurningBlood);
        relic.owner = owner;
        c.run.relics.push(relic);
    }
    c.fire_after_combat_victory();
    assert_eq!(c.cr(0).hp, 46);
    for owner in 1..c.players.len() as CId { assert_eq!(c.cr(owner).hp, 50); }
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn combat_end_cancels_active_and_parked_live_selectors() {
    use crate::run::{RunManager, RunOptions};
    for victory in [true, false] {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        c.run.relics.clear();
        c.add_player(80, 80, 3, 0, &deck);
        for owner in 0..2 {
            let mut relic = crate::relic::Relic::new(RelicId::ToastyMittens);
            relic.owner = owner;
            c.run.relics.push(relic);
        }
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.start_combat();
        assert!(c.pending_choice_for_player(0).is_some());
        assert!(c.pending_choice_for_player(1).is_some());
        if victory {
            for enemy in c.enemies.clone() { c.kill_creature(enemy); }
        } else {
            c.kill_creature(0);
            c.kill_creature(1);
        }
        c.check_combat_end();
        assert!(c.is_over);
        assert!(c.pending_choice.is_none());
        assert!(c.live_choices.is_empty());
        for owner in 0..2 {
            assert!(!c.activate_choice_for_player(owner));
            assert!(c.pending_choice_for_player(owner).is_none());
        }
        assert!(c.pending_turn_setup.is_none());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn teammate_before_hand_draw_powers_are_noops_for_the_starting_seat() {
    for id in [PowerId::CreativeAi, PowerId::SpectrumShift, PowerId::ForegoneConclusion] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.apply_power_to(1, id, 1, Some(1));
        let rng = c.run.rngs.combat_card_generation.counter;
        c.fire_setup_hooks(0, &[Hook::BeforeHandDraw], crate::combat::TurnStage::Draw);
        assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        assert!(c.players[1].hand.is_empty());
        assert_eq!(c.run.rngs.combat_card_generation.counter, rng);
    }
}

#[test]
fn creative_ai_uses_owners_multiplayer_character_power_pool_and_exact_rng_draws() {
    let mut c = combat();
    c.run.all_cards_unlocked = Some(true);
    c.players[3].character = Some(Character::Defect);
    c.set_character_pool(3, Character::Defect).unwrap();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));

    let (_, _, audited) = CSHARP_TRANSFORMS.iter().find(|(mp, id, _)|
        *mp && *id == "STRIKE_DEFECT").unwrap();
    let power_names: Vec<_> = audited.iter().copied().filter(|name| {
        CardId::ALL.into_iter().find(|id|
            id.name().strip_suffix("_CARD").unwrap_or(id.name()) == *name)
            .and_then(|id| card_info(id, false))
            .is_some_and(|info| info.card_type == CardType::Power)
    }).collect();
    assert_eq!(power_names.len(), 20, "audited four-player Defect Power pool");

    let p = c.apply_power_to(3, PowerId::CreativeAi, 1, Some(3)).unwrap();
    let before = c.run.rngs.combat_card_generation.counter;
    assert!(c.defect_before_hand_draw(p, 3));
    assert_eq!(c.run.rngs.combat_card_generation.counter - before, 19);
    assert_eq!(c.players[3].hand.len(), 1);
    let generated = &c.cards[c.players[3].hand[0] as usize];
    assert!(power_names.contains(&card_info(generated.id, false).unwrap().entry));
    assert_eq!(generated.owner, 3);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn unceasing_top_during_play_belongs_to_the_actor() {
    for top_owner in [0, 1] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        let mut top = crate::relic::Relic::new(RelicId::UnceasingTop);
        top.owner = top_owner;
        c.run.relics.push(top);
        let play = c.new_card(1, CardId::DefendIronclad, false).unwrap();
        let draw = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
        c.players[1].hand.push(play);
        c.players[1].draw.push(draw);
        c.players[1].energy = 3;
        assert!(c.play_card(1, 0, None));
        assert_eq!(c.players[1].hand.contains(&draw), top_owner == 1);
        assert!(c.players[0].hand.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn live_toolbox_resumes_draw_after_its_selection() {
    use crate::run::{RunManager, RunOptions};
    let manager = RunManager::new(7, RunOptions::default());
    let mut c = Combat::new(manager.run_ctx().clone(), 1);
    c.run.relics.clear();
    c.run.relics.push(crate::relic::Relic::new(RelicId::Toolbox));
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    assert!(c.fault.is_none(), "{:?}", c.fault);
    assert!(c.players[0].hand.is_empty());
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[0].hand.len(), 6);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn live_later_seat_checks_empty_hand_before_first_seat_choice_opens() {
    use crate::run::{RunManager, RunOptions};
    let manager = RunManager::new(7, RunOptions::default());
    let mut c = Combat::new(manager.run_ctx().clone(), 1);
    c.run.relics.clear();
    let deck: Vec<_> = (100..106).map(|id|
        crate::card::Card::from_factory(CardId::StrikeIronclad, false, id).unwrap()).collect();
    c.add_player(80, 80, 3, 0, &deck);
    c.run.relics.push(crate::relic::Relic::new(RelicId::ToastyMittens));
    let mut top = crate::relic::Relic::new(RelicId::UnceasingTop);
    top.owner = 1;
    c.run.relics.push(top);
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    // Mind Rot reduces the automatic hand draw, not Unceasing Top's extra draw.
    c.apply_power_to(1, PowerId::MindRot, 5, Some(1));
    c.start_combat();
    assert!(c.fault.is_none(), "{:?}", c.fault);
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.players[1].hand.len(), 1, "AutoPrePlay accepts Top before executor unpause");
    assert_eq!(c.hist.extra_draws_this_turn[1], 1);
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[1].hand.len(), 1, "later-seat auto-pre-play must not run twice");
}

#[test]
fn turn_start_walk_keeps_listener_owner_separate_from_starting_player() {
    for mode in [crate::combat::ChoiceScheduling::Sequential, crate::combat::ChoiceScheduling::Live] {
        let mut c = combat();
        c.run.choice_scheduling = mode;
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for owner in 0..3 {
            for id in [CardId::StrikeIronclad, CardId::DefendIronclad] {
                let k = c.new_card(owner, id, false).unwrap();
                c.players[owner].hand.push(k);
            }
        }
        c.apply_power_to(2, PowerId::ToolsOfTheTrade, 1, Some(2));
        for id in [RelicId::ToastyMittens, RelicId::BloodVial] {
            let mut relic = crate::relic::Relic::new(id);
            relic.owner = 1;
            c.run.relics.push(relic);
        }
        c.cr_mut(1).hp = 40;
        c.fire_after_player_turn_start(0);
        assert!(c.pending_choice.is_none(), "teammate listeners guard player == Owner");
        c.fire_after_player_turn_start(1);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, 1);
        assert_eq!(c.cr(1).hp, 40);
        assert!(c.resolve_pending_choice(Some(0)));
        assert_eq!(c.players[1].exhaust.len(), 1);
        assert_eq!(c.power_amount(1, PowerId::Strength), 1);
        assert_eq!(c.cr(1).hp, 42, "Late runs after awaited selector");
        assert_eq!(c.players[0].hand.len(), 2);
        assert_eq!(c.players[2].hand.len(), 2);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn targeted_potion_selectors_use_the_target_seat_and_keep_card_ownership() {
    for potion in [PotionId::Ashwater, PotionId::Gamblersbrew,
        PotionId::Touchofinsanity, PotionId::Liquidmemories, PotionId::Dropletofprecognition] {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [CardId::StrikeIronclad, CardId::DefendIronclad] {
            let k = c.new_card(2, id, false).unwrap();
            match potion {
                PotionId::Liquidmemories => c.players[2].discard.push(k),
                PotionId::Dropletofprecognition => c.players[2].draw.push(k),
                _ => c.players[2].hand.push(k),
            }
        }
        c.potion_effect_for_player(1, potion, Some(2));
        assert_eq!(c.pending_choice.as_ref().unwrap().player, 2);
        let selected = c.choice_options[0];
        assert!(c.resolve_pending_choice(Some(0)));
        if c.pending_choice.is_some() { assert!(c.resolve_pending_choice(None)); }
        assert_eq!(c.cards[selected as usize].owner, 2);
        assert!(c.players[0].hand.is_empty());
        assert!(c.players[1].hand.is_empty());
        assert!(c.fault.is_none(), "{potion:?}: {:?}", c.fault);
    }
}

#[test]
fn victory_revive_reactivates_burning_blood_before_victory_hooks() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c.run.relics.push(crate::relic::Relic::new(RelicId::BurningBlood));
    c.kill_creature(0);
    assert!(!c.players[0].hooks_active);
    for e in c.enemies.clone() { c.kill_creature(e); }
    assert!(c.won);
    assert!(c.players[0].hooks_active);
    assert_eq!(c.cr(0).hp, 7, "ReviveBeforeCombatEnd 1 HP then Burning Blood 6");
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn autoplay_card_callback_death_invalidates_remaining_card_listeners() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.apply_power_to(e, PowerId::Thorns, 2, Some(e));
    c.cr_mut(1).hp = 1;
    let first = c.new_card(1, CardId::HowlFromBeyond, false).unwrap();
    let second = c.new_card(1, CardId::HowlFromBeyond, false).unwrap();
    c.players[1].exhaust.extend([first, second]);
    // HowlFromBeyond.cs:32-38 runs at AutoPostPlay (Hook.cs:699-711),
    // not BeforeHandDraw; the first callback's death invalidates the next card.
    c.fire_before_turn_end(Side::Player);
    assert!(c.cr(1).is_dead());
    assert!(c.players[1].exhaust.is_empty());
    assert!(c.hist.play_start_cards.contains(&first));
    assert!(!c.hist.play_start_cards.contains(&second));
    assert!(!c.is_over);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn poison_turn_start_death_invalidates_later_callbacks_in_the_same_snapshot() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let osty = c.summon_osty(1, 8).unwrap();
    c.cr_mut(1).hp = 1;
    // C# PoisonPower.AfterSideTurnStart/Trigger kills first; PrepTime's pending
    // callback must fail CombatState.IsModelValidForHooks after DeactivateHooks.
    c.apply_power_to(1, PowerId::Poison, 1, None);
    c.apply_power_to(1, PowerId::PrepTime, 3, Some(1));
    c.apply_power_to(2, PowerId::PrepTime, 4, Some(2));
    c.fire_after_side_turn_start(Side::Player);
    assert!(c.cr(1).is_dead());
    assert!(c.cr(osty).is_dead());
    assert!(!c.players[1].hooks_active);
    assert!(!c.has_power(1, PowerId::Vigor));
    assert_eq!(c.power_amount(2, PowerId::Vigor), 4);
    assert!(!c.is_over);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn thorns_reaction_kills_card_owner_without_ending_teammates_play_phase() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.apply_power_to(e, PowerId::Thorns, 2, Some(e));
    c.cr_mut(1).hp = 1;
    let osty = c.summon_osty(1, 8).unwrap();
    let strike = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
    c.players[1].hand.push(strike);
    c.players[1].energy = 3;
    // C# ThornsPower.BeforeDamageReceived -> CreatureCmd.Kill during OnPlayWrapper.
    assert!(c.play_card(1, 0, Some(0)));
    assert!(c.cr(1).is_dead());
    assert!(c.cr(osty).is_dead());
    assert!(c.pending_play.is_empty());
    assert!(c.players[1].play.is_empty());
    assert!(c.players[1].discard.is_empty());
    assert!(c.players[1].hand.is_empty());
    assert_eq!(c.players[1].energy, 0);
    assert!(!c.players[1].hooks_active);
    assert!(!c.is_over);
    let defend = c.new_card(2, CardId::DefendIronclad, false).unwrap();
    c.players[2].hand.push(defend);
    c.players[2].energy = 3;
    assert!(c.play_card(2, 0, None));
    assert_eq!(c.cr(2).block, 5);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn constrict_turn_end_death_does_not_stop_remaining_players_or_the_next_round() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.cr_mut(1).hp = 1;
    c.apply_power_to(1, PowerId::Constrict, 1, Some(e));
    c.apply_power_to(2, PowerId::Constrict, 1, Some(e));
    // C# ConstrictPower.AfterSideTurnEnd damages each participating owner.
    c.end_player_turn();
    assert!(c.cr(1).is_dead());
    assert!(!c.players[1].hooks_active);
    assert_eq!(c.players[1].energy, 0);
    assert!(c.players[1].hand.is_empty());
    assert!(c.cr(2).is_alive());
    assert!(c.cr(2).hp < 50);
    assert_eq!(c.round, 2);
    assert!(!c.is_over);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn player_turn_death_cleans_osty_piles_and_continues_the_living_players_turn() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let osty = c.summon_osty(1, 8).unwrap();
    let card = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    c.players[1].hand.push(card);
    c.players[1].energy = 3;
    c.players[1].stars = 4;
    c.cr_mut(1).hp = 1;
    c.deal_damage(Some(1), 1, 1, Props::UNPOWERED.or(Props::UNBLOCKABLE));
    assert!(c.cr(1).is_dead());
    assert!(c.cr(osty).is_dead());
    assert!(!c.players[1].hooks_active);
    assert!(c.players[1].hand.is_empty());
    assert_eq!(c.players[1].energy, 0);
    assert_eq!(c.players[1].stars, 0);
    assert!(!c.is_over);
    let ally = c.new_card(2, CardId::DefendIronclad, false).unwrap();
    c.players[2].hand.push(ally);
    c.players[2].energy = 3;
    assert!(c.play_card(2, 0, None));
    assert_eq!(c.cr(2).block, 5);
    c.end_player_turn();
    assert_eq!(c.round, 2);
    assert!(c.cr(2).is_alive());
    assert_eq!(c.players[1].energy, 0);
    assert!(c.players[1].hand.is_empty());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn entropy_requires_unlock_configuration_and_uses_shared_selection_rng() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let old = c.new_card(1, CardId::StrikeIronclad, true).unwrap();
    c.players[1].hand.push(old);
    c.apply_power_to(1, PowerId::Entropy, 1, Some(1));
    c.run.all_cards_unlocked = Some(true);
    let before = c.run.rngs.combat_card_selection.counter;
    let transforms = c.run.rngs.transformations.counter;
    c.fire_after_player_turn_start(1);
    let new = c.players[1].hand[0];
    assert_ne!(new, old);
    assert_eq!(c.cards[new as usize].owner, 1);
    assert!(!c.cards[new as usize].upgraded);
    assert_ne!(c.cards[new as usize].id, CardId::StrikeIronclad);
    assert_eq!(c.run.rngs.combat_card_selection.counter, before + 1);
    assert_eq!(c.run.rngs.transformations.counter, transforms);
    use crate::piles::Lifecycle;
    assert!(c.hist.card_lifecycle.ends_with(&[
        Lifecycle::Changed(new, PileKind::Hand),
        Lifecycle::ChangedLate(new, PileKind::Hand),
        Lifecycle::TransformedFrom(old),
        Lifecycle::TransformedTo(new),
        Lifecycle::Generated(new, Some(1)),
    ]), "{:?}", c.hist.card_lifecycle);
    assert!(c.fault.is_none(), "{:?}", c.fault);
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let old = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
    c.players[1].hand.push(old);
    c.apply_power_to(1, PowerId::Entropy, 1, Some(1));
    c.fire_after_player_turn_start(1);
    assert_eq!(c.fault, Some("Entropy requires explicit all-unlocked run configuration"));
    assert_eq!(c.players[1].hand[0], old);
}

#[test]
fn gremlin_merc_steals_per_target_and_returns_only_that_targets_reward() {
    let mut c = combat();
    c.setup_encounter("GREMLIN_MERC_NORMAL", &mut Rng::new(1));
    let merc = c.enemies[0];
    c.run.gold = 45;
    c.players[1].gold = Some(7);
    c.players[2].gold = Some(0);
    c.players[3].gold = Some(99);
    c.cr_mut(3).hp = 0;
    c.gremlin_merc_steal(merc);
    assert_eq!(c.run.gold, 25);
    assert_eq!(c.players[1].gold, Some(0));
    assert_eq!(c.players[3].gold, Some(99));
    let thefts: Vec<_> = c.cr(merc).power_list().iter().map(|&p| &c.powers[p as usize])
        .filter(|p| p.id == PowerId::Thievery).map(|p| (p.target, p.vars[0])).collect();
    assert_eq!(thefts, vec![(Some(0), 20), (Some(1), 7), (Some(2), 0), (Some(3), 0)]);
    c.kill_creature(merc);
    let fat = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == "FAT_GREMLIN").unwrap();
    c.kill_creature(fat);
    assert_eq!(c.run.extra_gold_reward, 20);
    assert_eq!(c.players[1].extra_gold_reward, 7);
    assert_eq!(c.players[2].extra_gold_reward, 0);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn gremlin_merc_fails_closed_on_unknown_gold_and_escape_returns_nothing() {
    let mut c = combat();
    c.setup_encounter("GREMLIN_MERC_NORMAL", &mut Rng::new(1));
    let merc = c.enemies[0];
    c.gremlin_merc_steal(merc);
    assert_eq!(c.fault, Some("THIEVERY requires target player's initial gold"));
    let mut c = combat();
    c.setup_encounter("GREMLIN_MERC_NORMAL", &mut Rng::new(1));
    let merc = c.enemies[0];
    c.run.gold = 25;
    for p in 1..4 { c.players[p].gold = Some(25); }
    c.gremlin_merc_steal(merc);
    c.kill_creature(merc);
    let fat = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == "FAT_GREMLIN").unwrap();
    c.escape_monster(fat);
    assert_eq!(c.run.extra_gold_reward, 0);
    assert!(c.players.0.iter().all(|p| p.extra_gold_reward == 0));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn midnight_counts_global_exhausts_and_entered_history() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let midnight = c.new_card(1, CardId::Midnight, false).unwrap();
    c.players[1].hand.push(midnight);
    let exhausted = c.new_card(2, CardId::Shiv, false).unwrap();
    c.players[2].exhaust.push(exhausted);
    c.fire_after_card_exhausted(exhausted);
    assert_eq!(c.cards[midnight as usize].cost, 11);
    let entered = c.new_card(3, CardId::Midnight, true).unwrap();
    c.players[3].hand.push(entered);
    c.fire_card_after_card_entered_combat(entered);
    assert_eq!(c.cards[entered as usize].cost, 11);
    assert_eq!(c.cards[entered as usize].damage, Some(72));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn the_ball_passes_to_living_teammate_and_keeps_damage_growth() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.cr_mut(2).hp = 0;
    c.cr_mut(3).hp = 0;
    let ball = c.new_card(0, CardId::TheBall, true).unwrap();
    c.players[0].hand.push(ball);
    c.players[0].energy = 3;
    assert!(c.play_card(0, 0, Some(0)));
    assert_eq!(c.cards[ball as usize].owner, 1);
    assert!(c.players[1].draw.contains(&ball));
    assert_eq!(c.cards[ball as usize].damage, Some(25));
    c.downgrade_card_for_dampen(ball);
    assert_eq!(c.cards[ball as usize].damage, Some(25));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn burrowed_stuns_only_tunneler_and_resumes_bite() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.apply_power_to(e, PowerId::Burrowed, 1, Some(e));
    c.power_after_block_broken(e);
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.current, crate::monster::STUNNED);
    assert_eq!(ai.state_id(ai.stun_follow_up), "BITE_MOVE");
    assert!(!c.has_power(e, PowerId::Burrowed));
    c.apply_power_to(e, PowerId::Burrowed, 1, Some(e));
    c.cr_mut(e).hp = 0;
    c.power_after_block_broken(e);
    assert!(!c.has_power(e, PowerId::Burrowed));
    for p in 1..4 {
        c.apply_power_to(p, PowerId::Burrowed, 1, Some(p));
        c.power_after_block_broken(p);
        assert!(c.has_power(p, PowerId::Burrowed));
    }
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn gamblers_brew_uses_selected_players_discard_history_and_draw() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for p in 1..4 {
        let discarded = c.new_card(p, CardId::DefendIronclad, false).unwrap();
        let drawn = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
        c.players[p].hand.push(discarded);
        c.players[p].draw.push(drawn);
        c.resolve_choice_effect(crate::choice::Resolver {
            src: crate::choice::ChoiceSrc::Potion(PotionId::Gamblersbrew),
            k: 0, target: Some(p), step: 2, arg: 0,
        }, &[discarded]);
        assert_eq!(c.cards[discarded as usize].owner, p);
        assert_eq!(c.players[p].discard.as_slice(), &[discarded]);
        assert_eq!(c.players[p].hand.as_slice(), &[drawn]);
    }
    assert_eq!(c.hist.discarded_this_turn, 3);
    assert!(c.players[PLAYER].hand.is_empty());
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn untargeted_damage_keeps_dealer_modifiers_but_not_target_modifiers() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let p = 1;
    let shiv = c.new_card(p, CardId::Shiv, false).unwrap();
    let outer = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
    c.active_card = Some(outer);
    c.cards[shiv as usize].energy_spent = 1;
    c.apply_power_to(p, PowerId::Accuracy, 3, Some(p));
    c.apply_power_to(p, PowerId::PhantomBlades, 2, Some(p));
    c.apply_power_to(p, PowerId::OneForAll, 4, Some(p));
    c.apply_power_to(p, PowerId::DoubleDamage, 1, Some(p));
    // These target-side powers must be neutral even when the dealer owns them.
    c.apply_power_to(p, PowerId::Tracking, 50, Some(p));
    c.apply_power_to(p, PowerId::Conqueror, 1, Some(p));
    c.apply_power_to(p, PowerId::Vulnerable, 1, Some(p));
    assert_eq!(c.modify_untargeted_card_damage(4, p, shiv), (4 + 3 + 2 + 4) * 2);
    let other_shiv = c.new_card(PLAYER, CardId::Shiv, false).unwrap();
    assert_eq!(c.modify_untargeted_card_damage(4, PLAYER, other_shiv), 4);
    assert_eq!(c.active_card, Some(outer));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn untargeted_damage_supports_osty_calcify_and_source_owned_multipliers() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let p = 1;
    let o = c.summon_osty(p, 10).unwrap();
    let card = c.new_card(p, CardId::Thrash, false).unwrap();
    c.apply_power_to(p, PowerId::Calcify, 4, Some(p));
    c.apply_power_to(p, PowerId::Lethality, 50, Some(p));
    c.apply_power_to(p, PowerId::Gigantification, 1, Some(p));
    // Neither Hang nor Knockdown can match a missing target.
    let enemy = c.enemies[0];
    c.apply_power_to(enemy, PowerId::Hang, 5, Some(p));
    c.apply_power_to(enemy, PowerId::Knockdown, 4, Some(p));
    assert_eq!(c.modify_untargeted_card_damage(8, o, card), (8 + 4) * 3 * 3 / 2);
    assert_eq!(c.modify_untargeted_card_damage(8, p, card), 8 * 3 * 3 / 2);
    assert_eq!(c.power_amount(p, PowerId::Gigantification), 1); // preview consumes nothing
    assert_eq!(c.fault, None);
}

#[test]
fn one_for_all_uses_card_owner_for_osty_attacks() {
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let p = 1;
    let o = c.summon_osty(p, 10).unwrap();
    let card = c.new_card(p, CardId::Poke, false).unwrap();
    c.active_card = Some(card);
    c.apply_power_to(p, PowerId::OneForAll, 3, Some(p));
    let enemy = c.enemies[0];
    assert_eq!(c.modify_damage(6, Some(o), enemy, Props::MOVE, Some(card)), 9);
    assert_eq!(c.fault, None);
}
