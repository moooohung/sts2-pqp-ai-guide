//! Every Defect card (r7 `cards/defect.py`, 91 effects), on the shared `kit` helpers and the
//! C#-shaped orb queue (`crate::orbs`). Prism, Kaleidoscope and similar relics put these into
//! Ironclad runs.

use super::kit::*;
use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, PilePos, Props};
use crate::orbs::OrbKind;
use crate::tables::*;
use arrayvec::ArrayVec;

fn resolver(id: CardId, k: KRef) -> Resolver {
    Resolver { src: ChoiceSrc::Card(id), k, target: None, step: 0, arg: 0 }
}

/// `card.energy_spent` (+1 when upgraded) for the X-cost orb cards.
fn x_plus_upgrade(c: &Combat, k: KRef) -> i32 {
    let card = &c.cards[k as usize];
    card.energy_spent + i32::from(card.upgraded)
}

/// Evoke the front orb `n` times; only the last evoke removes it (`OrbCmd.EvokeNext(dequeue)`).
fn evoke_front_times(c: &mut Combat, who: CId, n: i32) {
    for i in 0..n.max(0) {
        if c.players[who].orbs.orbs.is_empty() {
            break;
        }
        if i == n - 1 {
            c.evoke_front_orb(who);
        } else {
            c.evoke_front_orb_keep(who);
        }
    }
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, t: Option<CId>) -> bool {
    let id = c.cards[k as usize].id;
    let who = owner(c, k);
    match id {
        // C# Fuel.OnPlay: token energy, with the upgraded amount from its dynamic var.
        CardId::Fuel => gain_energy(c, who, var(c, k, VarKey::Energy, 1)),
        // ── BASIC ──
        CardId::StrikeDefect | CardId::Leap | CardId::BootSequence | CardId::DefendDefect => {
            if c.cards[k as usize].damage.is_some() {
                hit(c, k, target(t));
            } else {
                block(c, k);
            }
        }
        CardId::Zap => c.channel_orb(who, OrbKind::Lightning),
        CardId::Dualcast => evoke_front_times(c, who,2),
        // ── COMMON ──
        CardId::BallLightning => {
            hit(c, k, target(t));
            c.channel_orb(who, OrbKind::Lightning);
        }
        CardId::Barrage => {
            let n = c.players[who].orbs.orbs.len() as i32;
            hit_n(c, k, target(t), n);
        }
        CardId::BeamCell => {
            let t = target(t);
            hit(c, k, t);
            let v = var(c, k, VarKey::Vulnerable, 1);
            c.apply_power_to(t, PowerId::Vulnerable, v, None);
        }
        CardId::BoostAway => {
            block(c, k);
            status_to_discard(c, who, CardId::Dazed);
        }
        CardId::ChargeBattery => {
            block(c, k);
            let e = var(c, k, VarKey::Energy, 1);
            self_power(c, k, PowerId::EnergyNextTurn, e);
        }
        CardId::Claw => {
            hit(c, k, target(t));
            let inc = var(c, k, VarKey::Increase, 2);
            for q in c.pile_cards(who) {
                if c.cards[q as usize].id == CardId::Claw && c.cards[q as usize].owner == who {
                    increase_damage(c, q, inc);
                }
            }
        }
        CardId::ColdSnap => {
            hit(c, k, target(t));
            c.channel_orb(who, OrbKind::Frost);
        }
        // C# Rebound.OnPlay: finish damage before applying the owner's pile modifier.
        CardId::Rebound => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            self_power(c, k, PowerId::Rebound, 1);
        }
        CardId::CompileDriver => {
            hit(c, k, target(t));
            let n = c.count_distinct_orb_kinds(who);
            if n > 0 {
                draw(c, k, n);
            }
        }
        CardId::Coolheaded => {
            c.channel_orb(who, OrbKind::Frost);
            let n = var(c, k, VarKey::Cards, 1);
            draw(c, k, n);
        }
        CardId::FocusedStrikeCard => {
            hit(c, k, target(t));
            let n = var(c, k, VarKey::FocusPower, 1);
            self_power(c, k, PowerId::FocusedStrike, n);
        }
        CardId::GoForTheEyes => {
            let t = target(t);
            hit(c, k, t);
            if intends_to_attack(c, t) {
                let n = var(c, k, VarKey::Weak, 1);
                c.apply_power_to(t, PowerId::Weak, n, None);
            }
        }
        CardId::GunkUp => {
            let n = var(c, k, VarKey::Repeat, 3);
            hit_n(c, k, target(t), n);
            status_to_discard(c, who, CardId::Slimed);
        }
        CardId::Hologram => {
            block(c, k);
            if !c.players[who].discard.is_empty() {
                let cards: Vec<KRef> = c.players[who].discard.clone();
                c.request_card_choice(&cards, PileKind::Discard, resolver(id, k), false);
            }
        }
        CardId::Hotfix => {
            let n = var(c, k, VarKey::FocusPower, 2);
            self_power(c, k, PowerId::Hotfix, n);
        }
        CardId::LightningRod => {
            block(c, k);
            let n = var(c, k, VarKey::LightningRodPower, 2);
            self_power(c, k, PowerId::LightningRod, n);
        }
        CardId::MomentumStrike => {
            hit(c, k, target(t));
            c.cards[k as usize].set_energy_cost(0, crate::card::CostExpiry::Combat, false);
        }
        CardId::SweepingBeam => {
            hit_all(c, k);
            let n = var(c, k, VarKey::Cards, 1);
            draw(c, k, n);
        }
        CardId::Turbo => {
            let n = var(c, k, VarKey::Energy, 2);
            gain_energy(c, who, n);
            status_to_discard(c, who, CardId::Void);
        }
        CardId::Uproar => {
            hit_n(c, k, target(t), 2);
            let attacks = |c: &Combat, playable_only: bool| -> Vec<KRef> {
                c.players[who].draw.iter().copied().filter(|&q| {
                    let card = &c.cards[q as usize];
                    card.card_type == CardType::Attack && (!playable_only || !card.is_unplayable())
                }).collect()
            };
            let mut cands = attacks(c, true);
            if cands.is_empty() {
                cands = attacks(c, false);
            }
            if !cands.is_empty() {
                stable_shuffle(c, &mut cands);
                c.auto_play_card(cands[0], None, false);
            }
        }
        // ── UNCOMMON ──
        CardId::BulkUp => {
            let s = var(c, k, VarKey::Strength, 2);
            let d = var(c, k, VarKey::Dexterity, 2);
            self_power(c, k, PowerId::Strength, s);
            self_power(c, k, PowerId::Dexterity, d);
            let n = var(c, k, VarKey::OrbSlots, 1);
            c.remove_orb_slots(who, n);
        }
        CardId::Capacitor => {
            let n = var(c, k, VarKey::Repeat, 2);
            c.add_orb_slots(who, n);
        }
        CardId::Chaos => {
            for _ in 0..var(c, k, VarKey::Repeat, 1).max(0) {
                c.channel_random_orb(who);
            }
        }
        CardId::Chill => {
            for _ in c.hittable_enemies() {
                c.channel_orb(who, OrbKind::Frost);
            }
        }
        CardId::DarknessCard => {
            c.channel_orb(who, OrbKind::Dark);
            let n = var(c, k, VarKey::Passives, if c.cards[k as usize].upgraded { 2 } else { 1 });
            for i in 0..c.players[who].orbs.orbs.len() {
                if c.players[who].orbs.orbs[i].kind == OrbKind::Dark {
                    for _ in 0..n {
                        c.orb_passive_at(who, i);
                    }
                }
            }
        }
        CardId::DoubleEnergy => c.players[who].energy*= 2,
        CardId::EnergySurge => {
            // C# EnergySurge.OnPlay: GetTeammatesOf includes the owner.
            let n = var(c, k, VarKey::Energy, 2);
            for player in 0..c.players.len() as CId {
                if c.cr(player).is_alive() {
                    gain_energy(c, player, n);
                }
            }
        }
        CardId::Feral => {
            let n = var(c, k, VarKey::FeralPower, 1);
            self_power(c, k, PowerId::Feral, n);
        }
        CardId::FightThrough => {
            block(c, k);
            for _ in 0..var(c, k, VarKey::Wounds, 2) {
                status_to_discard(c, who, CardId::Wound);
            }
        }
        CardId::Ftl => {
            hit(c, k, target(t));
            let max = var(c, k, VarKey::PlayMax, if c.cards[k as usize].upgraded { 4 } else { 3 });
            if plays_finished(c, who) < max {
                let n = var(c, k, VarKey::Cards, 1);
                draw(c, k, n);
            }
        }
        CardId::Fusion => c.channel_orb(who, OrbKind::Plasma),
        CardId::Glacier => {
            block(c, k);
            c.channel_orb(who, OrbKind::Frost);
            c.channel_orb(who, OrbKind::Frost);
        }
        CardId::Glasswork => {
            block(c, k);
            c.channel_orb(who, OrbKind::Glass);
        }
        CardId::Hailstorm => {
            let n = var(c, k, VarKey::HailstormPower, 6);
            self_power(c, k, PowerId::Hailstorm, n);
        }
        CardId::IterationCard => {
            let n = var(c, k, VarKey::IterationPower, 2);
            self_power(c, k, PowerId::Iteration, n);
        }
        CardId::LoopCard => {
            let n = var(c, k, VarKey::Loop, 1);
            self_power(c, k, PowerId::Loop, n);
        }
        CardId::NullCard => {
            let t = target(t);
            hit(c, k, t);
            let n = var(c, k, VarKey::Weak, 2);
            c.apply_power_to(t, PowerId::Weak, n, None);
            c.channel_orb(who, OrbKind::Dark);
        }
        CardId::Overclock => {
            let n = var(c, k, VarKey::Cards, 2);
            draw(c, k, n);
            status_to_discard(c, who, CardId::Burn);
        }
        CardId::Refract => {
            let n = var(c, k, VarKey::Repeat, 2);
            hit_n(c, k, target(t), n);
            for _ in 0..n {
                c.channel_orb(who, OrbKind::Glass);
            }
        }
        CardId::RocketPunch => {
            hit(c, k, target(t));
            let n = var(c, k, VarKey::Cards, 1);
            c.draw_cards_at_end_of_play(who, n);
        }
        CardId::Scavenge => {
            let e = var(c, k, VarKey::Energy, 2);
            if c.players[who].hand.is_empty() {
                self_power(c, k, PowerId::EnergyNextTurn, e);
            } else {
                let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::Scrape => {
            hit(c, k, target(t));
            if c.is_over {
                return true;
            }
            let n = var(c, k, VarKey::Cards, 4);
            c.draw_cards_then_discard_nonzero(who,n);
        }
        CardId::ShadowShield => {
            block(c, k);
            c.channel_orb(who, OrbKind::Dark);
        }
        CardId::Skim => {
            let n = var(c, k, VarKey::Cards, 3);
            // Skim.OnPlay ends with awaited Draw; PendingPlay owns its continuation.
            c.draw_cards_at_end_of_play(who, n);
        }
        CardId::Smokestack => {
            let n = var(c, k, VarKey::SmokestackPower, 5);
            self_power(c, k, PowerId::Smokestack, n);
        }
        CardId::StormCard => {
            let n = var(c, k, VarKey::StormPower, 1);
            self_power(c, k, PowerId::Storm, n);
        }
        CardId::Subroutine => self_power(c, k, PowerId::Subroutine, 1),
        CardId::Sunder => {
            let t = target(t);
            let alive = c.cr(t).is_alive();
            hit(c, k, t);
            if alive && c.cr(t).is_dead() {
                let n = var(c, k, VarKey::Energy, 3);
                gain_energy(c, who, n);
            }
        }
        CardId::Synchronize => {
            let n = c.count_distinct_orb_kinds(who) * var(c, k, VarKey::CalcExtra, 2);
            if n > 0 {
                self_power(c, k, PowerId::Synchronize, n);
            }
        }
        CardId::Synthesis => {
            hit(c, k, target(t));
            self_power(c, k, PowerId::FreePower, 1);
        }
        CardId::Tempest => {
            for _ in 0..x_plus_upgrade(c, k).max(0) {
                c.channel_orb(who, OrbKind::Lightning);
            }
        }
        CardId::TeslaCoil => {
            let t = target(t);
            hit(c, k, t);
            // AttackCommand.Execute (including AfterAttack) completes before
            // the following OrbCmd.Passive calls.
            c.flush_pending_attack();
            let orbs = c.players[who].orbs.orbs.clone();
            for orb in orbs {
                if orb.kind == OrbKind::Lightning {
                    // TeslaCoil.OnPlay awaits Passive twice per Lightning orb
                    // when upgraded, with the explicit card target.
                    for _ in 0..if c.cards[k as usize].upgraded {2}else{1} {
                        let v = c.orb_passive_value(who, orb);
                        let activation = c.record_orb_activation(who, orb, "passive", false);
                        activation.target(t);
                        if v > 0 {
                            c.apply_damage(t, v, Props::UNPOWERED, Some(who));
                        }
                    }
                }
            }
        }
        CardId::ThunderCard => {
            let n = var(c, k, VarKey::ThunderPower, 6);
            self_power(c, k, PowerId::Thunder, n);
        }
        CardId::WhiteNoise => {
            // create_character_cards defaults to distinct=True (cards/factory.py).
            if let Some(&g) = c.generate_character_cards(who,Some(CardType::Power), None, 1, true).first() {
                c.set_temporary_free_this_turn(g);
                c.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        // ── RARE ──
        CardId::AdaptiveStrike => {
            hit(c, k, target(t));
            let iid = c.run.new_card_id();
            let mut clone = c.cards[k as usize].clone_with_id(iid);
            clone.set_energy_cost(0, crate::card::CostExpiry::Combat, false);
            let g = c.cards.len() as KRef;
            c.card_hook_mask |= CARD_HOOKS[clone.id.index()];
            c.cards.push(clone);
            c.add_generated_card(g, PileKind::Discard, PilePos::Bottom, true);
        }
        CardId::AllForOne => {
            hit(c, k, target(t));
            let discard: Vec<KRef> = c.players[who].discard.clone();
            for q in discard {
                let card = &c.cards[q as usize];
                if !matches!(card.card_type, CardType::Attack | CardType::Skill | CardType::Power) || card.x_cost {
                    continue;
                }
                if c.modified_card_cost(who, q) == 0 {
                    move_to_hand(c, q);
                }
            }
        }
        CardId::BufferCard => {
            let n = var(c, k, VarKey::BufferPower, 1);
            self_power(c, k, PowerId::Buffer, n);
        }
        CardId::ConsumingShadow => {
            for _ in 0..var(c, k, VarKey::Repeat, 2) {
                c.channel_orb(who, OrbKind::Dark);
            }
            let n = var(c, k, VarKey::ConsumingShadowPower, 1);
            self_power(c, k, PowerId::ConsumingShadow, n);
        }
        CardId::Coolant => {
            let n = var(c, k, VarKey::CoolantPower, 2);
            self_power(c, k, PowerId::Coolant, n);
        }
        CardId::CreativeAiCard => {
            let n = var(c, k, VarKey::CreativeAi, 1);
            self_power(c, k, PowerId::CreativeAi, n);
        }
        // HelloWorld.cs:19-23 applies one HelloWorldPower. The upgrade only
        // adds Innate, so the played effect stays at one stack.
        CardId::HelloWorldCard => self_power(c, k, PowerId::HelloWorld, 1),
        CardId::Defragment => {
            let n = var(c, k, VarKey::FocusPower, 1);
            self_power(c, k, PowerId::Focus, n);
        }
        CardId::EchoFormCard => {
            let n = var(c, k, VarKey::EchoForm, 1);
            self_power(c, k, PowerId::EchoForm, n);
        }
        CardId::FlakCannon => {
            let statuses: Vec<KRef> = c.pile_cards(who).into_iter()
                .filter(|&q| !c.players[who].exhaust.contains(&q) && c.cards[q as usize].is_status()).collect();
            for &q in &statuses {
                exhaust(c, q);
            }
            for _ in 0..statuses.len() {
                if c.cr(who).is_dead() {
                    break;
                }
                let alive = c.hittable_enemies();
                if alive.is_empty() {
                    break;
                }
                let e = alive[c.run.rngs.combat_targets.choice_index(alive.len())];
                hit(c, k, e);
            }
        }
        CardId::GeneticAlgorithm => {
            let base = var(c, k, VarKey::Block, 0);
            block_base(c, k, base);
            let inc = var(c, k, VarKey::Increase, 3);
            increase_block(c, k, inc);
        }
        CardId::HelixDrill => {
            let mut hits = c.hist.energy_spent_this_turn[who as usize];
            if c.players[who].play.contains(&k) {
                hits -= c.modified_card_cost(who, k);
            }
            hit_n(c, k, target(t), hits);
        }
        CardId::Hyperbeam => {
            hit_all(c, k);
            let n = var(c, k, VarKey::FocusPower, 3);
            self_power(c, k, PowerId::HyperbeamFocusDown, n);
        }
        CardId::IceLance => {
            hit(c, k, target(t));
            for _ in 0..var(c, k, VarKey::Repeat, 3) {
                c.channel_orb(who, OrbKind::Frost);
            }
        }
        // C# Ignition.OnPlay channels Plasma for cardPlay.Target.Player.
        CardId::Ignition => c.channel_orb(t.expect("IGNITION without target"), OrbKind::Plasma),
        CardId::MachineLearningCard => {
            let n = var(c, k, VarKey::Cards, 1);
            self_power(c, k, PowerId::MachineLearning, n);
        }
        CardId::MeteorStrike => {
            hit(c, k, target(t));
            for _ in 0..3 {
                c.channel_orb(who, OrbKind::Plasma);
            }
        }
        CardId::Modded => {
            let slots = var(c, k, VarKey::Repeat, 1);
            c.add_orb_slots(who, slots);
            let n = var(c, k, VarKey::Cards, 1);
            c.draw_cards_then_increase_combat_cost(who, n, k);
        }
        CardId::MultiCast => {
            let n = x_plus_upgrade(c, k);
            evoke_front_times(c, who,n);
        }
        CardId::Rainbow => {
            c.channel_orb(who, OrbKind::Lightning);
            c.channel_orb(who, OrbKind::Frost);
            c.channel_orb(who, OrbKind::Dark);
        }
        CardId::Reboot => {
            let hand: Vec<KRef> = c.players[who].hand.drain(..).collect();
            c.players[who].draw.extend(hand.iter().copied());
            for q in hand { c.fire_after_card_changed_piles(q, PileKind::Hand, None); }
            let n = var(c, k, VarKey::Cards, 4);
            c.shuffle_then_draw(who,n);
        }
        // C# Shatter.OnPlay: for each orb held after the hit, EvokeNext(dequeue: false) then
        // EvokeNext() - every orb is evoked twice.
        CardId::Shatter => {
            hit_all(c, k);
            for _ in 0..c.players[who].orbs.orbs.len() {
                evoke_front_times(c, who,2);
            }
        }
        CardId::SignalBoost => {
            let n = var(c, k, VarKey::SignalBoostPower, 1);
            self_power(c, k, PowerId::SignalBoost, n);
        }
        CardId::SpinnerCard => {
            if c.cards[k as usize].upgraded {
                c.channel_orb(who, OrbKind::Glass);
            }
            let n = var(c, k, VarKey::SpinnerPower, 1);
            self_power(c, k, PowerId::Spinner, n);
        }
        CardId::Supercritical => {
            let n = var(c, k, VarKey::Energy, 4);
            gain_energy(c, who, n);
        }
        CardId::TrashToTreasure => self_power(c, k, PowerId::TrashToTreasure, 1),
        CardId::Voltaic => {
            for _ in 0..c.hist.lightning_channeled {
                c.channel_orb(who, OrbKind::Lightning);
            }
        }
        CardId::Hibernate => {
            c.apply_power_to(who, PowerId::Hibernate, 1, Some(who));
            for _ in 0..var(c, k, VarKey::Repeat, 2).max(0) {
                c.channel_orb(who, OrbKind::Frost);
            }
        }
        CardId::OneForAll => {
            // C# OneForAll.OnPlay applies the power to CombatState.Players in seat order.
            let n = var(c, k, VarKey::OneForAllPower, 3);
            for player in 0..c.players.len() as CId {
                c.apply_power_to(player, PowerId::OneForAll, n, Some(who));
            }
        }
        // ── ANCIENT ──
        CardId::BiasedCognitionCard => {
            let f = var(c, k, VarKey::FocusPower, 4);
            let b = var(c, k, VarKey::BiasedCognitionPower, 1);
            self_power(c, k, PowerId::Focus, f);
            self_power(c, k, PowerId::BiasedCognition, b);
        }
        CardId::Quadcast => {
            let n = var(c, k, VarKey::Repeat, 4);
            evoke_front_times(c, who,n);
        }
        CardId::Compact => {
            block(c, k);
            let up = c.cards[k as usize].upgraded;
            let statuses: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> =
                c.players[who].hand.iter().copied().filter(|&q| c.cards[q as usize].is_status()).collect();
            transform_cards_in_pile(c, &statuses, CardId::Fuel, up);
        }
        CardId::ImitationLearning => {
            // C# ImitationLearning.OnPlay: instances are keyed by PlayerTarget.
            let Some(player) = t else {
                c.unported("ImitationLearning requires an ally target");
                return true;
            };
            let n = var(c, k, VarKey::ImitationLearningPower, 2);
            let existing = c.cr(who).power_list().into_iter().find(|&p|
                c.powers[p as usize].id == PowerId::ImitationLearning
                    && c.powers[p as usize].target == Some(player));
            if let Some(p) = existing {
                c.modify_power_instance(p, n, Some(who), Some(k));
            } else if let Some(p) = c.apply_power_to(who, PowerId::ImitationLearning, n, Some(who)) {
                c.powers[p as usize].target = Some(player);
            }
        }
        _ => return false,
    }
    true
}

/// Choice continuations.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    let ChoiceSrc::Card(id) = r.src else { return false };
    match id {
        CardId::Hologram => {
            if let Some(&q) = selected.first() {
                move_to_hand(c, q);
            }
        }
        CardId::Scavenge => {
            if let Some(&q) = selected.first() {
                exhaust(c, q);
            }
            let e = var(c, r.k, VarKey::Energy, 2);
            self_power(c, r.k, PowerId::EnergyNextTurn, e);
        }
        _ => return false,
    }
    true
}

/// ROCKET_PUNCH `after_card_generated_for_combat`: a Status the owner gets makes it free this turn.
pub(crate) fn on_after_card_generated(c: &mut Combat, listener: KRef, generated: KRef) {
    if c.cards[listener as usize].id != CardId::RocketPunch || !c.cards[generated as usize].is_status() {
        return;
    }
    if c.cards[generated as usize].owner != c.cards[listener as usize].owner {
        return;
    }
    let card = &mut c.cards[listener as usize];
    card.set_energy_cost(0, crate::card::CostExpiry::Turn, false);
}
