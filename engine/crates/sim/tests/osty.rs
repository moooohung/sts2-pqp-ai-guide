//! Ground truth: Commands/OstyCmd.Summon, CreatureCmd.Damage/Kill, DieForYouPower.
use sts2_sim::combat::{Combat, Props, RunCtx, RunRngs, REF_CACHE_WORDS};
use sts2_sim::creature::Creature;
use sts2_sim::rng::Rng;
use sts2_sim::tables::{PowerId, RoomType};

fn combat() -> Combat {
    let rng = Rng::new(1);
    let run = RunCtx {
        choice_scheduling: sts2_sim::combat::ChoiceScheduling::Sequential,
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
    Combat::new(run, 1)
}

fn hand_card(c: &mut Combat, owner: u8, id: sts2_sim::tables::CardId) -> u16 {
    let k = c.new_card(owner, id, false).unwrap();
    c.players[owner].hand.push(k);
    c.fire_card_after_card_entered_combat(k);
    k
}

fn enemy(c: &mut Combat, hp: i32) -> u8 {
    let e = c.creatures.len() as u8;
    let mut cr = Creature::player(hp, hp);
    cr.is_player = false;
    cr.side = sts2_sim::creature::Side::Enemy;
    c.creatures.push(cr);
    c.enemies.push(e);
    e
}

#[test]
fn sentry_mode_generates_base_gazes_for_only_its_owners_before_hand_draw() {
    use sts2_sim::{hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let k = c.new_card(p, CardId::SentryMode, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.cards[k as usize].original_cost, 1);
    c.apply_power_to(p, PowerId::SentryMode, 1, Some(p));
    c.fire(Hook::BeforeHandDraw, Ev::Creature(0), None);
    assert!(c.players[p].hand.is_empty());
    c.fire(Hook::BeforeHandDraw, Ev::Creature(p), None);
    assert_eq!(c.players[p].hand.len(), 2);
    for &q in &c.players[p].hand {
        assert_eq!(c.cards[q as usize].id, CardId::SweepingGaze);
        assert!(!c.cards[q as usize].upgraded);
        assert!(c.cards[q as usize].is_ethereal());
        assert_ne!(c.cards[q as usize].keywords & sts2_sim::tables::KW_EXHAUST, 0);
    }
    assert_eq!(c.hist.generated_combat, 2);
    // Each new token is still created/added with a full hand (overflow to Discard).
    while c.players[p].hand.len() < sts2_sim::combat::MAX_HAND_SIZE {
        hand_card(&mut c, p, CardId::DefendIronclad);
    }
    c.fire(Hook::BeforeHandDraw, Ev::Creature(p), None);
    assert_eq!(c.players[p].discard.len(), 2);
    assert_eq!(c.hist.generated_combat, 4);
    assert!(c.players[0].hand.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn sweeping_gaze_random_pet_attack_uses_targets_rng_and_missing_pet_skips_it() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let a = enemy(&mut c, 100);
    let b = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::SweepingGaze, true).unwrap();
    let before = c.run.rngs.combat_targets.counter;
    c.play_card_effect(k, None);
    assert_eq!(c.run.rngs.combat_targets.counter, before);
    let pet = c.summon_osty(p, 5).unwrap();
    c.apply_power_to(pet, PowerId::Strength, 3, Some(p));
    c.apply_power_to(p, PowerId::Strength, 100, Some(p));
    let mut expected_rng = c.run.rngs.combat_targets.clone();
    let chosen = [a, b][expected_rng.choice_index(2)];
    c.play_card_effect(k, None);
    assert_eq!(c.cr(chosen).hp, 82); // upgraded 15 + pet Strength 3
    assert_eq!(c.cr(a).hp + c.cr(b).hp, 182);
    assert_eq!(c.run.rngs.combat_targets.counter, before + 1);
    assert_eq!(c.hist.attacked_this_turn.as_slice(), &[pet]);
    assert_eq!(c.fault, None);
}

#[test]
fn spirit_of_ash_blocks_on_owned_ethereal_plays_not_exhausts_or_teammate_plays() {
    use sts2_sim::{hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let power = c.new_card(p, CardId::SpiritOfAsh, true).unwrap();
    c.play_card_effect(power, None);
    c.apply_power_to(p, PowerId::Dexterity, 100, Some(p));
    let theirs = hand_card(&mut c, 0, CardId::SweepingGaze);
    c.fire(Hook::BeforeCardPlayed, Ev::Card(theirs), None);
    assert_eq!(c.cr(p).block, 0);
    let gaze = hand_card(&mut c, p, CardId::SweepingGaze);
    assert!(c.play_card(p, 0, None)); // Ethereal counts even with no pet.
    assert_eq!(c.cr(p).block, 5);
    c.fire_after_card_exhausted(gaze);
    assert_eq!(c.cr(p).block, 5);
    let ordinary = hand_card(&mut c, p, CardId::DefendIronclad);
    c.fire(Hook::BeforeCardPlayed, Ev::Card(ordinary), None);
    assert_eq!(c.cr(p).block, 5);
    assert_eq!(c.fault, None);
}

#[test]
fn bury_is_canonical_powered_attack_and_reanimate_grows_or_revives_owners_pet() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 200);
    let base = c.new_card(p, CardId::Bury, false).unwrap();
    let upgraded = c.new_card(p, CardId::Bury, true).unwrap();
    c.play_card_effect(base, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 148);
    c.play_card_effect(upgraded, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 85);
    let pet = c.summon_osty(p, 5).unwrap();
    let reanimate = c.new_card(p, CardId::Reanimate, false).unwrap();
    c.play_card_effect(reanimate, None);
    assert_eq!(c.cr(pet).hp, 25);
    c.kill_creature(pet);
    c.upgrade_card(reanimate);
    c.play_card_effect(reanimate, None);
    assert_eq!(c.osty(p), Some(pet));
    assert_eq!(c.cr(pet).hp, 25);
    assert_eq!(c.cr(pet).max_hp, 25);
    assert_eq!(c.osty(0), None);
    assert_eq!(c.fault, None);
}

#[test]
fn enthralled_blocks_only_manual_plays_for_its_owner_while_in_hand() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let curse = hand_card(&mut c, p, CardId::Enthralled);
    let defend = hand_card(&mut c, p, CardId::DefendIronclad);
    let other = hand_card(&mut c, 0, CardId::DefendIronclad);
    c.players[0].energy = 3;
    c.players[p].energy = 3;
    assert!(!c.can_play_card(defend));
    assert!(c.can_play_card(curse));
    assert!(c.can_play_card(other));
    assert!(c.cards_allow_hand_card_play(defend, true));
    assert!(c.auto_play_card(defend, None, false));
    assert_eq!(c.cr(p).block, 5);
    assert!(c.players[p].hand.contains(&curse));
    assert!(c.play_card(p, 0, None));
    let another = hand_card(&mut c, p, CardId::DefendIronclad);
    assert!(c.can_play_card(another));
    // A curse elsewhere in combat does not restrict the player's hand.
    c.players[p].draw.push(curse);
    assert!(c.can_play_card(another));
    assert_eq!(c.fault, None);
}

#[test]
fn end_of_days_applies_doom_before_immediate_kill_snapshot_and_respects_artifact() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let a = enemy(&mut c, 37);
    let b = enemy(&mut c, 38);
    let blocked = enemy(&mut c, 1);
    c.apply_power_to(blocked, PowerId::Artifact, 1, None);
    let stacked = enemy(&mut c, 50);
    c.apply_power_to(stacked, PowerId::Doom, 13, Some(0));
    let k = c.new_card(0, CardId::EndOfDays, true).unwrap();
    c.play_card_effect(k, None);
    assert!(c.cr(a).is_dead() && c.cr(stacked).is_dead());
    assert_eq!(c.cr(b).hp, 38);
    assert_eq!(c.power_amount(b, PowerId::Doom), 37);
    assert_eq!(c.cr(blocked).hp, 1);
    assert_eq!(c.power_amount(blocked, PowerId::Artifact), 0);
    assert_eq!(c.power_amount(blocked, PowerId::Doom), 0);
    assert!(c.hist.attacked_this_turn.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn lethal_drain_power_still_shuffles_discard_candidates_without_upgrading_them() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let k = hand_card(&mut c, 0, CardId::DrainPower);
    let e = enemy(&mut c, 5);
    c.combat_started = true;
    let mut candidates = Vec::new();
    for id in [CardId::Bash, CardId::StrikeIronclad, CardId::DefendIronclad] {
        let q = c.new_card(0, id, false).unwrap();
        c.players[0].discard.push(q);
        candidates.push(q);
    }
    c.players[0].energy = 1;
    let before = c.run.rngs.combat_card_selection.counter;
    assert!(c.play_card(0, 0, Some(e as usize)));
    assert_eq!(c.cr(e).hp, 0);
    assert!(c.is_over);
    // TakeRandom uses UnstableShuffle: a three-card list consumes two draws even
    // though CardCmd.Upgrade refuses changes after the attack ended combat.
    assert_eq!(c.run.rngs.combat_card_selection.counter, before + 2);
    assert!(candidates.iter().all(|&q| !c.cards[q as usize].upgraded));
    assert!(!c.cards[k as usize].upgraded);
    assert_eq!(c.fault, None);
}

#[test]
fn snap_retained_card_keeps_retain_when_upgraded_and_upgrade_keyword_deltas_still_apply() {
    use sts2_sim::tables::{CardId, KW_ETHEREAL, KW_EXHAUST, KW_RETAIN};
    let mut c = combat();
    let snap = hand_card(&mut c, 0, CardId::Snap);
    let defend = hand_card(&mut c, 0, CardId::DefendIronclad);
    let e = enemy(&mut c, 100);
    c.play_card_effect(snap, Some(e));
    assert!(c.cards[defend as usize].is_retain());
    c.upgrade_card(defend);
    assert!(c.cards[defend as usize].is_retain());
    assert!(c.cards[defend as usize].upgraded);
    assert_eq!(c.cards[defend as usize].block, Some(8));
    // Transfigure.OnUpgrade removes Exhaust but no applied Retain/Ethereal.
    let transfigure = c.new_card(0, CardId::Transfigure, false).unwrap();
    c.cards[transfigure as usize].keywords |= KW_RETAIN | KW_ETHEREAL;
    assert_ne!(c.cards[transfigure as usize].keywords & KW_EXHAUST, 0);
    c.upgrade_card(transfigure);
    assert_eq!(c.cards[transfigure as usize].keywords & KW_EXHAUST, 0);
    assert_eq!(c.cards[transfigure as usize].keywords & (KW_RETAIN | KW_ETHEREAL), KW_RETAIN | KW_ETHEREAL);
    // Dredge.OnUpgrade adds Retain without replacing an applied Ethereal keyword.
    let dredge = c.new_card(0, CardId::Dredge, false).unwrap();
    c.cards[dredge as usize].keywords |= KW_ETHEREAL;
    c.upgrade_card(dredge);
    assert!(c.cards[dredge as usize].is_retain());
    assert!(c.cards[dredge as usize].is_ethereal());
    assert_eq!(c.fault, None);
}

#[test]
fn calcify_buffs_only_owners_osty_powered_attacks() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let o = c.summon_osty(p, 5).unwrap();
    let other = c.summon_osty(0, 5).unwrap();
    let k = c.new_card(p, CardId::CalcifyCard, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.calculate_damage(7, Some(o), e, Props::MOVE), 13);
    assert_eq!(c.calculate_damage(7, Some(other), e, Props::MOVE), 7);
    assert_eq!(c.calculate_damage(7, Some(p), e, Props::MOVE), 7);
    assert_eq!(c.calculate_damage(7, Some(o), e, Props::UNPOWERED), 7);
    assert_eq!(c.fault, None);
}


#[test]
fn danse_macabre_checks_resolved_energy_including_captured_x_and_ignores_dexterity() {
    use sts2_sim::{card::CostExpiry, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let other = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let k = c.new_card(0, CardId::DanseMacabre, true).unwrap();
    c.play_card_effect(k, None);
    c.apply_power_to(0, PowerId::Dexterity, 100, Some(0));
    let bash = c.new_card(0, CardId::Bash, false).unwrap();
    c.fire(Hook::BeforeCardPlayed, Ev::Card(bash), None);
    assert_eq!(c.cr(0).block, 6);
    c.cards[bash as usize].set_energy_cost(1, CostExpiry::Combat, false);
    c.fire(Hook::BeforeCardPlayed, Ev::Card(bash), None);
    assert_eq!(c.cr(0).block, 6);
    let x = c.new_card(0, CardId::Whirlwind, false).unwrap();
    c.cards[x as usize].captured_x = 2;
    c.cards[x as usize].energy_spent = 4; // resolved effect after Chemical X
    c.fire(Hook::BeforeCardPlayed, Ev::Card(x), None);
    assert_eq!(c.cr(0).block, 12);
    c.cards[x as usize].captured_x = 1;
    c.cards[x as usize].energy_spent = 3;
    c.fire(Hook::BeforeCardPlayed, Ev::Card(x), None);
    let theirs = c.new_card(other, CardId::Bash, false).unwrap();
    c.fire(Hook::BeforeCardPlayed, Ev::Card(theirs), None);
    assert_eq!(c.cr(0).block, 12);
    assert_eq!(c.fault, None);
}

#[test]
fn severance_materializes_three_base_souls_for_draw_discard_hand_in_order() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let e = enemy(&mut c, 100);
    let k = c.new_card(0, CardId::Severance, true).unwrap();
    let rng = c.run.rngs.shuffle.counter;
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 82);
    let souls = [c.players[0].draw[0], c.players[0].discard[0], c.players[0].hand[0]];
    for q in souls {
        assert_eq!(c.cards[q as usize].id, CardId::Soul);
        assert!(!c.cards[q as usize].upgraded);
    }
    assert_eq!(souls[1], souls[0] + 1);
    assert_eq!(souls[2], souls[1] + 1);
    assert_eq!(c.hist.generated_combat, 3);
    assert_eq!(c.run.rngs.shuffle.counter, rng + 1);
    assert_eq!(c.fault, None);
}

#[test]
fn squeeze_counts_other_owned_osty_attacks_in_all_piles_and_uses_pet_strength() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 500);
    let pet = c.summon_osty(p, 5).unwrap();
    c.apply_power_to(pet, PowerId::Strength, 2, Some(p));
    c.apply_power_to(p, PowerId::Strength, 100, Some(p));
    let k = hand_card(&mut c, p, CardId::Squeeze);
    c.upgrade_card(k);
    hand_card(&mut c, p, CardId::Snap);
    let a = c.new_card(p, CardId::HighFive, false).unwrap();
    let b = c.new_card(p, CardId::Rattle, false).unwrap();
    let d = c.new_card(p, CardId::Squeeze, false).unwrap();
    c.players[p].draw.push(a);
    c.players[p].exhaust.push(b);
    c.players[p].discard.push(d);
    hand_card(&mut c, 0, CardId::Poke);
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 444); // (30 + 6 * 4) + pet Strength 2
    c.kill_creature(pet);
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 444);
    assert_eq!(c.fault, None);
}

#[test]
fn times_up_scales_powered_damage_from_doom_keeps_doom_and_upgrades_retain() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let e = enemy(&mut c, 100);
    c.apply_power_to(e, PowerId::Doom, 10, Some(0));
    c.apply_power_to(0, PowerId::Strength, 2, Some(0));
    c.cr_mut(e).block = 5;
    let k = c.new_card(0, CardId::TimesUp, true).unwrap();
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 93);
    assert_eq!(c.power_amount(e, PowerId::Doom), 10);
    assert!(c.cards[k as usize].is_retain());
    let other = enemy(&mut c, 100);
    c.play_card_effect(k, Some(other));
    c.flush_pending_attack();
    assert_eq!(c.cr(other).hp, 98);
    assert_eq!(c.fault, None);
}

#[test]
fn neurosurge_upgraded_energy_draws_two_then_applies_owner_side_doom() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    enemy(&mut c, 100);
    let k = hand_card(&mut c, 0, CardId::Neurosurge);
    c.upgrade_card(k);
    c.players[0].energy = 2;
    let a = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    let b = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    c.players[0].draw.extend([a, b]);
    c.play_card_effect(k, None);
    assert_eq!(c.players[0].energy, 6);
    assert!(c.players[0].hand.contains(&a) && c.players[0].hand.contains(&b));
    assert_eq!(c.power_amount(0, PowerId::Doom), 0);
    c.fire(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(0, PowerId::Doom), 0);
    c.fire(Hook::AfterSideTurnStart, Ev::Side(Side::Player), None);
    assert_eq!(c.power_amount(0, PowerId::Doom), 3);
    assert_eq!(c.fault, None);
}

#[test]
fn undeath_clones_combat_instance_into_discard_after_powered_block() {
    use sts2_sim::tables::{CardId, KW_RETAIN};
    let mut c = combat();
    enemy(&mut c, 100);
    let k = c.new_card(0, CardId::Undeath, true).unwrap();
    c.cards[k as usize].keywords |= KW_RETAIN;
    c.cards[k as usize].add_combat_cost(1);
    c.apply_power_to(0, PowerId::Dexterity, 2, Some(0));
    c.play_card_effect(k, None);
    assert_eq!(c.cr(0).block, 11);
    let q = c.players[0].discard[0];
    assert_ne!(q, k);
    assert_eq!(c.cards[q as usize].id, CardId::Undeath);
    assert!(c.cards[q as usize].upgraded && c.cards[q as usize].cv.is_clone);
    assert!(c.cards[q as usize].is_retain());
    assert_eq!(c.cards[q as usize].block, Some(9));
    assert_eq!(c.cards[q as usize].local_energy_cost(), 1);
    assert_eq!(c.hist.generated_combat, 1);
    assert_eq!(c.fault, None);
}

#[test]
fn legion_of_bone_summons_for_caster_and_each_living_player_not_dead_players() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let dead = c.add_player(50, 0, 3, 0, &[]);
    let last = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let existing = c.summon_osty(0, 5).unwrap();
    let k = c.new_card(0, CardId::LegionOfBone, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.cr(existing).max_hp, 13);
    assert_eq!(c.cr(existing).hp, 13);
    assert_eq!(c.cr(c.osty(p).unwrap()).hp, 8);
    assert_eq!(c.cr(c.osty(last).unwrap()).hp, 8);
    assert_eq!(c.osty(dead), None);
    assert_eq!(c.fault, None);
}

#[test]
fn legion_of_bone_allows_twelve_pets_and_reuses_each_owners_dead_osty() {
    use sts2_sim::{relic::Relic, tables::{CardId, RelicId}};
    for caster in 0..4 {
        let mut c = combat();
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        for owner in 0..4 {
            for id in [RelicId::Byrdpip, RelicId::PaelsLegion] {
                let mut relic = Relic::new(id);
                relic.owner = owner;
                c.run.relics.push(relic);
            }
        }
        c.setup_encounter("WATERFALL_GIANT_BOSS", &mut Rng::new(1));
        c.start_combat();
        assert_eq!(c.allies.len(), 8);
        for owner in 0..4 { assert_eq!(c.players[owner].pets.len(), 2); }
        hand_card(&mut c, caster, CardId::LegionOfBone);
        c.players[caster].energy = 3;
        assert!(c.play_card(caster, 0, None));
        // LegionOfBone.cs:30-33 summons for every living player, regardless
        // of character. PlayerCmd.cs:185-200 registers another distinct pet.
        assert_eq!(c.allies.len(), 12);
        let osties: Vec<_> = (0..4).map(|owner| c.osty(owner).unwrap()).collect();
        for owner in 0..4 {
            assert_eq!(c.players[owner].pets.len(), 3);
            assert_eq!(c.cr(osties[owner as usize]).pet_owner, Some(owner));
            assert_eq!(c.cr(osties[owner as usize]).hp, 6);
        }
        c.kill_creature(osties[caster as usize]);
        hand_card(&mut c, caster, CardId::LegionOfBone);
        c.players[caster].energy = 3;
        assert!(c.play_card(caster, 0, None));
        // OstyCmd.cs:32-50 reuses Allies' dead Osty rather than allocating.
        assert_eq!(c.allies.len(), 12);
        for owner in 0..4 {
            assert_eq!(c.osty(owner), Some(osties[owner as usize]));
            assert_eq!(c.players[owner].pets.len(), 3);
            assert_eq!(c.cr(osties[owner as usize]).hp, if owner == caster { 6 } else { 12 });
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn underworld_counts_allied_player_and_pet_total_damage_not_owners_attacks() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let own_pet = c.summon_osty(0, 5).unwrap();
    let other_pet = c.summon_osty(p, 5).unwrap();
    let k = c.new_card(0, CardId::Underworld, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.cards[k as usize].keywords & sts2_sim::tables::KW_EXHAUST, 0);
    c.cr_mut(e).block = 5;
    c.apply_damage(e, 7, Props::MOVE, Some(p));
    assert_eq!(c.power_amount(e, PowerId::Doom), 7);
    c.apply_damage(e, 2, Props::MOVE, Some(0));
    c.apply_damage(e, 2, Props::MOVE, Some(own_pet));
    c.apply_damage(e, 2, Props::UNPOWERED, Some(p));
    assert_eq!(c.power_amount(e, PowerId::Doom), 7);
    c.apply_damage(e, 3, Props::MOVE, Some(other_pet));
    assert_eq!(c.power_amount(e, PowerId::Doom), 10);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert_eq!(c.power_amount(0, PowerId::Underworld), 1);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(0, PowerId::Underworld), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn enemy_side_end_doom_victory_switches_round_without_starting_a_new_turn() {
    use sts2_sim::creature::Side;
    let mut c = combat();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.combat_started = true;
    c.apply_power_to(e, PowerId::Doom, c.cr(e).hp, Some(0));
    c.end_player_turn();
    assert!(c.is_over && c.won);
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, 2);
    assert!(c.players[0].hand.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn primary_death_kills_the_entire_secondary_teammate_batch_before_victory() {
    let mut c = combat();
    let primary = enemy(&mut c, 100);
    let first = enemy(&mut c, 20);
    let second = enemy(&mut c, 20);
    c.apply_power_to(first, PowerId::Minion, 1, None);
    c.apply_power_to(second, PowerId::Minion, 1, None);
    c.combat_started = true;
    c.kill_creature(primary);
    assert_eq!(c.cr(first).hp, 0);
    assert_eq!(c.cr(second).hp, 0);
    assert!(c.enemies.is_empty());
    assert!(c.is_over && c.won);
    assert_eq!(c.fault, None);
}

#[test]
fn cacophony_counts_teammate_draws_with_independent_33_draw_instances() {
    use sts2_sim::{hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 300);
    c.cr_mut(e).block = 10;
    c.apply_power_to(0, PowerId::Strength, 50, Some(0));
    let k = c.new_card(0, CardId::Cacophony, false).unwrap();
    c.play_card_effect(k, None);
    let drawn = c.new_card(p, CardId::DefendIronclad, false).unwrap();
    for _ in 0..32 { c.fire(Hook::OnCardDrawn, Ev::Card(drawn), None); }
    assert_eq!(c.cr(e).hp, 300);
    let k2 = c.new_card(0, CardId::Cacophony, true).unwrap();
    c.play_card_effect(k2, None);
    assert_eq!(c.cr(0).power_list().iter().filter(|&&q| c.powers[q as usize].id == PowerId::Cacophony).count(), 2);
    let rng = c.run.rngs.combat_targets.counter;
    c.fire(Hook::OnCardDrawn, Ev::Card(drawn), None);
    assert_eq!(c.cr(e).hp, 244);
    for _ in 0..32 { c.fire(Hook::OnCardDrawn, Ev::Card(drawn), None); }
    assert_eq!(c.cr(e).hp, 145);
    assert_eq!(c.run.rngs.combat_targets.counter, rng + 2);
    assert_eq!(c.fault, None);
}

#[test]
fn enfeebling_touch_inherits_temporary_strength_and_sleight_checks_offset_type() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let flesh = c.new_card(p, CardId::SleightOfFlesh, true).unwrap();
    c.play_card_effect(flesh, None);
    c.apply_power_to(e, PowerId::Weak, 1, Some(0));
    assert_eq!(c.cr(e).hp, 100);
    c.cr_mut(e).block = 5;
    c.apply_power_to(e, PowerId::Weak, 1, Some(p));
    assert_eq!(c.cr(e).hp, 92);
    c.apply_power_to(e, PowerId::Strength, 20, Some(0));
    let touch = c.new_card(p, CardId::EnfeeblingTouch, true).unwrap();
    c.play_card_effect(touch, Some(e));
    // Strength offset -11 is a debuff even though resulting Strength remains +9.
    // The temporary wrapper itself is excluded, so exactly one extra 13 damage.
    assert_eq!(c.power_amount(e, PowerId::Strength), 9);
    assert_eq!(c.cr(e).hp, 79);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert_eq!(c.power_amount(e, PowerId::Strength), 9);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(e, PowerId::Strength), 20);
    assert_eq!(c.power_amount(e, PowerId::EnfeeblingTouch), 0);
    assert_eq!(c.cr(e).hp, 79);
    assert_eq!(c.fault, None);
}

#[test]
fn friendship_upgrade_changes_strength_loss_but_only_owners_max_energy() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let energy = c.players[p].energy;
    let k = c.new_card(p, CardId::Friendship, true).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.power_amount(p, PowerId::Strength), -1);
    assert_eq!(c.max_energy(p), 4);
    assert_eq!(c.max_energy(0), 3);
    assert_eq!(c.players[p].energy, energy);
    assert_eq!(c.fault, None);
}

#[test]
fn necro_mastery_uses_actual_owned_pet_hp_delta_including_direct_kill_not_healing() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let other = c.summon_osty(0, 5).unwrap();
    let k = c.new_card(p, CardId::NecroMasteryCard, true).unwrap();
    c.play_card_effect(k, None);
    let pet = c.osty(p).unwrap();
    assert_eq!(c.cr(pet).hp, 8);
    c.apply_power_to(p, PowerId::NecroMastery, 1, Some(p));
    c.apply_power_to(p, PowerId::Strength, 50, Some(p));
    c.cr_mut(e).block = 40;
    c.apply_damage(other, 2, Props::UNPOWERED, None);
    assert_eq!(c.cr(e).hp, 100);
    c.apply_damage(pet, 3, Props::UNPOWERED, None);
    assert_eq!(c.cr(e).hp, 94);
    c.heal(pet, 2);
    assert_eq!(c.cr(e).hp, 94);
    c.kill_creature(pet);
    assert_eq!(c.cr(e).hp, 80);
    assert_eq!(c.cr(e).block, 40);
    assert_eq!(c.fault, None);
}

#[test]
fn shroud_uses_doom_applier_not_target_or_delta_and_defy_blocks_before_weak() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::Shroud, true).unwrap();
    c.play_card_effect(k, None);
    c.apply_power_to(p, PowerId::Dexterity, 10, Some(p));
    c.apply_power_to(e, PowerId::Doom, 4, Some(p));
    assert_eq!(c.cr(p).block, 4);
    c.apply_power_to(e, PowerId::Doom, 2, Some(0));
    assert_eq!(c.cr(p).block, 4);
    c.creature_apply_power(e, PowerId::Doom, -1, Some(p), None, false, false);
    assert_eq!(c.cr(p).block, 8);
    let defy = c.new_card(p, CardId::Defy, true).unwrap();
    c.play_card_effect(defy, Some(e));
    assert_eq!(c.cr(p).block, 27);
    assert_eq!(c.power_amount(e, PowerId::Weak), 1);
    assert!(c.cards[defy as usize].is_ethereal());
    assert_eq!(c.fault, None);
}

#[test]
fn haunt_owned_soul_uses_targets_stream_and_bypasses_block_without_attack_modifiers() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::Haunt, true).unwrap();
    c.play_card_effect(k, None);
    c.cr_mut(e).block = 40;
    c.apply_power_to(p, PowerId::Strength, 50, Some(p));
    let soul = hand_card(&mut c, p, CardId::Soul);
    let before = c.run.rngs.combat_targets.counter;
    assert!(c.play_card(p, 0, None));
    assert_eq!(c.cards[soul as usize].owner, p);
    assert_eq!(c.cr(e).hp, 91);
    assert_eq!(c.cr(e).block, 40);
    assert_eq!(c.run.rngs.combat_targets.counter, before + 1);
    hand_card(&mut c, 0, CardId::Soul);
    assert!(c.play_card(0, 0, None));
    assert_eq!(c.cr(e).hp, 91);
    assert_eq!(c.fault, None);
}

#[test]
fn seance_selects_sorted_owned_draw_and_replaces_at_the_original_index_with_base_soul() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let k = hand_card(&mut c, p, CardId::Seance);
    let strike = c.new_card(p, CardId::StrikeIronclad, true).unwrap();
    let defend = c.new_card(p, CardId::DefendIronclad, false).unwrap();
    c.players[p].draw.extend([strike, defend]);
    c.play_card_effect(k, None);
    assert_eq!(c.choice_options.as_slice(), &[defend, strike]);
    assert!(c.resolve_pending_choice(Some(1)));
    let soul = c.players[p].draw[0];
    assert_ne!(soul, strike);
    assert_eq!(c.cards[soul as usize].id, CardId::Soul);
    assert!(!c.cards[soul as usize].upgraded);
    assert_eq!(c.cards[soul as usize].owner, p);
    assert_eq!(c.players[p].draw[1], defend);
    assert_eq!(c.hist.generated_combat, 1);
    assert!(c.players[0].draw.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn no_escape_uses_existing_doom_threshold_and_grimoire_defers_removal_rewards_to_combat_end() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let e = enemy(&mut c, 100);
    let k = hand_card(&mut c, 0, CardId::NoEscape);
    c.creature_apply_power(e, PowerId::Doom, 19, Some(0), None, false, false);
    c.play_card_effect(k, Some(e));
    assert_eq!(c.power_amount(e, PowerId::Doom), 34);
    c.play_card_effect(k, Some(e));
    assert_eq!(c.power_amount(e, PowerId::Doom), 59);
    assert_eq!(c.fault, None);
    let grimoire = hand_card(&mut c, 0, CardId::ForbiddenGrimoire);
    c.play_card_effect(grimoire, None);
    assert_eq!(c.power_amount(0, PowerId::ForbiddenGrimoire), 1);
    assert!(c.end_rewards.is_empty());
    c.fire(sts2_sim::tables::Hook::AfterCombatEnd, sts2_sim::hooks::Ev::None, None);
    assert!(matches!(c.end_rewards.as_slice(), [sts2_sim::combat::EndReward::CardRemoval { owner: 0 }]));
    assert_eq!(c.fault, None);
}

#[test]
fn snap_retains_owned_nonretain_card_even_without_pet_and_bodyguard_upgrades_summon() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let snap = hand_card(&mut c, p, CardId::Snap);
    let d = hand_card(&mut c, p, CardId::DefendNecrobinder);
    let retain = hand_card(&mut c, p, CardId::Spur);
    let e = enemy(&mut c, 100);
    c.play_card_effect(snap, Some(e));
    assert!(c.cards[d as usize].is_retain());
    assert!(c.cards[retain as usize].is_retain());
    assert_eq!(c.cr(e).hp, 100);
    let body = c.new_card(p, CardId::Bodyguard, true).unwrap();
    c.play_card_effect(body, None);
    assert_eq!(c.cr(c.osty(p).unwrap()).hp, 7);
    assert!(c.osty(0).is_none());
    c.play_card_effect(snap, Some(e));
    assert_eq!(c.cr(e).hp, 93);
    assert!(c.pending_choice.is_none());
    assert_eq!(c.fault, None);
}

#[test]
fn grave_warden_inserts_unupgraded_soul_and_devour_life_summons_only_for_owned_soul() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    let grave = c.new_card(p, CardId::GraveWarden, true).unwrap();
    c.play_card_effect(grave, None);
    assert_eq!(c.cr(p).block, 11);
    let soul = c.players[p].draw[0];
    assert_eq!(c.cards[soul as usize].id, CardId::Soul);
    assert!(!c.cards[soul as usize].upgraded);
    let devour = c.new_card(p, CardId::DevourLifeCard, true).unwrap();
    c.play_card_effect(devour, None);
    c.draw_cards(p, 1, false);
    let pos = c.players[p].hand.iter().position(|&q| q == soul).unwrap();
    assert!(c.play_card(p, pos, None));
    assert_eq!(c.cr(c.osty(p).unwrap()).hp, 2);
    assert!(c.osty(0).is_none());
    let demesne = c.new_card(p, CardId::Demesne, false).unwrap();
    c.play_card_effect(demesne, None);
    assert_eq!(c.max_energy(p), 4);
    assert_eq!(c.max_energy(0), 3);
    assert_eq!(c.modify_hand_draw(5, p), 6);
    assert_eq!(c.modify_hand_draw(5, 0), 5);
    assert_eq!(c.fault, None);
}

#[test]
fn drain_power_upgrades_only_eligible_owned_discard_and_shuffles_the_whole_candidate_list() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::DrainPower, false).unwrap();
    let mut eligible = Vec::new();
    for id in [CardId::Bash, CardId::DefendIronclad, CardId::StrikeIronclad] {
        let q = c.new_card(p, id, false).unwrap();
        c.players[p].discard.push(q);
        eligible.push(q);
    }
    let bane = c.new_card(p, CardId::AscendersBane, false).unwrap();
    c.players[p].discard.push(bane);
    let mut rng = c.run.rngs.combat_card_selection.clone();
    let mut expected = eligible.clone();
    rng.shuffle(&mut expected); // C# TakeRandom = UnstableShuffle followed by Take.
    c.play_card_effect(k, Some(e));
    assert_eq!(c.cr(e).hp, 90);
    for q in eligible { assert_eq!(c.cards[q as usize].upgraded, expected[..2].contains(&q)); }
    assert!(!c.cards[bane as usize].upgraded);
    assert_eq!(c.run.rngs.combat_card_selection.counter, rng.counter);
    assert_eq!(c.fault, None);
}

#[test]
fn bone_shards_attacks_once_then_blocks_and_kills_only_owners_pet() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let k = hand_card(&mut c, p, CardId::BoneShards);
    let a = enemy(&mut c, 100);
    let b = enemy(&mut c, 100);
    c.play_card_effect(k, None);
    assert_eq!((c.cr(a).hp, c.cr(b).hp, c.cr(p).block), (100, 100, 0));
    let other = c.summon_osty(0, 4).unwrap();
    let o = c.summon_osty(p, 4).unwrap();
    c.creature_apply_power(p, PowerId::Dexterity, 1, None, None, false, false);
    c.play_card_effect(k, None);
    assert_eq!((c.cr(a).hp, c.cr(b).hp, c.cr(p).block), (91, 91, 10));
    assert!(c.cr(o).is_dead());
    assert!(c.cr(other).is_alive());
    assert_eq!(c.hist.attacked_this_turn.iter().filter(|&&actor| actor == o).count(), 1);
    assert_eq!(c.fault, None);
}

#[test]
fn transfigure_modifies_base_replay_and_combat_cost_but_not_x_or_negative_costs() {
    use sts2_sim::{card::CostExpiry, tables::CardId};
    let mut c = combat();
    let k = hand_card(&mut c, 0, CardId::Transfigure);
    enemy(&mut c, 100);
    for id in [CardId::Bash, CardId::Eradicate, CardId::AscendersBane] {
        let q = hand_card(&mut c, 0, id);
        c.play_card_effect(k, None); // the sole other hand card is selected automatically.
        assert_eq!(c.cards[q as usize].replay, 1);
        if id == CardId::Bash {
            assert_eq!(c.cards[q as usize].cost, 3);
            c.cards[q as usize].set_energy_cost(0, CostExpiry::Turn, false);
            c.cards[q as usize].end_of_turn_cleanup();
            assert_eq!(c.cards[q as usize].cost, 3);
        } else {
            assert_eq!(c.cards[q as usize].cost, if id == CardId::Eradicate { 0 } else { -1 });
        }
        c.players[0].hand.retain(|card| *card != q);
    }
    assert_eq!(c.fault, None);
}

#[test]
fn veilpiercer_free_ethereal_overrides_borrowed_time_and_consumes_only_owner_stack() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let veil = hand_card(&mut c, p, CardId::Veilpiercer);
    let borrowed = hand_card(&mut c, p, CardId::BorrowedTime);
    let defile = hand_card(&mut c, p, CardId::Defile);
    let other = hand_card(&mut c, 0, CardId::Defile);
    c.players[p].energy = 3;
    c.play_card_effect(borrowed, None);
    assert_eq!(c.players[p].energy, 7);
    assert_eq!(c.modified_card_cost(p, defile), 2);
    c.play_card_effect(veil, Some(e));
    assert_eq!(c.modified_card_cost(p, defile), 0);
    assert_eq!(c.modified_card_cost(0, other), 1);
    c.fire(Hook::BeforeCardPlayed, Ev::Card(other), None);
    assert_eq!(c.power_amount(p, PowerId::Veilpiercer), 1);
    c.fire(Hook::BeforeCardPlayed, Ev::Card(defile), None);
    assert_eq!(c.power_amount(p, PowerId::Veilpiercer), 0);
    assert_eq!(c.modified_card_cost(p, defile), 2);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(p, PowerId::BorrowedTime), 1);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert_eq!(c.power_amount(p, PowerId::BorrowedTime), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn death_march_counts_only_owners_extra_draws_and_resets_each_side_turn() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::DeathMarch, true).unwrap();
    for who in [0, p] {
        for _ in 0..4 {
            let q = c.new_card(who, CardId::DefendIronclad, false).unwrap();
            c.players[who].draw.push(q);
        }
    }
    c.draw_cards(0, 2, false);
    c.draw_cards(p, 2, true);
    c.draw_cards(p, 2, false);
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 79); // upgraded base 9 + 2 * 6.
    c.hist.reset_side_turn();
    c.play_card_effect(k, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 70);
    assert_eq!(c.fault, None);
}

#[test]
fn cleanse_summons_before_sorted_draw_choice_and_exhausts_only_selected_owned_card() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let k = c.new_card(p, CardId::Cleanse, true).unwrap();
    enemy(&mut c, 100);
    let strike = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
    let defend = c.new_card(p, CardId::DefendIronclad, false).unwrap();
    c.players[p].draw.extend([strike, defend]);
    c.play_card_effect(k, None);
    assert_eq!(c.cr(c.osty(p).unwrap()).hp, 5);
    assert!(c.osty(0).is_none());
    assert_eq!(c.choice_options.as_slice(), &[defend, strike]);
    assert!(c.resolve_pending_choice(Some(0)));
    assert_eq!(c.players[p].draw.as_slice(), &[strike]);
    assert_eq!(c.players[p].exhaust.as_slice(), &[defend]);
    assert_eq!(c.hist.exhausted_this_turn, 1);
    // A sole candidate is selected automatically; empty draw does not open a prompt.
    c.play_card_effect(k, None);
    assert!(c.pending_choice.is_none());
    assert!(c.players[p].draw.is_empty());
    c.play_card_effect(k, None);
    assert!(c.pending_choice.is_none());
    assert_eq!(c.fault, None);
}

#[test]
fn defile_delay_and_pull_aggro_use_canonical_upgrades_and_card_owner() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    let defile = c.new_card(p, CardId::Defile, true).unwrap();
    let delay = c.new_card(p, CardId::Delay, true).unwrap();
    let pull = c.new_card(p, CardId::PullAggro, false).unwrap();
    c.creature_apply_power(p, PowerId::Strength, 2, None, None, false, false);
    c.play_card_effect(defile, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 81);
    c.play_card_effect(delay, None);
    assert_eq!(c.cr(p).block, 13);
    assert_eq!(c.power_amount(p, PowerId::EnergyNextTurn), 2);
    c.play_card_effect(pull, None);
    assert_eq!(c.cr(c.osty(p).unwrap()).hp, 4);
    assert_eq!(c.cr(p).block, 20);
    assert_eq!(c.cr(0).block, 0);
    assert_eq!(c.power_amount(0, PowerId::EnergyNextTurn), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn countdown_uses_owner_applier_and_combat_targets_only_on_owner_side() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let a = enemy(&mut c, 100);
    let b = enemy(&mut c, 100);
    let k = c.new_card(p, CardId::CountdownCard, true).unwrap();
    c.play_card_effect(k, None);
    let before = c.run.rngs.combat_targets.counter;
    c.fire(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy), None);
    assert_eq!(c.run.rngs.combat_targets.counter, before);
    c.fire(Hook::AfterSideTurnStart, Ev::Side(Side::Player), None);
    assert_eq!(c.run.rngs.combat_targets.counter, before + 1);
    assert_eq!(c.power_amount(a, PowerId::Doom) + c.power_amount(b, PowerId::Doom), 9);
    assert!(c.hist.doom_appliers_this_turn[p as usize]);
    assert!(!c.hist.doom_appliers_this_turn[0]);
    assert_eq!(c.fault, None);
}

#[test]
fn fetch_and_rattle_use_owners_pet_and_finished_card_or_attack_history() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let fetch = hand_card(&mut c, 0, CardId::Fetch);
    let rattle = hand_card(&mut c, 0, CardId::Rattle);
    let e = enemy(&mut c, 100);
    let d = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].draw.push(d);
    c.play_card_effect(fetch, Some(e));
    assert_eq!(c.players[0].draw.len(), 1); // missing pet: no draw.
    let o = c.summon_osty(0, 10).unwrap();
    c.play_card_effect(fetch, Some(e));
    assert!(c.players[0].hand.contains(&d));
    c.hist.played_this_turn.push((0, fetch));
    c.players[0].hand.retain(|q| *q != d);
    c.players[0].draw.push(d);
    c.play_card_effect(fetch, Some(e));
    assert_eq!(c.players[0].draw.len(), 1);
    let before = c.hist.attacked_this_turn.iter().filter(|&&a| a == o).count();
    let hp = c.cr(e).hp;
    c.play_card_effect(rattle, Some(e));
    assert_eq!(c.cr(e).hp, hp - 7 * (1 + before as i32));
    assert_eq!(c.hist.attacked_this_turn.iter().filter(|&&a| a == o).count(), before + 1);
    assert_eq!(c.fault, None);
}

#[test]
fn eidolon_replays_only_playable_ethereal_exhaust_and_pagestorm_draws_on_ethereal() {
    use sts2_sim::tables::{CardId, KW_ETHEREAL};
    let mut c = combat();
    let eidolon = hand_card(&mut c, 0, CardId::Eidolon);
    let page = hand_card(&mut c, 0, CardId::Pagestorm);
    enemy(&mut c, 100);
    let d = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.cards[d as usize].keywords |= KW_ETHEREAL;
    let bane = c.new_card(0, CardId::AscendersBane, false).unwrap();
    c.players[0].exhaust.extend([d, bane]);
    c.play_card_effect(eidolon, None);
    assert_eq!(c.cr(0).block, 5);
    assert_eq!(c.players[0].exhaust, vec![bane]);
    assert!(c.players[0].discard.contains(&d));
    let parse = c.new_card(0, CardId::Parse, false).unwrap();
    let strike = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    c.players[0].draw.extend([parse, strike]);
    c.play_card_effect(page, None);
    c.draw_cards(0, 1, false);
    assert!(c.players[0].hand.contains(&parse));
    assert!(c.players[0].hand.contains(&strike));
    assert_eq!(c.fault, None);
}

#[test]
fn debilitate_attacks_then_marks_and_expires_on_its_owners_side() {
    use sts2_sim::{creature::Side, hooks::Ev, tables::{CardId, Hook}};
    let mut c = combat();
    let k = hand_card(&mut c, 0, CardId::DebilitateCard);
    let e = enemy(&mut c, 100);
    c.play_card_effect(k, Some(e));
    assert_eq!(c.cr(e).hp, 90);
    assert_eq!(c.power_amount(e, PowerId::Debilitate), 2);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
    assert_eq!(c.power_amount(e, PowerId::Debilitate), 2);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(e, PowerId::Debilitate), 1);
    c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
    assert_eq!(c.power_amount(e, PowerId::Debilitate), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn shared_fate_reduces_only_its_owner_and_target_strength_including_upgrade() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let k = hand_card(&mut c, p, CardId::SharedFate);
    let e = enemy(&mut c, 100);
    c.play_card_effect(k, Some(e));
    assert_eq!((c.power_amount(p, PowerId::Strength), c.power_amount(e, PowerId::Strength)), (-2, -2));
    assert_eq!(c.power_amount(0, PowerId::Strength), 0);
    let up = c.new_card(p, CardId::SharedFate, true).unwrap();
    c.play_card_effect(up, Some(e));
    assert_eq!((c.power_amount(p, PowerId::Strength), c.power_amount(e, PowerId::Strength)), (-4, -5));
    assert_eq!(c.fault, None);
}

#[test]
fn deathbringer_applies_doom_before_weak_and_uses_its_owner() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let k = hand_card(&mut c, p, CardId::Deathbringer);
    let a = enemy(&mut c, 100);
    let b = enemy(&mut c, 100);
    let artifact = c.creature_apply_power(a, PowerId::Artifact, 1, None, None, false, false).unwrap();
    // Fresh enemy Artifact scales with player count; test exactly one remaining stack.
    c.powers[artifact as usize].amount = 1;
    c.play_card_effect(k, None);
    assert_eq!((c.power_amount(a, PowerId::Doom), c.power_amount(a, PowerId::Weak)), (0, 1));
    assert_eq!((c.power_amount(b, PowerId::Doom), c.power_amount(b, PowerId::Weak)), (21, 1));
    assert!(c.hist.doom_appliers_this_turn[p as usize]);
    assert!(!c.hist.doom_appliers_this_turn[0]);
    assert_eq!(c.fault, None);
}

#[test]
fn poke_defend_and_spur_use_the_card_owners_pet_block_and_healing() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let poke = hand_card(&mut c, p, CardId::Poke);
    let defend = hand_card(&mut c, p, CardId::DefendNecrobinder);
    let spur = hand_card(&mut c, p, CardId::Spur);
    let other = c.summon_osty(0, 4).unwrap();
    let e = enemy(&mut c, 100);
    c.play_card_effect(poke, Some(e));
    assert_eq!(c.cr(e).hp, 100);
    c.play_card_effect(spur, None);
    let o = c.osty(p).unwrap();
    assert_eq!((c.cr(o).hp, c.cr(o).max_hp), (3, 3));
    c.creature_apply_power(p, PowerId::Strength, 50, None, None, false, false);
    c.play_card_effect(poke, Some(e));
    assert_eq!(c.cr(e).hp, 94);
    c.cr_mut(o).max_hp = 9;
    c.cr_mut(o).hp = 1;
    c.play_card_effect(spur, None);
    assert_eq!((c.cr(o).hp, c.cr(o).max_hp), (9, 12)); // grow/heal 3, then heal 5.
    assert_eq!(c.cr(other).hp, 4);
    c.play_card_effect(defend, None);
    assert_eq!((c.cr(0).block, c.cr(p).block), (0, 5));
    assert_eq!(c.fault, None);
}

#[test]
fn negative_pulse_and_deaths_door_track_doom_applier_and_separate_block_gains() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let pulse = hand_card(&mut c, p, CardId::NegativePulse);
    let door = hand_card(&mut c, p, CardId::DeathsDoor);
    let other = hand_card(&mut c, 0, CardId::DeathsDoor);
    let a = enemy(&mut c, 100);
    let b = enemy(&mut c, 100);
    c.creature_apply_power(p, PowerId::Dexterity, 1, None, None, false, false);
    c.play_card_effect(pulse, None);
    assert_eq!((c.power_amount(a, PowerId::Doom), c.power_amount(b, PowerId::Doom)), (7, 7));
    assert_eq!(c.cr(p).block, 6);
    c.play_card_effect(door, None);
    assert_eq!(c.cr(p).block, 27); // 6 + three separate (6 + Dexterity 1) gains.
    c.play_card_effect(other, None);
    assert_eq!(c.cr(0).block, 6);
    assert_eq!(c.fault, None);
}

#[test]
fn deaths_door_distinguishes_blocked_new_doom_from_zero_offset_existing_stack() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let door = hand_card(&mut c, 0, CardId::DeathsDoor);
    let e = enemy(&mut c, 100);
    c.creature_apply_power(e, PowerId::Artifact, 1, None, None, false, false);
    c.creature_apply_power(e, PowerId::Doom, 5, Some(0), None, false, false);
    assert!(!c.hist.doom_appliers_this_turn[0]);
    c.play_card_effect(door, None);
    assert_eq!(c.cr(0).block, 6);
    c.creature_apply_power(e, PowerId::Doom, 5, Some(0), None, false, false);
    c.hist.reset_side_turn();
    c.creature_apply_power(e, PowerId::Artifact, 1, None, None, false, false);
    c.creature_apply_power(e, PowerId::Doom, 5, Some(0), None, false, false);
    assert_eq!(c.power_amount(e, PowerId::Doom), 5);
    assert!(c.hist.doom_appliers_this_turn[0]); // PowerCmd.ModifyAmount records zero offset.
    c.cr_mut(0).block = 0;
    c.play_card_effect(door, None);
    assert_eq!(c.cr(0).block, 18);
    c.hist.reset_side_turn();
    c.cr_mut(0).block = 0;
    c.play_card_effect(door, None);
    assert_eq!(c.cr(0).block, 6);
    assert_eq!(c.fault, None);
}

#[test]
fn dredge_skips_empty_or_full_cases_and_caps_selection_at_remaining_hand_space() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let d = hand_card(&mut c, 0, CardId::Dredge);
    enemy(&mut c, 100);
    c.play_card_effect(d, None);
    assert!(c.pending_choice.is_none());
    for _ in 0..9 { hand_card(&mut c, 0, CardId::DefendIronclad); }
    for _ in 0..3 {
        let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].discard.push(k);
    }
    c.play_card_effect(d, None);
    assert!(c.pending_choice.is_none());
    c.players[0].energy = 1;
    assert!(c.play_card(0, 0, None));
    let choice = c.pending_choice.as_ref().unwrap();
    assert_eq!((choice.min, choice.max), (1, 1));
    let selected = c.choice_options[1];
    assert!(c.resolve_pending_choice(Some(1)));
    assert_eq!(c.players[0].hand.len(), 10);
    assert!(c.players[0].hand.contains(&selected));
    assert_eq!(c.players[0].discard.len(), 2);
    assert!(c.players[0].exhaust.contains(&d));
    assert_eq!(c.fault, None);
}

#[test]
fn scythe_preserves_saved_growth_and_updates_only_owners_persistent_deck() {
    use sts2_sim::{card::Card, tables::CardId};
    let mut c = combat();
    let mut saved = Card::from_factory(CardId::TheScythe, false, 100).unwrap();
    saved.damage = Some(18);
    let p = c.add_player(50, 50, 3, 0, &[saved]);
    let k = c.players[p].starting_deck[0];
    let e = enemy(&mut c, 5000);
    c.play_card_effect(k, Some(e));
    assert_eq!(c.cards[k as usize].damage, Some(23));
    assert_eq!(c.player_deck(p)[0].damage, Some(23));
    c.cards[k as usize].damage = Some(77); // A separate combat damage modification.
    c.play_card_effect(k, Some(e));
    assert_eq!(c.cr(e).hp, 4905);
    assert_eq!(c.cards[k as usize].damage, Some(28)); // UpdateDamage uses saved growth only.
    assert_eq!(c.player_deck(p)[0].damage, Some(28));
    assert!(c.player_deck(0).is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn blight_strike_doom_uses_actual_result_after_block_intangible_and_buffer() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let k = hand_card(&mut c, 0, CardId::BlightStrike);
    let e = enemy(&mut c, 100);
    c.cr_mut(e).block = 5;
    c.play_card_effect(k, Some(e));
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (97, 8));
    c.creature_apply_power(e, PowerId::Intangible, 1, None, None, false, false);
    c.play_card_effect(k, Some(e));
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (96, 9));
    c.creature_apply_power(e, PowerId::Buffer, 1, None, None, false, false);
    c.play_card_effect(k, Some(e));
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (96, 9));
    assert_eq!(c.fault, None);
}

#[test]
fn capture_spirit_bypasses_block_strength_and_thorns_without_an_attack_scope() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let k = hand_card(&mut c, 0, CardId::CaptureSpirit);
    let e = enemy(&mut c, 100);
    c.cr_mut(e).block = 100;
    c.creature_apply_power(0, PowerId::Strength, 50, None, None, false, false);
    c.creature_apply_power(e, PowerId::Thorns, 5, None, None, false, false);
    c.creature_apply_power(e, PowerId::Skittish, 7, None, None, false, false);
    c.play_card_effect(k, Some(e));
    assert_eq!((c.cr(e).hp, c.cr(e).block, c.cr(0).hp), (97, 100, 50));
    assert_eq!(c.players[0].draw.len(), 3);
    assert!(c.hist.attacked_this_turn.is_empty() && c.attack_stack.is_empty());
    assert_eq!(c.fault, None);
}

#[test]
fn afterlife_dirge_and_scourge_are_owned_and_dirge_summons_separately() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let a = hand_card(&mut c, p, CardId::Afterlife);
    let d = c.new_card(p, CardId::Dirge, true).unwrap();
    let s = c.new_card(p, CardId::Scourge, false).unwrap();
    let e = enemy(&mut c, 100);
    c.play_card_effect(a, None);
    c.cards[d as usize].energy_spent = 3;
    c.play_card_effect(d, None);
    let o = c.osty(p).unwrap();
    assert_eq!((c.cr(o).hp, c.cr(o).max_hp), (18, 18));
    assert_eq!(c.hist.summons, vec![(p, 6), (p, 4), (p, 4), (p, 4)]);
    assert!(c.osty(0).is_none());
    assert_eq!(c.players[p].draw.len(), 3);
    assert!(c.players[p].draw.iter().all(|&q| c.cards[q as usize].upgraded));
    c.play_card_effect(s, Some(e));
    assert_eq!(c.power_amount(e, PowerId::Doom), 13);
    assert_eq!(c.players[p].draw.len(), 2);
    assert_eq!(c.players[0].hand.len(), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn wisp_grants_energy_only_to_its_owner() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let w = hand_card(&mut c, p, CardId::Wisp);
    c.play_card_effect(w, None);
    assert_eq!(c.players[0].energy, 0);
    assert_eq!(c.players[p].energy, 1);
    assert_eq!(c.fault, None);
}

#[test]
fn sculpting_strike_adds_local_ethereal_even_when_hex_already_adds_global_ethereal() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    hand_card(&mut c, 0, CardId::SculptingStrike);
    let strike = hand_card(&mut c, 0, CardId::StrikeIronclad);
    let fear = hand_card(&mut c, 0, CardId::Fear);
    let e = enemy(&mut c, 100);
    let hex = c.creature_apply_power(0, PowerId::Hex, 1, None, None, false, false).unwrap();
    assert!(c.cards[strike as usize].cv.affliction_added_ethereal);
    c.players[0].energy = 1;
    assert!(c.play_card(0, 0, Some(0)));
    assert_eq!(c.cr(e).hp, 91);
    assert!(c.pending_choice.is_none()); // Strike is the sole locally non-Ethereal option.
    c.remove_power(0, hex);
    assert!(c.cards[strike as usize].is_ethereal());
    assert!(c.cards[fear as usize].is_ethereal());
    assert!(!c.cards[strike as usize].cv.affliction_added_ethereal);
    assert_eq!(c.fault, None);
}

#[test]
fn lethality_boosts_first_attack_not_replays_and_tracks_players_independently() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let lethality = hand_card(&mut c, 0, CardId::LethalityCard);
    let first = hand_card(&mut c, 0, CardId::StrikeIronclad);
    hand_card(&mut c, 0, CardId::StrikeIronclad);
    hand_card(&mut c, p, CardId::StrikeIronclad);
    let e = enemy(&mut c, 200);
    c.play_card_effect(lethality, None);
    c.creature_apply_power(p, PowerId::Lethality, 75, None, None, false, false);
    c.cards[first as usize].replay = 1;
    c.players[0].energy = 3;
    c.players[p].energy = 3;
    assert!(c.play_card(0, 1, Some(0)));
    assert_eq!(c.cr(e).hp, 185); // first = 9, replay = 6.
    assert!(c.play_card(0, 1, Some(0)));
    assert_eq!(c.cr(e).hp, 179);
    assert!(c.play_card(p, 0, Some(0)));
    assert_eq!(c.cr(e).hp, 169); // floor(6 * 1.75), independent first attack.
    assert_eq!(c.fault, None);
}

#[test]
fn reaper_form_uses_block_plus_actual_hp_loss_and_includes_owners_pet() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let form = hand_card(&mut c, 0, CardId::ReaperForm);
    c.play_card_effect(form, None);
    let e = enemy(&mut c, 100);
    c.cr_mut(e).block = 5;
    c.deal_damage(Some(0), e, 8, Props::MOVE);
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (97, 8));
    c.creature_apply_power(e, PowerId::Buffer, 1, None, None, false, false);
    c.deal_damage(Some(0), e, 20, Props::MOVE);
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (97, 8));
    c.summon_osty(0, 4);
    let right = hand_card(&mut c, 0, CardId::RightHandHand);
    c.play_card_effect(right, Some(e));
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Doom)), (93, 12));
    c.deal_damage(Some(0), e, 3, Props::UNPOWERED);
    assert_eq!(c.power_amount(e, PowerId::Doom), 12);
    assert_eq!(c.fault, None);
}

#[test]
fn doom_kills_enemies_before_side_end_and_players_after_side_end() {
    use sts2_sim::creature::Side;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 10);
    enemy(&mut c, 100);
    c.creature_apply_power(e, PowerId::Doom, 10, None, None, false, false);
    c.creature_apply_power(p, PowerId::Doom, 50, None, None, false, false);
    c.fire_before_turn_end(Side::Player);
    assert!(c.cr(p).is_alive() && c.cr(e).is_alive());
    c.fire_after_turn_end(Side::Player);
    assert!(c.cr(p).death_processed);
    assert!(c.cr(e).is_alive());
    c.fire_before_turn_end(Side::Enemy);
    assert!(c.cr(e).death_processed);
    assert!(c.cr(0).is_alive());
    assert_eq!(c.fault, None);
}

#[test]
fn graveblast_empty_discard_finishes_without_choice() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let g = hand_card(&mut c, 0, CardId::Graveblast);
    let e = enemy(&mut c, 100);
    c.players[0].energy = 1;
    assert!(c.play_card(0, 0, Some(0)));
    assert_eq!(c.cr(e).hp, 96);
    assert!(c.pending_choice.is_none());
    assert!(c.players[0].discard.is_empty());
    assert!(c.players[0].exhaust.contains(&g));
    assert_eq!(c.fault, None);
}

#[test]
fn misery_keeps_strangle_instances_separate_for_different_appliers() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let m = hand_card(&mut c, 0, CardId::Misery);
    let source = enemy(&mut c, 100);
    let other = enemy(&mut c, 100);
    c.creature_apply_power(source, PowerId::Strangle, 3, Some(0), None, false, false);
    c.creature_apply_power(other, PowerId::Strangle, 5, Some(p), None, false, false);
    assert_eq!(c.fault, None);
    c.play_card_effect(m, Some(source));
    assert_eq!(c.fault, None);
    assert_eq!(c.cr(source).hp, 93);
    let power = c.find_power(other, PowerId::Strangle).unwrap();
    assert_eq!((c.powers[power as usize].amount, c.powers[power as usize].applier), (5, Some(p)));
    let instances: Vec<_> = c.cr(other).power_list().into_iter()
        .filter(|&q| c.powers[q as usize].id == PowerId::Strangle)
        .map(|q| (c.powers[q as usize].amount, c.powers[q as usize].applier)).collect();
    assert_eq!(instances, vec![(5, Some(p)), (3, Some(0))]);
}

#[test]
fn putrefy_artifact_order_protector_max_hp_soul_storm_and_upgraded_reave() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let putrefy = hand_card(&mut c, 0, CardId::Putrefy);
    let protector = hand_card(&mut c, 0, CardId::Protector);
    let storm = hand_card(&mut c, 0, CardId::SoulStorm);
    let reave = c.new_card(0, CardId::Reave, true).unwrap();
    let blocked = enemy(&mut c, 100);
    let target = enemy(&mut c, 100);
    c.creature_apply_power(blocked, PowerId::Artifact, 1, None, None, false, false);
    c.play_card_effect(putrefy, Some(blocked));
    assert_eq!(c.power_amount(blocked, PowerId::Weak), 0);
    assert_eq!(c.power_amount(blocked, PowerId::Vulnerable), 2);
    let o = c.summon_osty(0, 9).unwrap();
    c.cr_mut(o).hp = 1;
    c.play_card_effect(protector, Some(target));
    assert_eq!(c.cr(target).hp, 81);
    for _ in 0..2 {
        let s = c.new_card(0, CardId::Soul, false).unwrap();
        c.players[0].exhaust.push(s);
    }
    let other = c.new_card(0, CardId::Reap, false).unwrap();
    c.players[0].exhaust.push(other);
    c.play_card_effect(storm, Some(target));
    c.flush_pending_attack();
    assert_eq!(c.cr(target).hp, 64);
    for _ in 0..3 {
        let d = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].draw.push(d);
    }
    let mut rng = c.run.rngs.shuffle.clone();
    let position = rng.next_int(0, 3) as usize; // CardPileCmd.Add: NextInt(count + 1).
    c.play_card_effect(reave, Some(target));
    assert_eq!(c.cr(target).hp, 51);
    assert_eq!(c.players[0].draw.len(), 4);
    let soul = &c.cards[c.players[0].draw[position] as usize];
    assert_eq!(soul.id, CardId::Soul);
    assert!(soul.upgraded);
    assert_eq!(c.fault, None);
}

#[test]
fn sacrifice_uses_max_not_current_hp_kills_own_pet_then_applies_player_block_modifiers() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let s = hand_card(&mut c, p, CardId::Sacrifice);
    c.play_card_effect(s, None);
    assert_eq!(c.cr(p).block, 0);
    let other = c.summon_osty(0, 4).unwrap();
    let o = c.summon_osty(p, 9).unwrap();
    c.cr_mut(o).hp = 1;
    c.creature_apply_power(p, PowerId::Dexterity, 2, None, None, false, false);
    c.play_card_effect(s, None);
    assert_eq!(c.cr(p).block, 29);
    assert!(c.cr(o).death_processed);
    assert_eq!(c.cr(other).hp, 4);
    assert_eq!(c.fault, None);
}

#[test]
fn glimpse_beyond_generates_owned_souls_for_caster_and_living_teammates_only() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let dead = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 100);
    c.kill_creature(dead);
    let g = hand_card(&mut c, p, CardId::GlimpseBeyond);
    c.play_card_effect(g, None);
    for who in [0, p] {
        assert_eq!(c.players[who].draw.len(), 3);
        for &k in &c.players[who].draw {
            assert_eq!((c.cards[k as usize].id, c.cards[k as usize].owner), (CardId::Soul, who));
        }
    }
    assert!(c.players[dead].draw.is_empty());
    let soul = c.players[p].draw[0];
    c.play_card_effect(soul, None);
    assert_eq!(c.players[p].draw.len(), 1);
    assert_eq!(c.players[p].hand.len(), 3); // Glimpse plus two drawn Souls.
    assert_eq!(c.fault, None);
}

#[test]
fn graveblast_recovers_discard_and_reap_deals_canonical_damage() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let g = hand_card(&mut c, 0, CardId::Graveblast);
    let reap = c.new_card(0, CardId::Reap, false).unwrap();
    c.players[0].discard.push(reap);
    let e = enemy(&mut c, 100);
    c.play_card_effect(g, Some(e));
    assert_eq!(c.cr(e).hp, 96);
    assert!(c.players[0].hand.contains(&reap));
    assert!(c.players[0].discard.is_empty());
    c.play_card_effect(reap, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 69);
    assert_eq!(c.fault, None);
}

#[test]
fn misery_snapshots_debuffs_before_kill_preserves_applier_and_respects_artifact() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let m = hand_card(&mut c, 0, CardId::Misery);
    let t = enemy(&mut c, 5);
    let copied = enemy(&mut c, 100);
    let blocked = enemy(&mut c, 100);
    c.creature_apply_power(t, PowerId::Weak, 2, Some(0), None, false, false);
    c.creature_apply_power(blocked, PowerId::Artifact, 1, None, None, false, false);
    c.play_card_effect(m, Some(t));
    assert_eq!(c.cr(t).hp, 0);
    assert_eq!(c.power_amount(copied, PowerId::Weak), 2);
    let p = c.find_power(copied, PowerId::Weak).unwrap();
    assert_eq!(c.powers[p as usize].applier, Some(0));
    assert_eq!(c.power_amount(blocked, PowerId::Weak), 0);
    assert_eq!(c.power_amount(blocked, PowerId::Artifact), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn sic_em_marks_without_pet_and_only_appliers_pet_summons_even_on_blocked_hit() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let sic = hand_card(&mut c, p, CardId::SicEm);
    let e = enemy(&mut c, 100);
    c.play_card_effect(sic, Some(e));
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::SicEm)), (100, 3));
    let artifact = enemy(&mut c, 100);
    c.creature_apply_power(artifact, PowerId::Artifact, 1, None, None, false, false);
    assert_eq!(c.power_amount(artifact, PowerId::Artifact), 2); // Two-player Artifact scaling.
    c.play_card_effect(sic, Some(artifact));
    assert_eq!(c.power_amount(artifact, PowerId::Artifact), 1);
    assert_eq!(c.power_amount(artifact, PowerId::SicEm), 0);
    let a = c.summon_osty(0, 4).unwrap();
    let b = c.summon_osty(p, 4).unwrap();
    let wrong = hand_card(&mut c, 0, CardId::RightHandHand);
    let right = hand_card(&mut c, p, CardId::RightHandHand);
    c.cr_mut(e).block = 100;
    c.play_card_effect(wrong, Some(e));
    assert_eq!((c.cr(a).max_hp, c.cr(b).max_hp), (4, 4));
    c.play_card_effect(right, Some(e));
    assert_eq!((c.cr(e).hp, c.cr(b).max_hp, c.cr(b).hp), (100, 7, 7));
    c.fire_after_turn_end(sts2_sim::creature::Side::Player);
    assert_eq!(c.power_amount(e, PowerId::SicEm), 3);
    c.fire_after_turn_end(sts2_sim::creature::Side::Enemy);
    assert_eq!(c.power_amount(e, PowerId::SicEm), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn unleash_uses_current_pet_hp_and_pet_strength_fear_debuffs_after_damage() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let u = hand_card(&mut c, 0, CardId::Unleash);
    let f = hand_card(&mut c, 0, CardId::Fear);
    let e = enemy(&mut c, 100);
    c.play_card_effect(u, Some(e));
    assert_eq!(c.cr(e).hp, 100);
    let o = c.summon_osty(0, 10).unwrap();
    c.cr_mut(o).hp = 4;
    c.creature_apply_power(o, PowerId::Strength, 2, None, None, false, false);
    c.play_card_effect(u, Some(e));
    assert_eq!(c.cr(e).hp, 88); // 6 + current HP 4 + pet Strength 2.
    assert!(c.cards[f as usize].is_ethereal());
    c.play_card_effect(f, Some(e));
    assert_eq!(c.cr(e).hp, 81);
    assert_eq!(c.power_amount(e, PowerId::Vulnerable), 1);
    assert_eq!(c.fault, None);
}

#[test]
fn hang_doubles_only_after_hitting_and_pull_from_below_counts_owner_history() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let h = hand_card(&mut c, 0, CardId::Hang);
    let pull = hand_card(&mut c, 0, CardId::PullFromBelow);
    let e = enemy(&mut c, 200);
    for i in 0..3 {
        // Use the real play pipeline: HangPower reads the attacking card's model source.
        if i > 0 {
            assert_eq!(c.players[0].discard.pop(), Some(h));
            c.players[0].hand.insert(0, h);
        }
        c.players[0].energy = 1;
        assert!(c.play_card(0, 0, Some(0)));
    }
    assert_eq!((c.cr(e).hp, c.power_amount(e, PowerId::Hang)), (130, 8));
    c.hist.ethereal_plays_finished[0] = 2;
    c.play_card_effect(pull, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 120); // Hang does not multiply other cards.
    let eradicate = hand_card(&mut c, 0, CardId::Eradicate);
    c.cards[eradicate as usize].energy_spent = 3;
    c.play_card_effect(eradicate, Some(e));
    c.flush_pending_attack();
    assert_eq!(c.cr(e).hp, 87);
    assert_eq!(c.fault, None);
}

#[test]
fn high_five_is_one_pet_attack_then_vulnerable_and_only_owners_flatten_is_free() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let f0 = hand_card(&mut c, 0, CardId::Flatten);
    let fp = hand_card(&mut c, p, CardId::Flatten);
    let high = hand_card(&mut c, p, CardId::HighFive);
    assert!(!c.card_playable_by_logic(high));
    let o = c.summon_osty(p, 6).unwrap();
    assert!(c.card_playable_by_logic(high));
    c.creature_apply_power(o, PowerId::Strength, 2, None, None, false, false);
    c.creature_apply_power(p, PowerId::Strength, 20, None, None, false, false);
    let dead = enemy(&mut c, 5);
    let alive = enemy(&mut c, 50);
    c.play_card_effect(high, None);
    assert_eq!((c.cr(dead).hp, c.cr(alive).hp), (0, 37));
    assert_eq!(c.power_amount(alive, PowerId::Vulnerable), 2);
    assert_eq!(c.power_amount(dead, PowerId::Vulnerable), 0);
    assert_eq!(c.hist.attacked_this_turn, vec![o]);
    assert_eq!((c.cards[f0 as usize].cost, c.cards[fp as usize].cost), (2, 0));
    assert!(c.attack_stack.is_empty() && !c.pending_auto_attack);
    assert_eq!(c.fault, None);
}

#[test]
fn flatten_entering_after_blocked_attack_is_free_even_as_clone_but_not_next_turn() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let o = c.summon_osty(0, 6).unwrap();
    let e = enemy(&mut c, 50);
    c.cr_mut(e).block = 100;
    let right = hand_card(&mut c, 0, CardId::RightHandHand);
    c.play_card_effect(right, Some(e));
    assert_eq!(c.hist.attacked_this_turn, vec![o]);
    let f = hand_card(&mut c, 0, CardId::Flatten);
    assert_eq!(c.cards[f as usize].cost, 0);
    let clone = c.new_card(0, CardId::Flatten, false).unwrap();
    c.cards[clone as usize].cv.is_clone = true;
    c.players[0].hand.push(clone);
    c.fire_card_after_card_entered_combat(clone);
    assert_eq!(c.cards[clone as usize].cost, 0);
    c.cards[f as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[f as usize].cost, 2);
    c.hist.reset_side_turn();
    let next = hand_card(&mut c, 0, CardId::Flatten);
    assert_eq!(c.cards[next as usize].cost, 2);
    assert_eq!(c.fault, None);
}

#[test]
fn banshees_cry_uses_finished_ethereal_history_per_owner_and_clone_does_not_double_discount() {
    use sts2_sim::tables::{CardId, KW_ETHEREAL};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    enemy(&mut c, 50);
    let ethereal = hand_card(&mut c, p, CardId::DefendIronclad);
    c.cards[ethereal as usize].keywords |= KW_ETHEREAL;
    let banshee = hand_card(&mut c, p, CardId::BansheesCry);
    c.players[p].energy = 3;
    assert!(c.play_card(p, 0, None));
    assert_eq!(c.hist.ethereal_plays_finished[p as usize], 1);
    assert_eq!(c.cards[banshee as usize].cost, 7);
    c.cards[ethereal as usize].keywords &= !KW_ETHEREAL;
    let later = hand_card(&mut c, p, CardId::BansheesCry);
    assert_eq!(c.cards[later as usize].cost, 7);
    let other = hand_card(&mut c, 0, CardId::BansheesCry);
    assert_eq!(c.cards[other as usize].cost, 9);
    let clone = c.cards[later as usize].clone_with_id(100);
    let k = c.cards.len() as u16;
    c.cards.push(clone);
    c.players[p].hand.push(k);
    c.fire_card_after_card_entered_combat(k);
    assert_eq!(c.cards[k as usize].cost, 7);
    assert_eq!(c.fault, None);
}

#[test]
fn summon_grows_and_revives_same_pet() {
    let mut c = combat();
    assert_eq!(c.summon_osty(0, 0), None);
    let o = c.summon_osty(0, 6).unwrap();
    c.cr_mut(o).hp = 2;
    assert_eq!(c.summon_osty(0, 3), Some(o));
    assert_eq!((c.cr(o).max_hp, c.cr(o).hp), (9, 5));
    c.kill_creature(o);
    assert!(c.cr(o).in_combat && !c.cr(o).escaped);
    assert!(c.has_power(o, PowerId::DieForYou));
    assert_eq!(c.summon_osty(0, 4), Some(o));
    assert_eq!((c.cr(o).max_hp, c.cr(o).hp), (4, 4));
    assert!(!c.cr(o).death_processed);
    assert_eq!(c.hist.summons, vec![(0, 6), (0, 3), (0, 4)]);
    assert_eq!(c.fault, None);
}

#[test]
fn owner_block_then_osty_then_overkill() {
    let mut c = combat();
    let o = c.summon_osty(0, 6).unwrap();
    c.cr_mut(0).block = 3;
    c.apply_damage(0, 11, Props::MOVE, None);
    assert_eq!((c.cr(0).hp, c.cr(0).block, c.cr(o).hp), (48, 0, 0));
    assert!(c.cr(o).death_processed);
    assert_eq!(c.hist.damage.iter().map(|d| (d.target, d.unblocked)).collect::<Vec<_>>(),
               vec![(o, 6), (0, 2)]);
    assert_eq!(c.fault, None);
}

#[test]
fn unpowered_bypasses_pet_and_direct_pet_damage_uses_owner_block() {
    let mut c = combat();
    let o = c.summon_osty(0, 8).unwrap();
    c.apply_damage(0, 4, Props::UNPOWERED.or(Props::UNBLOCKABLE), None);
    assert_eq!((c.cr(0).hp, c.cr(o).hp), (46, 8));
    c.cr_mut(0).block = 5;
    c.apply_damage(o, 7, Props::MOVE, None);
    assert_eq!((c.cr(0).block, c.cr(o).hp), (0, 6));
    assert_eq!(c.fault, None);
}

#[test]
fn after_osty_buffer_only_consumed_by_overkill() {
    let mut c = combat();
    let o = c.summon_osty(0, 6).unwrap();
    c.creature_apply_power(0, PowerId::Buffer, 1, None, None, false, false);
    c.apply_damage(0, 4, Props::MOVE, None);
    assert_eq!((c.cr(o).hp, c.power_amount(0, PowerId::Buffer)), (2, 1));
    c.apply_damage(0, 5, Props::MOVE, None);
    assert_eq!((c.cr(0).hp, c.power_amount(0, PowerId::Buffer)), (50, 0));
    assert_eq!(c.fault, None);
}

#[test]
fn multiplayer_pets_are_owned_independently_and_die_with_owner() {
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let a = c.summon_osty(0, 4).unwrap();
    let b = c.summon_osty(p, 7).unwrap();
    c.apply_damage(p, 3, Props::MOVE, None);
    assert_eq!((c.cr(a).hp, c.cr(b).hp, c.cr(p).hp), (4, 4, 50));
    c.kill_creature(p);
    assert_eq!((c.cr(a).hp, c.cr(b).hp), (4, 0));
    assert!(c.cr(b).in_combat);
    assert_eq!(c.fault, None);
}

#[test]
fn osty_attack_uses_pet_strength_and_missing_pet_does_nothing() {
    use sts2_sim::card::Card;
    use sts2_sim::tables::CardId;
    let mut c = combat();
    c.cards.push(Card::from_factory(CardId::StrikeIronclad, false, 1).unwrap());
    let e = c.creatures.len() as u8;
    let mut enemy = Creature::player(50, 50);
    enemy.is_player = false;
    enemy.side = sts2_sim::creature::Side::Enemy;
    c.creatures.push(enemy);
    c.enemies.push(e);
    assert_eq!(c.osty_card_hit(0, e, 5), 0);
    let o = c.summon_osty(0, 6).unwrap();
    c.creature_apply_power(o, PowerId::Strength, 2, None, None, false, false);
    c.creature_apply_power(0, PowerId::Strength, 10, None, None, false, false);
    assert_eq!(c.osty_card_hit(0, e, 5), 7);
    assert!(c.attack_stack.is_empty());
    assert!(!c.pending_auto_attack);
    c.flush_pending_attack();
    assert_eq!(c.hist.damage.last().unwrap().dealer, Some(o));
    assert_eq!(c.fault, None);
}

#[test]
fn monster_multihit_finishes_redirected_pet_death_before_next_hit() {
    let mut c = combat();
    let o = c.summon_osty(0, 3).unwrap();
    let e = c.creatures.len() as u8;
    let mut enemy = Creature::player(50, 50);
    enemy.is_player = false;
    enemy.side = sts2_sim::creature::Side::Enemy;
    c.creatures.push(enemy);
    c.enemies.push(e);
    c.monster_attack_players(e, 5, 2);
    assert_eq!(c.cr(0).hp, 43);
    assert!(c.cr(o).death_processed && c.cr(o).in_combat);
    assert!(c.has_power(o, PowerId::DieForYou));
    assert_eq!(c.fault, None);
}

#[test]
fn melancholy_reduces_cost_in_each_owners_combat_piles_on_pet_death() {
    use sts2_sim::card::Card;
    use sts2_sim::tables::{CardId, CARD_HOOKS};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    for who in [0, p] {
        let mut card = Card::from_factory(CardId::Melancholy, false, who as u32).unwrap();
        card.owner = who;
        let k = c.cards.len() as u16;
        c.cards.push(card);
        c.players[who].hand.push(k);
    }
    c.card_hook_mask |= CARD_HOOKS[CardId::Melancholy.index()];
    let pet = c.summon_osty(0, 3).unwrap();
    c.kill_creature(pet);
    assert_eq!((c.cards[0].cost, c.cards[1].cost), (2, 2));
    c.play_card_effect(0, None);
    assert_eq!(c.cr(0).block, 13);
    assert_eq!(c.fault, None);
}

#[test]
fn melancholy_combat_cost_modifier_survives_turn_overrides_and_further_deaths() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let m = hand_card(&mut c, 0, CardId::Melancholy);
    let o = c.summon_osty(0, 3).unwrap();
    c.kill_creature(o);
    assert_eq!(c.cards[m as usize].cost, 2);
    c.cards[m as usize].set_energy_cost(0, sts2_sim::card::CostExpiry::Turn, false);
    c.cards[m as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[m as usize].cost, 2);
    c.cards[m as usize].set_energy_cost(2, sts2_sim::card::CostExpiry::Turn, false);
    c.summon_osty(0, 3);
    c.kill_creature(o);
    assert_eq!(c.cards[m as usize].cost, 1);
    c.cards[m as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[m as usize].cost, 1);
    c.cards[m as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[m as usize].cost, 1);
    assert_eq!(c.fault, None);
}

#[test]
fn turn_cost_cleanup_preserves_unplayable_negative_base_cost() {
    use sts2_sim::{card::Card, tables::CardId};
    let mut card = Card::from_factory(CardId::AscendersBane, false, 1).unwrap();
    assert_eq!(card.cost, -1);
    card.set_energy_cost(0, sts2_sim::card::CostExpiry::Turn, false);
    card.end_of_turn_cleanup();
    assert_eq!(card.cost, -1);
}

#[test]
fn right_hand_returns_only_from_discard_after_owners_two_energy_play() {
    use sts2_sim::card::Card;
    use sts2_sim::tables::{CardId, CARD_HOOKS};
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let mut right = Card::from_factory(CardId::RightHandHand, false, 1).unwrap();
    right.owner = p;
    c.cards.push(right);
    c.players[p].discard.push(0);
    c.card_hook_mask |= CARD_HOOKS[CardId::RightHandHand.index()];
    let mut played = Card::from_factory(CardId::StrikeIronclad, false, 2).unwrap();
    played.energy_spent = 2;
    c.cards.push(played);
    c.fire_card_late_effects(1);
    assert_eq!(&c.players[p].discard[..], &[0]);
    c.cards[1].owner = p;
    c.cards[1].energy_spent = 1;
    c.fire_card_late_effects(1);
    assert_eq!(&c.players[p].discard[..], &[0]);
    c.cards[1].energy_spent = 2;
    c.fire_card_late_effects(1);
    assert!(c.players[p].discard.is_empty());
    assert_eq!(&c.players[p].hand[..], &[0]);
    assert_eq!(c.fault, None);
}

#[test]
fn paper_phrog_and_cruelty_are_added_before_debilitate_doubles_vulnerable_bonus() {
    use sts2_sim::{relic::Relic, tables::RelicId};
    for (cruelty, expected) in [(0, 50), (25, 60)] {
        let mut c = combat();
        c.run.relics.push(Relic::new(RelicId::PaperPhrog));
        let e = enemy(&mut c, 200);
        c.creature_apply_power(e, PowerId::Vulnerable, 1, Some(0), None, false, false);
        c.creature_apply_power(e, PowerId::Debilitate, 2, Some(0), None, false, false);
        if cruelty > 0 {
            c.creature_apply_power(0, PowerId::Cruelty, cruelty, Some(0), None, false, false);
        }
        // C#: (1.5 + .25 + cruelty/100) * 2 - 1, times 20.
        assert_eq!(c.calculate_damage(20, Some(0), e, Props::MOVE), expected);
        c.deal_damage(Some(0), e, 20, Props::MOVE);
        assert_eq!(c.cr(e).hp, 200 - expected); // no separate relic ratio afterward.
        assert_eq!(c.fault, None);
    }
}

#[test]
fn osty_vulnerable_uses_owners_cruelty_unless_pet_has_its_own_without_paper_phrog() {
    use sts2_sim::{relic::Relic, tables::{CardId, RelicId}};
    let mut c = combat();
    c.run.relics.push(Relic::new(RelicId::PaperPhrog));
    let p = c.add_player(50, 50, 3, 0, &[]);
    let o = c.summon_osty(p, 10).unwrap();
    let e = enemy(&mut c, 200);
    c.creature_apply_power(p, PowerId::Cruelty, 25, None, None, false, false);
    c.creature_apply_power(e, PowerId::Vulnerable, 1, None, None, false, false);
    c.creature_apply_power(e, PowerId::Debilitate, 2, None, None, false, false);
    // Pet has no Player relics: 2*(1.5+.25)-1 = 2.5 from owner Cruelty only.
    let poke = hand_card(&mut c, p, CardId::Poke);
    c.play_card_effect(poke, Some(e));
    assert_eq!(c.cr(e).hp, 185); // 6 * 2.5
    c.creature_apply_power(o, PowerId::Cruelty, 50, None, None, false, false);
    c.play_card_effect(poke, Some(e));
    assert_eq!(c.cr(e).hp, 167); // direct pet Cruelty wins: 6 * 3
    assert_eq!(c.fault, None);
}

#[test]
fn autoplay_with_dead_owner_leaves_piles_effects_and_target_rng_untouched() {
    use sts2_sim::tables::CardId;
    let mut c = combat();
    let p = c.add_player(50, 50, 3, 0, &[]);
    let e = enemy(&mut c, 100);
    c.kill_creature(p);
    // Death cleanup empties the player's piles. Place an outstanding autoplay
    // candidate afterward to exercise AutoPlay's own guard, not that cleanup.
    let q = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
    c.players[p].exhaust.push(q);
    let before = c.run.rngs.combat_targets.counter;
    assert!(!c.auto_play_card(q, None, false));
    assert_eq!(c.players[p].exhaust.as_slice(), &[q]);
    assert!(c.players[p].discard.is_empty());
    assert_eq!(c.cr(e).hp, 100);
    assert_eq!(c.run.rngs.combat_targets.counter, before);
    assert_eq!(c.fault, None);
}
