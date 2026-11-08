//! C# Models/Powers hook contracts, including a nonzero player owner.
use sts2_sim::character::Character;
use sts2_sim::card::CId;
use sts2_sim::combat::{Combat, Props};
use sts2_sim::creature::{Creature, Side};
use sts2_sim::hooks::Ev;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::relic::Relic;
use sts2_sim::tables::{CardId, Hook, PowerId, RelicId};

fn combat() -> (Combat, CId, CId) {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let deck = run.deck.clone();
    let mut c = Combat::new(run, 1);
    c.add_player(80, 80, 3, 0, &deck);
    let mut enemy = Creature::player(100, 100);
    enemy.side = Side::Enemy;
    enemy.is_player = false;
    let e = c.creatures.len() as CId;
    c.creatures.push(enemy);
    c.enemies.push(e);
    (c, 1, e)
}

fn add_turn_enemy(c:&mut Combat)->CId {
    let (enemy,ai)=c.create_monster(sts2_sim::tables::MonsterKind::SludgeSpinner,0);
    let e=c.add_enemy(enemy,ai);
    c.cr_mut(e).hp=1000;
    c.cr_mut(e).max_hp=1000;
    e
}

#[test]
fn radiance_finishes_its_listener_after_lightning_rod_ends_combat() {
    use sts2_sim::orbs::OrbKind;
    for owner in [0,1] {
        let (mut c,_,enemy)=combat();
        c.combat_started=true;
        c.round=3;
        c.players[owner].energy=0;
        c.cr_mut(enemy).hp=1;
        c.players[owner].orbs.capacity=1;
        c.players[owner].orbs.base_slots=1;
        c.channel_orb(owner,OrbKind::Lightning);
        c.apply_power_to(owner,PowerId::LightningRod,1,Some(owner));
        c.apply_power_to(owner,PowerId::Radiance,1,Some(owner));
        c.fire_after_energy_reset(owner);
        assert!(c.is_over_or_ending());
        assert_eq!(c.players[owner].energy,0);
        assert_eq!(c.power_amount(owner,PowerId::Radiance),0);
        assert_eq!(c.fault,None);
    }
}

#[test]
fn nostalgia_counts_owner_attack_and_skill_starts_including_repeats() {
    // C# NostalgiaPower.ModifyCardPlayResultLocation counts CardPlaysStarted,
    // not cards finished, and excludes other players and non-Attack/Skill cards.
    for count in 1..=4 {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..count {
            c.add_player(80, 80, 3, 0, &deck);
        }
        add_turn_enemy(&mut c);
        for owner in 0..count as CId {
            c.apply_power_to(owner, PowerId::Nostalgia, 2, Some(owner));
        }
        // Later owners have zero starts and remain under their limit while this owner
        // exceeds its limit: their Nostalgia must not redirect this owner's second card.
        for owner in 0..count as CId {
            let power = c.new_card(owner, CardId::Inflame, false).unwrap();
            c.execute_card_play(power, None, false, false);
            let first = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.cards[first as usize].replay = 1;
            c.execute_card_play(first, None, false, false);
            assert_eq!(c.players[owner].draw.first(), Some(&first));
            let second = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.execute_card_play(second, None, false, false);
            assert!(c.players[owner].discard.contains(&second));
            assert!(!c.players[owner].draw.contains(&second));
        }
        c.start_player_turn();
        for owner in 0..count as CId {
            let exhaust = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.execute_card_play(exhaust, None, false, true);
            assert!(c.players[owner].exhaust.contains(&exhaust));
            let next = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.execute_card_play(next, None, false, false);
            assert_eq!(c.players[owner].draw.first(), Some(&next));
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn history_course_resumes_headbutt_before_later_players_in_sequential_recordings() {
    for count in 2..=4 {
        let mut run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        run.choice_scheduling = sts2_sim::combat::ChoiceScheduling::Sequential;
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..count {
            c.add_player(80, 80, 3, 0, &deck);
        }
        add_turn_enemy(&mut c);
        for owner in 0..count as CId {
            c.players[owner].draw = c.players[owner].starting_deck.clone();
            let mut relic = Relic::new(RelicId::HistoryCourse);
            relic.owner = owner;
            c.run.relics.push(relic);
            c.players[owner].turn_number = 1;
            let k = c.new_card(owner, if owner == 0 { CardId::Headbutt } else { CardId::StrikeIronclad }, false).unwrap();
            c.hist.finished_plays.push((owner, 1, k));
        }
        for _ in 0..2 {
            let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
            c.players[0].discard.push(k);
        }
        c.round = 2;
        c.start_player_turn();
        assert!(c.pending_choice.is_some());
        assert!(c.pending_turn_setup.is_some());
        assert_eq!(c.hist.finished_plays.len(), count);
        // HistoryCourse.AfterAutoPrePlayPhaseEntered awaits AutoPlay. The
        // synchronous test Selector serializes its answer; Live parks the stack.
        assert_eq!(c.fault, None);
        assert!(c.resolve_pending_choice_for_player(0, Some(0)));
        assert_eq!(c.fault, None);
        assert!(c.pending_choice.is_none());
        assert_eq!(c.hist.finished_plays.len(), count * 2);
    }
}

#[test]
fn calamity_samples_the_owners_attack_pool_using_current_amount() {
    let (mut c, who, _) = combat();
    c.players[who].character = Some(Character::Silent);
    c.apply_power_to(who, PowerId::Calamity, 1, Some(who));
    let attack = c.new_card(who, CardId::StrikeIronclad, false).unwrap();
    let other = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    c.fire(Hook::BeforeCardPlayed, Ev::Card(other), None);
    c.fire(Hook::AfterCardPlayed, Ev::Card(other), None);
    assert!(c.players[who].hand.is_empty());
    c.fire(Hook::BeforeCardPlayed, Ev::Card(attack), None);
    c.apply_power_to(who, PowerId::Calamity, 1, Some(who));
    let pool = sts2_sim::cardgen::character_pool(sts2_sim::character::CardPool::Silent, true, Some(sts2_sim::tables::CardType::Attack), None).unwrap();
    let mut rng = c.run.rngs.combat_card_generation.clone();
    let expected = [pool[rng.choice_index(pool.len())], pool[rng.choice_index(pool.len())]];
    c.fire(Hook::AfterCardPlayed, Ev::Card(attack), None);
    assert_eq!(c.players[who].hand.len(), 2);
    for (&k, id) in c.players[who].hand.iter().zip(expected) {
        assert_eq!(c.cards[k as usize].owner, who);
        assert_eq!(c.cards[k as usize].id, id);
    }
    c.fire(Hook::AfterCardPlayed, Ev::Card(attack), None);
    assert_eq!(c.players[who].hand.len(), 2);
}

#[test]
fn history_course_replays_only_its_owners_last_turn_original_attack() {
    let (mut c, who, enemy) = combat();
    let mut relic = Relic::new(RelicId::HistoryCourse);
    relic.owner = who;
    c.run.relics.push(relic);
    c.players[who].turn_number = 1;
    let attack = c.new_card(who, CardId::StrikeIronclad, false).unwrap();
    let damage = c.cards[attack as usize].damage.unwrap();
    c.execute_card_play(attack, Some(enemy), false, false);
    assert_eq!(c.cr(enemy).hp, 100 - damage);
    // The latest finished card is a Skill; C# still selects the previous Attack.
    let skill = c.new_card(who, CardId::DefendIronclad, false).unwrap();
    c.execute_card_play(skill, None, false, false);
    let block = c.cr(who).block;
    c.players[who].energy = 7;
    c.players[who].turn_number = 2;
    c.fire_after_player_turn_start(0);
    assert_eq!(c.cr(enemy).hp, 100 - damage);
    c.fire_after_player_turn_start(who);
    assert_eq!(c.cr(enemy).hp, 100 - damage);
    c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(0), None);
    assert_eq!(c.cr(enemy).hp, 100 - damage);
    c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(who), None);
    assert_eq!(c.cr(enemy).hp, 100 - 2 * damage);
    assert_eq!(c.cr(who).block, block);
    assert_eq!(c.players[who].energy, 7);
    let copy = c.hist.finished_plays.last().unwrap().2;
    assert!(c.cards[copy as usize].cv.is_dupe);
    assert_eq!(c.cards[copy as usize].owner, who);
    c.players[who].turn_number = 3;
    c.fire(Hook::AfterAutoPrePlayPhaseEntered, Ev::Creature(who), None);
    assert_eq!(c.cr(enemy).hp, 100 - 2 * damage);
    assert_eq!(c.fault, None);
}

#[test]
fn history_course_waits_for_side_start_prep_time_for_every_owner() {
    // C# CombatManager.StartTurn / RunAutoPrePlayPhase: PrepTime.AfterSideTurnStart
    // grants Vigor before any HistoryCourse replay, for one through four players.
    for count in 1..=4 {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..count {
            c.add_player(80, 80, 3, 0, &deck);
        }
        let e=add_turn_enemy(&mut c);
        let mut expected_damage = 0;
        for owner in 0..count as CId {
            let mut relic = Relic::new(RelicId::HistoryCourse);
            relic.owner = owner;
            c.run.relics.push(relic);
            c.players[owner].turn_number = 1;
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            let damage = c.cards[k as usize].damage.unwrap();
            c.execute_card_play(k, Some(e), false, false);
            expected_damage += 2 * damage + 4;
            c.apply_power_to(owner, PowerId::PrepTime, 4, Some(owner));
        }
        c.round = 2;
        c.start_player_turn();
        assert_eq!(c.cr(e).hp, 1000 - expected_damage, "{count} players");
        for owner in 0..count as CId {
            assert_eq!(c.players[owner].turn_number, 2);
            assert_eq!(c.power_amount(owner, PowerId::Vigor), 0);
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn free_power_late_cost_override_wins_over_spiked_gauntlets_and_spends_no_energy() {
    let (mut c, _, _) = combat();
    c.run.relics.push(Relic::new(RelicId::SpikedGauntlets));
    let card = c.new_card(0, CardId::DemonFormCard, false).unwrap();
    c.players[0].hand.push(card);
    let base_cost = c.cards[card as usize].cost;
    assert_eq!(c.modified_card_cost(0, card), base_cost + 1);
    c.apply_power_to(0, PowerId::FreePower, 1, Some(0));
    assert_eq!(c.modified_card_cost(0, card), 0);
    c.players[0].energy = 2;
    c.execute_card_play(card, None, true, false);
    assert_eq!(c.players[0].energy, 2);
    assert_eq!(c.cards[card as usize].energy_spent, 0);
    assert!(!c.has_power(0, PowerId::FreePower));
    assert!(c.has_power(0, PowerId::DemonForm));
    assert_eq!(c.fault, None);
}

#[test]
fn panache_instances_skip_their_first_play_and_reset_only_the_turn_counter() {
    let (mut c, who, enemy) = combat();
    let own = c.new_card(who, CardId::StrikeIronclad, false).unwrap();
    let other = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let a = c.apply_power_to(who, PowerId::Panache, 10, Some(who)).unwrap();
    c.fire(Hook::AfterCardPlayed, Ev::Card(own), None);
    for _ in 0..4 { c.fire(Hook::AfterCardPlayed, Ev::Card(own), None); }
    c.fire(Hook::AfterCardPlayed, Ev::Card(other), None);
    assert_eq!(c.cr(enemy).hp, 100);
    let b = c.apply_power_to(who, PowerId::Panache, 20, Some(who)).unwrap();
    assert_ne!(a, b);
    c.fire(Hook::AfterCardPlayed, Ev::Card(own), None);
    assert_eq!(c.cr(enemy).hp, 90);
    c.fire_after_turn_end(Side::Player);
    for _ in 0..5 { c.fire(Hook::AfterCardPlayed, Ev::Card(own), None); }
    assert_eq!(c.cr(enemy).hp, 60);
    assert_eq!(c.fault, None);
}

#[test]
fn rolling_boulders_grow_independently_on_their_owners_turn() {
    let (mut c, who, enemy) = combat();
    let a = c.apply_power_to(who, PowerId::RollingBoulder, 5, Some(who)).unwrap();
    let b = c.apply_power_to(who, PowerId::RollingBoulder, 10, Some(who)).unwrap();
    c.fire_after_player_turn_start(0);
    assert_eq!(c.cr(enemy).hp, 100);
    c.fire_after_player_turn_start(who);
    assert_eq!(c.cr(enemy).hp, 85);
    assert_eq!(c.powers[a as usize].amount, 10);
    assert_eq!(c.powers[b as usize].amount, 15);
    c.fire_after_player_turn_start(who);
    assert_eq!(c.cr(enemy).hp, 60);
    assert_eq!(c.fault, None);
}

#[test]
fn unsettling_lamp_doubles_dark_shackles_but_not_its_internal_strength() {
    let (mut c, _, enemy) = combat();
    c.run.relics.push(Relic::new(RelicId::UnsettlingLamp));
    let source = c.new_card(0, CardId::DarkShackles, false).unwrap();
    c.creature_apply_power(enemy, PowerId::DarkShackles, 9, Some(0), Some(source), false, false);
    assert_eq!(c.power_amount(enemy, PowerId::DarkShackles), 18);
    assert_eq!(c.power_amount(enemy, PowerId::Strength), -18);
    c.fire_after_turn_end(Side::Enemy);
    assert_eq!(c.power_amount(enemy, PowerId::Strength), 0);
    assert!(!c.has_power(enemy, PowerId::DarkShackles));
    assert_eq!(c.fault, None);
}

#[test]
fn flex_potion_strength_is_attached_before_its_wrapper_and_restacks_once() {
    let (mut c, who, _) = combat();
    c.apply_power_to(who, PowerId::FlexPotion, 5, Some(who));
    let order: Vec<_> = c.cr(who).power_list().into_iter().map(|p| c.powers[p as usize].id).collect();
    assert_eq!(order, vec![PowerId::Strength, PowerId::FlexPotion]);
    assert_eq!(c.power_amount(who, PowerId::Strength), 5);
    c.apply_power_to(who, PowerId::FlexPotion, 5, Some(who));
    assert_eq!(c.power_amount(who, PowerId::Strength), 10);
    c.fire_after_turn_end(Side::Player);
    assert_eq!(c.power_amount(who, PowerId::Strength), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn confused_uses_canonical_cost_and_only_its_owners_draws() {
    let (mut c, who, _) = combat();
    c.apply_power_to(who, PowerId::Confused, 1, Some(who));
    let own = c.new_card(who, CardId::StrikeIronclad, false).unwrap();
    let other = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let unplayable = c.new_card(who, CardId::Dazed, false).unwrap();
    c.cards[own as usize].set_energy_cost(-1, sts2_sim::card::CostExpiry::Combat, false);
    assert_eq!(c.cards[own as usize].local_energy_cost(), -1);
    let mut rng = c.run.rngs.combat_energy_costs.clone();
    let expected = rng.next_int(0, 3) as i32;
    c.fire(Hook::OnCardDrawn, Ev::Card(other), None);
    c.fire(Hook::OnCardDrawn, Ev::Card(unplayable), None);
    assert_eq!(c.run.rngs.combat_energy_costs.counter, 0);
    c.fire(Hook::OnCardDrawn, Ev::Card(own), None);
    assert_eq!(c.cards[own as usize].cost, expected);
    assert_eq!(c.modified_card_cost(who, own), expected);
    c.cards[own as usize].after_card_played_cost_cleanup();
    c.cards[own as usize].end_of_turn_cleanup();
    assert_eq!(c.cards[own as usize].cost, expected);
    assert_eq!(c.run.rngs.combat_energy_costs.counter, rng.counter);
    assert_eq!(c.fault, None);
}

#[test]
fn confused_absolute_cost_orders_necro_discounts_and_turn_overrides() {
    use sts2_sim::card::CostExpiry;
    for id in [CardId::Melancholy, CardId::BansheesCry] {
        let (mut c, who, _) = combat();
        c.apply_power_to(who, PowerId::Confused, 1, Some(who));
        let k = c.new_card(who, id, false).unwrap();
        c.cards[k as usize].add_combat_cost(-2);
        c.cards[k as usize].set_energy_cost(0, CostExpiry::Turn, false);
        let mut rng = c.run.rngs.combat_energy_costs.clone();
        let randomized = rng.next_int(0, 3) as i32;
        c.fire(Hook::OnCardDrawn, Ev::Card(k), None);
        assert_eq!(c.cards[k as usize].cost, randomized);
        assert_eq!(c.modified_card_cost(who, k), randomized);
        c.cards[k as usize].add_combat_cost(-1);
        c.cards[k as usize].set_energy_cost(0, CostExpiry::Turn, false);
        c.cards[k as usize].after_card_played_cost_cleanup();
        assert_eq!(c.cards[k as usize].cost, 0);
        c.cards[k as usize].end_of_turn_cleanup();
        assert_eq!(c.cards[k as usize].cost, (randomized - 1).max(0));
        assert_eq!(c.modified_card_cost(who, k), (randomized - 1).max(0));
        assert_eq!(c.run.rngs.combat_energy_costs.counter, rng.counter);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn free_power_is_a_hand_or_play_late_override_not_a_local_cost_write() {
    use sts2_sim::card::CostExpiry;
    let (mut c, _, _) = combat();
    c.run.relics.push(Relic::new(RelicId::SpikedGauntlets));
    let k = c.new_card(0, CardId::DemonFormCard, false).unwrap();
    c.cards[k as usize].set_energy_cost(2, CostExpiry::Combat, false);
    c.cards[k as usize].add_combat_cost(-1);
    c.cards[k as usize].set_energy_cost(1, CostExpiry::TurnOrPlayed, true);
    c.players[0].hand.push(k);
    assert_eq!(c.modified_card_cost(0, k), 2);
    c.apply_power_to(0, PowerId::FreePower, 1, Some(0));
    assert_eq!(c.modified_card_cost(0, k), 0);
    assert_eq!(c.cards[k as usize].cost, 1);
    c.players[0].hand.clear();
    c.players[0].discard.push(k);
    assert_eq!(c.modified_card_cost(0, k), 2);
    c.players[0].discard.clear();
    c.players[0].play.push(k);
    assert_eq!(c.modified_card_cost(0, k), 0);
    assert_eq!(c.cards[k as usize].cost, 1);
    assert_eq!(c.fault, None);
}

#[test]
fn negative_local_cost_bypasses_global_modifiers_before_final_clamping() {
    let (mut c, _, _) = combat();
    c.run.relics.push(Relic::new(RelicId::SpikedGauntlets));
    let k = c.new_card(0, CardId::DemonFormCard, false).unwrap();
    c.players[0].hand.push(k);
    c.cards[k as usize].add_combat_cost(-4);
    assert_eq!(c.cards[k as usize].local_energy_cost(), -1);
    assert_eq!(c.modified_card_cost(0, k), 0);
    c.apply_power_to(0, PowerId::Curious, 1, Some(0));
    assert_eq!(c.modified_card_cost(0, k), 0);
    assert_eq!(c.fault, None);
}

#[test]
fn confused_uses_zero_canonical_cost_for_x_cards_even_with_a_legacy_x_sentinel() {
    let (mut c, who, _) = combat();
    c.apply_power_to(who, PowerId::Confused, 1, Some(who));
    let k = c.new_card(who, CardId::Cascade, false).unwrap();
    assert!(c.cards[k as usize].x_cost);
    let mut rng = c.run.rngs.combat_energy_costs.clone();
    rng.next_int(0, 3);
    c.fire(Hook::OnCardDrawn, Ev::Card(k), None);
    assert_eq!(c.cards[k as usize].local_energy_cost(), 0);
    assert_eq!(c.modified_card_cost(who, k), 0);
    assert_eq!(c.cards[k as usize].cost, c.cards[k as usize].original_cost);
    assert_eq!(c.run.rngs.combat_energy_costs.counter, rng.counter);
    assert_eq!(c.fault, None);
}

#[test]
fn fasten_only_changes_owners_powered_defend_block() {
    let (mut c, who, _) = combat();
    c.apply_power_to(who, PowerId::Fasten, 3, Some(who));
    let defend = c.new_card(who, CardId::DefendIronclad, false).unwrap();
    let strike = c.new_card(who, CardId::StrikeIronclad, false).unwrap();
    assert_eq!(c.modify_block(5, who, Props::MOVE, Some(defend)), 8);
    assert_eq!(c.modify_block(5, who, Props::MOVE, Some(strike)), 5);
    assert_eq!(c.modify_block(5, who, Props::UNPOWERED, Some(defend)), 5);
    assert_eq!(c.modify_block(5, 0, Props::MOVE, Some(defend)), 5);
    assert_eq!(c.modify_block(5, who, Props::MOVE, None), 8);
    assert_eq!(c.fault, None);
}

#[test]
fn temporary_strength_stacks_and_expires_on_owners_side() {
    let (mut c, who, enemy) = combat();
    c.apply_power_to(enemy, PowerId::DarkShackles, 9, Some(who));
    c.apply_power_to(enemy, PowerId::DarkShackles, 4, Some(who));
    c.apply_power_to(who, PowerId::ReptileTrinket, 3, Some(who));
    assert_eq!(c.power_amount(enemy, PowerId::Strength), -13);
    assert_eq!(c.power_amount(who, PowerId::Strength), 3);
    c.fire_after_turn_end(Side::Player);
    assert_eq!(c.power_amount(enemy, PowerId::Strength), -13);
    assert_eq!(c.power_amount(who, PowerId::Strength), 0);
    c.fire_after_turn_end(Side::Enemy);
    assert_eq!(c.power_amount(enemy, PowerId::Strength), 0);
    assert!(!c.has_power(enemy, PowerId::DarkShackles));
    assert_eq!(c.fault, None);
}

#[test]
fn clay_waits_for_its_owners_block_clear_and_prep_time_for_its_side() {
    let (mut c, who, enemy) = combat();
    c.apply_power_to(who, PowerId::SelfFormingClay, 6, Some(who));
    c.apply_power_to(who, PowerId::PrepTime, 4, Some(who));
    c.fire(Hook::AfterBlockCleared, Ev::Creature(enemy), None);
    c.fire_after_side_turn_start(Side::Enemy);
    assert_eq!(c.cr(who).block, 0);
    assert_eq!(c.power_amount(who, PowerId::Vigor), 0);
    c.fire(Hook::AfterBlockCleared, Ev::Creature(who), None);
    c.fire_after_side_turn_start(Side::Player);
    assert_eq!(c.cr(who).block, 6);
    assert!(!c.has_power(who, PowerId::SelfFormingClay));
    assert_eq!(c.power_amount(who, PowerId::Vigor), 4);
    assert_eq!(c.fault, None);
}

#[test]
fn gambit_ignores_unpowered_and_blocked_hits_but_kills_on_unblocked_attack() {
    let (mut c, who, enemy) = combat();
    c.apply_power_to(who, PowerId::TheGambit, 1, Some(who));
    c.cr_mut(who).block = 5;
    c.deal_damage(Some(enemy), who, 5, Props::MOVE);
    c.deal_damage(Some(enemy), who, 1, Props::UNPOWERED);
    assert!(c.cr(who).is_alive());
    c.deal_damage(Some(enemy), who, 1, Props::MOVE);
    assert!(c.cr(who).is_dead());
    assert!(!c.has_power(who, PowerId::TheGambit));
    assert_eq!(c.fault, None);
}

#[test]
fn reflect_returns_only_blocked_powered_damage_and_expires() {
    let (mut c, who, enemy) = combat();
    c.apply_power_to(who, PowerId::Reflect, 1, Some(who));
    c.cr_mut(who).block = 3;
    c.deal_damage(Some(enemy), who, 5, Props::MOVE);
    assert_eq!(c.cr(enemy).hp, 97);
    assert_eq!(c.cr(who).hp, 78);
    // Buffer prevents HP loss but is not block and must not be reflected.
    c.apply_power_to(who, PowerId::Buffer, 1, Some(who));
    c.deal_damage(Some(enemy), who, 5, Props::MOVE);
    assert_eq!(c.cr(enemy).hp, 97);
    c.fire_after_side_turn_start(Side::Enemy);
    assert!(c.has_power(who, PowerId::Reflect));
    c.fire_after_side_turn_start(Side::Player);
    assert!(!c.has_power(who, PowerId::Reflect));
    assert_eq!(c.fault, None);
}
