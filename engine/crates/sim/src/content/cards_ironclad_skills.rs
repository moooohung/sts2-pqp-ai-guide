//! r7 `cards/ironclad.py` skill/power cards and the status/curse cards Ironclad can meet.

use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind, PilePos, Props};
use crate::choice::{ChoiceSrc, Resolver};
use crate::hooks::Ev;
use crate::tables::*;

#[inline]
fn owner(c: &Combat, k: KRef) -> CId {
    c.cards[k as usize].owner
}

#[inline]
fn var(c: &Combat, k: KRef, key: VarKey, default: i32) -> i32 {
    c.cards[k as usize].var(key).unwrap_or(default)
}

use super::kit::gain_energy;

fn lose_energy(c: &mut Combat, who: CId, amount: i32) {
    if amount > 0 && !c.is_over {
        let p = &mut c.players[who];
        p.energy = (p.energy - amount).max(0);
    }
}

#[allow(dead_code)] // Called by on_turn_end_in_hand once card-listener dispatch is wired.
fn self_damage(c: &mut Combat, k: KRef, amount: i32) {
    let who = owner(c, k);
    let props = Props::MOVE.or(Props::UNPOWERED);
    let damage = c.calculate_damage(amount, Some(who), who, props);
    c.apply_damage(who, damage, props, Some(who));
}

fn self_hp_loss(c: &mut Combat, k: KRef, amount: i32) {
    let who = owner(c, k);
    let props = Props::MOVE.or(Props::UNPOWERED).or(Props::UNBLOCKABLE);
    c.apply_damage(who, amount, props, Some(who));
}

#[allow(dead_code)] // Status-card hook helper; r7 deliberately has no dealer here.
fn status_hp_loss(c: &mut Combat, k: KRef, amount: i32) {
    let who = owner(c, k);
    let props = Props::MOVE.or(Props::UNPOWERED).or(Props::UNBLOCKABLE);
    c.apply_damage(who, amount, props, None);
    c.check_combat_end();
}

pub(super) fn gain_resolved_block(c: &mut Combat, target: CId, amount: i32) {
    let before = c.cr(target).block;
    c.gain_recorded_block(target,amount);
    if c.cr(target).block > before {
        c.record_block_gained_event(target, Props::MOVE, c.active_card);
        c.fire(
            Hook::AfterBlockGained,
            Ev::BlockGained {
                creature: target,
                amount: c.cr(target).block - before,
            },
            None,
        );
    }
}

pub(super) fn exhaust_from_hand(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let who = owner(c, k);
    if let Some(i) = c.players[who].hand.iter().position(|&x| x == k) {
        c.players[who].hand.remove(i);
    }
    c.players[who].exhaust.push(k);
    c.fire_after_card_changed_piles(k, PileKind::Hand, None);
    c.fire_after_card_exhausted(k);
}

fn apply_self_power(c: &mut Combat, k: KRef, id: PowerId, amount: i32) {
    let who = owner(c, k);
    c.apply_power_to(who, id, amount, Some(who));
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, target: Option<CId>) -> bool {
    let id = c.cards[k as usize].id;
    let who = owner(c, k);
    match id {
        CardId::DualWield => {
            // Models/Cards/DualWield.cs: select an Attack or Power from Owner's
            // hand, then CreateClone/AddGeneratedCardToCombat once (twice upgraded).
            let candidates: arrayvec::ArrayVec<KRef, 10> = c.players[who].hand.iter().copied()
                .filter(|&h| matches!(c.cards[h as usize].card_type, CardType::Attack | CardType::Power))
                .collect();
            if !candidates.is_empty() {
                let r = Resolver { src: ChoiceSrc::Card(id), k, target, step: 0, arg: 0 };
                c.request_card_choice_for_player(who, &candidates, PileKind::Hand, r, false);
            }
        }
        CardId::Armaments => {
            // r7 cards/ironclad.py:236-258
            c.gain_card_block(k);
            let candidates: arrayvec::ArrayVec<KRef, 10> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&h| {
                    let card = &c.cards[h as usize];
                    !card.upgraded && card_info(card.id, card.upgraded).is_some_and(|info| info.upgradable)
                })
                .collect();
            if candidates.is_empty() {
            } else if c.cards[k as usize].upgraded {
                for h in candidates {
                    c.upgrade_card(h);
                }
            } else {
                let r = Resolver { src: ChoiceSrc::Card(CardId::Armaments), k, target, step: 0, arg: 0 };
                c.request_card_choice(&candidates, PileKind::Hand, r, false);
            }
        }
        CardId::BloodWall => {
            self_hp_loss(c, k, var(c, k, VarKey::HpLoss, 2));
            if !c.is_over {
                c.gain_card_block(k);
            }
        }
        CardId::Bloodletting => {
            self_hp_loss(c, k, var(c, k, VarKey::HpLoss, 3));
            gain_energy(c, who, var(c, k, VarKey::Energy, 2));
        }
        CardId::Havoc => c.auto_play_from_draw(owner(c, k), 1, true),
        CardId::ShrugItOff => {
            c.gain_card_block(k);
            c.draw_cards_at_end_of_play(owner(c, k), var(c, k, VarKey::Cards, 1));
        }
        CardId::Tremble => {
            let t = target.expect("ANY_ENEMY card without target");
            c.apply_power_to(
                t,
                PowerId::Vulnerable,
                var(c, k, VarKey::Vulnerable, 2),
                Some(owner(c, k)),
            );
        }
        CardId::TrueGrit => {
            c.gain_card_block(k);
            if !c.players[who].hand.is_empty() {
                if c.cards[k as usize].upgraded {
                    let candidates: arrayvec::ArrayVec<KRef, 10> = c.players[who].hand.iter().copied().collect();
                    let r = Resolver {
                        src: ChoiceSrc::Card(CardId::TrueGrit),
                        k,
                        target,
                        step: 0,
                        arg: 0,
                    };
                    c.request_card_choice(&candidates, PileKind::Hand, r, false);
                } else {
                    let i = c.run.rngs.combat_card_selection.choice_index(c.players[who].hand.len());
                    exhaust_from_hand(c, c.players[who].hand[i]);
                }
            }
        }
        CardId::BattleTrance => {
            let who=owner(c,k);
            c.draw_cards_at_end_of_play(who,var(c,k,VarKey::Cards,3));
            if c.pending_choice.is_some()||c.signalled_setup_choice.is_some() {
                c.pending_play.last_mut().expect("Battle Trance frame").post_draw_power=Some((who,PowerId::NoDraw,1));
            } else {apply_self_power(c,k,PowerId::NoDraw,1);}
        }
        CardId::BurningPact => {
            if c.players[who].hand.is_empty() {
                c.draw_cards(owner(c, k), var(c, k, VarKey::Cards, 2), false);
            } else {
                let candidates: arrayvec::ArrayVec<KRef, 10> = c.players[who].hand.iter().copied().collect();
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::BurningPact),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                c.request_card_choice(&candidates, PileKind::Hand, r, false);
            }
        }
        CardId::Blaze => {
            // C# Blaze.OnPlay: permanent Strength belongs to cardPlay.Target.
            let who = owner(c, k);
            c.apply_power_to(
                target.expect("BLAZE without target"),
                PowerId::Strength,
                var(c, k, VarKey::Strength, 5),
                Some(who),
            );
        }
        CardId::DemonicShield => {
            let t = target.expect("ALLY card without target");
            self_hp_loss(c, k, var(c, k, VarKey::HpLoss, 1));
            if !c.is_over {
                let raw = c.cr(owner(c, k)).block;
                let block = c.calculate_block(raw, t, Props::MOVE, Some(k));
                gain_resolved_block(c, t, block);
            }
        }
        CardId::Dominate => {
            let t = target.expect("ANY_ENEMY card without target");
            let who = owner(c, k);
            c.apply_power_to(
                t,
                PowerId::Vulnerable,
                var(c, k, VarKey::Vulnerable, 1),
                Some(who),
            );
            let vuln = c.power_amount(t, PowerId::Vulnerable);
            c.apply_power_to(who, PowerId::Strength, vuln, Some(who));
        }
        CardId::DrumOfBattleCard => {
            c.draw_cards(owner(c, k), var(c, k, VarKey::Cards, 2), false);
        }
        CardId::EvilEye => {
            c.gain_card_block(k);
            if c.hist.exhausted_this_turn > 0 && !c.is_over_or_ending() {
                c.gain_card_block(k);
            }
        }
        CardId::ExpectAFight => {
            let who = owner(c, k);
            let strength = c.power_amount(who, PowerId::Strength).max(0);
            let raw = var(c, k, VarKey::CalcBase, 15) + var(c, k, VarKey::CalcExtra, 5) * strength;
            let block = c.calculate_block(raw, who, Props::MOVE, Some(k));
            gain_resolved_block(c, who, block);
        }
        CardId::FeelNoPainCard => {
            apply_self_power(c, k, PowerId::FeelNoPain, var(c, k, VarKey::Power, 3))
        }
        CardId::FlameBarrierCard => {
            c.gain_card_block(k);
            apply_self_power(
                c,
                k,
                PowerId::FlameBarrier,
                var(c, k, VarKey::DamageBack, 4),
            );
        }
        CardId::ForgottenRitual => gain_energy(c, who, var(c, k, VarKey::Energy, 3)),
        // r7 cards/status.py:1127-1130.
        CardId::Luminesce => gain_energy(c, who, var(c, k, VarKey::Energy, 2)),
        // r7 cards/ironclad.py:2246-2248.
        CardId::NotYet => {
            c.heal(owner(c, k), var(c, k, VarKey::Heal, 10));
        }
        // r7 cards/status.py:992-1001.
        CardId::Peck => {
            let t = target.expect("ANY_ENEMY card without target");
            let who = owner(c, k);
            let base = c.cards[k as usize].damage.unwrap_or(2);
            let hits = var(c, k, VarKey::Repeat, 3).max(0);
            for _ in 0..hits {
                if c.cr(t).is_dead() {
                    break;
                }
                let damage = c.calculate_damage(base, Some(who), t, Props::MOVE);
                c.apply_damage(t, damage, Props::MOVE, Some(who));
            }
        }
        // r7 cards/status.py:1061-1077.
        CardId::ToricToughness => {
            let who = owner(c, k);
            let before = c.cr(who).block;
            c.gain_card_block(k);
            let gained = c.cr(who).block - before;
            if let Some(p) = c.apply_power_to(
                who,
                PowerId::ToricToughness,
                var(c, k, VarKey::Turns, 2),
                Some(who),
            ) {
                // ToricToughnessPower.set_block writes the newest instanced power.
                c.powers[p as usize].payload = gained;
            }
        }
        CardId::InfernalBlade => {
            // r7 cards/ironclad.py infernal_blade: one random Ironclad attack, free this turn.
            if let Some(&g) = c.generate_character_cards(who,Some(CardType::Attack), None, 1, true).first() {
                c.set_temporary_free_this_turn(g);
                c.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        CardId::InfernoCard => {
            let who = owner(c, k);
            apply_self_power(c, k, PowerId::Inferno, var(c, k, VarKey::InfernoPower, 6));
            // fire: cards/ironclad.py:1292
            c.increment_power_self_damage(who, PowerId::Inferno);
        }
        CardId::Inflame => {
            apply_self_power(c, k, PowerId::Strength, var(c, k, VarKey::Strength, 2))
        }
        CardId::JugglingCard => apply_self_power(c, k, PowerId::Juggling, 1),
        CardId::RageCard => apply_self_power(c, k, PowerId::Rage, var(c, k, VarKey::Power, 3)),
        CardId::RuptureCard => {
            apply_self_power(c, k, PowerId::Rupture, var(c, k, VarKey::Strength, 1))
        }
        CardId::SecondWind => {
            c.start_exhaust_caller(k, None);
        }
        CardId::StampedeCard => {
            apply_self_power(c, k, PowerId::Stampede, var(c, k, VarKey::Power, 1))
        }
        CardId::StoneArmor => {
            apply_self_power(c, k, PowerId::Plating, var(c, k, VarKey::Plating, 4))
        }
        CardId::Taunt => {
            let t = target.expect("ANY_ENEMY card without target");
            c.gain_card_block(k);
            c.apply_power_to(
                t,
                PowerId::Vulnerable,
                var(c, k, VarKey::Vulnerable, 1),
                Some(owner(c, k)),
            );
        }
        CardId::ViciousCard => {
            apply_self_power(c, k, PowerId::Vicious, var(c, k, VarKey::Cards, 1))
        }
        CardId::AggressionCard => apply_self_power(c, k, PowerId::Aggression, 1),
        CardId::BarricadeCard => apply_self_power(c, k, PowerId::Barricade, 1),
        CardId::Brand => {
            self_hp_loss(c, k, var(c, k, VarKey::HpLoss, 1));
            if !c.is_over {
                if c.players[who].hand.is_empty() {
                    apply_self_power(c, k, PowerId::Strength, var(c, k, VarKey::Strength, 1));
                } else {
                    let candidates: arrayvec::ArrayVec<KRef, 10> = c.players[who].hand.iter().copied().collect();
                    let r = Resolver {
                        src: ChoiceSrc::Card(CardId::Brand),
                        k,
                        target,
                        step: 0,
                        arg: 0,
                    };
                    c.request_card_choice(&candidates, PileKind::Hand, r, false);
                }
            }
        }
        CardId::Cascade => {
            let count = c.cards[k as usize].energy_spent + i32::from(c.cards[k as usize].upgraded);
            if count > 0 {
                c.auto_play_from_draw(owner(c, k), count as usize, false);
            }
        }
        CardId::ColossusCard => {
            c.gain_card_block(k);
            apply_self_power(c, k, PowerId::Colossus, var(c, k, VarKey::Colossus, 1));
        }
        CardId::CrimsonMantle => {
            let who = owner(c, k);
            apply_self_power(
                c,
                k,
                PowerId::CrimsonMantle,
                var(c, k, VarKey::CrimsonMantlePower, 7),
            );
            // fire: cards/ironclad.py:1864
            c.increment_power_self_damage(who, PowerId::CrimsonMantle);
        }
        CardId::CrueltyCard => {
            apply_self_power(c, k, PowerId::Cruelty, var(c, k, VarKey::CrueltyPower, 25))
        }
        CardId::DarkEmbraceCard => apply_self_power(c, k, PowerId::DarkEmbrace, 1),
        CardId::DemonFormCard => {
            apply_self_power(c, k, PowerId::DemonForm, var(c, k, VarKey::Strength, 3))
        }
        CardId::HellraiserCard => apply_self_power(c, k, PowerId::Hellraiser, 1),
        // C# Entrench.OnPlay / CreatureCmd.GainBlock: Unpowered|Move means no
        // Dexterity/Frail, but retain card source, history props, and the resolved amount.
        CardId::Entrench => {
            if !c.is_over_or_ending() && !c.cr(who).is_dead() {
                let props = Props::UNPOWERED.or(Props::MOVE);
                let amount = c.calculate_block(c.cr(who).block, who, props, Some(k));
                c.gain_recorded_block(who,amount);
                if amount > 0 {
                    c.record_block_gained_event(who, props, Some(k));
                }
                c.fire(Hook::AfterBlockGained, Ev::BlockGained { creature: who, amount }, None);
            }
        }
        CardId::Impervious => c.gain_card_block(k),
        CardId::JuggernautCard => apply_self_power(
            c,
            k,
            PowerId::Juggernaut,
            var(c, k, VarKey::JuggernautPower, 5),
        ),
        CardId::Offering => {
            self_hp_loss(c, k, var(c, k, VarKey::HpLoss, 6));
            gain_energy(c, who, var(c, k, VarKey::Energy, 2));
            c.draw_cards_at_end_of_play(owner(c, k), var(c, k, VarKey::Cards, 3));
        }
        CardId::OneTwoPunchCard => {
            apply_self_power(c, k, PowerId::OneTwoPunch, var(c, k, VarKey::Attacks, 1))
        }
        CardId::PrimalForce => {
            let who = owner(c, k);
            let upgraded = c.cards[k as usize].upgraded;
            let attacks: arrayvec::ArrayVec<KRef, 10> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&h| c.cards[h as usize].card_type == CardType::Attack)
                .collect();
            for old in attacks {
                let Some(pos) = c.players[who].hand.iter().position(|&q| q == old) else {
                    continue;
                };
                let Some(new) = c.new_card(who, CardId::GiantRock, upgraded) else {
                    continue;
                };
                c.cards[new as usize].owner = who;
                c.players[who].hand[pos] = new;
                c.hist.generated_combat += 1;
                c.hist.generated_by_player_combat += 1;
                c.hist.generated_by_creator[who as usize] += 1;
                c.fire(Hook::AfterCardEnteredCombat, Ev::Card(new), None);
                c.fire_after_card_generated(new, true);
            }
        }
        CardId::Pyre => apply_self_power(c, k, PowerId::Pyre, var(c, k, VarKey::Energy, 1)),
        CardId::Stoke => {
            let cards = c.players[who].hand.clone();
            let count = cards.len();
            for h in cards {
                exhaust_from_hand(c, h);
            }
            // r7 cards/ironclad.py stoke: as many random Ironclad cards as were exhausted.
            let upgraded = c.cards[k as usize].upgraded;
            for g in c.generate_character_cards(who,None, None, count, false) {
                if upgraded {
                    c.upgrade_card(g);
                }
                c.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        CardId::TankCard => apply_self_power(c, k, PowerId::Tank, 1),
        CardId::Unmovable => apply_self_power(c, k, PowerId::Unmovable, 1),
        CardId::CorruptionCard => apply_self_power(c, k, PowerId::Corruption, 1),
        CardId::Slimed => {
            // Models/Cards/Slimed.cs:26-33 ends with an awaited Draw.
            // Result-pile exhaust and after-play hooks must wait for Stratagem.
            c.draw_cards_at_end_of_play(owner(c, k), var(c, k, VarKey::Cards, 1));
        }
        CardId::FranticEscape => {
            let who = owner(c, k);
            // C# FranticEscape.GetSandpitEnemy: inspect the first enemy with Sandpit.
            if let Some(enemy) = c.enemies.iter().copied().find(|&e| c.has_power(e, PowerId::Sandpit)) {
                let sandpit = c.cr(enemy).power_list().into_iter().find(|&p| {
                    c.powers[p as usize].id == PowerId::Sandpit
                        && c.powers[p as usize].target == Some(who)
                });
                if let Some(p) = sandpit {
                    // C# FranticEscape.OnPlay: modify only the SandpitPower whose Target is this
                    // card owner's creature.
                    c.powers[p as usize].amount += 1;
                }
            }
            let card = &mut c.cards[k as usize];
            // C# FranticEscape.OnPlay.
            card.add_combat_cost(1);
            card.cv.frantic_escape_cost_increase += 1;
        }
        CardId::Wound
        | CardId::Dazed
        | CardId::Burn
        | CardId::Wither
        | CardId::Void
        | CardId::Beckon
        | CardId::Debris
        | CardId::Infection
        | CardId::Soot
        | CardId::Toxic
        | CardId::Disintegration
        | CardId::MindRot
        | CardId::SlothStatus
        | CardId::WasteAway
        | CardId::AscendersBane
        | CardId::BadLuck
        | CardId::Clumsy
        | CardId::CurseOfTheBell
        | CardId::Debt
        | CardId::Decay
        | CardId::Doubt
        | CardId::Enthralled
        | CardId::Folly
        | CardId::Greed
        | CardId::Guilty
        | CardId::Injury
        | CardId::Normality
        | CardId::PoorSleep
        | CardId::Regret
        | CardId::Shame
        | CardId::SporeMind
        | CardId::Writhe => {}
        _ => return false,
    }
    true
}

/// r7 cards/ironclad.py:962 `drum_of_battle_after_exhausted`.
pub(crate) fn on_after_card_exhausted(c: &mut Combat, listener: KRef, exhausted: KRef) {
    if listener != exhausted || c.cards[listener as usize].id != CardId::DrumOfBattleCard {
        return;
    }
    if !c.cards[listener as usize].enchantments.is_empty() {
        c.unported("Drum of Battle enchantment replay");
        return;
    }
    let base = 1 + c.cards[listener as usize].replay + c.sword_sage_replays(listener);
    let (count, modifiers) = c.modify_card_play_count(base, listener);
    let (count, relic_mods) = c.relic_modify_card_play_count_all(count, listener);
    c.fire_after_modifying_card_play_count(listener, modifiers);
    for r in relic_mods {
        c.relic_after_modifying_card_play_count(r, c.run.relics[r].owner, listener);
    }
    let energy = var(c, listener, VarKey::Energy, 2);
    let who = owner(c, listener);
    for _ in 0..count {
        gain_energy(c, who, energy);
    }
}

/// r7 cards/status.py turn-end-in-hand hooks.
pub(crate) fn on_turn_end_in_hand(c: &mut Combat, k: KRef, cards_in_hand_at_turn_end: i32) {
    match c.cards[k as usize].id {
        CardId::Burn => self_damage(c, k, var(c, k, VarKey::Damage, 2)),
        CardId::Wither => self_damage(c, k, var(c, k, VarKey::Damage, 3)),
        CardId::Beckon => status_hp_loss(c, k, var(c, k, VarKey::HpLoss, 6)),
        CardId::Infection => self_damage(c, k, var(c, k, VarKey::Damage, 3)),
        CardId::Toxic => self_damage(c, k, var(c, k, VarKey::Damage, 5)),
        CardId::BadLuck => status_hp_loss(c, k, var(c, k, VarKey::HpLoss, 13)),
        CardId::Debt => {
            let loss = var(c, k, VarKey::Gold, 10).max(0);
            c.change_player_gold(c.cards[k as usize].owner, -loss);
        }
        CardId::Decay => self_damage(c, k, var(c, k, VarKey::Damage, 2)),
        CardId::Doubt => apply_self_power(c, k, PowerId::Weak, var(c, k, VarKey::Weak, 1)),
        CardId::Regret => status_hp_loss(c, k, cards_in_hand_at_turn_end),
        CardId::Shame => apply_self_power(c, k, PowerId::Frail, var(c, k, VarKey::Frail, 1)),
        _ => {}
    }
}

/// r7 cards/status.py:1450 `void_after_card_drawn`.
pub(crate) fn on_after_card_drawn(c: &mut Combat, listener: KRef, drawn: KRef) {
    if listener == drawn && c.cards[listener as usize].id == CardId::Void {
        lose_energy(c, owner(c, listener), var(c, listener, VarKey::Energy, 1));
    }
}

/// r7 cards/status.py chosen hooks.
pub(crate) fn on_chosen(c: &mut Combat, k: KRef) {
    let (power, key) = match c.cards[k as usize].id {
        CardId::Disintegration => (PowerId::Disintegration, VarKey::DisintegrationPower),
        CardId::MindRot => (PowerId::MindRot, VarKey::MindRotPower),
        CardId::SlothStatus => (PowerId::Sloth, VarKey::SlothPower),
        CardId::WasteAway => (PowerId::WasteAway, VarKey::WasteAwayPower),
        _ => return,
    };
    apply_self_power(c, k, power, var(c, k, key, 1));
}

/// r7 cards/status.py should-play hooks.
#[allow(dead_code)] // Card-listener dispatch is an engine packet.
pub(crate) fn on_should_play(
    c: &Combat,
    listener: KRef,
    played: KRef,
    is_auto_play: bool,
) -> Option<bool> {
    match c.cards[listener as usize].id {
        CardId::Enthralled => {
            Some(is_auto_play || c.cards[played as usize].id == CardId::Enthralled)
        }
        CardId::Normality => {
            let limit = var(c, listener, VarKey::CalcBase, 3);
            Some(c.hist.play_starts_this_turn < limit)
        }
        _ => None,
    }
}

/// r7 cards/status.py:1711-1715. Returns whether the persistent deck card remains.
pub(crate) fn on_after_combat_end_deck(card: &mut crate::card::Card) -> bool {
    if card.id != CardId::Guilty {
        return true;
    }
    let current = card.var(VarKey::Combats).unwrap_or(5);
    let remaining = (current - 1).max(0);
    if let Some((_, value)) = card.vars
        .iter_mut()
        .find(|(key, _)| *key == VarKey::Combats)
    {
        *value = remaining;
    }
    remaining > 0
}

/// Choice continuations for this group's cards; `false` = not ours.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    match r.src {
        ChoiceSrc::Card(CardId::DualWield) => {
            if let Some(&h) = selected.first() {
                let copies = var(c, r.k, VarKey::Cards, if c.cards[r.k as usize].upgraded { 2 } else { 1 });
                for _ in 0..copies { c.clone_card_to_hand(h); }
            }
        }
        // Armaments: resolver=combat.upgrade_card
        ChoiceSrc::Card(CardId::Armaments) => {
            if let Some(&h) = selected.first() {
                c.upgrade_card(h);
            }
        }
        ChoiceSrc::Card(CardId::TrueGrit) => {
            if let Some(&h) = selected.first() {
                exhaust_from_hand(c, h);
            }
        }
        ChoiceSrc::Card(CardId::BurningPact) => {
            if let Some(&h) = selected.first() {
                exhaust_from_hand(c, h);
            }
            if !c.is_over_or_ending() {
                c.draw_cards(owner(c, r.k), var(c, r.k, VarKey::Cards, 2), false);
            }
        }
        ChoiceSrc::Card(CardId::Brand) => {
            if let Some(&h) = selected.first() {
                exhaust_from_hand(c, h);
            }
            if !c.is_over_or_ending() {
                apply_self_power(c, r.k, PowerId::Strength, var(c, r.k, VarKey::Strength, 1));
            }
        }
        _ => return false,
    }
    true
}
